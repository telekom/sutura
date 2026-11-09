//! The mapping checked against the document, once, before any answer is read.
//!
//! What comes out is a [`Contract`]: every operation the mapping names exists as a `GET` the bearer
//! can call, every path parameter is bound, every paging parameter is declared, every required
//! parameter is sent, every pointer names a place the operation's answer schema declares, and every
//! request path is a same-origin path with nothing in it the adapter did not parse.

use std::collections::BTreeSet;

use serde_json::Value;
use sutura_domain::model::ModelName;

use crate::document::{Document, Operation};
use crate::mapping::{Columns, Fallbacks, Mapping, OperationId, Paging, ParameterName, PathSegment, Pointer};

/// The most bytes the mapping may hold before it is parsed.
pub const MAX_MAPPING_BYTES: usize = 64 * 1024;

/// Why a document and a mapping do not make a catalog. Checked at construction, before any request.
///
/// No variant carries a token or an answer; a document value is repeated at most 64 bytes long.
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("the {what} is larger than the {limit}-byte cap")]
    TooLarge { what: &'static str, limit: usize },
    #[error("the document is not JSON")]
    Document {
        #[source]
        cause: serde_json::Error,
    },
    #[error("the mapping is not in the mapping vocabulary")]
    Mapping {
        #[source]
        cause: serde_norway::Error,
    },
    #[error("the document is not OpenAPI 3.0 or 3.1 (found {found:?})")]
    UnsupportedVersion { found: Option<String> },
    #[error("the document references {reference:?}, which is not a local `#/` reference; nothing outside the document is read")]
    ExternalReference { reference: String },
    #[error("the document references {reference:?}, which it does not hold")]
    UnresolvedReference { reference: String },
    #[error("the document nests references deeper than {limit}")]
    ReferenceTooDeep { limit: usize },
    #[error("the document has no operation {operation}")]
    UnknownOperation { operation: OperationId },
    #[error("the document has more than one operation {operation}")]
    DuplicateOperation { operation: OperationId },
    #[error("the operation {operation} is not a GET; the catalog only reads")]
    NotAGet { operation: OperationId },
    #[error("the operation {operation} needs a credential other than a bearer token")]
    UnsupportedSecurity { operation: OperationId },
    #[error("the operation {operation} declares no 200 JSON answer schema")]
    NoJsonAnswer { operation: OperationId },
    /// The path is not literal unreserved segments and whole-segment parameters, so the adapter
    /// cannot prove the request stays on the declared endpoint.
    #[error("the path of {operation} is not one this adapter builds; it could leave the declared endpoint")]
    OffOriginPath { operation: OperationId },
    #[error("the operation {operation} needs the parameter {parameter}, which the mapping does not bind")]
    UnboundParameter { operation: OperationId, parameter: String },
    #[error("the parameter {parameter} is not a {location} parameter of {operation}")]
    UndeclaredParameter {
        operation: OperationId,
        parameter: ParameterName,
        location: &'static str,
    },
    #[error("the mapping binds the parameter {parameter}, which no mapped operation's path has")]
    UnusedParameter { parameter: ParameterName },
    #[error("the answer schema of {operation} declares nothing at {pointer:?}")]
    NotInSchema { operation: OperationId, pointer: String },
    #[error("the answer schema of {operation} does not declare {expected} at {pointer:?}")]
    NotShaped {
        operation: OperationId,
        pointer: String,
        expected: &'static str,
    },
}

/// The per-model request path: every fixed parameter in place, split where the model's name goes.
#[derive(Debug, Clone)]
pub(crate) struct ModelPath(Vec<String>);

impl ModelPath {
    pub(crate) fn render(&self, model: &ModelName) -> String {
        self.0.join(model.as_str())
    }
}

/// A mapping checked against its document.
#[derive(Debug)]
pub struct Contract {
    mapping: Mapping,
    models: String,
    relationships: Option<ModelPath>,
}

impl Contract {
    pub(crate) const fn mapping(&self) -> &Mapping {
        &self.mapping
    }

    /// The models listing's path, every parameter in place.
    pub(crate) fn models_path(&self) -> &str {
        &self.models
    }

    /// The links listing's path, when the mapping names one.
    pub(crate) const fn relationships_path(&self) -> Option<&ModelPath> {
        self.relationships.as_ref()
    }

    /// Parses a document and a mapping and checks the one against the other.
    ///
    /// Both are capped before they are parsed. Nothing is fetched: the document's `servers` are
    /// never read, and a non-local reference is refused.
    pub fn parse(document: &str, mapping: &str) -> Result<Self, ContractError> {
        let document = Document::parse(document)?;
        if mapping.len() > MAX_MAPPING_BYTES {
            return Err(ContractError::TooLarge {
                what: "mapping",
                limit: MAX_MAPPING_BYTES,
            });
        }
        let mapping: Mapping = serde_norway::from_str(mapping).map_err(|cause| ContractError::Mapping { cause })?;
        let mut used = BTreeSet::new();

        let models = &mapping.models;
        let listed = document.operation(&models.operation)?;
        let models_path = bind(&listed, &mapping, None, &mut used)?.concat();
        check_paging(&document, &listed, models.paging.as_ref())?;
        let entry = entry_schema(&document, &listed, &models.items)?;
        for pointer in core::iter::once(&models.name).chain(models.primary_key.as_ref()) {
            in_schema(&document, &listed, entry, pointer)?;
        }
        check_fallbacks(&document, &listed, entry, &models.description)?;
        check_columns(&document, &listed, entry, &models.columns)?;

        let relationships_path = match &mapping.relationships {
            None => None,
            Some(links) => {
                let linked = document.operation(&links.operation)?;
                let path = ModelPath(bind(&linked, &mapping, Some(&links.model_parameter), &mut used)?);
                check_paging(&document, &linked, links.paging.as_ref())?;
                let entry = entry_schema(&document, &linked, &links.items)?;
                for pointer in [&links.name, &links.target, &links.foreign_key] {
                    in_schema(&document, &linked, entry, pointer)?;
                }
                Some(path)
            }
        };
        if let Some(unused) = mapping.parameters.keys().find(|name| !used.contains(name.as_str())) {
            return Err(ContractError::UnusedParameter {
                parameter: unused.clone(),
            });
        }
        Ok(Self {
            mapping,
            models: models_path,
            relationships: relationships_path,
        })
    }
}

/// The operation's path with every fixed parameter in place, split around the per-model one.
///
/// Refused when a segment is neither a plain unreserved segment nor a whole-segment parameter, so
/// the path cannot climb, add an authority, start a query or carry an escape; and when a template
/// parameter is unbound.
fn bind(
    operation: &Operation<'_>,
    mapping: &Mapping,
    per_model: Option<&ParameterName>,
    used: &mut BTreeSet<String>,
) -> Result<Vec<String>, ContractError> {
    let off_origin = || ContractError::OffOriginPath {
        operation: operation.id.clone(),
    };
    let rest = operation.path.strip_prefix('/').ok_or_else(off_origin)?;
    let mut pieces = Vec::new();
    let mut current = String::from("/");
    for (index, segment) in rest.split('/').enumerate() {
        if index > 0 {
            current.push('/');
        }
        let Some(name) = segment.strip_prefix('{').and_then(|inner| inner.strip_suffix('}')) else {
            let segment = PathSegment::try_from(segment.to_owned()).map_err(|_unsafe| off_origin())?;
            current.push_str(segment.as_str());
            continue;
        };
        if per_model.is_some_and(|model| model.as_str() == name) {
            pieces.push(core::mem::take(&mut current));
            continue;
        }
        let value = mapping
            .parameters
            .iter()
            .find_map(|(declared, value)| (declared.as_str() == name).then_some(value))
            .ok_or_else(|| ContractError::UnboundParameter {
                operation: operation.id.clone(),
                parameter: name.to_owned(),
            })?;
        used.insert(name.to_owned());
        current.push_str(value.as_str());
    }
    pieces.push(current);
    if let Some(model) = per_model.filter(|_| pieces.len() == 1) {
        return Err(ContractError::UndeclaredParameter {
            operation: operation.id.clone(),
            parameter: model.clone(),
            location: "path",
        });
    }
    Ok(pieces)
}

/// The cursor is a place the answer schema declares, the paging parameters are declared query
/// parameters, and no other required query, header or
/// cookie parameter is left for the adapter to invent.
fn check_paging(document: &Document, operation: &Operation<'_>, paging: Option<&Paging>) -> Result<(), ContractError> {
    if let Some(paging) = paging {
        in_schema(document, operation, operation.answer, &paging.next)?;
    }
    let sent: Vec<&ParameterName> = paging
        .into_iter()
        .flat_map(|paging| [Some(&paging.cursor), paging.size.as_ref().map(|size| &size.parameter)])
        .flatten()
        .collect();
    if let Some(paging) = paging
        && operation
            .parameters
            .iter()
            .any(|known| known.required && known.location == "query" && known.name == paging.cursor.as_str())
    {
        return Err(ContractError::UnboundParameter {
            operation: operation.id.clone(),
            parameter: paging.cursor.clone(),
        });
    }
    for parameter in &sent {
        let declared = operation
            .parameters
            .iter()
            .any(|known| known.location == "query" && known.name == parameter.as_str());
        if !declared {
            return Err(ContractError::UndeclaredParameter {
                operation: operation.id.clone(),
                parameter: (*parameter).clone(),
                location: "query",
            });
        }
    }
    let unbound = operation
        .parameters
        .iter()
        .filter(|known| known.required && known.location != "path")
        .find(|known| known.location != "query" || !sent.iter().any(|name| name.as_str() == known.name));
    if let Some(unbound) = unbound {
        return Err(ContractError::UnboundParameter {
            operation: operation.id.clone(),
            parameter: unbound.name.to_owned(),
        });
    }
    Ok(())
}

fn not_in_schema(operation: &Operation<'_>, pointer: &Pointer) -> ContractError {
    ContractError::NotInSchema {
        operation: operation.id.clone(),
        pointer: pointer.as_str().to_owned(),
    }
}

fn not_shaped(operation: &Operation<'_>, pointer: &Pointer, expected: &'static str) -> ContractError {
    ContractError::NotShaped {
        operation: operation.id.clone(),
        pointer: pointer.as_str().to_owned(),
        expected,
    }
}

/// The schema of one listed entry: `items` reaches a list in the answer schema.
fn entry_schema<'document>(
    document: &'document Document,
    operation: &Operation<'document>,
    items: &Pointer,
) -> Result<&'document Value, ContractError> {
    let list = document
        .schema_at(operation.answer, items)
        .ok_or_else(|| not_in_schema(operation, items))?;
    document.entry_of(list).ok_or_else(|| not_shaped(operation, items, "a list"))
}

fn in_schema<'document>(
    document: &'document Document,
    operation: &Operation<'document>,
    schema: &'document Value,
    pointer: &Pointer,
) -> Result<&'document Value, ContractError> {
    document
        .schema_at(schema, pointer)
        .ok_or_else(|| not_in_schema(operation, pointer))
}

fn check_fallbacks(
    document: &Document,
    operation: &Operation<'_>,
    schema: &Value,
    fallbacks: &Fallbacks,
) -> Result<(), ContractError> {
    for pointer in fallbacks.iter() {
        in_schema(document, operation, schema, pointer)?;
    }
    Ok(())
}

fn check_columns(document: &Document, operation: &Operation<'_>, entry: &Value, columns: &Columns) -> Result<(), ContractError> {
    let at = in_schema(document, operation, entry, &columns.at)?;
    let column = match &columns.name {
        Some(name) => {
            let column = document
                .entry_of(at)
                .ok_or_else(|| not_shaped(operation, &columns.at, "a list"))?;
            in_schema(document, operation, column, name)?;
            column
        }
        None => document
            .value_of(at)
            .ok_or_else(|| not_shaped(operation, &columns.at, "a map"))?,
    };
    if let Some(data_type) = &columns.data_type {
        in_schema(document, operation, column, data_type)?;
    }
    if let Some(description) = &columns.description {
        check_fallbacks(document, operation, column, description)?;
    }
    Ok(())
}
