//! `docs/adr/0013`'s raw SQL tool: the port-facing execution path, split out of `lib.rs` because
//! that file hit the thousand-line limit `cargo xtask max-lines` enforces.
//!
//! One function beside the types it needs: [`run_sql`] mirrors [`crate::answer`]'s credential
//! handling exactly and differs only on the way out, where every execution failure becomes a
//! refusal rather than a [`RunSqlError`] - see that function's own documentation for why.

use sutura_domain::identity::{
    Agreed, BoundToTheRequest, CredentialBroker, Expiry, PresentedDisagreesWithPosture, RequestContext, SourceSet,
};
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse;

use crate::warehouses::Warehouses;
use crate::{exceeds_row_cap, now_in_unix_seconds};

/// Why running a raw statement did not produce an outcome.
///
/// **Deliberately not [`ServiceError`](crate::ServiceError).** That type's `Compile` and
/// `Federated` arms describe the compiler and the splitter, neither of which this path touches - a
/// raw statement is unparsed text, end to end. What is left is the credential half
/// [`answer`](crate::answer) also has, plus one arm of its own for a state the boot refusal is
/// supposed to make unreachable: the raw tool turned on over an adapter that does not accept raw
/// text at all.
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
    /// No data system is registered under the raw tool's configured source, or the one registered
    /// does not declare [`Warehouse::ACCEPTS_RAW_STATEMENTS`].
    ///
    /// **A wiring defect, not a caller-facing refusal.** `sutura_config`'s boot refusal is what is
    /// supposed to make this unreachable in a running deployment - the raw tool is refused at
    /// startup over an adapter that cannot honour it - so reaching this arm at all means the
    /// composition root and the boot check disagreed about what this build links.
    #[error("the raw SQL tool is enabled, and no configured data system accepts a raw statement")]
    NoAcceptingSource,
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
    const fn declined_before_minting(outcome: sutura_domain::raw::RawOutcome) -> Self {
        Self {
            outcome,
            executed_until: None,
        }
    }

    const fn under(credentials: &BoundToTheRequest, outcome: sutura_domain::raw::RawOutcome) -> Self {
        Self {
            outcome,
            executed_until: Some(credentials.not_after()),
        }
    }

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

/// Runs one literal statement against the deployment's configured source, or says why it will not.
///
/// # PR1's scope, stated as a limit rather than left implicit
///
/// **This targets the sole registered data system, and refuses `RunSqlError::NoAcceptingSource`
/// where more than one is open or none is.** `docs/adr/0013`'s showcase is one Postgres source; a
/// deployment naming which of several sources the raw tool may run over is future work, not a
/// decision this function makes by omission - a second source is refused rather than guessed at.
///
/// # Otherwise, this mirrors [`answer`](crate::answer)'s credential handling exactly
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
pub fn run_sql<W, B>(
    context: &RequestContext,
    statement: &sutura_domain::raw::RawStatement,
    broker: &B,
    warehouses: &Warehouses<W>,
) -> RunningRaw<B>
where
    W: Warehouse,
    B: CredentialBroker,
{
    use sutura_domain::raw::{RawOutcome, RawRefusalReason};

    let Some((source, warehouse)) = single_raw_capable_warehouse(warehouses) else {
        return Err(RunSqlError::NoAcceptingSource);
    };
    let requested = SourceSet::of(source.clone());
    let minted = broker
        .mint(context, &requested)
        .map_err(|cause| RunSqlError::Broker { cause })?;
    let credentials = match minted
        .agreeing_with(context.chain().subject(), &requested, now_in_unix_seconds())
        .map_err(|cause| RunSqlError::Credentials { cause })?
    {
        // No credential for this subject at this source. Unreachable through the only adapter wired
        // for this path today - `PostgresWarehouse` runs under one static, shared credential every
        // subject shares - and there is deliberately no `RawRefusalReason` variant for it yet: the
        // vocabulary is closed over what this PR's one adapter can provoke, and a future
        // per-subject-credential raw adapter is what would earn a dedicated one.
        Agreed::Refused { .. } => {
            return Ok(AnsweredRaw::declined_before_minting(RawOutcome::Refusal {
                reason: RawRefusalReason::SourceRefused,
            }));
        }
        Agreed::Granted { credentials } => credentials,
    };
    let presented = credentials
        .presented_for(source)
        .map_err(|cause| RunSqlError::Credentials { cause })?;
    presented
        .agrees_with(warehouse.posture(), source)
        .map_err(|cause| RunSqlError::Posture { cause })?;
    let Some(executed) = warehouse.execute_raw(statement, presented) else {
        return Err(RunSqlError::NoAcceptingSource);
    };
    let raw_rows = match executed {
        Ok(raw_rows) => raw_rows,
        Err(cause) => {
            let reason = if warehouse.result_did_not_fit(&cause) {
                RawRefusalReason::ResultTooLarge
            } else if warehouse.source_refused(&cause) {
                RawRefusalReason::SourceRefused
            } else {
                RawRefusalReason::StatementFailed
            };
            return Ok(AnsweredRaw::under(&credentials, RawOutcome::Refusal { reason }));
        }
    };
    if exceeds_row_cap(raw_rows.rows().len(), sutura_domain::plan::MAX_ROWS) {
        return Ok(AnsweredRaw::under(
            &credentials,
            RawOutcome::Refusal {
                reason: RawRefusalReason::TooManyRows {
                    limit: sutura_domain::plan::MAX_ROWS,
                },
            },
        ));
    }
    let (columns, values) = raw_rows.into_parts();
    let rows = values
        .iter()
        .map(|row| row.iter().map(sutura_domain::warehouse::Value::render).collect())
        .collect();
    Ok(AnsweredRaw::under(&credentials, RawOutcome::Rows { columns, rows }))
}

/// The one data system this deployment opened that can accept raw text, if there is exactly one open
/// at all.
///
/// **`None` for zero OR for more than one**, deliberately: a deployment with two open sources has no
/// way, in this PR, to say which one the raw tool means, and guessing would run a caller's statement
/// against a source they did not name.
fn single_raw_capable_warehouse<W>(warehouses: &Warehouses<W>) -> Option<(&SourceName, &W)>
where
    W: Warehouse,
{
    if warehouses.count() != 1 {
        return None;
    }
    warehouses.each().find(|_| W::ACCEPTS_RAW_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};

    use super::run_sql;
    use crate::Warehouses;
    use crate::tests::{asked_by_a_person, shared, source};
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
        )
        .expect("crediting succeeds and the row count is exactly the cap");
        let RawOutcome::Rows { ref rows, .. } = *answered.outcome() else {
            panic!("a result at the cap must be an answer: {:?}", answered.outcome());
        };
        assert_eq!(rows.len(), at_cap);
    }
}
