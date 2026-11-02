//! `docs/adr/0013`'s raw SQL tool: the port-facing execution path, split out of `lib.rs` because
//! that file hit the thousand-line limit `cargo xtask max-lines` enforces.
//!
//! [`run_sql`] passes a statement through to the one open source. `answer_sql` is the tool a
//! surface serves: the combiner plans the statement over the tables of the sources `tools.run_sql`
//! lists, each source's part a leg through [`Warehouse::execute_raw`], and the statement passes
//! through only where the combiner plans none. Every execution failure becomes a refusal rather
//! than a [`RunSqlError`] - see [`run_sql`] for why.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use sutura_domain::identity::{
    Agreed, BoundToTheRequest, CredentialBroker, Expiry, PresentedDisagreesWithPosture, RequestContext, SourceSet,
};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{FederationCombiner, MAX_ROWS, StatementLegs, StatementQuestion, StatementStopped, StatementTable};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement, RunSqlSources};
use sutura_domain::source::ExecutedAs;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{RawRows, Value, Warehouse};

use crate::warehouses::Warehouses;
use crate::{Validated, exceeds_row_cap, now_in_unix_seconds, scoped_for};

/// Why running a raw statement did not produce an outcome.
///
/// **Deliberately not [`ServiceError`](crate::ServiceError).** That type's `Compile` and
/// `Federated` arms describe the compiler and the splitter, neither of which this path touches - a
/// raw statement is the caller's text, end to end. What is left is the credential half
/// [`answer`](crate::answer) also has, plus two wiring defects of its own.
#[derive(Debug, thiserror::Error)]
pub enum RunSqlError<M> {
    /// The credential broker did not answer.
    #[error("the credential broker did not answer")]
    Broker {
        #[source]
        cause: M,
    },
    /// The broker's grant does not fit this request.
    #[error("the credentials that came back do not fit this request")]
    Credentials {
        #[source]
        cause: sutura_domain::identity::CredentialsDoNotFitTheRequest,
    },
    /// The presented leg disagrees with how this source was declared.
    #[error("the credentials that came back do not fit this request")]
    Posture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
    /// A source's adapter took no raw statement: it declares it accepts none, or it declared
    /// [`Warehouse::accepts_raw_statements`] and then answered none.
    ///
    /// **A wiring defect, not a caller-facing refusal.** The composition root refuses at startup a
    /// `tools.run_sql.sources` alias whose adapter accepts no raw statement, so reaching this arm
    /// means a library caller skipped that, or an adapter contradicts its own declaration.
    #[error("a data system the raw tool runs over answered no raw statement")]
    NoAcceptingSource,
    /// The tool's sources are not exactly one, and the combiner planned the statement over none.
    ///
    /// **A wiring defect.** A build without the `sql` feature refuses more than one listed source at
    /// startup, and the combiner of a build with it plans every statement.
    #[error("the raw tool runs over more than one source and nothing planned the statement across them")]
    Unplanned,
    /// The combiner planned a leg at a source `tools.run_sql.sources` does not list. Refused before
    /// anything is minted for it.
    ///
    /// **A wiring defect.** The combiner is handed only the tables of listed sources.
    #[error("the raw tool planned a leg at a source it does not run over")]
    Unlisted,
}

/// What running a raw statement produced, or why it could not.
pub type RunningRaw<B> = Result<AnsweredRaw, RunSqlError<<B as CredentialBroker>::Error>>;

/// One raw call's result: what the caller is told, and what it ran under - the
/// [`Answered`](crate::Answered) of the raw path, over [`sutura_domain::raw::RawOutcome`] rather
/// than [`ToolOutcome`](sutura_domain::query::ToolOutcome).
#[derive(Debug)]
pub struct AnsweredRaw {
    outcome: sutura_domain::raw::RawOutcome,
    executed_until: Option<Expiry>,
}

impl AnsweredRaw {
    #[inline]
    #[must_use]
    pub const fn outcome(&self) -> &sutura_domain::raw::RawOutcome {
        &self.outcome
    }

    #[inline]
    #[must_use]
    pub fn into_outcome(self) -> sutura_domain::raw::RawOutcome {
        self.outcome
    }

    #[inline]
    #[must_use]
    pub const fn executed_until(&self) -> Option<Expiry> {
        self.executed_until
    }
}

/// Which sources the raw tool runs over, and whether one statement may read more than one of them:
/// `tools.run_sql.sources` and `tools.run_sql.federation_allowed`.
#[derive(Debug, Clone)]
pub struct RunSqlScope {
    /// `None` is every open source, the scope a surface runs until a composition root lists one.
    sources: Option<RunSqlSources>,
    federation_allowed: bool,
}

impl RunSqlScope {
    /// Every open source, federating none.
    pub(crate) const EVERY_OPEN_SOURCE: Self = Self {
        sources: None,
        federation_allowed: false,
    };

    /// The tool over `sources`, planning a statement across them only where `federation_allowed`.
    #[must_use]
    pub const fn new(sources: RunSqlSources, federation_allowed: bool) -> Self {
        Self {
            sources: Some(sources),
            federation_allowed,
        }
    }
}

/// Passes one literal statement through to the deployment's one open source, or says why it will not.
///
/// More than one open source is [`RunSqlError::Unplanned`], never narrowed: which of several sources
/// the tool runs over is `tools.run_sql.sources`, which a surface reads, and a statement planned
/// across them is the combiner's.
///
/// # This mirrors [`answer`](crate::answer)'s credential handling exactly
///
/// Mint once, check the grant agrees with the request, check the presented leg agrees with the
/// adapter's declared posture - the same three findings behind the same one guard, for the same
/// reason: a broker is an adapter outside the hexagon, and its answer is input.
///
/// # What is different from [`answer`](crate::answer) on the way out, and why
///
/// **Every failure to execute becomes a refusal, never a [`RunSqlError`].** The statement is the
/// caller's own text, so a syntax error, a statement timeout, or the server refusing a write inside
/// the read-only transaction `docs/adr/0013`'s amendment wraps every call in are all answers *about
/// that statement* - not an infrastructure outage this deployment must page for. What remains an
/// `Err` is only what happens before the statement ever reaches the data system: the broker not
/// answering, or credentials that do not fit.
///
/// # The deadline is opened by the transport, before this call - `crate::answer`'s own shape
///
/// `deadline` is checked before each leg reaches the port (`telekom/sutura#1144`): a budget already
/// spent refuses the statement before it reaches the data system at all, the same pre-call check
/// `crate::answer` makes before `Warehouse::execute`. What an adapter does with the deadline once it
/// is handed to [`Warehouse::execute_raw`] is the adapter's own business.
pub fn run_sql<W, B>(
    context: &RequestContext,
    statement: &RawStatement,
    broker: &B,
    warehouses: &Warehouses<W>,
    deadline: Deadline,
) -> RunningRaw<B>
where
    W: Warehouse,
    B: CredentialBroker,
{
    let Some(source) = only(None, warehouses) else {
        return Err(RunSqlError::Unplanned);
    };
    let mut legs = RawLegs::new(context, broker, warehouses, None, deadline);
    let outcome = pass_through(&mut legs, statement, source)?;
    Ok(legs.answered(outcome))
}

/// The raw tool a surface serves: `statement` planned by `combiner` over the tables of the sources
/// `scope` lists that this caller's view holds, or passed through to the one source `scope` lists
/// where the combiner plans none.
///
/// `executed_as` names each source a planned answer read only where `scope.federation_allowed`;
/// with it off a planned answer read one source, and the outcome carries none, as a passed-through
/// one does.
pub(crate) fn answer_sql<W, B, C>(
    pinned: &Validated<PinnedDefinitions>,
    context: &RequestContext,
    statement: &RawStatement,
    broker: &B,
    warehouses: &Warehouses<W>,
    combiner: &C,
    scope: &RunSqlScope,
    working_set_bytes: u64,
    deadline: Deadline,
) -> RunningRaw<B>
where
    W: Warehouse,
    B: CredentialBroker,
    C: FederationCombiner,
{
    let listed = scope.sources.as_ref().map(RunSqlSources::as_slice);
    let tables = tables(pinned, context, warehouses, listed);
    let question = StatementQuestion::new(statement, &tables, scope.federation_allowed, working_set_bytes, deadline);
    let mut legs = RawLegs::new(context, broker, warehouses, listed, deadline);
    let outcome = match combiner.answer_statement(question, &mut legs) {
        Some(Ok(rows)) => Ok(answer(rows, legs.executed_as.take().filter(|_| scope.federation_allowed))),
        Some(Err(StatementStopped::Leg(stop))) => settled(Err(stop)),
        Some(Err(StatementStopped::SpansMoreThanOneSource)) => Ok(refusal(RawRefusalReason::PlanSpansMoreThanOneSource)),
        Some(Err(StatementStopped::NotPlanned)) => Ok(refusal(RawRefusalReason::StatementFailed)),
        Some(Err(StatementStopped::AnswerTooLarge)) => Ok(refusal(RawRefusalReason::ResultTooLarge)),
        Some(Err(StatementStopped::DeadlineExceeded)) => Ok(refusal(RawRefusalReason::DeadlineExceeded {
            budget_seconds: deadline.budget().seconds(),
        })),
        None => only(listed, warehouses).map_or_else(
            || Err(RunSqlError::Unplanned),
            |source| pass_through(&mut legs, statement, source),
        ),
    }?;
    Ok(legs.answered(outcome))
}

/// The one source `listed` names, or with no list the one open source.
fn only<'a, W>(listed: Option<&'a [SourceName]>, warehouses: &'a Warehouses<W>) -> Option<&'a SourceName>
where
    W: Warehouse,
{
    match listed {
        Some([source]) => Some(source),
        Some(_) => None,
        None => {
            let mut open = warehouses.each();
            match (open.next(), open.next()) {
                (Some((source, _)), None) => Some(source),
                _ => None,
            }
        }
    }
}

/// Every table this caller may read: each model its scoped view lists on a listed source - with no
/// list, on an open one - with the columns the pinned schema set holds for it. A model with a column
/// of no mapped type is no table, and neither is one at a source with no dialect.
fn tables<W>(
    pinned: &Validated<PinnedDefinitions>,
    context: &RequestContext,
    warehouses: &Warehouses<W>,
    listed: Option<&[SourceName]>,
) -> Vec<StatementTable>
where
    W: Warehouse,
{
    let schemas = pinned.schemas();
    scoped_for(pinned.get(), context)
        .models()
        .filter(|model| {
            listed.is_none_or(|listed| listed.contains(model.source())) && schemas.fell_back(model.name()).next().is_none()
        })
        .filter_map(|model| {
            let dialect = warehouses.get(model.source())?.type_dialect()?;
            let schema = schemas.of(model.name())?;
            Some(StatementTable::new(
                model.source().clone(),
                model.table().clone(),
                dialect,
                Arc::clone(schema),
            ))
        })
        .collect()
}

/// `statement` through to `source` as written, as one leg.
fn pass_through<W, B>(legs: &mut RawLegs<'_, W, B>, statement: &RawStatement, source: &SourceName) -> Settled<B::Error>
where
    W: Warehouse,
    B: CredentialBroker,
{
    // Before the broker is asked: an adapter that takes no raw text has no credential to be minted.
    if !legs.warehouses.get(source).is_some_and(W::accepts_raw_statements) {
        return Err(RunSqlError::NoAcceptingSource);
    }
    let ran = legs
        .open(&BTreeSet::from([source.clone()]))
        .and_then(|()| legs.run(source, statement));
    settled(ran.map(|rows| answer(rows, None)))
}

/// An outcome the caller reads, or a failure that is not the statement's.
type Settled<M> = Result<RawOutcome, RunSqlError<M>>;

/// Why a leg ended the call: a refusal the caller reads, or a failure that is not the statement's.
#[derive(Debug)]
pub(crate) enum LegStop<M> {
    Refused(RawRefusalReason),
    Failed(RunSqlError<M>),
}

/// A leg's refusal is the caller's answer; any other stop is this call's failure.
fn settled<M>(ran: Result<RawOutcome, LegStop<M>>) -> Settled<M> {
    match ran {
        Ok(outcome) => Ok(outcome),
        Err(LegStop::Refused(reason)) => Ok(refusal(reason)),
        Err(LegStop::Failed(failure)) => Err(failure),
    }
}

const fn refusal(reason: RawRefusalReason) -> RawOutcome {
    RawOutcome::Refusal { reason }
}

/// Rows as the caller reads them, or `TooManyRows` over the cap.
fn answer(rows: RawRows, executed_as: Option<ExecutedAs>) -> RawOutcome {
    if exceeds_row_cap(rows.rows().len(), MAX_ROWS) {
        return refusal(RawRefusalReason::TooManyRows { limit: MAX_ROWS });
    }
    let (columns, values) = rows.into_parts();
    let rows = values.iter().map(|row| row.iter().map(Value::render).collect()).collect();
    RawOutcome::Rows {
        columns,
        rows,
        executed_as,
    }
}

/// The legs of one statement, run through the raw port under one mint.
pub(crate) struct RawLegs<'a, W, B> {
    context: &'a RequestContext,
    broker: &'a B,
    warehouses: &'a Warehouses<W>,
    /// `None` lists every open source.
    listed: Option<&'a [SourceName]>,
    deadline: Deadline,
    credentials: Option<BoundToTheRequest>,
    executed_as: Option<ExecutedAs>,
}

impl<'a, W, B> RawLegs<'a, W, B> {
    const fn new(
        context: &'a RequestContext,
        broker: &'a B,
        warehouses: &'a Warehouses<W>,
        listed: Option<&'a [SourceName]>,
        deadline: Deadline,
    ) -> Self {
        Self {
            context,
            broker,
            warehouses,
            listed,
            deadline,
            credentials: None,
            executed_as: None,
        }
    }

    /// `outcome`, with the expiry of the credentials it ran under: `None` where nothing was minted.
    fn answered(&self, outcome: RawOutcome) -> AnsweredRaw {
        AnsweredRaw {
            outcome,
            executed_until: self.credentials.as_ref().map(BoundToTheRequest::not_after),
        }
    }

    /// Whether the tool runs over `source`.
    fn lists(&self, source: &SourceName) -> bool
    where
        W: Warehouse,
    {
        self.listed
            .map_or_else(|| self.warehouses.get(source).is_some(), |listed| listed.contains(source))
    }
}

impl<W, B> StatementLegs for RawLegs<'_, W, B>
where
    W: Warehouse,
    B: CredentialBroker,
{
    type Stop = LegStop<B::Error>;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        if !sources.iter().all(|source| self.lists(source)) {
            return Err(LegStop::Failed(RunSqlError::Unlisted));
        }
        let mut each = sources.iter().cloned();
        let Some(first) = each.next() else {
            return Ok(());
        };
        let requested = each.fold(SourceSet::of(first), SourceSet::and);
        let minted = self
            .broker
            .mint(self.context, &requested)
            .map_err(|cause| LegStop::Failed(RunSqlError::Broker { cause }))?;
        let credentials = match minted
            .agreeing_with(self.context.chain().subject(), &requested, now_in_unix_seconds())
            .map_err(|cause| LegStop::Failed(RunSqlError::Credentials { cause }))?
        {
            // No credential for this subject at a source, or an address outside its
            // `address_domains`: the raw vocabulary has one variant for both.
            Agreed::Refused { .. } | Agreed::AddressOutsideDomains { .. } => {
                return Err(LegStop::Refused(RawRefusalReason::SourceRefused));
            }
            Agreed::Granted { credentials } => credentials,
        };
        self.credentials = Some(credentials);
        self.executed_as = executed_as(self.warehouses, sources);
        Ok(())
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        if !self.lists(source) {
            return Err(LegStop::Failed(RunSqlError::Unlisted));
        }
        let (Some(warehouse), Some(credentials)) = (self.warehouses.get(source), self.credentials.as_ref()) else {
            return Err(LegStop::Failed(RunSqlError::Unplanned));
        };
        let presented = credentials
            .presented_for(source)
            .map_err(|cause| LegStop::Failed(RunSqlError::Credentials { cause }))?;
        presented
            .agrees_with(warehouse.posture(), source)
            .map_err(|cause| LegStop::Failed(RunSqlError::Posture { cause }))?;
        let budget_seconds = self.deadline.budget().seconds();
        // Checked before each leg is run and never re-derived - the same shape `crate::answer`'s own
        // pre-`execute` check takes (`docs/adr/0029`): a budget already spent here means the leg
        // is refused before the data system is asked at all.
        if self.deadline.remaining_at(Instant::now()).is_none() {
            return Err(LegStop::Refused(RawRefusalReason::DeadlineExceeded { budget_seconds }));
        }
        let Some(executed) = warehouse.execute_raw(statement, presented, self.deadline) else {
            return Err(LegStop::Failed(RunSqlError::NoAcceptingSource));
        };
        let rows = executed.map_err(|cause| {
            LegStop::Refused(if warehouse.result_did_not_fit(&cause) {
                RawRefusalReason::ResultTooLarge
            } else if warehouse.deadline_exceeded(&cause) {
                RawRefusalReason::DeadlineExceeded { budget_seconds }
            } else if warehouse.source_refused(&cause) {
                RawRefusalReason::SourceRefused
            } else {
                RawRefusalReason::StatementFailed
            })
        })?;
        if exceeds_row_cap(rows.rows().len(), MAX_ROWS) {
            return Err(LegStop::Refused(RawRefusalReason::TooManyRows { limit: MAX_ROWS }));
        }
        Ok(rows)
    }
}

/// One entry for each of `sources`, with the posture its adapter holds. A set holds no source twice,
/// so [`ExecutedAs::and`] refuses none.
fn executed_as<W>(warehouses: &Warehouses<W>, sources: &BTreeSet<SourceName>) -> Option<ExecutedAs>
where
    W: Warehouse,
{
    let mut each = sources
        .iter()
        .filter_map(|source| warehouses.get(source).map(|warehouse| (source, warehouse.posture())));
    let (first, posture) = each.next()?;
    each.try_fold(ExecutedAs::of(first.clone(), posture.clone()), |record, (source, posture)| {
        record.and(source.clone(), posture.clone())
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};

    use super::run_sql;
    use crate::Warehouses;
    use crate::tests::{asked_by_a_person, shared, source, test_deadline};
    use crate::tests_support::{FixedBroker, RawCapableWarehouse};

    fn statement(sql: &str) -> RawStatement {
        RawStatement::parse(sql).expect("a test statement is a statement")
    }

    /// `#666`'s review, finding 4: a wire-serialisation test can construct
    /// `RawRefusalReason::TooManyRows` directly and prove nothing about whether `run_sql` itself
    /// ever produces one. This fake warehouse returns `MAX_ROWS + 1` rows over the real port, so
    /// this is the first test that reaches `crate::exceeds_row_cap` through `run_sql` rather than
    /// through a typed literal - the mutation `crates/sutura-app/src/raw.rs:182`'s own cap ->
    /// `u32::MAX` reddens this test and no other.
    #[test]
    fn a_result_over_the_row_cap_is_refused_rather_than_returned() {
        let over_cap = usize::try_from(sutura_domain::plan::MAX_ROWS).expect("the cap fits a usize") + 1;
        let warehouse = RawCapableWarehouse::answering_rows(source(), shared(), over_cap);
        let warehouses = Warehouses::of(warehouse);
        let answered = run_sql(
            &asked_by_a_person(),
            &statement("select * from a_wide_table"),
            &FixedBroker::GrantsShared,
            &warehouses,
            test_deadline(),
        )
        .expect("crediting succeeds; only the row count refuses this call");
        assert!(
            matches!(
                answered.outcome(),
                RawOutcome::Refusal {
                    reason: RawRefusalReason::TooManyRows { .. }
                }
            ),
            "{:?}",
            answered.outcome()
        );
    }

    /// The row directly under the cap is not refused - the boundary the test above would not catch
    /// on its own.
    #[test]
    fn a_result_at_the_row_cap_is_returned() {
        let at_cap = usize::try_from(sutura_domain::plan::MAX_ROWS).expect("the cap fits a usize");
        let warehouse = RawCapableWarehouse::answering_rows(source(), shared(), at_cap);
        let warehouses = Warehouses::of(warehouse);
        let answered = run_sql(
            &asked_by_a_person(),
            &statement("select * from a_table"),
            &FixedBroker::GrantsShared,
            &warehouses,
            test_deadline(),
        )
        .expect("crediting succeeds and the row count is exactly the cap");
        let RawOutcome::Rows { ref rows, .. } = *answered.outcome() else {
            panic!("a result at the cap must be an answer: {:?}", answered.outcome());
        };
        assert_eq!(rows.len(), at_cap);
    }

    /// `telekom/sutura#1144`: a budget already spent refuses `run_sql` before the port is ever
    /// called - the raw path's own version of `crate::tests::deadline`'s
    /// `a_budget_spent_before_the_leg_starts_is_refused_and_the_data_system_is_never_asked`. The
    /// fake would answer with rows if reached, so a check that quietly disappeared would flip this
    /// outcome to `RawOutcome::Rows` rather than merely fail to assert.
    #[test]
    fn a_budget_already_spent_refuses_run_sql_before_the_port_is_ever_called() {
        use sutura_domain::warehouse::deadline::{Budget, Deadline};

        let warehouse = RawCapableWarehouse::answering_rows(source(), shared(), 1);
        let warehouses = Warehouses::of(warehouse);
        let spent = Deadline::opened_at(
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(10))
                .expect("ten seconds before now does not underflow the monotonic clock"),
            Budget::parse(std::time::Duration::from_secs(1)).expect("one second is a budget"),
        );
        let answered = run_sql(
            &asked_by_a_person(),
            &statement("select 1"),
            &FixedBroker::GrantsShared,
            &warehouses,
            spent,
        )
        .expect("a refusal is an Ok, so a client cannot retry it into an answer");
        let RawOutcome::Refusal {
            reason: RawRefusalReason::DeadlineExceeded { budget_seconds },
        } = *answered.outcome()
        else {
            panic!("a budget spent before the call must refuse, not {:?}", answered.outcome());
        };
        assert_eq!(budget_seconds, spent.budget().seconds());
    }

    /// `telekom/sutura#1144`: an adapter reporting `Warehouse::deadline_exceeded` for its own
    /// `execute_raw` failure maps to `RawRefusalReason::DeadlineExceeded`, never to
    /// `StatementFailed` - the raw path's own version of
    /// `crate::tests::deadline::running_out_of_time_is_a_refusal_and_not_a_503`.
    #[test]
    fn an_adapters_own_deadline_exceeded_maps_to_a_deadline_refusal_not_a_statement_failure() {
        let warehouse = RawCapableWarehouse::answering_rows(source(), shared(), 1);
        let warehouses = Warehouses::of(warehouse);
        let answered = run_sql(
            &asked_by_a_person(),
            &statement("time me out"),
            &FixedBroker::GrantsShared,
            &warehouses,
            test_deadline(),
        )
        .expect("crediting succeeds; only the statement's own execution refuses this call");
        let RawOutcome::Refusal {
            reason: RawRefusalReason::DeadlineExceeded { budget_seconds },
        } = *answered.outcome()
        else {
            panic!(
                "an adapter's own deadline_exceeded must map to DeadlineExceeded, not {:?}",
                answered.outcome()
            );
        };
        assert_eq!(budget_seconds, test_deadline().budget().seconds());
    }
}

/// `answer_sql` over a combiner that runs the statement at every listed source. Declared here rather
/// than in `tests.rs`, so the base run, which restores this file, does not compile it.
#[cfg(test)]
mod legs;
