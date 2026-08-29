//! The compose tier: one independent service instance per worktree, provisioned from here.
//!
//! # Why this is in `xtask` and not in a shipped binary
//!
//! Docker orchestration inside a release artifact is test scaffolding delivered to users. `xtask` is
//! the repo tool, it is never packaged, and it is what CI runs - which is the second half of the
//! reason: a service tier only a developer's binary could start is a tier CI cannot stand up.
//!
//! # The failure this exists to prevent
//!
//! Two worktrees of this repository - an agent's and a human's, or two agents' - run the service
//! tier at the same time and **neither notices the other**. A port collision does not fail cleanly:
//! docker binds the first claimant and the second gets a connection refused that looks like a broken
//! adapter, or worse, connects to the *neighbour's* container and passes against the wrong fixture.
//! A test that silently talked to another worktree's database is the outcome this module makes
//! impossible.
//!
//! Four things collide, and each has a per-worktree value:
//!
//! | What collides | Per-worktree value |
//! | --- | --- |
//! | Published ports | whatever docker and the operating system allocate, published ephemerally and read back |
//! | Compose project name, and its named volumes | derived from the worktree path by `sutura_dev::scope` |
//! | Container and network names | derived by compose FROM the project name, never written literally |
//! | The state a test reads | a discovery file inside that worktree, gitignored |
//!
//! **Ports are allocated, not derived**, and that reverses an earlier design. A hash into a port
//! range is a total function into a finite set, so collisions exist by construction; and
//! "refuse if the port is already bound" is check-then-bind, which leaves the whole window between
//! the check and docker's bind open. Allocation has neither defect and no window, because the
//! allocation and the bind are one operation. Naming stays derived because naming has no allocator -
//! and a hash collision in a NAME is a startup error somebody reads, while one in a PORT is a test
//! passing against the wrong fixture.
//!
//! # Not a nix check, deliberately
//!
//! The nix sandbox has no network and no docker socket, so this cannot be a `checks.*` derivation.
//! It is a `just` task and a CI job over nix-built artifacts instead.

mod docker;
mod lock;
mod teardown;

use std::path::Path;

use sutura_dev::discovery::{self, Endpoints};
use sutura_dev::scope::{SERVICES, Scope};

use crate::Verdict;
use crate::repo;

/// How long to wait for every service to report healthy, unless overridden.
const READY_TIMEOUT_SECS: u64 = 180;

/// How often to ask. Not a readiness mechanism: the GATE is the health report, and this is only how
/// often it is read. A fixed sleep instead of a gate is what produces a connection refused inside a
/// test, attributed to whatever the test happened to be doing.
const POLL_INTERVAL_MILLIS: u64 = 500;

/// Whether a missing docker is fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Requirement {
    /// A missing docker FAILS. In CI this tier is the only thing standing behind a network adapter,
    /// and a green run that quietly tested nothing is the failure the whole tier exists to prevent.
    Required,
    /// A missing docker SKIPS, loudly, naming what did not run. Docker is a host dependency this
    /// repository deliberately does not pin with nix, and a contributor without it has to be able to
    /// work on everything else.
    Optional,
}

/// Which way the docker-absent decision points, and it points differently on the two machine
/// classes - so both directions are read from one flag, in one function, with the reason here.
///
/// **Neither direction is the default.** What a wrong answer costs decides it: locally, a false
/// failure blocks a contributor who is not touching services; in CI, a false pass certifies an
/// adapter nothing exercised.
pub(crate) fn requirement(ci: Option<&str>, forced: Option<&str>) -> Requirement {
    if let Some(value) = forced {
        return if truthy(value) {
            Requirement::Required
        } else {
            Requirement::Optional
        };
    }
    if ci.is_some_and(truthy) {
        return Requirement::Required;
    }
    Requirement::Optional
}

/// GitHub Actions sets `CI=true`; a developer who exports `CI=0` means it.
fn truthy(value: &str) -> bool {
    !matches!(value.trim().to_lowercase().as_str(), "" | "0" | "false" | "no")
}

/// The requirement this process is running under.
fn requirement_from_env() -> Requirement {
    requirement(
        std::env::var("CI").ok().as_deref(),
        std::env::var("SUTURA_DEV_REQUIRE_DOCKER").ok().as_deref(),
    )
}

/// What a service reported as its published address, per service name.
type Published = Vec<(&'static str, String)>;

/// What `--with` asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Requested {
    /// The profiles to activate, in the order given. Empty means the default set only.
    Profiles(Vec<&'static str>),
    /// A profile name no service declares. Refused rather than ignored: a typo that silently
    /// started nothing would look exactly like a service that failed to come up.
    Unknown(String),
    /// `--with` with nothing after it.
    Missing,
}

/// Read `--with <profile>`, repeatable, against the profiles services actually declare.
///
/// Validated against the declaration rather than a literal list here, so a service that gains a
/// profile needs no edit in this file - and a name nothing declares cannot be quietly accepted.
pub(crate) fn requested(args: &[String], known: &[&'static str]) -> Requested {
    let mut chosen: Vec<&'static str> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg != "--with" {
            continue;
        }
        let Some(name) = rest.next() else {
            return Requested::Missing;
        };
        match known.iter().find(|profile| *profile == name) {
            Some(profile) if !chosen.contains(profile) => chosen.push(profile),
            Some(_) => {}
            None => return Requested::Unknown(name.clone()),
        }
    }
    Requested::Profiles(chosen)
}

/// The services this invocation will start: the default set, plus any whose profile was asked for.
///
/// The readiness gate and the port read-back both walk this, so a service left out of it is a
/// service nothing waits for and nothing records - which is the correct behaviour for one that was
/// never started, and would be a silent hole for one that was.
pub(crate) fn expected_services(active: &[&'static str]) -> Vec<&'static str> {
    SERVICES
        .iter()
        .filter(|service| service.is_default() || service.profile().is_some_and(|p| active.contains(&p)))
        .map(sutura_dev::scope::Service::name)
        .collect()
}

/// The worktree we are in, as a scope. Everything named is derived from this and nothing else - the
/// scope carries the canonical root, so there is no second path for the two to disagree about.
fn scope_here() -> Result<Scope, Verdict> {
    let Some(root) = repo::root() else {
        eprintln!("xtask compose: could not determine the repo root");
        return Err(Verdict::Fail);
    };
    Scope::from_root(&root).map_err(|problem| {
        eprintln!("xtask compose: {problem}");
        Verdict::Fail
    })
}

/// The docker gate, plus the skip-or-fail decision when it is not there.
fn require_docker(task: &str) -> Result<(), Verdict> {
    match docker::presence() {
        Ok(()) => Ok(()),
        Err(missing) => Err(absent(task, missing, requirement_from_env())),
    }
}

/// Report an absent runtime, in whichever direction this machine class points.
fn absent(task: &str, missing: docker::Missing, requirement: Requirement) -> Verdict {
    match requirement {
        Requirement::Optional => {
            println!("xtask {task}: SKIPPED - no container runtime ({missing:?})");
            println!("  {}", missing.remedy());
            println!("  Nothing ran. Docker is a host dependency and nix deliberately does not pin");
            println!("  it, so this skips on a developer machine and FAILS in CI - set");
            println!("  SUTURA_DEV_REQUIRE_DOCKER=1 to get the CI direction here.");
            Verdict::Pass
        }
        Requirement::Required => {
            eprintln!("xtask {task}: FAILED - no container runtime ({missing:?})");
            eprintln!("  {}", missing.remedy());
            eprintln!("  This tier is the only thing standing behind a network adapter. A run that");
            eprintln!("  skipped it would report green having tested nothing, which is the exact");
            eprintln!("  failure the tier exists to prevent. Set SUTURA_DEV_REQUIRE_DOCKER=0 to");
            eprintln!("  skip instead, and mean it.");
            Verdict::Fail
        }
    }
}

/// `dev-up`: bring this worktree's services up, wait for them to be healthy, write the endpoints.
pub(crate) fn run_up(args: &[String]) -> Verdict {
    let known = sutura_dev::scope::profiles();
    let active = match requested(args, &known) {
        Requested::Profiles(chosen) => chosen,
        Requested::Missing => {
            eprintln!("xtask dev-up: `--with` needs a profile name; this file declares {known:?}");
            return Verdict::Usage;
        }
        Requested::Unknown(name) => {
            eprintln!("xtask dev-up: no service declares the profile `{name}`; there is {known:?}");
            return Verdict::Usage;
        }
    };

    let Ok(scope) = scope_here() else {
        return Verdict::Fail;
    };
    let root = scope.root().to_path_buf();
    if let Err(verdict) = require_docker("dev-up") {
        return verdict;
    }
    if !root.join(docker::COMPOSE_FILE).is_file() {
        eprintln!(
            "xtask dev-up: {} is missing - there is nothing to provision",
            docker::COMPOSE_FILE
        );
        return Verdict::Fail;
    }

    // Held across the whole provision, so a second `dev-up` in this worktree waits rather than
    // racing it - and so a `dev-down` cannot start removing what this one is starting.
    let held = match lock::acquire(&scope) {
        Ok(held) => held,
        Err(problem) => {
            eprintln!("xtask dev-up: {problem}");
            return Verdict::Fail;
        }
    };

    let expected = expected_services(&active);
    println!("xtask dev-up: {} in {}", scope.project(), root.display());
    println!("  services  {}", expected.join(", "));
    if active.is_empty() {
        println!("  profiles  none - `--with <profile>` adds one of {known:?}");
    } else {
        println!("  profiles  {}", active.join(", "));
    }
    match docker::compose(&root, &scope.project(), &active, &["up", "--detach", "--remove-orphans"]) {
        Ok(out) if out.ok => {}
        Ok(out) => {
            eprintln!("xtask dev-up: `docker compose up` failed");
            eprint!("{}", out.stderr);
            return Verdict::Fail;
        }
        Err(cause) => {
            eprintln!("xtask dev-up: could not run docker: {cause}");
            return Verdict::Fail;
        }
    }

    if let Err(verdict) = wait_until_healthy(&root, &scope, &active, &expected) {
        return verdict;
    }

    let bound = match read_back_ports(&root, &scope, &active, &expected) {
        Ok(bound) => bound,
        Err(verdict) => return verdict,
    };
    let path = match discovery::publish(&scope, &bound) {
        Ok(path) => path,
        Err(problem) => {
            eprintln!("xtask dev-up: {problem}");
            return Verdict::Fail;
        }
    };

    println!("xtask dev-up: ok - {} service(s) healthy", expected.len());
    println!("  discovery {}", path.display());
    report_endpoints(&scope);
    println!("  lock      {} (released now)", held.path().display());
    drop(held);
    Verdict::Pass
}

/// Poll the health report until every expected service is ready, or the deadline passes.
///
/// A health GATE and not a sleep. The distinction is what happens when it is wrong: a sleep that was
/// too short surfaces as a connection refused inside somebody's test; this says which service never
/// became healthy and stops.
fn wait_until_healthy(root: &Path, scope: &Scope, profiles: &[&str], expected: &[&str]) -> Result<(), Verdict> {
    let budget = std::time::Duration::from_secs(
        std::env::var("SUTURA_DEV_READY_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(READY_TIMEOUT_SECS),
    );
    let started = std::time::Instant::now();
    // What the last poll was still waiting for. Carried out of the match so a timeout says which
    // service never arrived rather than only that one did not - and declared without a value,
    // because the only path that reads it is the one the `Waiting` arm assigns on.
    let mut last: Vec<String>;

    loop {
        let out = docker::compose(root, &scope.project(), profiles, &["ps", "--all", "--format", "json"]);
        let reported = match out {
            Ok(out) if out.ok => docker::parse_ps(&out.stdout),
            Ok(out) => {
                eprintln!("xtask dev-up: `docker compose ps` failed");
                eprint!("{}", out.stderr);
                return Err(Verdict::Fail);
            }
            Err(cause) => {
                eprintln!("xtask dev-up: could not run docker: {cause}");
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
                eprintln!("  `docker compose --project-name {} logs` has the detail.", scope.project());
                return Err(Verdict::Fail);
            }
            docker::Readiness::Waiting(why) => last = why,
        }

        if started.elapsed() >= budget {
            eprintln!("xtask dev-up: {} service(s) never became ready:", last.len());
            for line in &last {
                eprintln!("  {line}");
            }
            eprintln!("  waited {}s (SUTURA_DEV_READY_TIMEOUT_SECS)", budget.as_secs());
            return Err(Verdict::Fail);
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MILLIS));
    }
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
    for service in SERVICES.iter().filter(|s| expected.contains(&s.name())) {
        let container_port = service.container_port().to_string();
        let out = match docker::compose(root, &scope.project(), profiles, &["port", service.name(), &container_port]) {
            Ok(out) => out,
            Err(cause) => {
                eprintln!("xtask dev-up: could not run docker: {cause}");
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

/// `dev-down`: remove this worktree's project, and nothing else. `--dry-run` says what it would do.
pub(crate) fn run_down(args: &[String]) -> Verdict {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let Ok(scope) = scope_here() else {
        return Verdict::Fail;
    };
    let root = scope.root().to_path_buf();
    if let Err(verdict) = require_docker("dev-down") {
        return verdict;
    }

    // Rule 2: the lock is taken BEFORE the listing that decides the plan, and held across the
    // removal. Reading the listing outside it is the window this closes.
    let held = match lock::acquire(&scope) {
        Ok(held) => held,
        Err(problem) => {
            eprintln!("xtask dev-down: {problem}");
            return Verdict::Fail;
        }
    };

    let project = scope.project();
    let plan = teardown::plan(&project, &docker::projects(&root));
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
    let Ok(again) = scope_here() else {
        return Verdict::Fail;
    };
    let confirmed = teardown::plan(&again.project(), &docker::projects(&root));
    if !teardown::still_eligible(&plan, &again.project()) || confirmed.target != plan.target {
        eprintln!("xtask dev-down: the plan changed between deciding and removing - nothing removed");
        teardown::describe(&confirmed);
        return Verdict::Fail;
    }

    if let Some(target) = plan.target.as_deref() {
        match docker::compose(&root, target, &every_profile, &teardown::down_args()) {
            Ok(out) if out.ok => println!("  removed  {target}"),
            Ok(out) => {
                eprintln!("xtask dev-down: `docker compose down` failed");
                eprint!("{}", out.stderr);
                return Verdict::Fail;
            }
            Err(cause) => {
                eprintln!("xtask dev-down: could not run docker: {cause}");
                return Verdict::Fail;
            }
        }
    }

    // A stale discovery file is the one way discovery could hand back a wrong answer instead of an
    // error, so it goes with the containers rather than after them.
    if let Err(problem) = discovery::forget(&scope) {
        eprintln!("xtask dev-down: {problem}");
        return Verdict::Fail;
    }
    println!("xtask dev-down: ok");
    drop(held);
    Verdict::Pass
}

/// `dev-endpoints`: print what provisioning bound, from the discovery file and nowhere else.
pub(crate) fn run_endpoints(_args: &[String]) -> Verdict {
    let Ok(scope) = scope_here() else {
        return Verdict::Fail;
    };
    match Endpoints::discover(&scope) {
        Ok(endpoints) => {
            println!("xtask dev-endpoints: {}", endpoints.project());
            for (name, endpoint) in endpoints.services() {
                println!("  {name:<12} {endpoint}");
            }
            Verdict::Pass
        }
        Err(problem) => {
            eprintln!("xtask dev-endpoints: {problem}");
            Verdict::Fail
        }
    }
}

/// Print the endpoints just published, through the reader's door.
///
/// Deliberately re-read rather than printed from what was written: if the file and the values
/// disagree, the file is what every harness will see, so the file is what gets reported.
fn report_endpoints(scope: &Scope) {
    match Endpoints::discover(scope) {
        Ok(endpoints) => {
            for (name, endpoint) in endpoints.services() {
                println!("  {name:<12} {endpoint}");
            }
        }
        Err(problem) => eprintln!("  (could not read back the discovery file: {problem})"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Requirement, absent, requirement, truthy};
    use crate::Verdict;

    #[test]
    fn absent_docker_skips_locally_and_fails_in_ci() {
        // Both directions of the one flag, because a fail-open / fail-closed decision with only one
        // side tested is half a decision.
        assert_eq!(requirement(None, None), Requirement::Optional);
        assert_eq!(requirement(Some("true"), None), Requirement::Required);
        assert_eq!(requirement(Some("1"), None), Requirement::Required);

        assert_eq!(
            absent("dev-up", crate::compose::docker::Missing::Cli, Requirement::Optional),
            Verdict::Pass,
            "a developer without docker has to be able to work on everything else"
        );
        assert_eq!(
            absent("dev-up", crate::compose::docker::Missing::Cli, Requirement::Required),
            Verdict::Fail,
            "a CI run that skipped this would report green having tested nothing"
        );
    }

    #[test]
    fn the_flag_overrides_the_machine_class_in_both_directions() {
        // Needed in both: a developer reproducing a CI failure, and a CI job on a runner class
        // that deliberately has no docker.
        assert_eq!(requirement(None, Some("1")), Requirement::Required);
        assert_eq!(requirement(Some("true"), Some("0")), Requirement::Optional);
        assert_eq!(requirement(Some("true"), Some("false")), Requirement::Optional);
    }

    #[test]
    fn an_empty_or_negative_ci_variable_is_not_ci() {
        // `CI=` and `CI=0` are both things people export, and reading either as "in CI" would fail a
        // developer's run for a reason they did not choose.
        for value in ["", "0", "false", "no", " FALSE "] {
            assert!(!truthy(value), "{value:?} read as truthy");
        }
        for value in ["1", "true", "TRUE", "yes"] {
            assert!(truthy(value), "{value:?} read as falsy");
        }
    }

    #[test]
    fn the_identity_provider_starts_only_when_it_is_asked_for() {
        // The reviewer's question turned into a mechanism: `dev-up` with no flag does not start
        // keycloak, so CI does not pay for a service nothing here can use yet.
        let default_set = super::expected_services(&[]);
        assert!(default_set.contains(&"postgres"));
        assert!(default_set.contains(&"clickhouse"));
        assert!(
            !default_set.contains(&"keycloak"),
            "the default set must not include the identity provider: {default_set:?}"
        );

        let with_identity = super::expected_services(&["identity"]);
        assert!(with_identity.contains(&"keycloak"), "{with_identity:?}");
        assert_eq!(
            with_identity.len(),
            default_set.len() + 1,
            "asking for a profile must ADD to the default set, not replace it"
        );
    }

    #[test]
    fn an_unknown_profile_is_refused_rather_than_silently_starting_nothing() {
        // A typo that quietly started the default set would look exactly like a service that failed
        // to come up - the reader would go looking at containers instead of at their command line.
        let known = ["identity"];
        let args = |v: &[&str]| -> Vec<String> { v.iter().map(|s| String::from(*s)).collect() };

        assert_eq!(
            super::requested(&args(&["--with", "identty"]), &known),
            super::Requested::Unknown(String::from("identty"))
        );
        assert_eq!(super::requested(&args(&["--with"]), &known), super::Requested::Missing);
        assert_eq!(
            super::requested(&args(&["--with", "identity"]), &known),
            super::Requested::Profiles(vec!["identity"])
        );
        // Repeated is not two, and no flag is none.
        assert_eq!(
            super::requested(&args(&["--with", "identity", "--with", "identity"]), &known),
            super::Requested::Profiles(vec!["identity"])
        );
        assert_eq!(super::requested(&args(&[]), &known), super::Requested::Profiles(Vec::new()));
    }

    #[test]
    fn teardown_enables_every_profile_so_nothing_survives_it() {
        // `docker compose down` only considers services in ACTIVE profiles, so a destroy that ran
        // without them would leave a profiled container and its named volume behind while reporting
        // success - the silent-success failure the teardown contract is about. `dev-down` takes no
        // `--with` for exactly this reason.
        let every = sutura_dev::scope::profiles();
        assert!(every.contains(&"identity"), "{every:?}");

        let args = super::docker::scoped_args(std::path::Path::new("/repo"), "sutura-dev-aaaa1111", &every);
        for profile in &every {
            assert!(
                args.contains(&String::from(*profile)),
                "`{profile}` is not on the destroy's command line: {args:?}"
            );
        }
        // The flag belongs to `docker compose`, before the subcommand - on `up` it is not a flag at
        // all, so a profile appended after it would be read as a service name.
        let profile_at = args.iter().position(|a| a == "--profile");
        assert!(profile_at.is_some_and(|at| at > 0), "{args:?}");
    }

    /// The compose file's text, or `None` where the repo root cannot be found.
    fn compose_text() -> Option<String> {
        let root = crate::repo::root()?;
        std::fs::read_to_string(root.join(super::docker::COMPOSE_FILE)).ok()
    }

    #[test]
    fn every_image_is_reached_through_one_registry_variable() {
        // A network behind a registry mirror has NO route to the public registry, and an unprefixed
        // reference does not fall back - it fails. So a bare `image: postgres:18-alpine` is not a
        // style problem, it is a service that cannot start there. Reviewed once, checked from now on.
        let Some(text) = compose_text() else { return };

        let images: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter_map(|line| line.strip_prefix("image: "))
            .collect();
        assert!(images.len() >= 2, "the file should declare several images: {images:?}");

        let mut defaults: Vec<&str> = Vec::new();
        for image in &images {
            let default = image
                .strip_prefix("${")
                .and_then(|rest| rest.split('}').next())
                .and_then(|inside| inside.split(":-").nth(1));
            let Some(default) = default else {
                panic!("`{image}` does not come through a registry variable with a default");
            };
            defaults.push(default);
        }
        // ONE value an operator sets. Two defaults that disagree is the drift this catches: half the
        // tier would follow the override and half would not, and only one service would fail.
        defaults.sort_unstable();
        defaults.dedup();
        assert_eq!(defaults.len(), 1, "the registry defaults disagree: {defaults:?}");
    }

    #[test]
    fn the_fixture_credential_is_defined_once() {
        // Two definitions is how they drift and one service silently gets a different password. The
        // anchor is the definition; every use is an alias. A literal repeated in a service block
        // would pass a reading and fail here.
        let Some(text) = compose_text() else { return };

        for anchor in ["fixture-user", "fixture-password"] {
            let defined = text.matches(&format!("&{anchor}")).count();
            let used = text.matches(&format!("*{anchor}")).count();
            assert_eq!(defined, 1, "`{anchor}` is defined {defined} times, not once");
            assert!(used >= 2, "`{anchor}` is aliased {used} time(s) - inline it or use it");
        }

        // And no service writes the value directly. `_USER`/`_PASSWORD` keys must alias.
        for line in text.lines().map(str::trim) {
            let is_credential = line.starts_with("POSTGRES_PASSWORD:")
                || line.starts_with("CLICKHOUSE_PASSWORD:")
                || line.starts_with("KC_BOOTSTRAP_ADMIN_PASSWORD:")
                || line.starts_with("POSTGRES_USER:")
                || line.starts_with("CLICKHOUSE_USER:")
                || line.starts_with("KC_BOOTSTRAP_ADMIN_USERNAME:");
            if is_credential {
                assert!(
                    line.contains("*fixture-"),
                    "`{line}` writes a credential instead of aliasing the one definition"
                );
            }
        }
    }

    #[test]
    fn the_compose_file_publishes_no_fixed_host_port() {
        // The mechanism, read off the file that implements it. A `"5432:5432"` here would give every
        // worktree the same host port, which is the collision this whole tier is about - and it
        // would pass every other gate in the repository, because no gate reads a compose file.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let path = root.join(super::docker::COMPOSE_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            panic!("{} is missing", path.display());
        };
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            let number = index + 1;
            // A published-port entry is a list item under `ports:`. The ephemeral form is a single
            // container port; anything with a colon has named a host port.
            if let Some(entry) = trimmed.strip_prefix("- ") {
                let value = entry.trim().trim_matches('"');
                if value.chars().all(|c| c.is_ascii_digit()) && !value.is_empty() {
                    continue;
                }
                assert!(
                    !value.chars().next().is_some_and(|c| c.is_ascii_digit()),
                    "{}:{number}: `{value}` names a host port - publish ephemerally instead",
                    path.display()
                );
            }
            assert!(
                !trimmed.starts_with("container_name:"),
                "{}:{number}: a literal container name collides between worktrees",
                path.display()
            );
        }
    }
}
