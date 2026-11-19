//! The Arrow schema of a bundle's tables, pinned beside the bundle.
//!
//! A catalog's column `data_type` is text, and [`TypeDialect`] reads it as an Arrow type in the
//! spelling of the data system the model's source is. [`PinnedSchemas`](crate::warehouse::schema::PinnedSchemas) holds what the catalog
//! declared for each model whose source has a dialect, and nothing else: no data source is read for
//! it, at the load or at a refresh. A column whose type the dialect does not list, or that the catalog gives no
//! type, is a `Utf8` column and is recorded as having fallen back.

mod types;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, SchemaRef};
use sha2::{Digest as _, Sha256};

use crate::catalog::{COLLATION_METADATA_KEY, Collation, Definitions, Metric, Model, Relationship};
use crate::definitions::nibble;
use crate::model::{ColumnName, ModelName, SourceName};
use crate::pinned::NotValidated;

pub use types::{TypeDialect, UnboundedNumber};

/// The digest of a [`PinnedSchemas`].
///
/// Lower-case hex SHA-256 over each pinned model's name and, in order, each column's name, Arrow
/// type, nullability, local-operator marker and, for a text column with one on record, its
/// collation.
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

/// The Arrow schema of each model whose source has a dialect, from the columns its catalog declared.
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
    /// Pins the catalog's columns of each model whose source has an entry in `dialects`.
    ///
    /// # Errors
    ///
    /// [`NotValidated::FederatedModelUntyped`] for the first federated model - one a relationship
    /// joins to a model on another source - that declares no column type at all. That is a missing
    /// declaration, and not a type to fall back from. A model no relationship federates is pinned
    /// with every column fallen back, and is no table of the SQL tool.
    pub fn pin(definitions: &Definitions, dialects: &BTreeMap<SourceName, TypeDialect>) -> Result<Self, NotValidated> {
        Self::pin_with_unbounded_numbers(definitions, dialects, &BTreeSet::new())
    }

    /// Pins each source's explicitly selected exact representation for unbounded Oracle NUMBER.
    pub fn pin_with_unbounded_numbers(
        definitions: &Definitions,
        dialects: &BTreeMap<SourceName, TypeDialect>,
        numbers: &BTreeSet<SourceName>,
    ) -> Result<Self, NotValidated> {
        let mut policies = BTreeMap::new();
        for model in definitions.models().values() {
            for column in model.columns() {
                let key = (model.source(), model.table(), column.name());
                if policies
                    .insert(key, column.text_policy())
                    .is_some_and(|previous| previous != column.text_policy())
                    || (!column.text_policy().is_empty() && !dialects.contains_key(model.source()))
                {
                    return Err(NotValidated::TextPolicy {
                        model: model.name().clone(),
                    });
                }
            }
        }
        check_metric_text_policies(definitions)?;
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
        for model in definitions.models().values() {
            let Some(dialect) = dialects.get(model.source()) else {
                continue;
            };
            if federated.contains(model.name()) && model.columns().all(|column| column.data_type().is_none()) {
                return Err(NotValidated::FederatedModelUntyped {
                    model: model.name().clone(),
                });
            }
            let mut fields = Vec::with_capacity(model.columns().len());
            for column in model.columns() {
                let mapped = column.data_type().and_then(|declared| {
                    dialect
                        .arrow_with_unbounded_number(declared, numbers.contains(model.source()).then_some(UnboundedNumber::Utf8))
                });
                if mapped.is_none() {
                    fell_back
                        .entry(model.name().clone())
                        .or_insert_with(BTreeSet::new)
                        .insert(column.name().clone());
                }
                let mapped_text = mapped.as_ref() == Some(&DataType::Utf8);
                let mut field = Field::new(
                    column.name().as_str(),
                    mapped.unwrap_or(DataType::Utf8),
                    column.nullable().unwrap_or(true),
                );
                if *dialect == TypeDialect::Oracle
                    && numbers.contains(model.source())
                    && column
                        .data_type()
                        .is_some_and(|declared| declared.as_str().eq_ignore_ascii_case("NUMBER"))
                {
                    field = field.with_metadata(HashMap::from([(
                        String::from("sutura:unbounded_number"),
                        String::from("utf8"),
                    )]));
                }
                if !column.text_policy().is_empty() {
                    if !mapped_text || field.metadata().contains_key("sutura:unbounded_number") {
                        return Err(NotValidated::TextPolicy {
                            model: model.name().clone(),
                        });
                    }
                    let encoded = serde_json::to_string(column.text_policy()).map_err(|_cause| NotValidated::TextPolicy {
                        model: model.name().clone(),
                    })?;
                    field.metadata_mut().insert(String::from("sutura:text_policy"), encoded);
                    if *dialect == TypeDialect::Oracle
                        && column.data_type().is_some_and(|declared| {
                            declared.as_str().eq_ignore_ascii_case("clob") || declared.as_str().eq_ignore_ascii_case("nclob")
                        })
                    {
                        field.metadata_mut().insert(
                            String::from("sutura:oracle_lob"),
                            column
                                .data_type()
                                .map_or_else(String::new, |declared| declared.as_str().to_ascii_lowercase()),
                        );
                    }
                }
                if field.data_type() == &DataType::Utf8 {
                    dialect
                        .engine_collation()
                        .unwrap_or_else(|| column.collation())
                        .pin_onto(&mut field);
                }
                fields.push(field);
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

    /// The pinned schema of one model, `None` where its source has no dialect.
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

/// The set of a deployment with no model on a source that declares a dialect.
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

/// Certified execution does not transform protected text. Refuse every column a permitted
/// question (including its anchor and optional dimensions) can read, before validation executes SQL.
fn check_metric_text_policies(definitions: &Definitions) -> Result<(), NotValidated> {
    for metric in definitions.metrics().values() {
        let mut reads = BTreeSet::new();
        reads.extend(
            metric
                .required_filters()
                .iter()
                .map(|filter| (metric.model(), filter.column())),
        );
        let mut facts = BTreeSet::from([metric.model()]);
        if let Some(measure) = metric.measure() {
            for term in measure.terms() {
                let model = term.model().unwrap_or_else(|| metric.model());
                facts.insert(model);
                reads.insert((model, term.column()));
            }
        }
        let mut targets = BTreeSet::new();
        for dimension in metric.dimensions().values() {
            let mut model = metric.model();
            for relationship in dimension
                .via()
                .into_iter()
                .flatten()
                .filter_map(|name| definitions.relationship(name))
            {
                relationship_reads(relationship, &mut reads);
                model = relationship.target_model();
                targets.insert(model);
            }
            reads.insert((model, dimension.column()));
        }
        if let Some(calendars) = metric.shared_calendars().filter(|_| facts.len() > 1) {
            for calendar in calendars.iter() {
                targets.insert(calendar);
                reads.insert((calendar, metric.time_column()));
            }
            // The second fact uses its own links into the shared dimensions and calendars.
            for relationship in definitions.relationships().values().filter(|relationship| {
                facts.contains(relationship.origin_model()) && targets.contains(relationship.target_model())
            }) {
                relationship_reads(relationship, &mut reads);
            }
        } else {
            reads.insert((metric.model(), metric.time_column()));
        }
        for (name, column) in reads {
            if let Some(model) = definitions.model(name) {
                refuse_protected_read(definitions, metric, model, column)?;
            }
        }
        // An authored expression has no closed column vocabulary. It cannot establish disjointness.
        if metric.measure().is_none()
            && let Some(model) = definitions.model(metric.model())
            && let Some((protected, column)) = definitions
                .models()
                .values()
                .filter(|other| other.source() == model.source())
                .find_map(|protected| {
                    protected
                        .columns()
                        .find(|column| !column.text_policy().is_empty())
                        .map(|column| (protected, column))
                })
        {
            return Err(NotValidated::MetricReadsProtectedText {
                metric: metric.name().clone(),
                model: protected.name().clone(),
                column: column.name().clone(),
            });
        }
    }
    Ok(())
}

fn relationship_reads<'a>(relationship: &'a Relationship, reads: &mut BTreeSet<(&'a ModelName, &'a ColumnName)>) {
    for key in relationship.keys().iter() {
        reads.insert((relationship.origin_model(), key.origin()));
        reads.insert((relationship.target_model(), key.target()));
    }
}

fn refuse_protected_read(
    definitions: &Definitions,
    metric: &Metric,
    model: &Model,
    column: &ColumnName,
) -> Result<(), NotValidated> {
    for protected in definitions.models().values().filter(|other| physical_alias(model, other)) {
        if let Some(protected_column) = protected
            .columns()
            .find(|other| other.name().as_str().eq_ignore_ascii_case(column.as_str()) && !other.text_policy().is_empty())
        {
            return Err(NotValidated::MetricReadsProtectedText {
                metric: metric.name().clone(),
                model: protected.name().clone(),
                column: protected_column.name().clone(),
            });
        }
    }
    Ok(())
}

/// An unqualified name may resolve to the qualified table on the same connection. Fold case
/// conservatively: some engines fold even quoted identifiers, so exact spelling is not a barrier.
fn physical_alias(left: &Model, right: &Model) -> bool {
    left.source() == right.source()
        && left.table_name().as_str().eq_ignore_ascii_case(right.table_name().as_str())
        && match (left.table().qualifier(), right.table().qualifier()) {
            (Some(left), Some(right)) => {
                left.dataset().as_str().eq_ignore_ascii_case(right.dataset().as_str())
                    && match (left.project(), right.project()) {
                        (Some(left), Some(right)) => left.as_str().eq_ignore_ascii_case(right.as_str()),
                        _ => true,
                    }
            }
            _ => true,
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
            if let Some(lob) = field.metadata().get("sutura:oracle_lob") {
                part(&mut hash, "sutura:oracle_lob");
                part(&mut hash, lob);
            }
            if let Some(policy) = field.metadata().get("sutura:text_policy") {
                part(&mut hash, "sutura:text_policy");
                part(&mut hash, policy);
            }
            if let Some(collation) = Collation::of_field(field).encoded() {
                part(&mut hash, COLLATION_METADATA_KEY);
                part(&mut hash, &collation);
            }
            hash.update([
                u8::from(field.is_nullable()),
                u8::from(
                    field
                        .metadata()
                        .get("sutura:unbounded_number")
                        .is_some_and(|value| value == "utf8"),
                ),
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

#[cfg(test)]
mod text_policy_tests {
    use super::{PinnedSchemas, TypeDialect};
    use crate::catalog::{Column, Definitions, Description, Model, TextPolicy};
    use crate::model::{ColumnName, ModelName, SourceName, TableName};
    use crate::pinned::NotValidated;
    use std::collections::BTreeMap;

    #[test]
    fn a_text_policy_refuses_nontext_columns_and_unprotected_physical_aliases() {
        let source = SourceName::parse("data").expect("a source");
        let policy: TextPolicy = serde_json::from_value(serde_json::json!({"max_text_bytes": 128})).expect("a bounded policy");
        let model = |name: &str, declared: &str, policy: TextPolicy| {
            Model::new(
                ModelName::parse(name).expect("a model"),
                source.clone(),
                TableName::parse("events").expect("a table"),
                [
                    Column::from_metadata(ColumnName::parse("body").expect("a column"), Some(declared), None, None)
                        .expect("a declared column")
                        .with_text_policy(policy),
                ],
                Description::default(),
            )
        };
        let pin = |models| {
            let definitions = Definitions::assemble(models, vec![], vec![]).expect("physical models");
            PinnedSchemas::pin(&definitions, &BTreeMap::from([(source.clone(), TypeDialect::Postgres)]))
        };
        for models in [
            vec![model("events", "BIGINT", policy.clone())],
            vec![model("events", "GEOGRAPHY", policy.clone())],
            vec![
                model("events", "TEXT", policy.clone()),
                model("alias", "TEXT", TextPolicy::empty()),
            ],
        ] {
            assert!(matches!(pin(models), Err(NotValidated::TextPolicy { .. })));
        }
        pin(vec![model("events", "TEXT", policy)]).expect("a mapped text column accepts the policy");
    }
}
