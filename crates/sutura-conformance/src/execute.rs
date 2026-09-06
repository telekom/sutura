//! The execute pack: what every implementor of the execution port must do with a plan.
//!
//! **Every body here is generic in the port and mentions no adapter.** That is the property
//! `docs/adr/0012` is built on: an assertion that appears twice will disagree with itself, and the
//! disagreement will be read as a difference between two data systems rather than as a difference
//! between two copies of a test. An adapter contributes a constructor, never an assertion.
//!
//! Each function is one behaviour and returns [`Conformed`], so there are exactly three outcomes and
//! a reader can tell them apart: the behaviour HELD, the adapter DECLINED it and said why, or a
//! typed [`Fault`] names what disagreed. [`crate::execute_packs`] gives each one a `#[test]` name
//! per adapter; nothing here panics, so a pack can also be called directly.
//!
//! # What this pack does not reach
//!
//! - **Ordering inside a leg.** A [`sutura_domain::plan::LegPlan`] carries no row limit and
//!   no statement that an order was promised, so [`a_leg_is_executed`] asserts content only -
//!   `sutura_domain::warehouse::agreement`'s own header says a leg comparing against a plan that
//!   claimed no order should not call the order assertion.
//! - **Impersonation, in either direction.** `docs/adr/0012` decides that a declared absence of
//!   impersonation gets no pack: the fallback a negative one would assert as correct is the one
//!   `docs/adr/0008` forbids, and the direction worth worrying about is not observable from this
//!   port at all. The mechanism is the boot refusal, tested over a composition root.
//! - **Which error an adapter refused with.** `Self::Error` is the adapter's own type, so a pack
//!   sees only that a call failed. [`a_leg_is_refused`] is written around that limit rather than
//!   through it - see its own doc.

use sutura_domain::plan::Executable;
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_domain::warehouse::{PreFlight, RowSet, Warehouse};

use crate::corpus::{self, Case};
use crate::{Conformed, Declination, Fault, Outcome};

/// What every comparison in this pack is made at.
///
/// One constant, named from the domain rather than chosen here: a pack with a tolerance of its own
/// would be the second comparison policy `sutura_domain::warehouse::agreement` exists to prevent,
/// and the difference would be read as a difference between two data systems.
const TOLERANCE: RealTolerance = RealTolerance::DIFFERENTIAL;

/// One case's rows, or the fault that stopped them.
///
/// Named rather than spelled out, because the spelled-out form is past `clippy.toml`'s
/// type-complexity threshold and the lint's own instruction is to factor it into a definition.
type Answered<W> = Result<RowSet, Fault<<W as Warehouse>::Error>>;

/// The answer's labels are the ones the plan projects, in the order it projects them.
///
/// A separate behaviour from the two below rather than a consequence of them, because it is a
/// separate diagnosis: `result_labels` is the domain's single statement of what a result carries, so
/// an adapter that renamed or reordered a column has a defect in its projection rather than a wrong
/// number. `agree_on_content` would also catch it and would report it as a content disagreement,
/// which is the less useful of the two readings.
pub fn labels_are_the_plans_own<W>(warehouse: &W) -> Conformed<W::Error>
where
    W: Warehouse,
{
    for case in corpus::cases() {
        let answered = answer(warehouse, &case)?;
        let projected = case.plan().result_labels();
        if answered.columns() != projected {
            return Err(Fault::Labels {
                case: case.name(),
                projected,
                answered: answered.columns().to_vec(),
            });
        }
    }
    Ok(Outcome::Held)
}

/// The rows are the reference's rows, as a multiset.
///
/// THE conformance claim: one plan, one answer, whatever executed it. A multiset rather than a set,
/// because a duplicated row is exactly what a fan-out defect produces.
pub fn content_agrees_with_the_reference<W>(warehouse: &W) -> Conformed<W::Error>
where
    W: Warehouse,
{
    for case in corpus::cases() {
        let answered = answer(warehouse, &case)?;
        agree_on_content(case.expected(), &answered, TOLERANCE).map_err(|disagreement| Fault::Content {
            case: case.name(),
            disagreement: Box::new(disagreement),
        })?;
    }
    Ok(Outcome::Held)
}

/// The rows are in the order the plan's `ORDER BY` claims.
///
/// Separate from [`content_agrees_with_the_reference`] and asked after it, which is the domain
/// policy's own instruction: the first symptom of a wrong number would otherwise be reported as a
/// sort order. Every plan in the corpus groups, so every one of them emits an order to claim.
pub fn order_agrees_with_the_reference<W>(warehouse: &W) -> Conformed<W::Error>
where
    W: Warehouse,
{
    for case in corpus::cases() {
        let answered = answer(warehouse, &case)?;
        agree_on_order(case.expected(), &answered, TOLERANCE).map_err(|disagreement| Fault::Order {
            case: case.name(),
            disagreement: Box::new(disagreement),
        })?;
    }
    Ok(Outcome::Held)
}

/// One plan, asked twice, answered the same way twice.
///
/// The weakest behaviour in the pack and the only one that needs no reference, which is why it is
/// kept: it is what the reference comparison degenerates to for an adapter whose rows are right and
/// whose plan is non-deterministic - an unstable tie order, a cached result that went stale, a
/// connection that reset the session between calls. Both halves are compared, so a stable answer in
/// an unstable order is still a failure.
pub fn one_plan_asked_twice_answers_the_same_way<W>(warehouse: &W) -> Conformed<W::Error>
where
    W: Warehouse,
{
    for case in corpus::cases() {
        let first = answer(warehouse, &case)?;
        let again = answer(warehouse, &case)?;
        agree_on_content(&first, &again, TOLERANCE).map_err(|disagreement| Fault::Content {
            case: case.name(),
            disagreement: Box::new(disagreement),
        })?;
        agree_on_order(&first, &again, TOLERANCE).map_err(|disagreement| Fault::Order {
            case: case.name(),
            disagreement: Box::new(disagreement),
        })?;
    }
    Ok(Outcome::Held)
}

/// A pre-flight that accepted the plan is followed by an answer.
///
/// The port states that the check and the execution take the same
/// [`Executable`] *"so the two cannot disagree about what this adapter accepts"*, and this is that
/// sentence as an assertion. Two directions are faults: a pre-flight that refuses a plan the adapter
/// can execute, and one that accepts a plan the adapter then cannot answer.
///
/// **An adapter that answers [`PreFlight::NotAsked`] DECLINES this behaviour**, and the declination
/// is the honest reading rather than a pass: nothing was checked, so nothing about the check has
/// been established. The limit worth stating next to it - the declination is observed at run time
/// rather than read off a typed declaration, because the port has no capability constant for a
/// pre-flight the way it has one for a leg. Where that constant exists the pack would select on it
/// and a mismatched declaration would not build.
pub fn a_preflight_that_accepts_is_followed_by_an_answer<W>(warehouse: &W) -> Conformed<W::Error>
where
    W: Warehouse,
{
    let mut accepted = 0_usize;
    for case in corpus::cases() {
        let executable = Executable::Query(case.plan());
        let checked = warehouse
            .dry_run(executable, &corpus::presented())
            .map_err(|cause| Fault::PreFlightRefused {
                case: case.name(),
                cause,
            })?;
        match checked {
            PreFlight::NotAsked => {}
            PreFlight::Accepted => {
                accepted = accepted.saturating_add(1);
                warehouse
                    .execute(executable, &corpus::presented())
                    .map_err(|cause| Fault::AcceptedThenDidNotAnswer {
                        case: case.name(),
                        cause,
                    })?;
            }
        }
    }
    if accepted == 0 {
        return Ok(Outcome::Declined(Declination::OffersNoPreFlight));
    }
    Ok(Outcome::Held)
}

/// A leg reaches the answer a whole plan reaches.
///
/// Selected for an adapter that declares `EXECUTES_LEGS`. The leg reads the same table over the same
/// range as one of the whole-plan cases and must answer that case's rows, which is what makes it a
/// conformance claim rather than a smoke test: an adapter with a leg-rendering path of its own has
/// to land on the number the whole-plan path lands on.
///
/// Content only. See this module's header for why a leg gets no order assertion.
pub fn a_leg_is_executed<W>(warehouse: &W) -> Conformed<W::Error>
where
    W: Warehouse,
{
    let case = corpus::leg_case();
    let answered = warehouse
        .execute(Executable::Leg(case.leg()), &corpus::presented())
        .map_err(|cause| Fault::NotAnswered {
            case: case.name(),
            cause,
        })?;
    agree_on_content(case.expected(), &answered, TOLERANCE).map_err(|disagreement| Fault::Content {
        case: case.name(),
        disagreement: Box::new(disagreement),
    })?;
    Ok(Outcome::Held)
}

/// A leg is refused by an adapter that declares it does not execute one.
///
/// Selected for an adapter that leaves `EXECUTES_LEGS` at its default. This is the direction
/// `docs/adr/0012` calls *a declared absence with something to try*: the value of it is that nothing
/// upstream builds a leg today, so this is the only thing that exercises the adapter's own guard -
/// and an adapter that quietly computed one instead would be surfacing half an answer under a
/// certified metric name.
///
/// **A whole plan is executed FIRST, and that is not a warm-up.** A pack cannot see which error an
/// adapter refused with - `Self::Error` is the adapter's own type - so on its own this behaviour
/// would be green for an adapter that failed for any reason at all, including one that cannot reach
/// its data system. Answering a whole plan immediately before is what makes the refusal evidence
/// about the leg. The residual limit: the refusal is still only *an* error, so an adapter that
/// refused a leg for the wrong reason passes.
pub fn a_leg_is_refused<W>(warehouse: &W) -> Conformed<W::Error>
where
    W: Warehouse,
{
    a_leg_is_refused_over(warehouse, &corpus::cases())
}

/// The same behaviour, over cases a caller supplies.
///
/// **A seam, and a narrow one, for the branch above it cannot otherwise reach.**
/// [`crate::Fault::EmptyCorpus`] is what stops this behaviour being green over nothing - the guard
/// [`crate::census`] provides for every other behaviour and the one place it is a `Fault` instead -
/// and with the corpus reached through [`corpus::cases`] alone no fake could empty it, so the
/// variant was unprovokable and the claim *every fault is provoked* was seven of eight.
///
/// It is the beginning of what a file-backed corpus needs anyway: a corpus the pack is handed
/// rather than one it calls. Every other behaviour still reads [`corpus::cases`] directly, so this
/// is one seam and not a parameter threaded through the pack.
pub fn a_leg_is_refused_over<W>(warehouse: &W, cases: &[Case]) -> Conformed<W::Error>
where
    W: Warehouse,
{
    let Some(case) = cases.first() else {
        return Err(Fault::EmptyCorpus);
    };
    drop(answer(warehouse, case)?);
    let leg = corpus::leg_case();
    if let Ok(answered) = warehouse.execute(Executable::Leg(leg.leg()), &corpus::presented()) {
        return Err(Fault::ALegWasAnswered {
            case: leg.name(),
            rows: answered.rows().len(),
        });
    }
    Ok(Outcome::Held)
}

/// One case, executed.
fn answer<W>(warehouse: &W, case: &Case) -> Answered<W>
where
    W: Warehouse,
{
    warehouse
        .execute(Executable::Query(case.plan()), &corpus::presented())
        .map_err(|cause| Fault::NotAnswered {
            case: case.name(),
            cause,
        })
}
