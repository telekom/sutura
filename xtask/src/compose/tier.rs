//! Changing the tier, and reading back what it bound.
//!
//! **Its own module because `compose.rs` was seven lines under the thousand-line limit
//! `cargo xtask max-lines` enforces and cannot exempt**, and the seam is the one `changed`'s own doc
//! draws: every function here either ISSUES a compose subcommand that changes the tier, reads back
//! what such a call published, or says what a killed one left behind. What stays in `compose.rs` is
//! the four `just` tasks, the argv they parse, the docker preflight and the endpoint table a person
//! reads - none of which changes anything.
//!
//! The layer below is `docker` (the subprocess and its bounded wait), `health` (readiness) and
//! `teardown` (whether there is anything to remove). This is the layer between those and a task, and
//! the ordering decisions it carries - the discovery file removed before the tier changes, the
//! report that removes nothing - are argued at the function that makes them.

use std::path::{Path, PathBuf};

use sutura_dev::discovery;
use sutura_dev::scope::{SERVICES, Scope};

use super::{docker, health, teardown};
use crate::Verdict;

/// What a service reported as its published address, per service name.
type Published = Vec<(&'static str, String)>;

/// Change the tier with this worktree's discovery file removed FIRST.
///
/// **The order is what makes it hold, and it holds for everything inside the closure.** From the
/// moment provisioning or teardown starts, an endpoint file written by an earlier run is a claim
/// nothing has checked - host ports are ephemeral by design, so a recreated container does not come
/// back on the one it names, and `sutura_dev::provisioned` has no revalidation and no fallback,
/// deliberately. Removing it first means every failure path between here and `discovery::publish`
/// is covered by construction; a `forget` remembered at each early return is covered until somebody
/// adds the next one, which is exactly what happened - `run_up` had none at all, and `run_down`'s
/// killed-`down` arm returned above the one it had.
///
/// The direction that survives is a missing file over running containers: a harness gets
/// `NotProvisioned`, an error somebody reads, rather than a connection to whatever holds that port
/// now - `discovery::forget`'s own "wrong answer instead of an error".
///
/// **NOT a mechanism.** Nothing makes a third tier-changing path come through here and every test
/// would still pass; it is one placement per task, the distinction `docker::bounded`'s header draws
/// about its own wait loop. The generalisable form is one layer down, where `Budget::of` already
/// reads *this is a provisioning call* off the argv.
///
/// **A second limit: the file is the granularity, not one service.** `publish` writes the whole
/// document, so a failed `dev-up` also drops a *different* provisioner's entry -
/// `nix/postgres-tier.nix`'s, which republishes on `start` - exactly as a successful one did.
pub(super) fn with_endpoints_forgotten<T>(
    task: &str,
    scope: &Scope,
    change_the_tier: impl FnOnce() -> Result<T, Verdict>,
) -> Result<T, Verdict> {
    if let Err(problem) = discovery::forget(scope) {
        eprintln!("xtask {task}: {problem}");
        return Err(Verdict::Fail);
    }
    change_the_tier()
}

/// Issue one compose subcommand that CHANGES the tier, and report the two ways it can fail.
///
/// One function because `dev-up` and `dev-down` reported it in copies differing only in the task
/// name - and one of those copies is `report_abandoned`, the guard whose own doc says a guard
/// written twice is one the third caller forgets. The subcommand in the message comes off the
/// arguments that are run, so it cannot name a call other than the one that failed.
fn changed(task: &str, root: &Path, project: &str, profiles: &[&str], args: &[&str]) -> Result<(), Verdict> {
    match docker::compose(root, project, profiles, args) {
        Ok(out) if out.ok => Ok(()),
        Ok(out) => {
            eprintln!(
                "xtask {task}: `docker compose {}` failed",
                args.first().copied().unwrap_or_default()
            );
            eprint!("{}", out.stderr);
            Err(Verdict::Fail)
        }
        Err(cause) => {
            eprintln!("xtask {task}: {cause}");
            report_abandoned(&cause, project);
            Err(Verdict::Fail)
        }
    }
}

/// Bring the tier up, wait for it, and record what it bound. The path written, on success.
pub(super) fn provision(root: &Path, scope: &Scope, active: &[&str], expected: &[&str]) -> Result<PathBuf, Verdict> {
    let project = scope.project();
    changed("dev-up", root, &project, active, &["up", "--detach", "--remove-orphans"])?;
    health::wait_until_healthy(root, scope, active, expected)?;
    let bound = read_back_ports(root, scope, active, expected)?;
    discovery::publish(scope, &bound).map_err(|problem| {
        eprintln!("xtask dev-up: {problem}");
        Verdict::Fail
    })
}

/// Remove one project's containers, network and named volumes.
///
/// Takes the target and not the plan: whether there IS one is `teardown::plan`'s decision, and
/// re-deciding it here would put a success-having-removed-nothing arm inside the function whose
/// job is to destroy.
pub(super) fn remove(root: &Path, target: &str, profiles: &[&str]) -> Result<(), Verdict> {
    changed("dev-down", root, target, profiles, &teardown::down_args())?;
    println!("  removed  {target}");
    Ok(())
}

/// Ask docker which host port each service actually landed on.
///
/// This is the allocation being read back. Nothing here chooses a port; if the read-back fails, the
/// provision fails rather than falling back to a guess - a fallback is how a constant gets in.
/// Only the services that were STARTED are read back: a service in a profile nobody asked for has
/// no container and therefore no port, and asking for one would fail the provision over a service
/// that was correctly absent.
fn read_back_ports(root: &Path, scope: &Scope, profiles: &[&str], expected: &[&str]) -> Result<Published, Verdict> {
    let mut bound = Vec::new();
    let project = scope.project();
    for service in SERVICES.iter().filter(|s| expected.contains(&s.name())) {
        let container_port = service.container_port().to_string();
        let out = match docker::compose(root, &project, profiles, &["port", service.name(), &container_port]) {
            Ok(out) => out,
            Err(cause) => {
                eprintln!("xtask dev-up: {cause}");
                return Err(Verdict::Fail);
            }
        };
        let published = if out.ok { docker::first_published(&out.stdout) } else { None };
        let Some(published) = published else {
            eprintln!(
                "xtask dev-up: docker published no host port for {}:{container_port}",
                service.name()
            );
            eprint!("{}", out.stderr);
            return Err(Verdict::Fail);
        };
        bound.push((service.name(), String::from(published)));
    }
    Ok(bound)
}

/// Print what a killed provisioning call left behind, if it left anything.
///
/// Whether it did is [`docker::Failed::left_running`]'s answer, not a test at each call site: both
/// `dev-up` and `dev-down` issue a provisioning call, and a guard written twice is a guard the third
/// caller forgets.
fn report_abandoned(cause: &docker::Failed, project: &str) {
    if !cause.left_running() {
        return;
    }
    for line in abandoned(project) {
        eprintln!("  {line}");
    }
}

/// What a provisioning call that never answered leaves behind, and who removes it.
///
/// **The decision, as a value rather than three `eprintln!`s.** Killing the child does not stop the
/// containers it had already started, so something has to be said about them. Three options, and the
/// tool deliberately does NOT tear down:
///
/// | Option | Why not |
/// | --- | --- |
/// | Tear down, then report | `down --volumes` destroys named volumes, and a timeout is not proof the tier is wrong. `up` is idempotent, so this may be the second `dev-up` over a tier that was ALREADY running and healthy - and then one slow call destroys a working tier. That shape is measured defect #2 in `nix/with-tier.sh`: an unconditional teardown on exit took down a server somebody else had started. |
/// | Retry, then tear down | If the cause is a wedged daemon the teardown goes through the same socket and cannot succeed either, so it turns one bounded failure into two. |
/// | Report, and name the task that removes them | What this does. |
///
/// **The limit, next to the claim: `dev-up` can exit non-zero with containers running.** That is
/// deliberate, and what makes it safe is not this report - it is that no discovery file survives
/// the failure. Publishing only after readiness was never enough alone, and that is worth
/// recording: the gap was a file from an EARLIER run, which this one never touched.
/// [`with_endpoints_forgotten`] is the half that closes it, and fail-closed lives in the two
/// together rather than here.
pub(super) fn abandoned(project: &str) -> [String; 3] {
    [
        format!("containers are NOT removed: killing docker does not stop what it had started - {project}"),
        String::from("a timeout is not proof they are wrong - these subcommands are idempotent, so a retry reuses the pull"),
        String::from(teardown::REMOVES_THIS_WORKTREE),
    ]
}
