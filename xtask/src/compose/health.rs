//! The readiness gate: poll the health report until every expected service is ready.
//!
//! **Its own module because the loop needed a seam.** The deadline used to be read only AFTER
//! `docker compose ps` returned, so a daemon that wedged *after* the pre-flight passed hung
//! `just dev-up` forever with `SUTURA_DEV_READY_TIMEOUT_SECS` never consulted - a loop around an
//! unbounded call is not a bound, whatever its own deadline says. The call is bounded now by the
//! query budget in `docker::bounded`, and the loop takes its poll as an argument, so what it does
//! when the runtime STOPS ANSWERING is a unit test rather than a wedged host.

use std::path::Path;
use std::time::{Duration, Instant};

use sutura_dev::scope::Scope;

use super::docker;
use super::docker::budget_from_env;
use crate::Verdict;

/// How long to wait for every service to report healthy, unless overridden.
///
/// **This is a DEADLINE and not a sleep, which is what makes a generous value cheap.** The gate
/// returns the moment every expected service reports healthy, so raising this costs nothing on a
/// tier that comes up and only changes how long a BROKEN one takes to say so. It was 180, which was
/// the whole budget for one alpine container; the `DataHub` stack spends more than that before GMS
/// is asked its first question - three stores to become healthy, then a migration job that creates
/// the topics, the schema and the indices and has to EXIT, then a JVM with a 45-second start period.
/// A budget that expired mid-migration would report the platform as never ready when it was still
/// arriving, which is the failure mode a reader trusts least.
const READY_TIMEOUT_SECS: u64 = 900;

/// How often to ask. Not a readiness mechanism: the GATE is the health report, and this is only how
/// often it is read. A fixed sleep instead of a gate is what produces a connection refused inside a
/// test, attributed to whatever the test happened to be doing.
const POLL_INTERVAL_MILLIS: u64 = 500;

/// Six hours. A tier that has not become healthy by then is broken rather than slow, and a
/// deadline settable past it would restore the indefinite wait this module exists to remove.
const READY_TIMEOUT_MAX_SECS: u64 = 21_600;

/// The readiness deadline on this host.
///
/// Read through the docker tier's own budget reader rather than a second copy of the same five
/// lines, which is where the clamp comes from too: this was the one budget in the tier with no
/// floor, so a `0` degraded the gate to a single poll.
fn deadline() -> Duration {
    budget_from_env("SUTURA_DEV_READY_TIMEOUT_SECS", READY_TIMEOUT_SECS, READY_TIMEOUT_MAX_SECS)
}

/// What a reader does about a readiness gate that failed with the tier already running.
///
/// A value rather than two `eprintln!`s, for the reason `super::abandoned` is one: it is a decision
/// about what a failed run leaves behind, and a decision that only exists as output is a decision
/// no test can read. Two lines and not three - this run did not kill a provisioning call, so
/// `abandoned`'s "a timeout is not proof they are wrong" is about a different failure.
///
/// **Why it is not `Failed::left_running`'s answer:** that one answers for the CALL, and a status
/// query starts nothing. What started containers here is the RUN, which only the caller that
/// issued the provision knows about - so it is said once, at this gate's failure exit.
fn tier_is_up(project: &str) -> [String; 2] {
    [
        format!("the containers are already up - `docker compose --project-name {project} ps` says what state they are in"),
        String::from(super::teardown::REMOVES_THIS_WORKTREE),
    ]
}

/// Poll the health report until every expected service is ready, or the deadline passes.
///
/// A health GATE and not a sleep. The distinction is what happens when it is wrong: a sleep that was
/// too short surfaces as a connection refused inside somebody's test; this says which service never
/// became healthy and stops.
pub(crate) fn wait_until_healthy(root: &Path, scope: &Scope, profiles: &[&str], expected: &[&str]) -> Result<(), Verdict> {
    let project = scope.project();
    poll_until_ready(expected, deadline(), &project, || {
        docker::compose(root, &project, profiles, &["ps", "--all", "--format", "json"])
    })
    // On the FUNCTION's failure exit and not on one arm of the loop, because "a provision came
    // first" is a precondition of this gate rather than a property of how it failed: `up --detach`
    // returned ok above every one of the four ways below. Printed on the silent-runtime arm only,
    // it missed the arm most likely to be reached - the deadline expiring - which also leaves the
    // tier up.
    .inspect_err(|_| {
        for line in tier_is_up(&project) {
            eprintln!("  {line}");
        }
    })
}

/// The loop, over an injected poll.
///
/// **The poll is a parameter because the case that mattered was otherwise untestable.** With
/// `docker::compose` called inline, the only way to exercise *the runtime stopped answering* was a
/// wedged daemon - which is exactly why the defect survived a bounded pre-flight: no test could be
/// red without one.
fn poll_until_ready(
    expected: &[&str],
    deadline: Duration,
    project: &str,
    mut poll: impl FnMut() -> Result<docker::Output, docker::Failed>,
) -> Result<(), Verdict> {
    let started = Instant::now();
    loop {
        let reported = match poll() {
            Ok(out) if out.ok => docker::parse_ps(&out.stdout),
            Ok(out) => {
                eprintln!("xtask dev-up: `docker compose ps` failed");
                eprint!("{}", out.stderr);
                return Err(Verdict::Fail);
            }
            // A poll that produced no answer FAILS the gate rather than taking another lap. Asking
            // a runtime that has stopped answering again until the readiness deadline expires would
            // spend the whole deadline and then report the SERVICES as never ready, which sends a
            // reader to the containers when the fault is the daemon.
            Err(cause) => {
                eprintln!("xtask dev-up: {cause}");
                return Err(Verdict::Fail);
            }
        };

        match docker::readiness(&reported, expected) {
            docker::Readiness::Ready => return Ok(()),
            docker::Readiness::Failed(why) => {
                eprintln!("xtask dev-up: a service will not become ready:");
                for line in &why {
                    eprintln!("  {line}");
                }
                eprintln!("  `docker compose --project-name {project} logs` has the detail.");
                return Err(Verdict::Fail);
            }
            // The deadline is checked HERE rather than after the match, because this is the only
            // arm that does not return - and the names it carries are what a timeout has to print,
            // so keeping them in scope is what makes the report say which service never arrived.
            docker::Readiness::Waiting(why) => {
                if started.elapsed() >= deadline {
                    eprintln!("xtask dev-up: {} service(s) never became ready:", why.len());
                    for line in &why {
                        eprintln!("  {line}");
                    }
                    eprintln!("  waited {}s (SUTURA_DEV_READY_TIMEOUT_SECS)", deadline.as_secs());
                    return Err(Verdict::Fail);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::super::docker::TIMEOUT_MIN_SECS;
    use super::{READY_TIMEOUT_SECS, poll_until_ready};
    use crate::Verdict;
    use crate::compose::docker::{Budget, Failed, Output};

    /// One `ps` answer, in the shape the pure parser reads.
    fn answering(rows: &str) -> Output {
        Output {
            stdout: String::from(rows),
            stderr: String::new(),
            ok: true,
        }
    }

    /// A runtime that accepted the call and returned nothing inside its budget.
    fn silent() -> Result<Output, Failed> {
        Err(Failed::Silent(Budget::of(&["ps", "--all", "--format", "json"])))
    }

    #[test]
    fn a_daemon_that_wedges_after_the_preflight_fails_the_readiness_gate_rather_than_hanging_it() {
        // The defect this module's header describes, pinned at the loop rather than at the process:
        // a poll that produced no answer must fail the gate, not take another lap.
        let mut polls = 0_u32;
        let started = Instant::now();
        let verdict = poll_until_ready(
            &["postgres"],
            Duration::from_secs(READY_TIMEOUT_SECS),
            "sutura-dev-aaaa1111",
            || {
                polls += 1;
                silent()
            },
        );

        assert_eq!(verdict, Err(Verdict::Fail));
        // Handed the REAL deadline, so these two are the assertions that carry the property: a loop
        // that merely terminates eventually satisfies neither.
        assert_eq!(polls, 1, "a silent runtime must not be asked again");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the gate consumed its deadline instead of failing: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_service_that_never_arrives_still_spends_the_deadline_and_stops() {
        // The other direction, and the one the deadline exists for: a container that is coming up
        // and never finishes. A zero deadline puts the timeout branch first, so this says the loop
        // still STOPS - a gate that only failed on a silent runtime would wait forever here.
        let mut polls = 0_u32;
        let verdict = poll_until_ready(&["postgres"], Duration::ZERO, "sutura-dev-aaaa1111", || {
            polls += 1;
            Ok(answering(r#"[{"Service":"postgres","State":"running","Health":"starting"}]"#))
        });

        assert_eq!(verdict, Err(Verdict::Fail));
        assert_eq!(polls, 1);
    }

    #[test]
    fn a_readiness_gate_that_gave_up_says_the_tier_is_still_up() {
        // `up --detach` returned ok before this gate ran, so the containers exist whatever the gate
        // then decided - and the reader was being handed a wedged-daemon remedy and nothing else.
        // Read off the value rather than off the comment, the way `abandoned`'s report is.
        let report = super::tier_is_up("sutura-dev-aaaa1111");
        assert!(
            report.iter().any(|line| line.contains("sutura-dev-aaaa1111")),
            "the report must name the project whose containers are up: {report:?}"
        );
        assert!(
            report.iter().any(|line| line.contains("just dev-down")),
            "the report must name the task that removes them: {report:?}"
        );
    }

    #[test]
    fn the_readiness_deadline_is_clamped_at_both_ends() {
        // Read through the public path, the way the probe budget's own test is: this was the one
        // budget in the tier with no floor, so `SUTURA_DEV_READY_TIMEOUT_SECS=0` degraded the gate
        // to a single poll. Bounds and not a value, because asserting the DEFAULT here would read
        // the host environment - which is exactly how the budget-contrast assertion went red on a
        // configuration the change itself invites.
        let deadline = super::deadline();
        assert!(deadline >= Duration::from_secs(TIMEOUT_MIN_SECS), "{deadline:?}");
        assert!(deadline <= Duration::from_secs(super::READY_TIMEOUT_MAX_SECS), "{deadline:?}");
    }

    #[test]
    fn a_healthy_tier_passes_on_the_first_poll() {
        // The ordinary case, asserted beside the two failures: a gate that failed on everything
        // would satisfy both of those and be useless.
        let verdict = poll_until_ready(
            &["postgres"],
            Duration::from_secs(READY_TIMEOUT_SECS),
            "sutura-dev-aaaa1111",
            || Ok(answering(r#"[{"Service":"postgres","State":"running","Health":"healthy"}]"#)),
        );
        assert_eq!(verdict, Ok(()));
    }
}
