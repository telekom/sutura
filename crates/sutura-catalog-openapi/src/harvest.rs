//! Answers into definitions: each listed entry read through the mapping's pointers.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use serde_json::Value;
use sutura_domain::catalog::{Column, Description, JoinKey, JoinKeys, Model, Relationship};
use sutura_domain::model::{ColumnName, InvalidIdentifier, JoinType, ModelName, RelationshipName, SourceName, TableName};

use crate::contract::Contract;
use crate::mapping::{Columns, OperationId, Paging, Pointer};
use crate::{AnswerReader, OpenApiError, Target, shown};

/// The most entries one listing may return across all its pages.
pub const MAX_ENTRIES: usize = 100_000;
/// The most bytes a cursor may hold before it is sent back.
pub const MAX_CURSOR_BYTES: usize = 2048;

pub(crate) struct Harvest<'load, R> {
    pub(crate) contract: &'load Contract,
    pub(crate) reader: &'load R,
    pub(crate) source: &'load SourceName,
    pub(crate) started: Instant,
}

/// The place a pointer read, for an error.
struct At<'place> {
    operation: &'place OperationId,
    pointer: &'place Pointer,
    on: Option<&'place str>,
}

impl At<'_> {
    fn missing(&self) -> OpenApiError {
        OpenApiError::Missing {
            operation: self.operation.clone(),
            pointer: self.pointer.as_str().to_owned(),
            on: self.on.map(shown),
        }
    }

    fn unexpected(&self, expected: &'static str) -> OpenApiError {
        OpenApiError::Unexpected {
            operation: self.operation.clone(),
            pointer: self.pointer.as_str().to_owned(),
            on: self.on.map(shown),
            expected,
        }
    }

    /// The text the pointer must reach.
    fn text<'value>(&self, value: &'value Value) -> Result<&'value str, OpenApiError> {
        self.pointer
            .find(value)
            .ok_or_else(|| self.missing())?
            .as_str()
            .ok_or_else(|| self.unexpected("text"))
    }
}

impl<R: AnswerReader> Harvest<'_, R> {
    pub(crate) fn models(&self) -> Result<Vec<Model>, OpenApiError> {
        let mapped = &self.contract.mapping().models;
        let entries = self.list(
            &mapped.operation,
            self.contract.models_path(),
            &mapped.items,
            mapped.paging.as_ref(),
        )?;
        entries.iter().map(|entry| self.model(entry)).collect()
    }

    fn model(&self, entry: &Value) -> Result<Model, OpenApiError> {
        let mapped = &self.contract.mapping().models;
        let operation = &mapped.operation;
        let raw = At {
            operation,
            pointer: &mapped.name,
            on: None,
        }
        .text(entry)?;
        let name = identifier(raw, |raw| ModelName::parse(raw), "model", raw)?;
        let table = identifier(raw, |raw| TableName::parse(raw), "table", raw)?;
        let columns = columns(operation, &mapped.columns, entry, raw)?;
        let primary_key = match mapped.primary_key.as_ref() {
            None => Vec::new(),
            Some(pointer) => primary_key(
                &At {
                    operation,
                    pointer,
                    on: Some(raw),
                },
                entry,
            )?,
        };
        let text = mapped
            .description
            .first_text(entry)
            .ok_or_else(|| OpenApiError::MissingDescription { on: shown(raw) })?;
        let description = Description::parse(text).map_err(|cause| OpenApiError::Description { on: shown(raw), cause })?;
        Model::new(name, self.source.clone(), table, columns, description)
            .with_primary_key(primary_key)
            .map_err(|cause| OpenApiError::Inconsistent { cause })
    }

    /// Each model's outgoing links, one listing per model, as many-to-one joins onto the target's
    /// single-column primary key.
    pub(crate) fn relationships(&self, models: &[Model]) -> Result<Vec<Relationship>, OpenApiError> {
        let (Some(mapped), Some(path)) = (&self.contract.mapping().relationships, self.contract.relationships_path()) else {
            return Ok(Vec::new());
        };
        let keys: BTreeMap<&ModelName, &ColumnName> = models
            .iter()
            .filter_map(|model| {
                let mut key = model.primary_key().iter();
                match (key.next(), key.next()) {
                    (Some(only), None) => Some((model.name(), only)),
                    _ => None,
                }
            })
            .collect();
        let mut relationships = Vec::new();
        for model in models {
            let origin = model.name();
            let entries = self.list(&mapped.operation, &path.render(origin), &mapped.items, mapped.paging.as_ref())?;
            let at = |pointer| At {
                operation: &mapped.operation,
                pointer,
                on: Some(origin.as_str()),
            };
            for entry in &entries {
                let Some(foreign_key) = mapped.foreign_key.find(entry) else {
                    continue;
                };
                let foreign_key = foreign_key
                    .as_str()
                    .ok_or_else(|| at(&mapped.foreign_key).unexpected("text"))?;
                let foreign_key = identifier(foreign_key, |raw| ColumnName::parse(raw), "foreign key", origin.as_str())?;
                let raw = at(&mapped.name).text(entry)?;
                let name = identifier(raw, |raw| RelationshipName::parse(raw), "relationship", origin.as_str())?;
                let target = at(&mapped.target).text(entry)?;
                let target = identifier(target, |raw| ModelName::parse(raw), "target model", origin.as_str())?;
                let Some(&key) = keys.get(&target) else {
                    return Err(OpenApiError::TargetKeyUnknown {
                        relationship: name,
                        target,
                    });
                };
                relationships.push(Relationship::new(
                    name,
                    origin.clone(),
                    target,
                    JoinType::ManyToOne,
                    JoinKeys::single(JoinKey::Equal {
                        origin: foreign_key,
                        target: key.clone(),
                    }),
                ));
            }
        }
        Ok(relationships)
    }

    /// Every entry of one listing, page by page. Whole or refused: a repeated cursor, a page that
    /// holds nothing and still names a next one, an over-long cursor and a listing past
    /// [`MAX_ENTRIES`] are each refused rather than followed or cut short.
    fn list(
        &self,
        operation: &OperationId,
        path: &str,
        items: &Pointer,
        paging: Option<&Paging>,
    ) -> Result<Vec<Value>, OpenApiError> {
        let at = |pointer| At {
            operation,
            pointer,
            on: None,
        };
        let size = paging
            .and_then(|paging| paging.size.as_ref())
            .map(|size| (&size.parameter, size.value.to_string()));
        let mut entries = Vec::new();
        let mut cursor: Option<String> = None;
        let mut followed = BTreeSet::new();
        loop {
            let mut query = Vec::with_capacity(2);
            if let Some((parameter, value)) = &size {
                query.push((*parameter, value.as_str()));
            }
            if let (Some(paging), Some(cursor)) = (paging, &cursor) {
                query.push((&paging.cursor, cursor.as_str()));
            }
            let target = Target::new(path, &query);
            let mut answer = self.reader.get(&target, self.started).map_err(|cause| OpenApiError::Read {
                operation: operation.clone(),
                cause: Box::new(cause),
            })?;
            let next = match paging.map(|paging| (paging, paging.next.find(&answer))) {
                None | Some((_, None)) => None,
                Some((_, Some(Value::String(next)))) if next.is_empty() => None,
                Some((_, Some(Value::String(next)))) => Some(next.clone()),
                Some((paging, Some(_))) => return Err(at(&paging.next).unexpected("text")),
            };
            let page = match items.take(&mut answer) {
                Some(Value::Array(page)) => page,
                Some(_) => return Err(at(items).unexpected("a list")),
                None => return Err(at(items).missing()),
            };
            let returned = page.len();
            entries.extend(page);
            if entries.len() > MAX_ENTRIES {
                return Err(OpenApiError::TooManyEntries {
                    operation: operation.clone(),
                    max: MAX_ENTRIES,
                });
            }
            let Some(next) = next else {
                return Ok(entries);
            };
            if returned == 0 {
                return Err(OpenApiError::NoProgress {
                    operation: operation.clone(),
                });
            }
            if next.len() > MAX_CURSOR_BYTES {
                return Err(OpenApiError::CursorTooLong {
                    operation: operation.clone(),
                    limit: MAX_CURSOR_BYTES,
                });
            }
            if !followed.insert(next.clone()) {
                return Err(OpenApiError::RepeatedCursor {
                    operation: operation.clone(),
                });
            }
            cursor = Some(next);
        }
    }
}

fn columns(operation: &OperationId, mapped: &Columns, entry: &Value, on: &str) -> Result<Vec<Column>, OpenApiError> {
    let at = |pointer| At {
        operation,
        pointer,
        on: Some(on),
    };
    let found = mapped.at.find(entry).ok_or_else(|| at(&mapped.at).missing())?;
    let named: Vec<(&str, &Value)> = match &mapped.name {
        None => found
            .as_object()
            .ok_or_else(|| at(&mapped.at).unexpected("a map"))?
            .iter()
            .map(|(name, column)| (name.as_str(), column))
            .collect(),
        Some(name) => found
            .as_array()
            .ok_or_else(|| at(&mapped.at).unexpected("a list"))?
            .iter()
            .map(|column| at(name).text(column).map(|text| (text, column)))
            .collect::<Result<_, _>>()?,
    };
    named
        .into_iter()
        .map(|(raw, column)| {
            let name = identifier(raw, |raw| ColumnName::parse(raw), "column", on)?;
            let data_type = match mapped.data_type.as_ref().map(|pointer| (pointer, pointer.find(column))) {
                None | Some((_, None)) => None,
                Some((pointer, Some(found))) => Some(found.as_str().ok_or_else(|| at(pointer).unexpected("text"))?),
            };
            let description = mapped.description.as_ref().and_then(|fallbacks| fallbacks.first_text(column));
            Column::from_metadata(name, data_type, description, None).map_err(|refusal| {
                let (column, cause) = refusal.into_parts();
                OpenApiError::ColumnDescription {
                    on: shown(on),
                    column,
                    cause,
                }
            })
        })
        .collect()
}

/// A key named by one string or a list of strings.
fn primary_key(at: &At<'_>, entry: &Value) -> Result<Vec<ColumnName>, OpenApiError> {
    let on = at.on.unwrap_or_default();
    match at.pointer.find(entry) {
        None => Ok(Vec::new()),
        Some(Value::String(one)) => Ok(vec![identifier(one, |raw| ColumnName::parse(raw), "primary key", on)?]),
        Some(Value::Array(many)) => many
            .iter()
            .map(|column| {
                let raw = column.as_str().ok_or_else(|| at.unexpected("text or a list of text"))?;
                identifier(raw, |raw| ColumnName::parse(raw), "primary key", on)
            })
            .collect(),
        Some(_) => Err(at.unexpected("text or a list of text")),
    }
}

fn identifier<T>(
    raw: &str,
    parse: impl Fn(&str) -> Result<T, InvalidIdentifier>,
    kind: &'static str,
    on: &str,
) -> Result<T, OpenApiError> {
    parse(raw).map_err(|cause| OpenApiError::Identifier {
        kind,
        value: shown(raw),
        on: shown(on),
        cause,
    })
}
