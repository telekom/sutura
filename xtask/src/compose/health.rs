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

/// The readiness deadline on this host.
fn deadline() -> Duration {
    Duration::from_secs(
        std::env::var("SUTURA_DEV_READY_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(READY_TIMEOUT_SECS),
    )
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
    // What the last poll was still waiting for. Carried out of the match so a timeout says which
    // service never arrived rather than only that one did not - and declared without a value,
    // because the only path that reads it is the one the `Waiting` arm assigns on.
    let mut last: Vec<String>;

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
            docker::Readiness::Waiting(why) => last = why,
        }

        if started.elapsed() >= deadline {
            eprintln!("xtask dev-up: {} service(s) never became ready:", last.len());
            for line in &last {
                eprintln!("  {line}");
            }
            eprintln!("  waited {}s (SUTURA_DEV_READY_TIMEOUT_SECS)", deadline.as_secs());
            return Err(Verdict::Fail);
        }
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

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
        // The defect, at the loop rather than at the process. The deadline was read only after
        // `docker compose ps` RETURNED, so a `ps` that never returned meant this budget was never
        // consulted and `just dev-up` blocked with no output naming a cause.
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
        // It failed rather than looping, and it did so having been handed the real fifteen-minute
        // deadline: this is the assertion that the gate no longer waits on a runtime that has
        // stopped answering, and it is not satisfied by a loop that merely terminates eventually.
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
    fn a_healthy_tier_passes_on_the_first_poll() {
        // A bound that fails closed on everything is not a bound, it is an outage that still prints
        // a reason - so the ordinary case is asserted beside the two failures.
        let verdict = poll_until_ready(
            &["postgres"],
            Duration::from_secs(READY_TIMEOUT_SECS),
            "sutura-dev-aaaa1111",
            || Ok(answering(r#"[{"Service":"postgres","State":"running","Health":"healthy"}]"#)),
        );
        assert_eq!(verdict, Ok(()));
    }
}
