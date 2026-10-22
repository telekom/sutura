//! The federated splitter: a question over several data systems becomes a fact leg and up to
//! [`MAX_LOOKUP_LEGS`](sutura_domain::query::MAX_LOOKUP_LEGS) lookup legs.
//!
//! **A module rather than three functions in `plan.rs`, and the seam is the same one `chain`
//! documents: one concept, one file, one reader.** Split out under `cargo xtask max-lines`'s limit:
//! `plan.rs` plus this module's own tests is the shape that landed at over 1000 lines once
//! `telekom/sutura#967` added a per-key rendering and refusal path, and the fix is the same one
//! this crate already had a name for - move the code, not compress it.
//!
//! Nothing here decides which plan SHAPE a question gets - `super::plan` does that, and calls
//! [`federated_plan`] only once it has already decided there is at least one lookup leg, and at most the cap.

use sutura_domain::catalog::{ColumnType, JoinKey, Model, Relationship};
use sutura_domain::federation::{Carried, Federation};
use sutura_domain::measure::Measure;
use sutura_domain::model::{ColumnName, SourceName, TableName};
use sutura_domain::plan::leg::LegTerm;
use sutura_domain::plan::{
    AnswerKey, FederatedPlan, FederatedPlanError, InternalLabel, LegPlan, LinkKind, LookupLeg, LookupLegs, PlanBucket,
    PlanColumn, PlanKey, PlanTerm, QuotedColumnType, ResultLabel, StatementTables, labels,
};
use sutura_domain::query::RefusalReason;

use super::PlanError;
use super::chain::{Lookups, chain_joins, column_of, hop_join};
use crate::resolve::{Resolution, ResolvedFilter, ResolvedJoin};

/// Splits a question over several data systems into a fact leg and its lookup legs.
///
/// The metric's own rows (and any same-source dimension) form the fact leg; each crossing
/// relationship forms one lookup leg ([`Lookups`]). A leg's link is the crossing relationship's join
/// column, projected from the leg that holds its origin - the fact leg, or the lookup leg an earlier
/// crossing opened - and from the leg it opens, under the same label, so the combiner can find it. A
/// chain's hops before its first crossing join on the fact leg, and its hops after a crossing join
/// inside that crossing's leg. A measure that cannot decompose (a distinct count) is pulled up: the
/// fact leg carries its column as one more key and the combine counts the distinct values above.
// The splitter builds every leg, their keys, their filters and the links in one pass over the
// resolution; it is a single act of splitting a resolved question, and it returns Err from several
// places that far apart to make a reviewer see the splitter's refusals together.
pub(super) fn federated_plan(
    resolution: &Resolution<'_>,
    closed: &Measure,
    lookups: &Lookups<'_, '_>,
) -> Result<FederatedPlan, PlanError> {
    let metric = resolution.metric;
    let model = resolution.model;
    // Two readings of "the table", for the reason `mono_plan` gives: the path is what a leg's `FROM`
    // names, the bare name is what every column in that leg is qualified by.
    let own_path = model.table();
    let own_table = model.table_name();

    // `of_metric`, not `of`: a term naming the metric's own model stays on the first fact leg.
    let federation = Federation::of_metric(closed, model.name());
    // The guard below is unreachable - `plan` calls this only with at least one lookup leg - but a
    // panic here would be reachable from a catalog plus a question, so it refuses instead.
    //
    // A2: it fabricates no `RefusalReason` - see `PlanError::NoRemoteJoin`.
    let Some(first) = lookups.first() else {
        return Err(PlanError::NoRemoteJoin);
    };
    // One link per lookup leg: the combiner links two legs on a single column, so a leg's crossing
    // relationship must declare exactly one key, and its two columns must be kinds that compare.
    let mut crossing_keys: Vec<&JoinKey> = Vec::with_capacity(lookups.len());
    for leg in lookups.iter() {
        let relationship = leg.crossing.relationship;
        crossing_keys.push(one_crossing_key(relationship, leg.crossing.model.source())?);
        // Decided from the catalog's declared types before either leg runs, which is what makes the
        // refusal free: the combiner's own check needs both legs' schemas, which means running them.
        link_types_agree(relationship, leg.origin, leg.crossing.model)?;
    }

    // **A link's label is reserved, and it used to be the physical join column's text.** That text
    // is a legal dimension name, and it sat in the same result namespace as the public dimension
    // labels beside it - so a metric with a legal dimension named `customer_key`, backed by a
    // different column, produced two fact columns under one label and the combiner refused the
    // answer. `InternalLabel` is a namespace a question cannot spell into; the dimension stays legal.

    // The fact leg groups by its local dimension keys plus the origin of each link it carries, so the
    // lookup legs can be joined to it above.
    let mut fact_keys: Vec<PlanKey> = Vec::new();
    for key in resolution.keys.iter().filter(|key| lookups.index_of(key).is_none()) {
        fact_keys.push(PlanKey::new(
            ResultLabel::dimension(key.dimension.name()),
            column_of(key, own_table),
        ));
    }
    for (at, (leg, key)) in lookups
        .iter()
        .zip(&crossing_keys)
        .enumerate()
        .filter(|(_, (leg, _))| leg.parent.is_none())
    {
        fact_keys.push(origin_key(link_label(at), leg.origin.table_name(), key));
    }

    // Filters split by which leg owns the column they constrain.
    let local_filters: Vec<&ResolvedFilter> = resolution
        .filters
        .iter()
        .filter(|filter| lookups.index_of(&filter.dimension).is_none())
        .collect();

    // `telekom/sutura#780`: a cross-model ratio buckets and bounds both fact legs on the shared
    // calendar's column, which each leg joins below; every other metric on its own `time_column`.
    let time_table = resolution
        .cross
        .as_ref()
        .map_or(own_table, |cross| cross.calendar.table_name());
    let time_column = PlanColumn::new(time_table.clone(), metric.time_column().clone());
    let fact_bindings = super::predicates_and_params(resolution, &local_filters, own_table, &time_column)?;

    // `telekom/sutura#780`: a cross-model ratio's leaves split by the model each reads, each column
    // qualified by the table its own leg reads.
    let second_table = resolution.cross.as_ref().map_or(own_table, |cross| cross.second.table_name());
    let (terms, second_terms) = leg_terms(&federation, own_table, second_table, &mut fact_keys);

    // Same-source hops (dimensions on the metric's own system) stay joins on the fact leg. Each
    // chain stops at its crossing hop, which is the link the lookup leg above carries rather than a
    // `JOIN` on this leg's statement; the hops after it are the lookup leg's joins, built below. Same
    // builder as the whole-answer path: two copies of "one join per hop, qualified by the previous
    // hop's target" is how one of them came to be qualified by the metric's table instead.
    let mut joins = chain_joins(resolution, own_table);
    if let Some(cross) = resolution.cross.as_ref() {
        joins.push(hop_join(cross.own_to_calendar, own_table, cross.calendar));
    }

    let bucket = PlanBucket::new(ResultLabel::bucket(), resolution.grain, time_column);

    // **Where the fact leg's tables stop being a list and become a checked set - the same guard the
    // whole-answer path goes through, reached from the other plan shape.** A leg keeps every
    // same-source hop as a `JOIN` of its own, so two paths ending in one name render under one
    // implicit alias inside ONE leg's statement: reproduced, and worse there than on the whole-answer
    // path, because a leg's rows are combined above it and nothing downstream sees the statement.
    // `sutura_domain::plan::tables` holds the measurement and the argument for refusing rather than
    // aliasing; `LegPlan::Fact` takes the checked set as a field, so this call is not something a
    // future leg producer can forget.
    let fact_tables =
        StatementTables::parse(own_path.clone(), joins).map_err(|ambiguous| RefusalReason::PlanTablesShareAnIdentifier {
            table: ambiguous.alias().clone(),
        })?;

    // The answer's group-by keys in question order, each naming which leg's result it is read from.
    // Question order is the mono path's column order too, so a federated answer aligns with a
    // single-source one (and with a future federated differential that reads rows by position).
    let answer_keys: Vec<AnswerKey> = resolution
        .keys
        .iter()
        .map(|key| {
            let label = ResultLabel::dimension(key.dimension.name());
            lookups
                .index_of(key)
                .map_or_else(|| AnswerKey::fact(label.clone()), |leg| AnswerKey::lookup(leg, label.clone()))
        })
        .collect();

    let fact = LegPlan::Fact {
        source: model.source().clone(),
        metric: metric.name().clone(),
        tables: fact_tables,
        bucket: bucket.clone(),
        keys: fact_keys,
        terms,
        bindings: fact_bindings,
        range: resolution.range,
    };

    let second_fact = second_fact_leg(resolution, first.crossing, &bucket, second_terms)?;
    let plan = FederatedPlan::new(
        metric.name().clone(),
        ResultLabel::measure(metric.name()),
        bucket,
        fact,
        second_fact,
        lookup_legs(resolution, lookups, &crossing_keys)?,
        federation,
        answer_keys,
    )?;
    // A `top` still ranks the answer, just above the combine - the splitter's one case,
    // `github.com/telekom/sutura#777`'s case 2. A federated `top` is never pushed into a leg's own
    // statement; every federated `top` ranks after the combine instead.
    //
    // **The cause is no longer erased, and this is the whole of `telekom/sutura#338`.** It used to
    // become `RefusalReason::FederationNotExecutable`, which is ALSO what a build whose adapter does
    // not declare `EXECUTES_LEGS` gets - so a plan this workspace could not assemble read exactly
    // like the deployment simply not being able to execute a leg. `FederatedPlan::new`'s `?` above
    // leaves it as `PlanError::NotAssembled` now, keeping the typed cause, because a defect in our
    // own wiring is not a governance answer and a caller must not be handed one it could retry.
    Ok(match resolution.top {
        Some(top) => plan.with_top(top),
        None => plan,
    })
}

/// The first fact leg's terms, then the second's.
pub(super) type TermsByLeg = (Vec<LegTerm>, Vec<LegTerm>);

/// The fact legs' terms for `federation`'s leaves, projected under the one labelling rule the combiner
/// reads back - in the same reserved namespace as the link, for the same reason: `metric__{n}` is a
/// legal dimension name too, and over a 63-character metric name it also crossed the identifier limit
/// a data system truncates silently.
///
/// A leaf that names another model is the second fact's, and each is qualified by the table its own
/// leg reads. Both legs are labelled by position through [`labels`], and D9 in `FederatedPlan::new`
/// checks each carries its own share. A distinct count's leaf is no term: the leg groups by its
/// column, one row per distinct value, so it becomes one more key of `fact_keys` and the combine
/// counts them.
pub(super) fn leg_terms(
    federation: &Federation,
    own_table: &TableName,
    second_table: &TableName,
    fact_keys: &mut Vec<PlanKey>,
) -> TermsByLeg {
    let leaf_labels = labels(federation);
    let mut terms: Vec<LegTerm> = Vec::with_capacity(leaf_labels.len());
    let mut second_terms: Vec<LegTerm> = Vec::new();
    for (leaf, &label) in federation.carried().iter().zip(leaf_labels.iter()) {
        let owning_table = (if leaf.model().is_some() { second_table } else { own_table }).clone();
        let plan_term = match **leaf {
            Carried::Aggregated { pushed, ref column, .. } => PlanTerm::Aggregate {
                aggregate: pushed.push(),
                column: PlanColumn::new(owning_table, column.clone()),
            },
            Carried::CountIf { ref column, .. } => PlanTerm::CountIf {
                column: PlanColumn::new(owning_table, column.clone()),
            },
            // `FederatedPlan::new` checks the key is on the leg under this label.
            Carried::Keys { ref column, .. } => {
                fact_keys.push(PlanKey::new(
                    ResultLabel::internal(label),
                    PlanColumn::new(owning_table, column.clone()),
                ));
                continue;
            }
        };
        let leg_term = LegTerm::new(plan_term, ResultLabel::internal(label));
        if leaf.model().is_some() {
            second_terms.push(leg_term);
        } else {
            terms.push(leg_term);
        }
    }
    (terms, second_terms)
}

/// The label a lookup leg's link travels under, on the leg that opens it and the leg it joins onto.
fn link_label(leg: usize) -> ResultLabel {
    ResultLabel::internal(InternalLabel::link(leg))
}

/// One lookup leg per crossing path, each joined onto the leg that holds its link's origin.
///
/// A leg groups by its own link, the origin of each link a later leg joins onto it through, and the
/// dimensions read from it. LEFT when the leg carries no filter (an unmatched row survives), INNER
/// when it does: `docs/adr/0009` decides the direction, for each leg and not once for the plan.
fn lookup_legs(
    resolution: &Resolution<'_>,
    lookups: &Lookups<'_, '_>,
    crossing_keys: &[&JoinKey],
) -> Result<LookupLegs, PlanError> {
    let mut legs: Vec<LookupLeg> = Vec::with_capacity(lookups.len());
    for (at, (shape, key)) in lookups.iter().zip(crossing_keys).enumerate() {
        let table = shape.crossing.model.table_name();
        let mut keys = vec![PlanKey::new(
            link_label(at),
            PlanColumn::new(table.clone(), key.target().clone()),
        )];
        for (child, (later, later_key)) in lookups
            .iter()
            .zip(crossing_keys)
            .enumerate()
            .filter(|(_, (later, _))| later.parent == Some(at))
        {
            keys.push(origin_key(link_label(child), later.origin.table_name(), later_key));
        }
        for dimension in resolution
            .keys
            .iter()
            .filter(|dimension| lookups.index_of(dimension) == Some(at))
        {
            keys.push(PlanKey::new(
                ResultLabel::dimension(dimension.dimension.name()),
                column_of(dimension, table),
            ));
        }
        let filters: Vec<&ResolvedFilter> = resolution
            .filters
            .iter()
            .filter(|filter| lookups.index_of(&filter.dimension) == Some(at))
            .collect();
        // The lookup leg's tables are the same checked set as the fact leg's, for the same reason: a
        // chain that crossed and carries on joins inside this statement, so two tables answering to
        // one identifier there would render one implicit alias.
        let tables = StatementTables::parse(shape.crossing.model.table().clone(), shape.joins.clone()).map_err(|ambiguous| {
            RefusalReason::PlanTablesShareAnIdentifier {
                table: ambiguous.alias().clone(),
            }
        })?;
        let leg = LegPlan::Lookup {
            source: shape.crossing.model.source().clone(),
            table: tables,
            keys,
            bindings: super::requested_for(&filters, table)?,
        };
        let include_unmatched = filters.is_empty();
        legs.push(match shape.parent {
            None => LookupLeg::onto_fact(leg, include_unmatched),
            Some(parent) => LookupLeg::onto_lookup(leg, parent, include_unmatched),
        });
    }
    Ok(LookupLegs::parse(legs).map_err(FederatedPlanError::from)?)
}

/// The one key a crossing relationship links its two legs on, refusing a compound one.
///
/// The combiner links the legs on ONE column under ONE label, so a compound crossing key would need
/// a label per key: refused BY NAME as `FederationLinkCompound`. A key truncated to a grain is one
/// column too: the fact leg truncates it at the source ([`origin_key`]) and the lookup leg
/// projects the target as it is.
fn one_crossing_key<'r>(relationship: &'r Relationship, source: &SourceName) -> Result<&'r JoinKey, PlanError> {
    if relationship.keys().len() != 1 {
        return Err(PlanError::Refused(RefusalReason::FederationLinkCompound {
            source: source.clone(),
            relationship: relationship.name().clone(),
        }));
    }
    Ok(relationship.keys().first())
}

/// The fact leg's link key: the origin column, truncated to the key's grain when it declares one.
fn origin_key(label: ResultLabel, table: &TableName, key: &JoinKey) -> PlanKey {
    let column = PlanColumn::new(table.clone(), key.origin().clone());
    match key.grain() {
        Some(grain) => PlanKey::truncated(label, column, grain),
        None => PlanKey::new(label, column),
    }
}

/// A column's declared type, when its model declares the column and a type for it.
fn declared_type<'m>(model: &'m Model, column: &ColumnName) -> Option<&'m ColumnType> {
    model.column(column)?.data_type()
}

/// Refuses a crossing whose two key columns are declared as different kinds of value.
///
/// **Only when both types are declared and both are known, and everything else defers.** A column
/// with no declared type, or a type [`LinkKind::declared`] does not classify, says nothing about
/// whether the keys can match, so the combiner's check over the legs' real schemas decides it
/// later exactly as before. A refusal here is a claim about the catalog and not about the data: a
/// catalog that mis-declares a key refuses a join that would have matched.
fn link_types_agree(relationship: &Relationship, fact_side: &Model, lookup_side: &Model) -> Result<(), PlanError> {
    let key = relationship.keys().first();
    let (Some(fact), Some(lookup)) = (
        declared_type(fact_side, key.origin()),
        declared_type(lookup_side, key.target()),
    ) else {
        return Ok(());
    };
    // A truncated origin reaches the combiner as the date `bucket_expr` casts it to, whatever type it is declared as.
    let fact_kind = if key.grain().is_some() {
        Some(LinkKind::Date)
    } else {
        LinkKind::declared(fact)
    };
    match (fact_kind, LinkKind::declared(lookup)) {
        (Some(on_fact), Some(on_lookup)) if on_fact != on_lookup => {
            Err(PlanError::Refused(RefusalReason::FederationLinkTypeMismatch {
                relationship: relationship.name().clone(),
                fact_type: QuotedColumnType::from(fact),
                lookup_type: QuotedColumnType::from(lookup),
            }))
        }
        _ => Ok(()),
    }
}

/// The second fact leg of a cross-model ratio (`telekom/sutura#780`): its OWN statement over the
/// second fact's table, joined to the shared calendar, bucketed and bounded on the calendar exactly
/// as the first leg is, and linked to the lookup through the second fact's own relationship.
///
/// `Ok(None)` when no leaf names another model - every one-model metric. The link is the second
/// fact's relationship into the model the first fact crosses into, joining the SAME lookup column
/// the lookup leg projects under the link label, truncated to the same grain or not at all: a
/// second fact linking on raw values beside a first fact linking on months would meet the lookup on
/// different values. Without such a relationship the two facts share no dimension and the ratio is
/// refused by name rather than linked on a column borrowed from the first fact's key.
fn second_fact_leg(
    resolution: &Resolution<'_>,
    crossing: &ResolvedJoin<'_>,
    bucket: &PlanBucket,
    second_terms: Vec<LegTerm>,
) -> Result<Option<LegPlan>, PlanError> {
    if second_terms.is_empty() {
        return Ok(None);
    }
    let Some(cross) = resolution.cross.as_ref() else {
        return Err(PlanError::NoSecondFactModel);
    };
    let second = cross.second;
    let crossing_key = crossing.relationship.keys().first();
    let Some(link) = cross.second_relationships.iter().find(|relationship| {
        relationship.target_model() == crossing.model.name()
            && relationship.keys().len() == 1
            && relationship.keys().first().target() == crossing_key.target()
            && relationship.keys().first().grain() == crossing_key.grain()
    }) else {
        return Err(PlanError::Refused(RefusalReason::CrossModelRatioWithoutSharedDimension {
            metric: resolution.metric.name().clone(),
            model: second.name().clone(),
        }));
    };
    let link_key = link.keys().first();
    let tables = StatementTables::parse(
        second.table().clone(),
        vec![hop_join(cross.second_to_calendar, second.table_name(), cross.calendar)],
    )
    .map_err(|ambiguous| RefusalReason::PlanTablesShareAnIdentifier {
        table: ambiguous.alias().clone(),
    })?;
    Ok(Some(LegPlan::Fact {
        source: second.source().clone(),
        metric: resolution.metric.name().clone(),
        tables,
        bucket: bucket.clone(),
        keys: vec![origin_key(link_label(0), second.table_name(), link_key)],
        terms: second_terms,
        bindings: super::range_only(resolution.range, bucket.column())?,
        range: resolution.range,
    }))
}
