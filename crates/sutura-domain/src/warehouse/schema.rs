//! The Arrow schema a data system returns for the tables a bundle names, pinned beside the bundle.
//!
//! A catalog's column `data_type` is text for a person and never a cast, so the Arrow type of a
//! column comes from the data system.
//! [`Warehouse::table_schemas`](crate::warehouse::Warehouse::table_schemas) reads it at the load
//! and at each refresh, under the deployment's configured identity, because no caller exists at
//! either. [`PinnedSchemas`](crate::warehouse::schema::PinnedSchemas) holds what was read for the
//! columns the bundle names, and nothing else: a column the source returns and the bundle does not
//! name is not pinned.

use std::collections::BTreeMap;
use std::sync::Arc;

use arrow_schema::{Schema, SchemaRef};
use sha2::{Digest as _, Sha256};

use crate::catalog::Definitions;
use crate::definitions::nibble;
use crate::model::{ColumnName, ModelName, QualifiedTable, SourceName};
use crate::pinned::NotValidated;

/// What one data system said about the schemas of the tables it was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableSchemas {
    /// This adapter has no way to read a table's schema. Its source stays off the pinned set.
    NotAsked,
    /// The schema of each table the data system returned. A table it does not have is absent.
    Read(SchemasByTable),
}

/// One data system's schemas, by the table they describe.
pub type SchemasByTable = BTreeMap<QualifiedTable, SchemaRef>;

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

/// The Arrow schema of each model whose source returned one, cut to the columns the model names.
///
/// Built from the bundle of the same load, and held in one snapshot with it. The key of that
/// snapshot is the bundle's `DefinitionDigest` and this set's [`SchemaDigest`], because a content
/// hash of an unchanged bundle stays the same over a changed source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedSchemas {
    models: BTreeMap<ModelName, SchemaRef>,
    digest: SchemaDigest,
}

impl PinnedSchemas {
    /// Pins what each source returned against the models the bundle declares on it.
    ///
    /// A source absent from `read` answered [`TableSchemas::NotAsked`], and its models are not
    /// pinned.
    ///
    /// # Errors
    ///
    /// [`NotValidated::ColumnNotAtSource`] for the first column a model names that its source did
    /// not return, a table the source does not have included.
    pub fn pin(definitions: &Definitions, read: &BTreeMap<SourceName, SchemasByTable>) -> Result<Self, NotValidated> {
        let mut models = BTreeMap::new();
        for model in definitions.models().values() {
            let Some(tables) = read.get(model.source()) else {
                continue;
            };
            let schema = tables.get(model.table());
            let mut fields = Vec::with_capacity(model.columns().len());
            for column in model.columns() {
                let found = schema.and_then(|schema| schema.fields().find(column.name().as_str()));
                let Some((_, field)) = found else {
                    return Err(NotValidated::ColumnNotAtSource(Box::new(ColumnNotAtSource {
                        model: model.name().clone(),
                        source: model.source().clone(),
                        table: model.table().clone(),
                        column: column.name().clone(),
                    })));
                };
                fields.push(Arc::clone(field));
            }
            models.insert(model.name().clone(), Arc::new(Schema::new(fields)));
        }
        let digest = digest_of(&models);
        Ok(Self { models, digest })
    }

    /// The pinned schema of one model, `None` where its source returned none.
    #[must_use]
    pub fn of(&self, model: &ModelName) -> Option<&SchemaRef> {
        self.models.get(model)
    }

    #[inline]
    #[must_use]
    pub const fn digest(&self) -> &SchemaDigest {
        &self.digest
    }
}

/// The set of a deployment where no source returned a schema.
impl Default for PinnedSchemas {
    fn default() -> Self {
        let models = BTreeMap::new();
        let digest = digest_of(&models);
        Self { models, digest }
    }
}

/// Every part is a netstring (`<length>:<bytes>`), so no two sets hash the same bytes.
fn digest_of(models: &BTreeMap<ModelName, SchemaRef>) -> SchemaDigest {
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
            hash.update([u8::from(field.is_nullable())]);
        }
    }
    SchemaDigest(
        hash.finalize()
            .iter()
            .flat_map(|byte| [nibble(byte >> 4_u8), nibble(byte & 0x0f_u8)])
            .collect(),
    )
}

/// A column the bundle names that its source did not return, which refuses the load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnNotAtSource {
    model: ModelName,
    source: SourceName,
    table: QualifiedTable,
    column: ColumnName,
}

impl ColumnNotAtSource {
    #[inline]
    #[must_use]
    pub const fn model(&self) -> &ModelName {
        &self.model
    }

    #[inline]
    #[must_use]
    pub const fn column(&self) -> &ColumnName {
        &self.column
    }
}

impl core::fmt::Display for ColumnNotAtSource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "model {} names column {}, and {} does not return it for {}",
            self.model, self.column, self.source, self.table
        )
    }
}

/// A data system that could not be asked for its tables' schemas, which refuses the load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaNotRead {
    source: SourceName,
    message: String,
    chain: Vec<String>,
}

impl SchemaNotRead {
    /// The adapter's own failure, walked to text because its type cannot cross into the domain.
    #[must_use]
    pub const fn of(source: SourceName, message: String, chain: Vec<String>) -> Self {
        Self { source, message, chain }
    }

    #[inline]
    #[must_use]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }
}

impl core::fmt::Display for SchemaNotRead {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} did not return its tables' schemas: {}", self.source, self.message)?;
        for cause in &self.chain {
            write!(f, ": {cause}")?;
        }
        Ok(())
    }
}
