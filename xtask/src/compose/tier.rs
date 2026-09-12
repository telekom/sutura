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

use super::{docker, health, lock, teardown};
use crate::Verdict;

/// What a service reported as its published address, per service name.
type Published = Vec<(&'static str, String)>;

/// The demo's one named volume, as declared in `compose.services.yaml`.
const DEMO_VOLUME: &str = "demo-webui-data";

/// Change the tier with this worktree's DOCKER endpoint entries withdrawn FIRST.
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
/// **The granularity is one PROVISIONER's entries, and it used to be the whole file** -
/// `github.com/telekom/sutura#317`. `discovery::forget` was a `remove_file`, so this function
/// withdrew a nix-native tier's claim over a server that was still running, on every failing path
/// as well as on `dev-down`. For Postgres that healed on the next `nix/with-tier.sh`; for any
/// other nix tier it stayed dropped until that task was run again. It now withdraws what
/// `discovery::publish` published and nothing else, and the last entry out still takes the file
/// with it - so the fail-closed direction below is unchanged for the entries this task owns.
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
fn changed(task: &str, root: &Path, project: &str, profiles: &[&str], args: &[&str], removal: &str) -> Result<(), Verdict> {
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
            report_abandoned(&cause, project, removal);
            Err(Verdict::Fail)
        }
    }
}

/// Bring the tier up, wait for it, and record what it bound. The path written, on success.
///
/// `names` restricts the `up` to those services, which is what makes `--only` a SELECTION rather
/// than a widening: naming a service with `--no-deps` starts it and nothing else, so neither the
/// default set nor a future dependency is dragged in beside it. `None` is the ordinary `up`, which
/// starts every service in the active profiles.
pub(super) fn provision(
    root: &Path,
    scope: &Scope,
    active: &[&str],
    expected: &[&str],
    names: Option<&[&'static str]>,
) -> Result<PathBuf, Verdict> {
    let project = scope.project();
    let removal = names.map_or(teardown::REMOVES_THIS_WORKTREE, |_| teardown::REMOVES_DEMO);
    let mut args: Vec<&str> = vec!["up", "--detach", "--remove-orphans"];
    if let Some(names) = names {
        // The enforcement behind "only": a dependency added in the Compose file must not silently
        // widen this selection. It makes startup fail instead, until the lifecycle owns that
        // dependency explicitly.
        args.push("--no-deps");
        args.extend_from_slice(names);
    }
    changed("dev-up", root, &project, active, &args, removal)?;
    health::wait_until_healthy(root, scope, active, expected, removal)?;
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
    changed(
        "dev-down",
        root,
        target,
        profiles,
        &teardown::down_args(),
        teardown::REMOVES_THIS_WORKTREE,
    )?;
    println!("  removed  {target}");
    Ok(())
}

/// Remove the NAMED services' containers from one project, and nothing else.
///
/// Named volumes are deliberately absent from this Compose command. `down --volumes <service>`
/// attempts every named volume the project declares; one belonging to another service survives
/// only while a container still holds it. [`remove_selected_volume`] removes the demo volume by its
/// exact project-scoped name instead.
pub(super) fn remove_services(root: &Path, project: &str, profiles: &[&str], services: &[&str]) -> Result<(), Verdict> {
    let Some(args) = teardown::scoped_down_args(services) else {
        eprintln!("xtask dev-down: an empty service selection would remove the whole project - refusing");
        return Err(Verdict::Fail);
    };
    changed("dev-down", root, project, profiles, &args, teardown::REMOVES_DEMO)?;
    println!("  removed  {} in {project}", services.join(", "));
    Ok(())
}

/// Remove one selected Compose volume, resolved by its project and logical-name labels.
fn remove_selected_volume(root: &Path, project: &str, logical: &str) -> Result<(), Verdict> {
    let listed = docker::labeled_volumes(root, project, logical).map_err(|cause| {
        eprintln!("xtask dev-down: {cause}");
        Verdict::Fail
    })?;
    if !listed.ok {
        eprintln!("xtask dev-down: `docker volume ls` failed");
        eprint!("{}", listed.stderr);
        return Err(Verdict::Fail);
    }
    let expected = format!("{project}_{logical}");
    let found: Vec<&str> = listed.stdout.lines().filter(|line| !line.is_empty()).collect();
    if found.is_empty() {
        println!("  removed  no volume - {expected} did not exist");
        return Ok(());
    }
    if found.as_slice() != [expected.as_str()] {
        eprintln!(
            "xtask dev-down: the `{logical}` label did not resolve to its project-scoped volume \
             `{expected}` - no volume removed: {found:?}"
        );
        return Err(Verdict::Fail);
    }
    let removed = docker::remove_volume(root, &expected).map_err(|cause| {
        eprintln!("xtask dev-down: {cause}");
        Verdict::Fail
    })?;
    if !removed.ok {
        eprintln!("xtask dev-down: `docker volume rm` failed for {expected}");
        eprint!("{}", removed.stderr);
        return Err(Verdict::Fail);
    }
    println!("  removed  volume {expected}");
    Ok(())
}

/// The same contract as [`with_endpoints_forgotten`], for a teardown that removes only `services`.
///
/// Withdraw only those services' entries: a scoped destroy that called `discovery::forget` would
/// withdraw a `clickhouse` this worktree's own `dev-up` published and is still serving, which is the
/// failure `github.com/telekom/sutura#317` removed arriving through the service dimension.
pub(super) fn with_selected_endpoints_forgotten<T>(
    task: &str,
    scope: &Scope,
    services: &[&str],
    change_the_tier: impl FnOnce() -> Result<T, Verdict>,
) -> Result<T, Verdict> {
    if let Err(problem) = discovery::forget_services(scope, services) {
        eprintln!("xtask {task}: {problem}");
        return Err(Verdict::Fail);
    }
    change_the_tier()
}

/// Remove the demo service, its container and its named volume, and nothing else.
///
/// **The demo lifecycle's teardown.** The Compose call names the service but carries no global
/// `--volumes`; the second call resolves the one volume through both Compose labels and then
/// requires its ordinary project-scoped name. A neighbour keeps its container, volumes and the
/// shared project network.
pub(super) fn down_scoped(scope: &Scope, root: &Path, profile: &'static str, services: &[&str], dry_run: bool) -> Verdict {
    let held = match lock::acquire(scope) {
        Ok(held) => held,
        Err(problem) => {
            eprintln!("xtask dev-down: {problem}");
            return Verdict::Fail;
        }
    };
    let project = scope.project();
    println!("xtask dev-down: {project} in {}", root.display());
    println!(
        "  remove   {} (containers and named volumes; the rest of the project is untouched)",
        services.join(", ")
    );
    println!("  volume   {project}_{DEMO_VOLUME} (only with this project's two Compose labels)");
    println!("  spared   every other service in this worktree's project, on purpose");
    if dry_run {
        println!("xtask dev-down: dry run - nothing was removed");
        drop(held);
        return Verdict::Pass;
    }
    // This profile's entries only: a scoped teardown that called `forget` would withdraw a
    // `clickhouse` this worktree's own `dev-up` published and is still serving.
    let removal = with_selected_endpoints_forgotten("dev-down", scope, services, || {
        remove_services(root, &project, &[profile], services)?;
        remove_selected_volume(root, &project, DEMO_VOLUME)
    });
    if let Err(verdict) = removal {
        return verdict;
    }
    println!("xtask dev-down: ok");
    drop(held);
    Verdict::Pass
}

/// Remove this worktree's whole compose project: every profile's containers, its network and its
/// named volumes.
pub(super) fn down_whole(scope: &Scope, root: &Path, dry_run: bool) -> Verdict {
    // Rule 2: the lock is taken BEFORE the listing that decides the plan, and held across the
    // removal. Reading the listing outside it is the window this closes.
    let held = match lock::acquire(scope) {
        Ok(held) => held,
        Err(problem) => {
            eprintln!("xtask dev-down: {problem}");
            return Verdict::Fail;
        }
    };

    let project = scope.project();
    let plan = teardown::plan(&project, &docker::projects(root));
    println!("xtask dev-down: {} in {}", project, root.display());
    teardown::describe(&plan);

    // EVERY profile, not the ones this invocation asked for - `dev-down` takes no `--with`, and
    // that is the point. `docker compose down` only considers services in ACTIVE profiles, so a
    // destroy run without them would leave a profiled service's container and its named volume
    // behind **while reporting success**. Derived from the declaration, so a new profile is covered
    // without a second edit here.
    let every_profile = sutura_dev::scope::profiles();
    if !every_profile.is_empty() {
        println!("  profiles  {} (all of them, so nothing survives)", every_profile.join(", "));
    }

    if dry_run {
        println!("xtask dev-down: dry run - nothing was removed");
        drop(held);
        return Verdict::Pass;
    }

    // THE RE-CHECK, and it is a second computation rather than a second look at a variable: the
    // scope is derived again from the filesystem, and the project listing is read again. Deciding
    // and removing are two moments, so the question gets asked twice - and a target that moved
    // between them is refused rather than removed, which is the fail-closed direction.
    //
    // **What it cannot catch, stated with the claim:** the lock is what closes the window, and this
    // re-check is what notices if something got through it anyway. A neighbour that respects
    // neither is not something a check here can see.
    let Ok(again) = super::scope_here() else {
        return Verdict::Fail;
    };
    let confirmed = teardown::plan(&again.project(), &docker::projects(root));
    if !teardown::still_eligible(&plan, &again.project()) || confirmed.target() != plan.target() {
        eprintln!("xtask dev-down: the plan changed between deciding and removing - nothing removed");
        teardown::describe(&confirmed);
        return Verdict::Fail;
    }

    // BEFORE the containers, not after: a `down` that was killed, or exited non-zero, returned
    // above the `forget` that stood here and left a readable file over a tier in an unknown state.
    let removal = with_endpoints_forgotten("dev-down", scope, || {
        plan.target().map_or(Ok(()), |target| remove(root, target, &every_profile))
    });
    if let Err(verdict) = removal {
        return verdict;
    }
    println!("xtask dev-down: ok");
    drop(held);
    Verdict::Pass
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
fn report_abandoned(cause: &docker::Failed, project: &str, removal: &str) {
    if !cause.left_running() {
        return;
    }
    for line in abandoned(project, removal) {
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
pub(super) fn abandoned(project: &str, removal: &str) -> [String; 3] {
    [
        format!("containers are NOT removed: killing docker does not stop what it had started - {project}"),
        String::from("a timeout is not proof they are wrong - these subcommands are idempotent, so a retry reuses the pull"),
        String::from(removal),
    ]
}
#[cfg(test)]
mod scoped_startup_tests {
    use super::with_selected_endpoints_forgotten;
    use sutura_dev::discovery::{Endpoints, Provisioner};
    use sutura_dev::scope::Scope;

    #[test]
    fn scoped_startup_withdraws_only_selected_stale_discovery_entries() {
        let root = std::env::temp_dir().join(format!("sutura-scoped-start-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temporary worktree");
        let scope = Scope::from_root(&root).expect("scope");
        std::fs::create_dir_all(scope.state_dir()).expect("state directory");
        std::fs::write(
            scope.state_dir().join("endpoints.json"),
            r#"{"project":"p","services":{"demo":{"host":"127.0.0.1","port":1,"provisioner":"docker"},"clickhouse":{"host":"127.0.0.1","port":2,"provisioner":"docker"},"postgres":{"host":"/tmp/p","port":3,"provisioner":"nix"}}}"#,
        ).expect("discovery fixture");
        let mut called = false;
        let result = with_selected_endpoints_forgotten("dev-up --only demo", &scope, &["demo"], || {
            called = true;
            Ok::<_, crate::Verdict>(())
        });
        assert!(result.is_ok() && called, "scoped startup did not run its change closure");
        let endpoints = Endpoints::discover(&scope).expect("unrelated discovery entries survive");
        endpoints.endpoint("demo").unwrap_err();
        assert_eq!(
            endpoints
                .endpoint("clickhouse")
                .expect("same provisioner survives")
                .provisioner(),
            Provisioner::Docker
        );
        assert_eq!(
            endpoints
                .endpoint("postgres")
                .expect("other provisioner survives")
                .provisioner(),
            Provisioner::Nix
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
