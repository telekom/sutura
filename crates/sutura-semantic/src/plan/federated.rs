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

use sutura_domain::federation::{Carried, Federation};
use sutura_domain::measure::Measure;
use sutura_domain::model::TableName;
use sutura_domain::plan::leg::LegTerm;
use sutura_domain::plan::{
    FederatedPlan, IncoherentBindings, InternalLabel, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanPredicate,
    PlanTerm, PredicateOrigin, ResultLabel, StatementTables, labels,
};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ParamValue;

use super::PlanError;
use super::chain::{chain_joins, column_of, every_remote_dimension, is_remote};
use crate::resolve::{Resolution, ResolvedFilter};

/// Splits a two-source question into a fact leg and a lookup leg.
///
/// The metric's own rows (and any same-source dimension) form the fact leg; everything on the one
/// remote data system forms the lookup leg. The link between them is the relationship's join column,
/// grouped into the fact leg and projected from the lookup leg under the same label - so the combiner
/// can find it. A measure that cannot decompose is refused rather than pulled up.
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

    let federation = Federation::of(closed);
    // The combiner cannot re-count a distinct aggregate, so a measure that needs that is refused.
    if let Some(keys) = federation.carried().iter().find_map(|leaf| match **leaf {
        Carried::Keys { pulled, .. } => Some(pulled.above()),
        _ => None,
    }) {
        return Err(PlanError::Refused(RefusalReason::MeasureDoesNotFederate {
            metric: metric.name().clone(),
            aggregate: keys,
        }));
    }

    // One remote data system (more are refused upstream); its dimensions must all be reached through
    // one first hop, because the combiner links the legs on a single column - and a remote
    // dimension's HOP 1 is what the link is: it is the hop that crosses into the remote system, and
    // from the second hop on a chain is same-source by the consistency check, so a remote chain's
    // hops 2..n would live ON the lookup leg, which the splitter does not plan (see below). The
    // guards below are unreachable - `plan` calls this only for exactly one remote source, and a
    // remote dimension has a chain by construction - but a panic here would be reachable from a
    // catalog plus a question, so they refuse instead.
    //
    // A2: neither guard fabricates a `RefusalReason` any more - see `PlanError::NoRemoteJoin`.
    let Some(first_remote) = every_remote_dimension(resolution).next() else {
        return Err(PlanError::NoRemoteJoin);
    };
    // `.first`: the link into the remote leg is the chain's first hop.
    let Some(first_join) = first_remote.join.as_ref().and_then(|hops| hops.first()) else {
        return Err(PlanError::NoRemoteJoin);
    };
    let relationship = first_join.relationship;
    let remote_path = first_join.model.table();
    let remote_table = first_join.model.table_name();
    let remote_source = first_join.model.source();
    for dim in every_remote_dimension(resolution) {
        let Some(join) = dim.join.as_ref().and_then(|hops| hops.first()) else {
            continue;
        };
        if join.relationship.name() != relationship.name() {
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
    if relationship.keys().as_slice().len() != 1 {
        return Err(PlanError::Refused(RefusalReason::FederationLinkCompound {
            source: remote_source.clone(),
            relationship: relationship.name().clone(),
        }));
    }
    let crossing_key = relationship.keys().as_slice().first().ok_or(PlanError::NoRemoteJoin)?;

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
        PlanColumn::new(own_table.clone(), crossing_key.origin().clone()),
    ));

    // The lookup leg projects the join target plus the remote dimension keys.
    let mut lookup_keys: Vec<PlanKey> = vec![PlanKey::new(
        link_label,
        PlanColumn::new(remote_table.clone(), crossing_key.target().clone()),
    )];
    for key in resolution.keys.iter().filter(|key| is_remote(key, model.source())) {
        lookup_keys.push(PlanKey::new(
            ResultLabel::dimension(key.dimension.name()),
            PlanColumn::new(remote_table.clone(), key.dimension.column().clone()),
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

    let time_column = PlanColumn::new(own_table.clone(), metric.time_column().clone());
    let fact_bindings = super::predicates_and_params(resolution, &local_filters, own_table, &time_column)?;
    let lookup_bindings = requested_for(&remote_filters, remote_table)?;

    // The fact leg's terms, projected under the one labelling rule the combiner reads back - in the
    // same reserved namespace as the link, for the same reason: `metric__{n}` is a legal dimension
    // name too, and over a 63-character metric name it also crossed the identifier limit a data
    // system truncates silently.
    let leaf_labels = labels(&federation);
    let mut terms: Vec<LegTerm> = Vec::with_capacity(leaf_labels.len());
    for (leaf, &label) in federation.carried().iter().zip(leaf_labels.iter()) {
        let plan_term = match **leaf {
            Carried::Aggregated { pushed, ref column, .. } => PlanTerm::Aggregate {
                aggregate: pushed.push(),
                column: PlanColumn::new(own_table.clone(), column.clone()),
            },
            Carried::CountIf { ref column, .. } => PlanTerm::CountIf {
                column: PlanColumn::new(own_table.clone(), column.clone()),
            },
            // Unreachable: the refusal above returned for any Keys leaf.
            Carried::Keys { .. } => {
                return Err(PlanError::Refused(RefusalReason::MeasureDoesNotFederate {
                    metric: metric.name().clone(),
                    aggregate: sutura_domain::model::Aggregate::CountDistinct,
                }));
            }
        };
        terms.push(LegTerm::new(plan_term, ResultLabel::internal(label)));
    }

    // Same-source hops (dimensions on the metric's own system) stay joins on the fact leg. Each
    // chain stops at its crossing hop, which is the link the lookup leg above carries rather than a
    // `JOIN` on this leg's statement - and no hop may follow it, which `plan` refuses before
    // dispatching here. Same builder as the whole-answer path: two copies of "one join per hop,
    // qualified by the previous hop's target" is how one of them came to be qualified by the
    // metric's table instead.
    let joins = chain_joins(resolution, own_table, model.source());

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

    let lookup = sutura_domain::plan::LegPlan::Lookup {
        source: remote_source.clone(),
        table: remote_path.clone(),
        keys: lookup_keys,
        bindings: lookup_bindings,
    };

    let plan = FederatedPlan::new(
        metric.name().clone(),
        ResultLabel::measure(metric.name()),
        bucket,
        fact,
        // No second fact leg from a question: `plan` refuses a cross-model ratio before this
        // splitter runs (`telekom/sutura#780`), because no shape of it renders a second leg yet.
        None,
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

/// The predicates a lookup leg carries: only the caller's own remote filters, bound on the remote
/// table.
fn requested_for(requested: &[&ResolvedFilter<'_>], remote_table: &TableName) -> Result<PlanBindings, IncoherentBindings> {
    let mut params: Vec<ParamValue> = Vec::new();
    let mut filters: Vec<PlanFilter> = Vec::new();
    for filter in requested {
        let bind = params.len();
        params.push(ParamValue::Text(filter.value.clone()));
        filters.push(PlanFilter::new(
            PredicateOrigin::Requested,
            PlanPredicate::Equals {
                column: PlanColumn::new(remote_table.clone(), filter.dimension.dimension.column().clone()),
                param: bind,
            },
        ));
    }
    PlanBindings::parse(filters, params)
}
