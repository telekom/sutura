//! The part of an `OpenAPI` 3.0/3.1 document this adapter reads: the version, each operation's path,
//! method, parameters, security and `200` JSON answer schema, and the local references between them.
//!
//! The document is a contract the mapping is checked against, never a source of URLs: `servers`,
//! `externalDocs`, `links` and callbacks are not read, and a `$ref` that is not local (`#/...`) is
//! refused anywhere in the document before anything else is read, so nothing is fetched or opened
//! on its behalf. JSON only: no YAML alias can expand inside the document.

use serde_json::Value;

use crate::mapping::{OperationId, Pointer};
use crate::{ContractError, shown};

/// The most bytes the document may hold before it is parsed.
pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
/// The most local references one lookup follows, and the deepest a schema walk goes.
const MAX_REFERENCE_DEPTH: usize = 16;

const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

/// A parsed document, with every reference in it local.
#[derive(Debug)]
pub(crate) struct Document(Value);

/// One `GET` operation, with its path-item parameters folded in.
pub(crate) struct Operation<'document> {
    pub(crate) id: &'document OperationId,
    pub(crate) path: &'document str,
    pub(crate) parameters: Vec<Parameter<'document>>,
    /// The `200` answer's `application/json` schema.
    pub(crate) answer: &'document Value,
}

pub(crate) struct Parameter<'document> {
    pub(crate) name: &'document str,
    pub(crate) location: &'document str,
    pub(crate) required: bool,
}

impl Document {
    pub(crate) fn parse(text: &str) -> Result<Self, ContractError> {
        if text.len() > MAX_DOCUMENT_BYTES {
            return Err(ContractError::TooLarge {
                what: "document",
                limit: MAX_DOCUMENT_BYTES,
            });
        }
        let root: Value = serde_json::from_str(text).map_err(|cause| ContractError::Document { cause })?;
        check_version(&root)?;
        refuse_external_references(&root)?;
        Ok(Self(root))
    }

    /// Follows local references until a value that is not one.
    pub(crate) fn resolve<'value>(&'value self, mut value: &'value Value) -> Result<&'value Value, ContractError> {
        for _ in 0..MAX_REFERENCE_DEPTH {
            let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
                return Ok(value);
            };
            value = reference
                .strip_prefix('#')
                .and_then(|pointer| self.0.pointer(pointer))
                .ok_or_else(|| ContractError::UnresolvedReference {
                    reference: shown(reference),
                })?;
        }
        Err(ContractError::ReferenceTooDeep {
            limit: MAX_REFERENCE_DEPTH,
        })
    }

    /// The operation the mapping names, refused unless it is exactly one `GET` the adapter can call
    /// with a bearer and read as JSON.
    pub(crate) fn operation<'document>(
        &'document self,
        id: &'document OperationId,
    ) -> Result<Operation<'document>, ContractError> {
        let mut found = None;
        let paths = self.0.get("paths").and_then(Value::as_object).into_iter().flatten();
        for (path, item) in paths {
            let item = self.resolve(item)?;
            for method in METHODS {
                let Some(operation) = item.get(method) else { continue };
                if operation.get("operationId").and_then(Value::as_str) != Some(id.as_str()) {
                    continue;
                }
                if found.is_some() {
                    return Err(ContractError::DuplicateOperation { operation: id.clone() });
                }
                found = Some((path.as_str(), item, method, operation));
            }
        }
        let (path, item, method, operation) = found.ok_or_else(|| ContractError::UnknownOperation { operation: id.clone() })?;
        if method != "get" {
            return Err(ContractError::NotAGet { operation: id.clone() });
        }
        if !self.bearer_suffices(operation)? {
            return Err(ContractError::UnsupportedSecurity { operation: id.clone() });
        }
        let parameters = self.parameters(item, operation)?;
        let answer = self
            .answer(operation)
            .ok_or_else(|| ContractError::NoJsonAnswer { operation: id.clone() })?;
        Ok(Operation {
            id,
            path,
            parameters,
            answer: self.resolve(answer)?,
        })
    }

    /// The path item's parameters, then the operation's, an operation's entry replacing a path
    /// item's of the same name and location.
    fn parameters<'document>(
        &'document self,
        item: &'document Value,
        operation: &'document Value,
    ) -> Result<Vec<Parameter<'document>>, ContractError> {
        let mut parameters: Vec<Parameter<'document>> = Vec::new();
        for declared in [item, operation]
            .into_iter()
            .filter_map(|owner| owner.get("parameters").and_then(Value::as_array))
            .flatten()
        {
            let declared = self.resolve(declared)?;
            let (Some(name), Some(location)) = (
                declared.get("name").and_then(Value::as_str),
                declared.get("in").and_then(Value::as_str),
            ) else {
                continue;
            };
            let required = location == "path" || declared.get("required").and_then(Value::as_bool) == Some(true);
            parameters.retain(|known| !(known.name == name && known.location == location));
            parameters.push(Parameter {
                name,
                location,
                required,
            });
        }
        Ok(parameters)
    }

    /// Whether some security alternative of the operation needs nothing but a bearer token.
    ///
    /// The operation's own `security` replaces the document's. An empty requirement (`{}`) needs
    /// nothing; a scheme counts when it is `http` with the `bearer` scheme, `oauth2` or
    /// `openIdConnect`, because each of those takes the token as `Authorization: Bearer`.
    fn bearer_suffices(&self, operation: &Value) -> Result<bool, ContractError> {
        let Some(requirements) = operation
            .get("security")
            .or_else(|| self.0.get("security"))
            .and_then(Value::as_array)
        else {
            return Ok(true);
        };
        if requirements.is_empty() {
            return Ok(true);
        }
        for requirement in requirements {
            let Some(schemes) = requirement.as_object() else { continue };
            let mut every = true;
            for name in schemes.keys() {
                let scheme = self
                    .0
                    .get("components")
                    .and_then(|components| components.get("securitySchemes"))
                    .and_then(|schemes| schemes.get(name.as_str()));
                let Some(scheme) = scheme else {
                    every = false;
                    break;
                };
                let scheme = self.resolve(scheme)?;
                every &= match scheme.get("type").and_then(Value::as_str) {
                    Some("http") => scheme
                        .get("scheme")
                        .and_then(Value::as_str)
                        .is_some_and(|word| word.eq_ignore_ascii_case("bearer")),
                    Some("oauth2" | "openIdConnect") => true,
                    _ => false,
                };
            }
            if every {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn answer<'document>(&'document self, operation: &'document Value) -> Option<&'document Value> {
        let response = operation.get("responses")?.get("200")?;
        let response = self.resolve(response).ok()?;
        let content = response.get("content")?.as_object()?;
        let (_, body) = content
            .iter()
            .find(|(media, _)| media.as_str() == "application/json" || media.ends_with("+json"))?;
        body.get("schema")
    }

    /// The schema a pointer reaches from `schema`, or `None` when the schema declares no such place.
    ///
    /// A token descends into a named property, else into `additionalProperties`, else (for a
    /// list) into `items`, else into the first of `allOf`/`oneOf`/`anyOf` that declares it. An
    /// unconstrained schema declares nothing, so a pointer through one is refused.
    pub(crate) fn schema_at<'value>(&'value self, schema: &'value Value, pointer: &Pointer) -> Option<&'value Value> {
        let mut at = self.resolve(schema).ok()?;
        for token in pointer.tokens() {
            at = self.descend(at, &token, MAX_REFERENCE_DEPTH)?;
        }
        Some(at)
    }

    fn descend<'value>(&'value self, schema: &'value Value, token: &str, depth: usize) -> Option<&'value Value> {
        let depth = depth.checked_sub(1)?;
        let direct = schema
            .get("properties")
            .and_then(|properties| properties.get(token))
            .or_else(|| schema.get("additionalProperties").filter(|inner| inner.is_object()))
            .or_else(|| {
                schema
                    .get("items")
                    .filter(|_| !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()))
            });
        if let Some(found) = direct {
            return self.resolve(found).ok();
        }
        ["allOf", "oneOf", "anyOf"]
            .into_iter()
            .filter_map(|keyword| schema.get(keyword).and_then(Value::as_array))
            .flatten()
            .filter_map(|alternative| self.resolve(alternative).ok())
            .find_map(|alternative| self.descend(alternative, token, depth))
    }

    /// The schema of one entry of the list `schema` declares, through `allOf`/`oneOf`/`anyOf`.
    pub(crate) fn entry_of<'value>(&'value self, schema: &'value Value) -> Option<&'value Value> {
        self.shaped(schema, "items", MAX_REFERENCE_DEPTH)
    }

    /// The schema of each value of the map `schema` declares.
    pub(crate) fn value_of<'value>(&'value self, schema: &'value Value) -> Option<&'value Value> {
        self.shaped(schema, "additionalProperties", MAX_REFERENCE_DEPTH)
    }

    fn shaped<'value>(&'value self, schema: &'value Value, keyword: &str, depth: usize) -> Option<&'value Value> {
        let depth = depth.checked_sub(1)?;
        if let Some(inner) = schema.get(keyword).filter(|inner| inner.is_object()) {
            return self.resolve(inner).ok();
        }
        ["allOf", "oneOf", "anyOf"]
            .into_iter()
            .filter_map(|combinator| schema.get(combinator).and_then(Value::as_array))
            .flatten()
            .filter_map(|alternative| self.resolve(alternative).ok())
            .find_map(|alternative| self.shaped(alternative, keyword, depth))
    }
}

/// `openapi: 3.0.x` or `3.1.x`. A Swagger 2.0 document is refused by its own version word.
fn check_version(root: &Value) -> Result<(), ContractError> {
    let declared = root.get("openapi").and_then(Value::as_str);
    if declared.is_some_and(|version| version.starts_with("3.0.") || version.starts_with("3.1.")) {
        return Ok(());
    }
    let found = declared.map(|version| format!("openapi {}", shown(version))).or_else(|| {
        root.get("swagger")
            .map(|version| format!("swagger {}", shown(version.as_str().unwrap_or("?"))))
    });
    Err(ContractError::UnsupportedVersion { found })
}

/// Refuses the first `$ref` anywhere in the document that is not a local `#/` pointer.
fn refuse_external_references(root: &Value) -> Result<(), ContractError> {
    let mut pending = vec![root];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref") {
                    let local = reference.as_str().is_some_and(|text| text.starts_with("#/"));
                    if !local {
                        return Err(ContractError::ExternalReference {
                            reference: shown(reference.as_str().unwrap_or("<not a string>")),
                        });
                    }
                }
                pending.extend(object.values());
            }
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    Ok(())
}
