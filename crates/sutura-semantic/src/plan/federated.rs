//! The federated splitter: a two-source question becomes a fact leg and a lookup leg.
//!
//! **A module rather than three functions in `plan.rs`, and the seam is the same one `chain`
//! documents: one concept, one file, one reader.** Split out under `cargo xtask max-lines`'s limit:
//! `plan.rs` plus this module's own tests is the shape that landed at over 1000 lines once
//! `telekom/sutura#967` added a per-key rendering and refusal path, and the fix is the same one
//! this crate already had a name for - move the code, not compress it.
//!
//! Nothing here decides which plan SHAPE a question gets - `super::plan` does that, and calls
//! [`federated_plan`] only once it has already decided there is exactly one remote source.

use sutura_domain::catalog::{ColumnType, Model, Relationship};
use sutura_domain::federation::{Carried, Federation};
use sutura_domain::measure::Measure;
use sutura_domain::model::ColumnName;
use sutura_domain::plan::leg::LegTerm;
use sutura_domain::plan::{
    FederatedPlan, InternalLabel, LegPlan, LinkKind, PlanBucket, PlanColumn, PlanKey, PlanTerm, QuotedColumnType, ResultLabel,
    StatementTables, labels,
};
use sutura_domain::query::RefusalReason;

use super::PlanError;
use super::chain::{chain_joins, column_of, crossing, every_remote_dimension, hop_join, is_remote, lookup_joins};
use crate::resolve::{Resolution, ResolvedFilter, ResolvedJoin};

/// Splits a two-source question into a fact leg and a lookup leg.
///
/// The metric's own rows (and any same-source dimension) form the fact leg; everything on the one
/// remote data system forms the lookup leg. The link between them is the crossing relationship's join
/// column, grouped into the fact leg and projected from the lookup leg under the same label - so the
/// combiner can find it. A chain's hops before the crossing join on the fact leg and its hops after
/// it join inside the lookup leg. A measure that cannot decompose (a distinct count) is pulled up: the fact leg
/// carries its column as one more key and the combine counts the distinct values above.
// The splitter builds both legs, their keys, their filters and the link in one pass over the
// resolution; it is a single act of splitting a resolved question, and it returns Err from several
// places that far apart to make a reviewer see the splitter's refusals together.
pub(super) fn federated_plan(resolution: &Resolution<'_>, closed: &Measure) -> Result<FederatedPlan, PlanError> {
    let metric = resolution.metric;
    let model = resolution.model;
    // Two readings of "the table", for the reason `mono_plan` gives: the path is what a leg's `FROM`
    // names, the bare name is what every column in that leg is qualified by.
    let own_path = model.table();
    let own_table = model.table_name();

    // `of_metric`, not `of`: a term naming the metric's own model stays on the first fact leg.
    let federation = Federation::of_metric(closed, model.name());
    // One remote data system (more are refused upstream); its dimensions must all cross through one
    // relationship, because the combiner links the legs on a single column - and a remote
    // dimension's CROSSING hop is what the link is: the hop that lands on the remote system. The
    // hops before it are same-source joins of the fact leg, and the hops after it are joins of the
    // lookup leg. The guards below are unreachable - `plan` calls this only for exactly one remote
    // source, and a remote dimension has a crossing by construction - but a panic here would be
    // reachable from a catalog plus a question, so they refuse instead.
    //
    // A2: neither guard fabricates a `RefusalReason` any more - see `PlanError::NoRemoteJoin`.
    let Some(first_remote) = every_remote_dimension(resolution).next() else {
        return Err(PlanError::NoRemoteJoin);
    };
    let Some(first) = first_remote.join.as_ref().and_then(|hops| crossing(hops, model.source())) else {
        return Err(PlanError::NoRemoteJoin);
    };
    let first_join = first.hop;
    let relationship = first_join.relationship;
    let remote_path = first_join.model.table();
    let remote_table = first_join.model.table_name();
    let remote_source = first_join.model.source();
    // The fact leg's table that holds the link's origin column: the one the crossing hop starts at,
    // which is the metric's own table when the chain crosses at its first hop. Every dimension
    // crossing through this relationship starts it from the same model, so one table serves them all.
    let link_model = first.before.last().map_or(model, |hop| hop.model);
    let link_table = link_model.table_name();
    for dim in every_remote_dimension(resolution) {
        let Some(cut) = dim.join.as_ref().and_then(|hops| crossing(hops, model.source())) else {
            continue;
        };
        if cut.hop.relationship.name() != relationship.name() {
            return Err(PlanError::Refused(RefusalReason::FederationLinkAmbiguous {
                source: remote_source.clone(),
            }));
        }
    }

    // **The link's label is reserved, and it used to be the physical join column's text.** That text
    // is a legal dimension name, and it sat in the same result namespace as the public dimension
    // labels beside it - so a metric with a legal dimension named `customer_key`, backed by a
    // different column, produced two fact columns under one label and the combiner refused the
    // answer. `InternalLabel` is a namespace a question cannot spell into; the dimension stays legal.
    let link_label = ResultLabel::internal(InternalLabel::Link);

    // The combiner links the two legs on ONE column under ONE label. A single-key crossing
    // relationship projects that one key on each side; a compound crossing one would need a label
    // per key, which the lookup leg does not carry - refused BY NAME as
    // `FederationLinkCompound` rather than reused from `FederationLinkAmbiguous`, which names two
    // RELATIONSHIPS crossing at once and would tell a caller something untrue about one correctly
    // declared relationship with more than one key.
    if relationship.keys().len() != 1 {
        return Err(PlanError::Refused(RefusalReason::FederationLinkCompound {
            source: remote_source.clone(),
            relationship: relationship.name().clone(),
        }));
    }
    let crossing_key = relationship.keys().first();
    // Decided from the catalog's declared types before either leg runs, which is what makes the
    // refusal free: the combiner's own check needs both legs' schemas, which means running them.
    link_types_agree(relationship, link_model, first_join.model)?;

    // The fact leg groups by its local dimension keys plus the join origin, so the lookup leg can be
    // joined to it above.
    let mut fact_keys: Vec<PlanKey> = Vec::new();
    for key in resolution.keys.iter().filter(|key| !is_remote(key, model.source())) {
        fact_keys.push(PlanKey::new(
            ResultLabel::dimension(key.dimension.name()),
            column_of(key, own_table),
        ));
    }
    fact_keys.push(PlanKey::new(
        link_label.clone(),
        PlanColumn::new(link_table.clone(), crossing_key.origin().clone()),
    ));

    // The lookup leg projects the join target plus the remote dimension keys.
    let mut lookup_keys: Vec<PlanKey> = vec![PlanKey::new(
        link_label,
        PlanColumn::new(remote_table.clone(), crossing_key.target().clone()),
    )];
    for key in resolution.keys.iter().filter(|key| is_remote(key, model.source())) {
        lookup_keys.push(PlanKey::new(
            ResultLabel::dimension(key.dimension.name()),
            column_of(key, remote_table),
        ));
    }

    // Filters split by which leg owns the column they constrain.
    let local_filters: Vec<&ResolvedFilter> = resolution
        .filters
        .iter()
        .filter(|filter| !is_remote(&filter.dimension, model.source()))
        .collect();
    let remote_filters: Vec<&ResolvedFilter> = resolution
        .filters
        .iter()
        .filter(|filter| is_remote(&filter.dimension, model.source()))
        .collect();

    // `telekom/sutura#780`: a cross-model ratio buckets and bounds both fact legs on the shared
    // calendar's column, which each leg joins below; every other metric on its own `time_column`.
    let time_table = resolution
        .cross
        .as_ref()
        .map_or(own_table, |cross| cross.calendar.table_name());
    let time_column = PlanColumn::new(time_table.clone(), metric.time_column().clone());
    let fact_bindings = super::predicates_and_params(resolution, &local_filters, own_table, &time_column)?;
    let lookup_bindings = super::requested_for(&remote_filters, remote_table)?;

    // The fact leg's terms, projected under the one labelling rule the combiner reads back - in the
    // same reserved namespace as the link, for the same reason: `metric__{n}` is a legal dimension
    // name too, and over a 63-character metric name it also crossed the identifier limit a data
    // system truncates silently.
    let leaf_labels = labels(&federation);
    let mut terms: Vec<LegTerm> = Vec::with_capacity(leaf_labels.len());
    let mut second_terms: Vec<LegTerm> = Vec::new();
    // `telekom/sutura#780`: a cross-model ratio's leaves split by the model each reads - `None`
    // (the metric's own, erased by `of_metric`) on the first fact leg, `Some` on the second, each
    // column qualified by the table its own leg reads. Both legs are labelled by position through
    // `labels(&federation)`, and D9 in `FederatedPlan::new` checks each leg carries its own share.
    let second_table = resolution.cross.as_ref().map_or(own_table, |cross| cross.second.table_name());
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
            // No term: the leg groups by the column, one row per distinct value, and the combine
            // counts them. `FederatedPlan::new` checks the key is on the leg under this label.
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

    // Same-source hops (dimensions on the metric's own system) stay joins on the fact leg. Each
    // chain stops at its crossing hop, which is the link the lookup leg above carries rather than a
    // `JOIN` on this leg's statement; the hops after it are the lookup leg's joins, built below. Same
    // builder as the whole-answer path: two copies of "one join per hop, qualified by the previous
    // hop's target" is how one of them came to be qualified by the metric's table instead.
    let mut joins = chain_joins(resolution, own_table, model.source());
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
    let answer_keys: Vec<sutura_domain::plan::AnswerKey> = resolution
        .keys
        .iter()
        .map(|key| {
            let label = ResultLabel::dimension(key.dimension.name());
            if is_remote(key, model.source()) {
                sutura_domain::plan::AnswerKey::lookup(label)
            } else {
                sutura_domain::plan::AnswerKey::fact(label)
            }
        })
        .collect();

    // LEFT when the lookup carries no filter (an unmatched fact row survives), INNER when it does.
    // `docs/adr/0009` decides the direction.
    let include_unmatched = remote_filters.is_empty();

    let fact = sutura_domain::plan::LegPlan::Fact {
        source: model.source().clone(),
        metric: metric.name().clone(),
        tables: fact_tables,
        bucket: bucket.clone(),
        keys: fact_keys,
        terms,
        bindings: fact_bindings,
        range: resolution.range,
    };

    // **The lookup leg's tables are the same checked set as the fact leg's, for the same reason:** a
    // chain that crossed and carries on joins inside this statement, so two tables answering to one
    // identifier there would render one implicit alias.
    let lookup_tables =
        StatementTables::parse(remote_path.clone(), lookup_joins(resolution, model.source())).map_err(|ambiguous| {
            RefusalReason::PlanTablesShareAnIdentifier {
                table: ambiguous.alias().clone(),
            }
        })?;
    let lookup = sutura_domain::plan::LegPlan::Lookup {
        source: remote_source.clone(),
        table: lookup_tables,
        keys: lookup_keys,
        bindings: lookup_bindings,
    };

    let second_fact = second_fact_leg(resolution, first_join, &bucket, second_terms)?;
    let plan = FederatedPlan::new(
        metric.name().clone(),
        ResultLabel::measure(metric.name()),
        bucket,
        fact,
        second_fact,
        lookup,
        include_unmatched,
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
    match (LinkKind::declared(fact), LinkKind::declared(lookup)) {
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
/// the lookup leg projects under the link label; without one the two facts share no dimension and
/// the ratio is refused by name rather than linked on a column borrowed from the first fact's key.
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
    let lookup_column = crossing.relationship.keys().first().target();
    let Some(link) = cross.second_relationships.iter().find(|relationship| {
        relationship.target_model() == crossing.model.name()
            && relationship.keys().len() == 1
            && relationship.keys().first().target() == lookup_column
    }) else {
        return Err(PlanError::Refused(RefusalReason::CrossModelRatioWithoutSharedDimension {
            metric: resolution.metric.name().clone(),
            model: second.name().clone(),
        }));
    };
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
        keys: vec![PlanKey::new(
            ResultLabel::internal(InternalLabel::Link),
            PlanColumn::new(second.table_name().clone(), link.keys().first().origin().clone()),
        )],
        terms: second_terms,
        bindings: super::range_only(resolution.range, bucket.column())?,
        range: resolution.range,
    }))
}
