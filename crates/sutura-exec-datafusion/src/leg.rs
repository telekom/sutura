//! One leg of a federated question, as a logical plan, with no SQL anywhere in it.
//!
//! **The whole-plan path's sibling, and it is a separate module for `sutura_sql::generate_leg`'s
//! reason** - four things differ from a whole answer, and each of them is a shape a flag on the
//! other path could not express:
//!
//! 1. It projects a LIST of term columns rather than one measure expression. That is 0009's
//!    Decision 2 at this layer: a [`LegTerm`](sutura_domain::plan::LegTerm) is a
//!    [`PlanTerm`](sutura_domain::plan::PlanTerm) and there is no shape in it that divides, so
//!    [`translate::measure_expression`](crate::translate::measure_expression) is never called here
//!    and a per-leg quotient is unrepresentable rather than avoided.
//! 2. The bucket and the joins belong to the fact leg alone. A lookup leg reads a table with no
//!    time column, so it groups by its projected keys and that is its whole aggregate.
//! 3. It emits no `LIMIT`. A leg is not an answer: `sutura_domain::plan::MAX_ROWS` caps one
//!    answer's rows, and a cap applied per leg would refuse a question no answer was too large for.
//!    What bounds a leg here is the working-set ceiling in `crate::pool`.
//! 4. The filter is OPTIONAL. A whole plan always carries the two bounds of its range, which is why
//!    [`DataFusionError::NoPredicate`] exists on that path; a lookup leg for a remote dimension
//!    carrying no filter has no predicate at all, and no filter node is the correct plan rather
//!    than an error.
//!
//! **Everything else is shared with the whole-plan path on purpose** - [`column`],
//! [`bucket_expression`], [`term_expression`], [`predicate`] and [`outputs`] - so a change to how a
//! column is referenced, how a grain truncates, how a term aggregates or how a result is projected
//! cannot apply to one path and not the other. That is the same one-definition argument
//! `sutura_sql::generate_leg` makes for the renderer, and it is what makes this path's agreement
//! with the whole-plan path a property of the code rather than of two tests.
//!
//! **What this module is NOT: a combiner.** It runs one source's share and hands back that share's
//! rows. The join and the re-aggregation above the legs are
//! `sutura_domain::plan::FederatedPlan::combine`, a pure domain function, and nothing in this crate
//! is on that path - `docs/adr/0007`'s *the engine is also a data source* is the sentence this
//! module lives under.

use datafusion::common::JoinType as EngineJoin;
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder};
use sutura_domain::model::JoinType;
use sutura_domain::plan::{LegPlan, PlanJoin};

use crate::DataFusionError;
use crate::collect::outputs;
use crate::translate::{bucket_expression, column, predicate, term_expression};

/// The same-source hops this leg keeps as joins of its own.
///
/// **THE arm split, and a lookup leg has none.** A dimension on another data system is a second leg,
/// never a join - which is what makes the empty slice here a statement about federation rather than
/// an omission. Exhaustive, so a third leg shape cannot arrive without saying what it joins.
pub(crate) fn joins(leg: &LegPlan) -> &[PlanJoin] {
    match *leg {
        LegPlan::Fact { ref tables, .. } => tables.joins(),
        LegPlan::Lookup { .. } => &[],
    }
}

/// One leg, as a logical plan: scan, joins, optional filter, aggregate, projection, sort.
///
/// The scans arrive already resolved because a table lookup needs the session and this function
/// needs none - which keeps everything that turns the leg's own vocabulary into nodes synchronous
/// and in one place. `from` is [`LegPlan::table`]'s scan and `joined` is [`joins`]'s, in that order.
pub(crate) fn logical(
    leg: &LegPlan,
    from: LogicalPlan,
    joined: Vec<(&PlanJoin, LogicalPlan)>,
) -> Result<LogicalPlan, DataFusionError> {
    let mut builder = LogicalPlanBuilder::from(from);
    for (join, right) in joined {
        let on = column(join.origin()).eq(column(join.target()));
        // LEFT, and matched exhaustively, for the whole-plan path's reasons unchanged: an INNER join
        // drops every fact row whose same-source dimension row is missing, so a grouped leg totals
        // less than an ungrouped one with nothing raising an error, and a fourth cardinality is a
        // compile error rather than a silently wrong plan.
        builder = match join.join_type() {
            JoinType::OneToOne | JoinType::ManyToOne | JoinType::OneToMany => builder
                .join_on(right, EngineJoin::Left, [on])
                .map_err(|cause| DataFusionError::Build { cause })?,
        };
    }

    // Folded in leg order, which is parameter order - `predicate` resolves each value by the index
    // the leg recorded, so a filter that stopped binding one cannot shift the rest.
    let mut conjuncts = Vec::with_capacity(leg.filters().len());
    for filter in leg.filters() {
        conjuncts.push(predicate(leg.params(), filter.predicate())?);
    }
    let mut remaining = conjuncts.into_iter();
    if let Some(first) = remaining.next() {
        builder = builder
            .filter(remaining.fold(first, Expr::and))
            .map_err(|cause| DataFusionError::Build { cause })?;
    }

    // Keys, then the bucket if this leg has one, which is the order `LegPlan::result_labels` states
    // - so the aggregate's output fields line up with the labels position for position.
    let mut grouping: Vec<Expr> = leg.keys().iter().map(|key| column(key.column())).collect();
    let mut aggregates = Vec::new();
    match *leg {
        LegPlan::Fact {
            ref bucket, ref terms, ..
        } => {
            grouping.push(bucket_expression(bucket.grain(), bucket.column()));
            // Zero to four of them, and EMPTY is not a special case: a fact leg with no terms groups
            // by its key list and projects it, which is the distinct key set an exact
            // `CountDistinct` needs above.
            for term in terms {
                aggregates.push(term_expression(term.term())?);
            }
        }
        // No bucket and no term. It groups by its projected keys, which is the distinct set of
        // dimension rows surviving its own filters.
        LegPlan::Lookup { .. } => {}
    }
    let group_count = grouping.len();
    builder = builder
        .aggregate(grouping, aggregates)
        .map_err(|cause| DataFusionError::Build { cause })?;

    let labels = leg.result_labels();
    let (projection, ordering) = outputs(builder.schema(), &labels, group_count)?;
    // Sorted by what it groups by and NOT limited, which is the fourth difference above. `sort_by`
    // is ascending nulls-last, which is what `generate_leg`'s `ordered_nulls_last` renders - the
    // ordering the combiner above reads two legs back in.
    builder
        .project(projection)
        .and_then(|projected| projected.sort_by(ordering))
        .and_then(LogicalPlanBuilder::build)
        .map_err(|cause| DataFusionError::Build { cause })
}
