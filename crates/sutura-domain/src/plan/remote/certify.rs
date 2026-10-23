//! The receiver's half: a leg parsed against this node's own pinned definitions.
//!
//! Every name the document carries is looked up here, in the definitions the process pinned at
//! boot, and every join is rebuilt from this node's own relationship. A remote leg may only name
//! what its metric declares ([`surface`]): each join is a hop of one of the metric's dimension
//! chains or the planner's join to its calendar, each dimension key or filter is a declared
//! dimension reached through its own chain, each filter value is in the dimension's allowlist, and
//! each link key is one of its metric's crossing hops. A lookup leg names its metric, whose digest
//! is checked as a fact leg's is, and is rooted where one of that metric's chains crosses into
//! this source. A fact leg is checked in this order: the metric's
//! digest; the root table and the joins, each from the root or an earlier join's target; the grain
//! and the bucket column; the range's length; the terms; the joins against the metric's surface
//! and the calendar join; the keys and filters, each bounded before it is walked; the range's
//! bounds; the required filters; every other filter.

mod surface;

use std::collections::BTreeMap;

use self::surface::Surface;
use super::{LegWire, WireBucket, WireKey, WireLabel, WireTerm};
use crate::calendar::TimeRange;
use crate::catalog::{Definitions, DimensionValue, Metric, Model, Relationship};
use crate::measure::RequiredFilter;
use crate::model::{ColumnName, DimensionName, Grain, MetricName, QualifiedTable, RelationshipName, SourceName, TableName};
use crate::pinned::{MetricDigest, NotAgreed, PinnedDefinitions};
use crate::plan::{
    AmbiguousTables, IncoherentBindings, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanFilter,
    PlanJoin, PlanKey, PlanPredicate, PredicateOrigin, ResultLabel, StatementTables, range_filters,
};
use crate::query::{MAX_DIMENSIONS, MAX_FILTERS, MAX_LOOKUP_LEGS, MAX_RANGE_DAYS};
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
    /// No model of the source reads that table, the metric does not read it, or, for a lookup leg,
    /// none of the metric's chains crosses into it.
    #[error("no certified model of source {source_name} reads table {table}")]
    TableNotCertified { source_name: SourceName, table: QualifiedTable },
    /// The relationship is not declared here, does not start inside the statement, leaves the
    /// source, or is not a join the leg's metric declares.
    #[error("relationship {relationship} is not a join of this statement here")]
    RelationshipNotCertified { relationship: RelationshipName },
    /// The column is not one the statement's tables declare here.
    #[error("column {column} of table {table} is not declared here")]
    ColumnNotCertified { table: TableName, column: ColumnName },
    /// The metric is not defined here at that grain.
    #[error("metric {metric} is not defined here at grain {grain:?}")]
    GrainNotDeclared { metric: MetricName, grain: Grain },
    /// The bucket is not the column this node buckets the metric on for a leg on that source: the
    /// metric's time column on its own table, or for a ratio across two models on the shared
    /// calendar listed on the metric's own source, for a leg on that source only.
    #[error("metric {metric} does not bucket on column {column} of table {table}")]
    BucketNotTimeColumn {
        metric: MetricName,
        table: TableName,
        column: ColumnName,
    },
    /// A term reads a column the measure does not read on the leg's root table.
    #[error("metric {metric} does not measure column {column} of table {table}")]
    TermNotMetrics {
        metric: MetricName,
        table: TableName,
        column: ColumnName,
    },
    /// The leg joins its calendar other than through the relationship this node joins it by.
    #[error("the leg of metric {metric} joins its calendar through another relationship")]
    CalendarNotJoined { metric: MetricName },
    /// A key names a dimension this node does not declare on that column, through the leg's joins.
    #[error("dimension {dimension} is not declared on column {column} of table {table} through this leg's joins")]
    DimensionNotDeclared {
        dimension: DimensionName,
        table: TableName,
        column: ColumnName,
    },
    /// A link key is not a key column of one of the metric's crossing hops, at the end the leg holds.
    #[error("column {column} of table {table} is not a key of a crossing this leg's metric declares")]
    LinkNotDeclared { table: TableName, column: ColumnName },
    /// One link label does not carry every key part of one of the metric's crossing hops, each once.
    #[error("link {leg} does not carry every key part of one crossing this leg's metric declares")]
    LinkIncomplete { leg: usize },
    /// A lookup leg carries a key that only a fact leg's measure can name.
    #[error("a lookup leg does not carry a measure key on column {column} of table {table}")]
    KeyNotDeclared { table: TableName, column: ColumnName },
    /// A filter is neither a required filter of the metric nor a filter on a filterable dimension.
    #[error("the filter on column {column} of table {table} is not one this node declares")]
    FilterNotDeclared { table: TableName, column: ColumnName },
    /// A filter binds a value the dimension's allowlist does not hold.
    #[error("the filter on column {column} of table {table} binds a value this node does not allow")]
    ValueNotAllowed { table: TableName, column: ColumnName },
    /// The leg spans more days than this node plans for one question.
    #[error("a leg spans {days} days, more than {limit}")]
    RangeTooLong { days: i32, limit: i32 },
    /// The leg's first two predicates are not its range's bounds on the bucket column, bound to the
    /// range's own dates.
    #[error("the leg of metric {metric} is not bounded by its range on its bucket column")]
    RangeNotBound { metric: MetricName },
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
    root: &'d Model,
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

/// The dimension keys, one link for each key part of each lookup leg's crossing, and at most two
/// distinct keys: what a splitter's leg can group by. A crossing has at most as many key parts as
/// the widest relationship `definitions` declares.
pub(super) fn max_keys(definitions: &Definitions) -> usize {
    let parts = definitions
        .relationships()
        .values()
        .map(|relationship| relationship.keys().len())
        .max()
        .unwrap_or(1);
    MAX_DIMENSIONS
        .saturating_add(MAX_LOOKUP_LEGS.saturating_mul(parts))
        .saturating_add(2)
}

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
        .enumerate()
        .map(|(index, &(relationship, target))| {
            let origin = models
                .iter()
                .take(index.saturating_add(1))
                .find(|model| model.name() == relationship.origin_model())
                .ok_or_else(|| not_a_join(relationship.name()))?;
            Ok(PlanJoin::hop(relationship, origin.table_name(), target))
        })
        .collect::<Result<Vec<_>, LegNotCertified>>()?;
    Ok(Statement {
        tables: StatementTables::parse(table.clone(), joins)?,
        root,
        models,
    })
}

/// What a fact leg's measure lets a key name: the metric and the columns it measures on the root.
struct Measured<'m> {
    metric: &'m MetricName,
    columns: &'m [PlanColumn],
}

/// What a leg is read against: this node's definitions, its metric's surface and the joins the
/// leg names.
struct Scope<'s> {
    definitions: &'s Definitions,
    surface: &'s Surface<'s>,
    joins: &'s [RelationshipName],
}

/// The parts one link label carries, each with its column, in the order the leg lists them.
type Carried = Vec<(usize, PlanColumn)>;

/// The keys, each a declared column of the statement under a label the leg may carry: a dimension
/// reached through its own chain, a link on one of the metric's crossing hops, or, on a fact leg,
/// a column the measure reads on the root table.
fn keys(
    wire: Vec<WireKey>,
    statement: &Statement<'_>,
    scope: &Scope<'_>,
    measured: Option<&Measured<'_>>,
) -> Result<Vec<PlanKey>, LegNotCertified> {
    within(LegPart::Keys, wire.len(), max_keys(scope.definitions))?;
    let mut links: BTreeMap<usize, Carried> = BTreeMap::new();
    let keys = wire
        .into_iter()
        .map(|key| {
            let column = &key.column;
            statement.certifies(column)?;
            let (table, name) = (column.table().clone(), column.column().clone());
            match key.label {
                WireLabel::Dimension(ref dimension) if !scope.surface.reaches(dimension, column, scope.joins) => {
                    return Err(LegNotCertified::DimensionNotDeclared {
                        dimension: dimension.clone(),
                        table,
                        column: name,
                    });
                }
                WireLabel::Internal(InternalLabel::Link { part, .. })
                    if !scope.surface.links(scope.definitions, statement, part, column) =>
                {
                    return Err(LegNotCertified::LinkNotDeclared { table, column: name });
                }
                WireLabel::Internal(InternalLabel::Leaf(_)) => match measured {
                    None => return Err(LegNotCertified::KeyNotDeclared { table, column: name }),
                    Some(&Measured { metric, columns })
                        if !columns.contains(column) || statement.root.table_name() != column.table() =>
                    {
                        return Err(LegNotCertified::TermNotMetrics {
                            metric: metric.clone(),
                            table,
                            column: name,
                        });
                    }
                    Some(_) => {}
                },
                WireLabel::Internal(InternalLabel::Link { leg, part }) => {
                    links.entry(leg).or_default().push((part, column.clone()));
                }
                WireLabel::Dimension(_) => {}
            }
            Ok(PlanKey::new(key.label.into_label(), key.column))
        })
        .collect::<Result<Vec<_>, LegNotCertified>>()?;
    for (&leg, carried) in &links {
        let carried: Vec<(usize, &PlanColumn)> = carried.iter().map(|(part, column)| (*part, column)).collect();
        if !scope.surface.completes(scope.definitions, statement, &carried) {
            return Err(LegNotCertified::LinkIncomplete { leg });
        }
    }
    Ok(keys)
}

/// Every join is one `surface` declares.
fn declared_joins(joins: &[RelationshipName], surface: &Surface<'_>) -> Result<(), LegNotCertified> {
    joins.iter().find(|join| !surface.joins(join)).map_or(Ok(()), |relationship| {
        Err(LegNotCertified::RelationshipNotCertified {
            relationship: relationship.clone(),
        })
    })
}

/// Every filter is a required filter (`required` says which) or a requested filter on a filterable
/// dimension reached through the leg's joins, every value it binds in that dimension's allowlist.
fn declared_filters(
    wire: &[PlanFilter],
    params: &[ParamValue],
    (surface, joins): (&Surface<'_>, &[RelationshipName]),
    required: impl Fn(&PlanFilter) -> bool,
) -> Result<(), LegNotCertified> {
    wire.iter().try_for_each(|filter| {
        let column = filter.predicate().column();
        let undeclared = || LegNotCertified::FilterNotDeclared {
            table: column.table().clone(),
            column: column.column().clone(),
        };
        if filter.origin() == PredicateOrigin::Definition {
            return if required(filter) { Ok(()) } else { Err(undeclared()) };
        }
        let bound: Vec<usize> = match *filter.predicate() {
            PlanPredicate::Equals { param, .. } => vec![param],
            PlanPredicate::In { ref params, .. } | PlanPredicate::NotIn { ref params, .. } => params.iter().copied().collect(),
            PlanPredicate::AtOrAfter { .. }
            | PlanPredicate::Before { .. }
            | PlanPredicate::NotEquals { .. }
            | PlanPredicate::IsTrue { .. }
            | PlanPredicate::IsNotNull { .. } => return Err(undeclared()),
        };
        let mut dimensions = surface.filterable(column, joins).peekable();
        if dimensions.peek().is_none() {
            return Err(undeclared());
        }
        let allowed = |param: &usize, dimension: &crate::catalog::Dimension| {
            matches!(params.get(*param), Some(ParamValue::Text(text))
                if DimensionValue::parse(text).is_ok_and(|value| dimension.permits(&value)))
        };
        if dimensions.any(|dimension| bound.iter().all(|param| allowed(param, dimension))) {
            Ok(())
        } else {
            Err(LegNotCertified::ValueNotAllowed {
                table: column.table().clone(),
                column: column.column().clone(),
            })
        }
    })
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

/// The column this node buckets `metric` on for a leg on `source`, as the planner does: its time
/// column on its own table for a metric that reads one model, and for a ratio across two models the
/// time column on the shared calendar listed on the metric's own source, for a leg on that source
/// only. `None` when the planner would bucket no leg on `source`.
fn time_column(definitions: &Definitions, metric: &Metric, source: &SourceName) -> Option<PlanColumn> {
    let own = definitions.model(metric.model())?;
    let crosses = metric
        .measure()
        .is_some_and(|m| m.models().into_iter().flatten().any(|second| second != metric.model()));
    let model = match (crosses, metric.shared_calendars()) {
        (false, _) => own,
        (true, Some(calendars)) if own.source() == source => calendars
            .iter()
            .filter_map(|name| definitions.model(name))
            .find(|calendar| calendar.source() == own.source())?,
        (true, _) => return None,
    };
    Some(PlanColumn::new(model.table_name().clone(), metric.time_column().clone()))
}

/// Every column `metric`'s measure reads, on the table of the model it reads that column from.
fn measured(definitions: &Definitions, metric: &Metric) -> Vec<PlanColumn> {
    let Some(measure) = metric.measure() else {
        return Vec::new();
    };
    measure
        .columns()
        .into_iter()
        .zip(measure.models())
        .filter_map(|(column, model)| {
            let table = definitions
                .model(model.unwrap_or_else(|| metric.model()))?
                .table_name()
                .clone();
            Some(PlanColumn::new(table, column.clone()))
        })
        .collect()
}

/// Whether the leg's first two predicates are `range`'s bounds on `bucket`, bound to `range`'s dates.
fn bounded(range: TimeRange, bucket: &PlanColumn, filters: &[PlanFilter], params: &[ParamValue]) -> bool {
    let mut dates = Vec::with_capacity(2);
    let bounds = range_filters(range, bucket, &mut dates);
    filters.get(..2) == Some(bounds.as_slice()) && params.get(..2) == Some(dates.as_slice())
}

/// Whether `filter` is this node's own required filter, bound to the value this node declares.
fn is_required(required: &RequiredFilter, own_table: &TableName, filter: &PlanFilter, params: &[ParamValue]) -> bool {
    let column = PlanColumn::new(own_table.clone(), required.column().clone());
    let bound = |param: usize, value: &DimensionValue| params.get(param) == Some(&ParamValue::Text(String::from(value.as_str())));
    filter.origin() == PredicateOrigin::Definition
        && match (required, filter.predicate()) {
            (RequiredFilter::Equals { value, .. }, PlanPredicate::Equals { column: at, param })
            | (RequiredFilter::NotEquals { value, .. }, PlanPredicate::NotEquals { column: at, param }) => {
                *at == column && bound(*param, value)
            }
            (RequiredFilter::IsTrue { .. }, PlanPredicate::IsTrue { column: at })
            | (RequiredFilter::IsNotNull { .. }, PlanPredicate::IsNotNull { column: at }) => *at == column,
            _ => false,
        }
}

/// A fact leg's fields as they arrived, before any of them is read.
struct FactWire {
    source: SourceName,
    metric: MetricDigest,
    table: QualifiedTable,
    joins: Vec<RelationshipName>,
    bucket: WireBucket,
    keys: Vec<WireKey>,
    terms: Vec<WireTerm>,
    filters: Vec<PlanFilter>,
    range: TimeRange,
}

/// A lookup leg's fields as they arrived, before any of them is read.
struct LookupWire {
    source: SourceName,
    metric: MetricDigest,
    table: QualifiedTable,
    joins: Vec<RelationshipName>,
    keys: Vec<WireKey>,
    filters: Vec<PlanFilter>,
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
            keys,
            terms,
            filters,
            range,
        } => certify_fact(
            FactWire {
                source,
                metric,
                table,
                joins,
                bucket,
                keys,
                terms,
                filters,
                range,
            },
            pinned,
            params,
        ),
        LegWire::Lookup {
            source,
            metric,
            table,
            joins,
            keys,
            filters,
        } => certify_lookup(
            LookupWire {
                source,
                metric,
                table,
                joins,
                keys,
                filters,
            },
            pinned,
            params,
        ),
    }
}

/// The metric's grain, and its bucket on the column this node buckets it on for a leg on `source`.
fn certified_bucket(
    (definitions, declared, source): (&Definitions, &Metric, &SourceName),
    statement: &Statement<'_>,
    grain: Grain,
    column: &PlanColumn,
) -> Result<(), LegNotCertified> {
    if !declared.supports_grain(grain) {
        return Err(LegNotCertified::GrainNotDeclared {
            metric: declared.name().clone(),
            grain,
        });
    }
    if time_column(definitions, declared, source).as_ref() != Some(column) {
        return Err(LegNotCertified::BucketNotTimeColumn {
            metric: declared.name().clone(),
            table: column.table().clone(),
            column: column.column().clone(),
        });
    }
    statement.certifies(column)
}

/// The terms, each a column the measure reads on the leg's root table.
fn certified_terms(
    terms: Vec<WireTerm>,
    statement: &Statement<'_>,
    leaves: &Measured<'_>,
) -> Result<Vec<LegTerm>, LegNotCertified> {
    within(LegPart::Terms, terms.len(), MAX_TERMS)?;
    terms
        .into_iter()
        .map(|WireTerm { label, term }| {
            if !leaves.columns.contains(term.column()) || statement.root.table_name() != term.column().table() {
                return Err(LegNotCertified::TermNotMetrics {
                    metric: leaves.metric.clone(),
                    table: term.column().table().clone(),
                    column: term.column().column().clone(),
                });
            }
            statement.certifies(term.column())?;
            Ok(LegTerm::new(term, label.into_label()))
        })
        .collect()
}

/// When the bucket reads a calendar, the leg joins it through the relationship this node joins the
/// leg's root to it by.
fn certified_calendar(
    (definitions, declared): (&Definitions, &Metric),
    statement: &Statement<'_>,
    bucket: &PlanColumn,
    joins: &[RelationshipName],
) -> Result<(), LegNotCertified> {
    let on_calendar = bucket.table() != statement.root.table_name();
    if on_calendar && !surface::calendar_hop(definitions, declared, statement.root).is_some_and(|hop| joins.contains(hop.name()))
    {
        return Err(LegNotCertified::CalendarNotJoined {
            metric: declared.name().clone(),
        });
    }
    Ok(())
}

fn certify_fact(wire: FactWire, pinned: &PinnedDefinitions, params: Vec<ParamValue>) -> Result<LegPlan, LegNotCertified> {
    let declared = pinned.agrees_on(&wire.metric)?;
    let definitions = pinned.definitions();
    let name = declared.name();
    let measure = declared.measure();
    let reads = |model: &Model| {
        model.name() == declared.model()
            || measure.is_some_and(|m| m.models().into_iter().flatten().any(|second| second == model.name()))
    };
    let joins = &wire.joins;
    let statement = statement(definitions, &wire.source, &wire.table, joins, reads)?;
    let WireBucket { grain, column } = wire.bucket;
    certified_bucket((definitions, declared, &wire.source), &statement, grain, &column)?;
    if wire.range.days() > MAX_RANGE_DAYS {
        return Err(LegNotCertified::RangeTooLong {
            days: wire.range.days(),
            limit: MAX_RANGE_DAYS,
        });
    }
    let measures = measured(definitions, declared);
    let leaves = Measured {
        metric: name,
        columns: &measures,
    };
    let terms = certified_terms(wire.terms, &statement, &leaves)?;
    let calendar = surface::calendar_hop(definitions, declared, statement.root).map(Relationship::name);
    let on_calendar = (column.table() != statement.root.table_name()).then_some(calendar).flatten();
    let surface = Surface::of_fact(definitions, declared, statement.root, on_calendar);
    declared_joins(joins, &surface)?;
    certified_calendar((definitions, declared), &statement, &column, joins)?;
    let scope = Scope {
        definitions,
        surface: &surface,
        joins,
    };
    let keys = keys(wire.keys, &statement, &scope, Some(&leaves))?;
    let own = definitions
        .model(declared.model())
        .is_some_and(|model| model.table() == &wire.table);
    let required = if own { declared.required_filters() } else { &[] };
    filters(&wire.filters, &statement, required.len())?;
    if !bounded(wire.range, &column, &wire.filters, &params) {
        return Err(LegNotCertified::RangeNotBound { metric: name.clone() });
    }
    let own_table = statement.root.table_name();
    if let Some(missing) = required.iter().find(|required| {
        !wire
            .filters
            .iter()
            .any(|filter| is_required(required, own_table, filter, &params))
    }) {
        return Err(LegNotCertified::RequiredFilterMissing {
            metric: name.clone(),
            column: missing.column().clone(),
        });
    }
    let held = |filter: &PlanFilter| {
        required
            .iter()
            .any(|required| is_required(required, own_table, filter, &params))
    };
    declared_filters(wire.filters.get(2..).unwrap_or_default(), &params, (&surface, joins), held)?;
    Ok(LegPlan::Fact {
        source: wire.source,
        metric: name.clone(),
        tables: statement.tables,
        bucket: PlanBucket::new(ResultLabel::bucket(), grain, column),
        keys,
        terms,
        bindings: PlanBindings::parse(wire.filters, params)?,
        range: wire.range,
    })
}

fn certify_lookup(wire: LookupWire, pinned: &PinnedDefinitions, params: Vec<ParamValue>) -> Result<LegPlan, LegNotCertified> {
    let declared = pinned.agrees_on(&wire.metric)?;
    let definitions = pinned.definitions();
    let entered = |model: &Model| surface::crossed_into(definitions, declared, model);
    let joins = &wire.joins;
    let statement = statement(definitions, &wire.source, &wire.table, joins, entered)?;
    let surface = Surface::of_lookup(definitions, declared, statement.root);
    declared_joins(joins, &surface)?;
    let scope = Scope {
        definitions,
        surface: &surface,
        joins,
    };
    let keys = keys(wire.keys, &statement, &scope, None)?;
    filters(&wire.filters, &statement, 0)?;
    declared_filters(&wire.filters, &params, (&surface, joins), |_| false)?;
    Ok(LegPlan::Lookup {
        source: wire.source,
        metric: declared.name().clone(),
        table: statement.tables,
        keys,
        bindings: PlanBindings::parse(wire.filters, params)?,
    })
}
