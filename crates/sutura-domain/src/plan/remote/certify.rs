//! The receiver's half: a leg parsed against this node's own pinned definitions.
//!
//! Every name the document carries is looked up here, in the definitions the process pinned at
//! boot, and every join is rebuilt from this node's own relationship. Nothing the sender wrote is
//! stored unless this node declares it. The order is the cheapest diagnostic first: the metric's
//! digest, then sizes, then the statement's tables, then the columns, then the required filters.

use super::{LegWire, WireBucket, WireKey, WireTerm};
use crate::catalog::{Definitions, Model};
use crate::measure::RequiredFilter;
use crate::model::{ColumnName, Grain, MetricName, QualifiedTable, RelationshipName, SourceName, TableName};
use crate::pinned::{NotAgreed, PinnedDefinitions};
use crate::plan::{
    AmbiguousTables, IncoherentBindings, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanKey,
    PlanPredicate, PredicateOrigin, ResultLabel, StatementTables,
};
use crate::query::{MAX_DIMENSIONS, MAX_FILTERS};
use crate::warehouse::ParamValue;

/// The part of a leg whose length is bounded before anything walks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegPart {
    Joins,
    Keys,
    Terms,
    Filters,
}

/// Why this node does not run a leg it was sent.
///
/// Typed, and it names catalog identifiers only: no column type, no row and no definition text
/// crosses back to a sender, which is the closed-code rule of ADR 0022.
#[derive(Debug, thiserror::Error)]
pub enum LegNotCertified {
    /// The leg's metric is not defined here as the sender defined it.
    #[error(transparent)]
    Metric(#[from] NotAgreed),
    /// A part of the leg is longer than anything this node's own definitions could make.
    #[error("a leg carries more than {limit} entries of {part:?}")]
    TooMany { part: LegPart, limit: usize },
    /// No model of the source reads that table, or the metric does not read it.
    #[error("no certified model of source {source_name} reads table {table}")]
    TableNotCertified { source_name: SourceName, table: QualifiedTable },
    /// The relationship is not declared here, does not start inside the statement, or leaves the source.
    #[error("relationship {relationship} is not a join of this statement here")]
    RelationshipNotCertified { relationship: RelationshipName },
    /// The column is not one the statement's tables declare here.
    #[error("column {column} of table {table} is not declared here")]
    ColumnNotCertified { table: TableName, column: ColumnName },
    /// The metric is not defined here at that grain.
    #[error("metric {metric} is not defined here at grain {grain:?}")]
    GrainNotDeclared { metric: MetricName, grain: Grain },
    /// The bucket does not truncate the metric's own time column.
    #[error("metric {metric} buckets on another column than {column}")]
    BucketNotTimeColumn { metric: MetricName, column: ColumnName },
    /// A term reads a column the metric's own measure does not.
    #[error("metric {metric} does not measure column {column}")]
    TermNotMetrics { metric: MetricName, column: ColumnName },
    /// A required filter of this node's definition of the metric is not in the leg.
    #[error("the leg omits the required filter of metric {metric} on column {column}")]
    RequiredFilterMissing { metric: MetricName, column: ColumnName },
    #[error(transparent)]
    Tables(#[from] AmbiguousTables),
    #[error(transparent)]
    Bindings(#[from] IncoherentBindings),
}

/// The models one statement reads: the root and the target of each join.
struct Statement<'d> {
    models: Vec<&'d Model>,
    tables: StatementTables,
}

impl Statement<'_> {
    fn certifies(&self, column: &PlanColumn) -> Result<(), LegNotCertified> {
        if self
            .models
            .iter()
            .any(|model| model.table_name() == column.table() && model.has_column(column.column()))
        {
            Ok(())
        } else {
            Err(LegNotCertified::ColumnNotCertified {
                table: column.table().clone(),
                column: column.column().clone(),
            })
        }
    }
}

const fn within(part: LegPart, len: usize, limit: usize) -> Result<(), LegNotCertified> {
    if len > limit {
        return Err(LegNotCertified::TooMany { part, limit });
    }
    Ok(())
}

/// The dimension keys, the link, and at most two distinct keys: what a splitter's leg can group by.
const MAX_KEYS: usize = MAX_DIMENSIONS + 3;

/// One term or a ratio of two, and `Avg` expands each into two.
const MAX_TERMS: usize = 4;

fn statement<'d>(
    definitions: &'d Definitions,
    source: &SourceName,
    table: &QualifiedTable,
    relationships: &[RelationshipName],
    reads: impl Fn(&Model) -> bool,
) -> Result<Statement<'d>, LegNotCertified> {
    within(LegPart::Joins, relationships.len(), definitions.relationships().len())?;
    let root = definitions
        .models()
        .values()
        .find(|model| model.table() == table && model.source() == source && reads(model))
        .ok_or_else(|| LegNotCertified::TableNotCertified {
            source_name: source.clone(),
            table: table.clone(),
        })?;
    let not_a_join = |relationship: &RelationshipName| LegNotCertified::RelationshipNotCertified {
        relationship: relationship.clone(),
    };
    let declared = relationships
        .iter()
        .map(|name| {
            let relationship = definitions.relationship(name).ok_or_else(|| not_a_join(name))?;
            let target = definitions
                .model(relationship.target_model())
                .filter(|target| target.source() == source)
                .ok_or_else(|| not_a_join(name))?;
            Ok((relationship, target))
        })
        .collect::<Result<Vec<_>, LegNotCertified>>()?;
    let mut models = vec![root];
    models.extend(declared.iter().map(|&(_, target)| target));
    let joins = declared
        .iter()
        .map(|&(relationship, target)| {
            let origin = models
                .iter()
                .find(|model| model.name() == relationship.origin_model())
                .ok_or_else(|| not_a_join(relationship.name()))?;
            Ok(PlanJoin::hop(relationship, origin.table_name(), target))
        })
        .collect::<Result<Vec<_>, LegNotCertified>>()?;
    Ok(Statement {
        tables: StatementTables::parse(table.clone(), joins)?,
        models,
    })
}

fn keys(wire: Vec<WireKey>, statement: &Statement<'_>) -> Result<Vec<PlanKey>, LegNotCertified> {
    within(LegPart::Keys, wire.len(), MAX_KEYS)?;
    wire.into_iter()
        .map(|key| {
            statement.certifies(&key.column)?;
            Ok(PlanKey::new(key.label.into_label(), key.column))
        })
        .collect()
}

fn filters(wire: &[PlanFilter], statement: &Statement<'_>, required: usize) -> Result<(), LegNotCertified> {
    within(
        LegPart::Filters,
        wire.len(),
        MAX_FILTERS.saturating_add(required).saturating_add(2),
    )?;
    wire.iter()
        .try_for_each(|filter| statement.certifies(filter.predicate().column()))
}

/// Whether `filters` holds this node's own required filter, bound to the value this node declares.
fn requires(required: &RequiredFilter, own_table: &TableName, filters: &[PlanFilter], params: &[ParamValue]) -> bool {
    let column = PlanColumn::new(own_table.clone(), required.column().clone());
    let bound = |param: usize, value: &crate::catalog::DimensionValue| {
        params.get(param) == Some(&ParamValue::Text(String::from(value.as_str())))
    };
    filters
        .iter()
        .filter(|filter| filter.origin() == PredicateOrigin::Definition)
        .any(|filter| match (required, filter.predicate()) {
            (RequiredFilter::Equals { value, .. }, PlanPredicate::Equals { column: at, param })
            | (RequiredFilter::NotEquals { value, .. }, PlanPredicate::NotEquals { column: at, param }) => {
                *at == column && bound(*param, value)
            }
            (RequiredFilter::IsTrue { .. }, PlanPredicate::IsTrue { column: at })
            | (RequiredFilter::IsNotNull { .. }, PlanPredicate::IsNotNull { column: at }) => *at == column,
            _ => false,
        })
}

/// The leg this node runs, or the reason it does not: [`LegWire::parse`]'s body.
pub(super) fn certify(wire: LegWire, pinned: &PinnedDefinitions, params: Vec<ParamValue>) -> Result<LegPlan, LegNotCertified> {
    match wire {
        LegWire::Fact {
            source,
            metric,
            table,
            joins,
            bucket,
            keys: wire_keys,
            terms,
            filters: wire_filters,
            range,
        } => {
            pinned.agrees_on(&metric)?;
            let definitions = pinned.definitions();
            let name = metric.metric();
            let declared = definitions
                .metric(name)
                .ok_or_else(|| NotAgreed::MetricUnknown { metric: name.clone() })?;
            let measure = declared.measure();
            let reads = |model: &Model| {
                model.name() == declared.model()
                    || measure.is_some_and(|m| m.models().into_iter().flatten().any(|second| second == model.name()))
            };
            let statement = statement(definitions, &source, &table, &joins, reads)?;
            let WireBucket { grain, column } = bucket;
            let own = definitions.model(declared.model()).is_some_and(|m| m.table() == &table);
            let required = if own { declared.required_filters() } else { &[] };
            if !declared.supports_grain(grain) {
                return Err(LegNotCertified::GrainNotDeclared {
                    metric: name.clone(),
                    grain,
                });
            }
            if column.column() != declared.time_column() {
                return Err(LegNotCertified::BucketNotTimeColumn {
                    metric: name.clone(),
                    column: column.column().clone(),
                });
            }
            statement.certifies(&column)?;
            within(LegPart::Terms, terms.len(), MAX_TERMS)?;
            let terms = terms
                .into_iter()
                .map(|WireTerm { label, term }| {
                    let measured = measure.is_some_and(|m| m.columns().contains(&term.column().column()));
                    if !measured {
                        return Err(LegNotCertified::TermNotMetrics {
                            metric: name.clone(),
                            column: term.column().column().clone(),
                        });
                    }
                    statement.certifies(term.column())?;
                    Ok(LegTerm::new(term, label.into_label()))
                })
                .collect::<Result<Vec<_>, LegNotCertified>>()?;
            let keys = keys(wire_keys, &statement)?;
            filters(&wire_filters, &statement, required.len())?;
            let own_table = statement.models.first().map(|m| m.table_name().clone());
            if let Some(own_table) = own_table.filter(|_| own)
                && let Some(missing) = required
                    .iter()
                    .find(|filter| !requires(filter, &own_table, &wire_filters, &params))
            {
                return Err(LegNotCertified::RequiredFilterMissing {
                    metric: name.clone(),
                    column: missing.column().clone(),
                });
            }
            Ok(LegPlan::Fact {
                source,
                metric: name.clone(),
                tables: statement.tables,
                bucket: PlanBucket::new(ResultLabel::bucket(), grain, column),
                keys,
                terms,
                bindings: PlanBindings::parse(wire_filters, params)?,
                range,
            })
        }
        LegWire::Lookup {
            source,
            table,
            joins,
            keys: wire_keys,
            filters: wire_filters,
        } => {
            let statement = statement(pinned.definitions(), &source, &table, &joins, |_| true)?;
            let keys = keys(wire_keys, &statement)?;
            filters(&wire_filters, &statement, 0)?;
            Ok(LegPlan::Lookup {
                source,
                table: statement.tables,
                keys,
                bindings: PlanBindings::parse(wire_filters, params)?,
            })
        }
    }
}
