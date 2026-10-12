//! The Arrow schema of the tables a bundle federates, pinned beside the bundle.
//!
//! A catalog's column `data_type` is text, and [`TypeDialect`] reads it as an Arrow type in the
//! spelling of the data system the model's source is. [`PinnedSchemas`](crate::warehouse::schema::PinnedSchemas) holds what the catalog
//! declared for each federated model, and nothing else: no data source is read for it, at the load
//! or at a refresh. A column whose type the dialect does not list, or that the catalog gives no
//! type, is a `Utf8` column and is recorded as having fallen back.

mod types;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, SchemaRef};
use sha2::{Digest as _, Sha256};

use crate::catalog::Definitions;
use crate::definitions::nibble;
use crate::model::{ColumnName, ModelName, SourceName};
use crate::pinned::NotValidated;

pub use types::TypeDialect;

/// The digest of a [`PinnedSchemas`]: lower-case hex SHA-256 over each pinned model's name and,
/// in order, each column's name, Arrow type and nullability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct SchemaDigest(String);

impl SchemaDigest {
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for SchemaDigest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The Arrow schema of each federated model, from the columns its catalog declared.
///
/// Built from the bundle of the same load, and held in one snapshot with it. The key of that
/// snapshot is the bundle's `DefinitionDigest` and this set's [`SchemaDigest`]. The digest covers
/// each column's fallback as well as its type, so a column that stops falling back moves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedSchemas {
    models: BTreeMap<ModelName, SchemaRef>,
    fell_back: BTreeMap<ModelName, BTreeSet<ColumnName>>,
    digest: SchemaDigest,
}

impl PinnedSchemas {
    /// Pins the catalog's columns of each federated model: one whose relationship joins it to a
    /// model on another source. A model whose source has no entry in `dialects` is not pinned.
    ///
    /// # Errors
    ///
    /// [`NotValidated::FederatedModelUntyped`] for the first model of a pinned source that declares
    /// no column type at all. That is a missing declaration, and not a type to fall back from.
    pub fn pin(definitions: &Definitions, dialects: &BTreeMap<SourceName, TypeDialect>) -> Result<Self, NotValidated> {
        let federated: BTreeSet<&ModelName> = definitions
            .relationships()
            .values()
            .filter_map(|relationship| {
                let origin = definitions.model(relationship.origin_model())?;
                let target = definitions.model(relationship.target_model())?;
                (origin.source() != target.source()).then_some([origin.name(), target.name()])
            })
            .flatten()
            .collect();
        let mut models = BTreeMap::new();
        let mut fell_back = BTreeMap::new();
        for model in definitions.models().values().filter(|model| federated.contains(model.name())) {
            let Some(dialect) = dialects.get(model.source()) else {
                continue;
            };
            if model.columns().all(|column| column.data_type().is_none()) {
                return Err(NotValidated::FederatedModelUntyped {
                    model: model.name().clone(),
                });
            }
            let mut fields = Vec::with_capacity(model.columns().len());
            for column in model.columns() {
                let mapped = column.data_type().and_then(|declared| dialect.arrow(declared));
                if mapped.is_none() {
                    fell_back
                        .entry(model.name().clone())
                        .or_insert_with(BTreeSet::new)
                        .insert(column.name().clone());
                }
                fields.push(Field::new(
                    column.name().as_str(),
                    mapped.unwrap_or(DataType::Utf8),
                    column.nullable().unwrap_or(true),
                ));
            }
            models.insert(model.name().clone(), Arc::new(Schema::new(fields)));
        }
        let digest = digest_of(&models, &fell_back);
        Ok(Self {
            models,
            fell_back,
            digest,
        })
    }

    /// The pinned schema of one model, `None` where it is not federated or its source has no dialect.
    #[must_use]
    pub fn of(&self, model: &ModelName) -> Option<&SchemaRef> {
        self.models.get(model)
    }

    /// The columns of one model that are `Utf8` because their type was missing or not listed.
    pub fn fell_back(&self, model: &ModelName) -> impl Iterator<Item = &ColumnName> {
        self.fell_back.get(model).into_iter().flatten()
    }

    #[inline]
    #[must_use]
    pub const fn digest(&self) -> &SchemaDigest {
        &self.digest
    }
}

/// The set of a deployment with no federated model on a source that declares a dialect.
impl Default for PinnedSchemas {
    fn default() -> Self {
        let (models, fell_back) = (BTreeMap::new(), BTreeMap::new());
        let digest = digest_of(&models, &fell_back);
        Self {
            models,
            fell_back,
            digest,
        }
    }
}

/// Every part is a netstring (`<length>:<bytes>`), so no two sets hash the same bytes.
fn digest_of(models: &BTreeMap<ModelName, SchemaRef>, fell_back: &BTreeMap<ModelName, BTreeSet<ColumnName>>) -> SchemaDigest {
    fn part(hash: &mut Sha256, text: &str) {
        hash.update(text.len().to_string().as_bytes());
        hash.update(b":");
        hash.update(text.as_bytes());
    }
    let mut hash = Sha256::new();
    for (model, schema) in models {
        part(&mut hash, model.as_str());
        part(&mut hash, &schema.fields().len().to_string());
        for field in schema.fields() {
            part(&mut hash, field.name());
            part(&mut hash, &field.data_type().to_string());
            hash.update([
                u8::from(field.is_nullable()),
                u8::from(
                    fell_back
                        .get(model)
                        .is_some_and(|columns| columns.iter().any(|column| column.as_str() == field.name())),
                ),
            ]);
        }
    }
    SchemaDigest(
        hash.finalize()
            .iter()
            .flat_map(|byte| [nibble(byte >> 4_u8), nibble(byte & 0x0f_u8)])
            .collect(),
    )
}

#[cfg(test)]
mod tests;
