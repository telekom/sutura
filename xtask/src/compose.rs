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
//! The nix sandbox has neither a network nor a docker socket, so a DOCKER-based tier cannot be a
//! `checks.*` derivation. (A service needing neither, like the sandbox's Unix-socket Postgres, can
//! be - see `nix/postgres-tier.nix`.) This one is a `just` task and a CI job over nix-built
//! artifacts instead.

mod docker;
mod health;
mod lock;
mod teardown;
mod tier;

/// Gates that read the compose file's TEXT rather than exercise the functions here. Their own
/// module because they are a separable concern and this one is near its line budget.
#[cfg(test)]
mod file;

use sutura_dev::discovery::Endpoints;
use sutura_dev::provisioned;
use sutura_dev::requirement::{FORCE, Requirement};
use sutura_dev::scope::{SERVICES, Scope};

use crate::Verdict;
use crate::repo;

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
        Err(missing) => Err(absent(task, missing, Requirement::from_env())),
    }
}

/// Report an absent runtime, in whichever direction this machine class points.
///
/// The DIRECTION is not decided here: `sutura_dev::requirement` owns it, because the harness that
/// reads the discovery file has to make the same decision and two copies of it would drift. What is
/// decided here is the wording, which is about docker and belongs beside the docker gate.
///
/// **With one override, and it is the reason the bounded probe distinguishes silence from refusal.**
/// `Requirement::Optional` exists for a machine that does not have docker - nix deliberately does not
/// pin it, so that is a legitimate configuration and skipping is correct. A daemon that is installed,
/// running and never answers is a different machine: it HAS the tier and the tier is faulty. Skipping
/// there produces exactly the outcome the tier exists to prevent - `just dev-up` exiting zero having
/// provisioned nothing, no discovery file written, the tier tests then declining, and the whole run
/// green. So [`docker::Missing::may_be_skipped`] can veto the optional direction, and only that one
/// variant does.
fn absent(task: &str, missing: docker::Missing, requirement: Requirement) -> Verdict {
    let requirement = if missing.may_be_skipped() {
        requirement
    } else {
        Requirement::Required
    };
    match requirement {
        Requirement::Optional => {
            println!("xtask {task}: SKIPPED - no container runtime ({missing:?})");
            println!("  {}", missing.remedy());
            println!("  Nothing ran. Docker is a host dependency and nix deliberately does not pin");
            println!("  it, so this skips on a developer machine and FAILS in CI - set");
            println!("  {FORCE}=1 to get the CI direction here.");
            Verdict::Pass
        }
        Requirement::Required => {
            eprintln!("xtask {task}: FAILED - no container runtime ({missing:?})");
            eprintln!("  {}", missing.remedy());
            eprintln!("  This tier is the only thing standing behind a network adapter. A run that");
            eprintln!("  skipped it would report green having tested nothing, which is the exact");
            eprintln!("  failure the tier exists to prevent.");
            if missing.may_be_skipped() {
                eprintln!("  Set {FORCE}=0 to skip instead, and mean it.");
            } else {
                // Deliberately NOT offering the knob here. It would be the wrong advice: the daemon
                // is installed and running, so skipping does not describe this machine - it just
                // hides a fault behind a green run.
                eprintln!("  {FORCE} does NOT apply: the daemon is installed and running, so this");
                eprintln!("  is a fault on a machine that has the tier, not a machine without one.");
            }
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
    let path = match tier::with_endpoints_forgotten("dev-up", &scope, || tier::provision(&root, &scope, &active, &expected)) {
        Ok(path) => path,
        Err(verdict) => return verdict,
    };

    println!("xtask dev-up: ok - {} service(s) healthy", expected.len());
    println!("  discovery {}", path.display());
    report_endpoints(&scope);
    println!("  lock      {} (released now)", held.path().display());
    drop(held);
    Verdict::Pass
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
    if !teardown::still_eligible(&plan, &again.project()) || confirmed.target() != plan.target() {
        eprintln!("xtask dev-down: the plan changed between deciding and removing - nothing removed");
        teardown::describe(&confirmed);
        return Verdict::Fail;
    }

    // BEFORE the containers, not after: a `down` that was killed, or exited non-zero, returned
    // above the `forget` that stood here and left a readable file over a tier in an unknown state.
    let removal = tier::with_endpoints_forgotten("dev-down", &scope, || {
        plan.target()
            .map_or(Ok(()), |target| tier::remove(&root, target, &every_profile))
    });
    if let Err(verdict) = removal {
        return verdict;
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

/// `dev-endpoint <service>`: one endpoint, on stdout, and nothing else on stdout.
///
/// **The output shape is the feature.** `just dev-endpoints` prints a table for a person to read;
/// this prints `host:port` and a newline, so it substitutes into a shell - which is what lets
/// somebody following `examples/` reach a provisioned service without knowing that scopes, compose
/// projects or ephemeral ports exist. Every diagnostic goes to stderr for the same reason: a
/// `$(..)` that captured an explanation would produce a connection string made of prose.
///
/// A person asking for a value gets a FAILURE when there is none, never a skip. The skip direction
/// belongs to a test run that has other work to do; here the value was the whole request.
pub(crate) fn run_endpoint(args: &[String]) -> Verdict {
    let Some(service) = args.first() else {
        eprintln!("xtask dev-endpoint: needs a service name - this tier declares {}", declared());
        return Verdict::Usage;
    };
    let Ok(scope) = scope_here() else {
        return Verdict::Fail;
    };
    match provisioned::in_worktree(scope.root(), service) {
        Ok(endpoint) => {
            println!("{endpoint}");
            Verdict::Pass
        }
        Err(problem) => {
            eprintln!("xtask dev-endpoint: {problem}");
            Verdict::Fail
        }
    }
}

/// The service names this tier declares, for a usage line. NOT their ports: there is no such thing
/// until docker has bound one, and printing a placeholder is how a constant gets copied.
fn declared() -> String {
    SERVICES
        .iter()
        .map(sutura_dev::scope::Service::name)
        .collect::<Vec<&str>>()
        .join(", ")
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
    use sutura_dev::requirement::Requirement;

    use super::absent;
    use crate::Verdict;

    #[test]
    fn absent_docker_skips_locally_and_fails_in_ci() {
        // The DIRECTION now lives in `sutura_dev::requirement`, with its own tests, because the
        // harness that reads the discovery file has to make the same decision - so what is asserted
        // here is the half this module owns: which verdict each direction produces for a missing
        // container runtime.
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
    fn a_timed_out_provision_reports_what_it_left_running_and_removes_nothing() {
        // The decision `abandoned`'s doc argues, read off the value rather than off the comment.
        // Three properties, and the first two are what a report that had quietly cleaned up would
        // fail: it names the project whose containers survived, it does not read as if they were
        // removed, and it names the task that removes them.
        let report = super::tier::abandoned("sutura-dev-aaaa1111");
        assert!(
            report.iter().any(|line| line.contains("sutura-dev-aaaa1111")),
            "the report must name the project whose containers survived: {report:?}"
        );
        assert!(
            report.iter().any(|line| line.contains("NOT removed")),
            "the report must not read as if the containers were cleaned up: {report:?}"
        );
        assert!(
            report.iter().any(|line| line.contains("just dev-down")),
            "the report must name the task that removes them: {report:?}"
        );
    }

    #[test]
    fn a_failing_provision_leaves_no_claim_of_its_own_and_takes_no_neighbours_with_it() {
        // The claim `abandoned`'s doc rests on - "nothing a harness reads can point at a
        // half-provisioned tier" - was not true: `run_up` never forgot, so a file an earlier
        // successful run published outlived every failure path, naming an ephemeral port the
        // recreated container no longer owns. `run_down`'s killed-`down` arm returned above its
        // `forget` for the same effect.
        //
        // Asserted on the ORDER and not on the call: the closure sees the claim already gone,
        // which is the property a `forget` bolted onto each early return does not have.
        //
        // **And on WHICH entries went.** `github.com/telekom/sutura#317`: this used to withdraw a
        // nix-native tier's claim over a server that was still running, because the file was the
        // granularity rather than one provisioner's entries. So the fixture carries a neighbour's
        // entry - written as the text `nix/tier-endpoints.nix` merges, since a fixture minted
        // through `publish` would only prove the tool preserves its own writes - and this cell
        // fails if a `dev-up` that never even reached docker takes it away.
        let root = std::env::temp_dir().join(format!("sutura-forget-{}", std::process::id()));
        let state = root.join(".sutura-dev");
        std::fs::create_dir_all(&state).expect("temp dirs are creatable");
        std::fs::write(
            state.join("endpoints.json"),
            r#"{"project":"sutura","services":{"keycloak":{"host":"127.0.0.1","port":51999,"provisioner":"nix"}}}"#,
        )
        .expect("the fixture is writable");
        let scope = sutura_dev::scope::Scope::from_root(&root).expect("a real directory is a scope");
        let published = sutura_dev::discovery::publish(&scope, &[("clickhouse", String::from("127.0.0.1:60660"))])
            .expect("the temp worktree is writable");
        assert!(published.is_file(), "the fixture did not publish anything to forget");

        let mut seen_by_the_tier_change = None;
        let outcome: Result<(), Verdict> = super::tier::with_endpoints_forgotten("dev-up", &scope, || {
            seen_by_the_tier_change = sutura_dev::discovery::Endpoints::discover(&scope)
                .ok()
                .map(|found| found.endpoint("clickhouse").is_ok());
            Err(Verdict::Fail)
        });

        assert_eq!(outcome, Err(Verdict::Fail), "the failure has to propagate unchanged");
        assert_eq!(
            seen_by_the_tier_change,
            Some(false),
            "this task's own claim was still readable while the tier was being changed"
        );
        let survivors = sutura_dev::discovery::Endpoints::discover(&scope)
            .expect("the neighbour's entry keeps the file alive")
            .services()
            .map(|(name, _endpoint)| String::from(name))
            .collect::<Vec<_>>();
        assert_eq!(
            survivors,
            vec![String::from("keycloak")],
            "a failed dev-up withdrew an entry it did not publish, or kept one it did"
        );
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn the_identity_provider_starts_only_when_it_is_asked_for() {
        // The reviewer's question turned into a mechanism: `dev-up` with no flag does not start
        // keycloak, so CI does not pay for a service nothing here can use yet.
        let default_set = super::expected_services(&[]);
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
        // Named as well as walked: the `DataHub` stack carries three NAMED VOLUMES, so a destroy
        // that ran without its profile would leave a MySQL data directory and an OpenSearch index
        // behind - and the next `dev-up` would come back onto another branch's migration.
        assert!(every.contains(&"datahub"), "{every:?}");

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

    /// The one variable every container in this repository is reached through.
    const REGISTRY_VARIABLE: &str = "SUTURA_IMAGE_REGISTRY";

    /// Every registry default the compose tier's `image:` lines name, deduplicated.
    ///
    /// A helper rather than a local, because the compose file is no longer the only place here that
    /// names a container: `pixi.toml`'s gcloud login task runs one too, and it CANNOT spell the
    /// fallback the compose file's way - pixi runs a task in its own shell, which does not
    /// implement `${VAR:-default}` - so the two agree by comparison or they do not agree at all.
    fn registry_defaults(text: &str) -> Vec<String> {
        let images: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter_map(|line| line.strip_prefix("image: "))
            .collect();
        assert!(images.len() >= 2, "the file should declare several images: {images:?}");

        let mut defaults: Vec<String> = Vec::new();
        for image in &images {
            let default = image
                .strip_prefix("${")
                .and_then(|rest| rest.split('}').next())
                .and_then(|inside| inside.split(":-").nth(1));
            let Some(default) = default else {
                panic!("`{image}` does not come through a registry variable with a default");
            };
            defaults.push(String::from(default));
        }
        defaults.sort_unstable();
        defaults.dedup();
        defaults
    }

    #[test]
    fn every_image_is_reached_through_one_registry_variable() {
        // A network behind a registry mirror has NO route to the public registry, and an unprefixed
        // reference does not fall back - it fails. So a bare `image: postgres:18-alpine` is not a
        // style problem, it is a service that cannot start there. Reviewed once, checked from now on.
        let Some(text) = compose_text() else { return };
        // ONE value an operator sets. Two defaults that disagree is the drift this catches: half the
        // tier would follow the override and half would not, and only one service would fail.
        let defaults = registry_defaults(&text);
        assert_eq!(defaults.len(), 1, "the registry defaults disagree: {defaults:?}");
    }

    #[test]
    fn the_gcloud_login_container_follows_the_same_registry_default() {
        // The second place a container is named, and the one a reading of the compose file misses.
        // `pixi.toml`'s `gl` task runs the Google Cloud CLI to authenticate a developer, and on a
        // network behind a registry mirror an unprefixed reference fails there for exactly the
        // reason it fails for a service. It cannot be written the compose file's way, so the shapes
        // differ and only the DEFAULT can be compared - which is the half that drifts: a mirror
        // override would move the tier and leave the login pointing at a registry with no route.
        let Some(compose) = compose_text() else { return };
        let Some(root) = crate::repo::root() else { return };
        let Ok(pixi) = std::fs::read_to_string(root.join("pixi.toml")) else {
            return;
        };

        let defaults = registry_defaults(&compose);
        let Some(expected) = defaults.first() else {
            panic!("the compose tier names no registry default to compare against");
        };

        // Comments are skipped because the argument for the variable is written out beside the
        // task, and prose naming it is not a reference to it.
        let references: Vec<&str> = pixi
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#') && line.contains(REGISTRY_VARIABLE))
            .collect();
        // FAIL CLOSED. None found means either the login task stopped reaching the registry
        // variable or this scan stopped finding it, and both are the defect this test is for.
        assert!(
            !references.is_empty(),
            "no line in pixi.toml reaches a container through `{REGISTRY_VARIABLE}`"
        );
        for line in &references {
            assert!(
                line.contains(expected.as_str()),
                "pixi.toml reaches a container through `{REGISTRY_VARIABLE}` but its default is \
                 not the compose tier's `{expected}`: {line}"
            );
        }
    }

    #[test]
    fn no_service_in_the_tier_is_built_here() {
        // The other half of the registry rule, and the half a reading passes. `image:` going through
        // the variable is checked above; a service with `build:` has no `image:` line to check, so it
        // slips past that test entirely - and a build fetches from wherever its Dockerfile points,
        // which the variable cannot prefix because it prefixes IMAGES.
        //
        // This is not hypothetical: the one thing somebody would reach for a `build:` to do here is
        // put an OAuth token validator into Postgres, since core ships none and upstream's stub
        // reaches no installed artifact. That build would `curl` the PostgreSQL source, which works
        // on a laptop and fails exactly on the network the variable was added for. A derived image
        // published to that registry and referenced by `image:` is the route that works, and this
        // check permits it - it bans building here, not deriving.
        let Some(text) = compose_text() else { return };

        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            assert!(
                !(trimmed == "build:" || trimmed.starts_with("build: ")),
                "{}:{}: a service built here bypasses the registry variable - publish a derived \
                 image to that registry and reference it with `image:` instead",
                super::docker::COMPOSE_FILE,
                index + 1
            );
        }
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
            // Every key in the tier that carries a credential VALUE, per service. A key added by a
            // new service and not added here is the drift this cannot see, which is why the list is
            // grown by the change that adds the service - the `MYSQL_*` and `EBEAN_*` keys arrived
            // with `DataHub`, whose GMS reaches the same database the store declares, so a literal
            // in either place is two definitions of one password.
            let is_credential = line.starts_with("POSTGRES_PASSWORD:")
                || line.starts_with("CLICKHOUSE_PASSWORD:")
                || line.starts_with("KC_BOOTSTRAP_ADMIN_PASSWORD:")
                || line.starts_with("MYSQL_PASSWORD:")
                || line.starts_with("MYSQL_ROOT_PASSWORD:")
                || line.starts_with("EBEAN_DATASOURCE_PASSWORD:")
                || line.starts_with("POSTGRES_USER:")
                || line.starts_with("CLICKHOUSE_USER:")
                || line.starts_with("MYSQL_USER:")
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
        if let Some((number, problem)) = first_port_problem(&text) {
            panic!("{}:{number}: {problem}", path.display());
        }
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            let number = index + 1;
            assert!(
                !trimmed.starts_with("container_name:"),
                "{}:{number}: a literal container name collides between worktrees",
                path.display()
            );
        }
    }

    const MISSING_LOOPBACK: &str = "a long-form port needs its own loopback `host_ip`";
    const INLINE_PORT: &str = "flow-style ports are unsupported because their fields evade this guard";

    fn first_port_problem(text: &str) -> Option<(usize, &'static str)> {
        let mut ports_indent = None;
        let mut long_port = None;

        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            let trimmed = line.trim();
            let indent = line.len() - line.trim_start().len();

            if let Some(parent_indent) = ports_indent {
                if !trimmed.is_empty() && !trimmed.starts_with('#') && indent <= parent_indent {
                    if let Some((target_line, _, false)) = long_port {
                        return Some((target_line, MISSING_LOOPBACK));
                    }
                    ports_indent = None;
                    long_port = None;
                } else {
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    if let Some(entry) = trimmed.strip_prefix("- ") {
                        if let Some((target_line, _, false)) = long_port {
                            return Some((target_line, MISSING_LOOPBACK));
                        }
                        long_port = None;
                        let value = entry.trim().trim_matches('"');
                        if value.starts_with('{') || value.starts_with('[') {
                            return Some((number, INLINE_PORT));
                        }
                        if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
                            continue;
                        }
                        let Some(target) = value.strip_prefix("target:") else {
                            return Some((number, "a port entry is not an ephemeral container port"));
                        };
                        let target = target.trim();
                        if target.is_empty() || !target.chars().all(|c| c.is_ascii_digit()) {
                            return Some((number, "a long-form target must be a literal container port"));
                        }
                        long_port = Some((number, indent, false));
                        continue;
                    }
                    if trimmed.starts_with("published:") {
                        return Some((number, "a long-form port names a fixed host port"));
                    }
                    if let Some(host) = trimmed.strip_prefix("host_ip:") {
                        let Some((_, item_indent, has_loopback)) = long_port.as_mut() else {
                            return Some((number, "a `host_ip` is not attached to a long-form port"));
                        };
                        if indent <= *item_indent || *has_loopback {
                            return Some((number, "a long-form port has a misplaced or duplicate `host_ip`"));
                        }
                        if host.trim() != "127.0.0.1" {
                            return Some((number, "a published port must bind to loopback"));
                        }
                        *has_loopback = true;
                        continue;
                    }
                    return Some((number, "an unsupported long-form port field evades this guard"));
                }
            }

            if trimmed.starts_with("ports:") {
                if trimmed != "ports:" {
                    return Some((number, INLINE_PORT));
                }
                ports_indent = Some(indent);
            }
        }

        let (target_line, _, has_loopback) = long_port?;
        (!has_loopback).then_some((target_line, MISSING_LOOPBACK))
    }

    #[test]
    fn the_datahub_platform_starts_only_when_it_is_asked_for() {
        // Five containers, three of them JVMs, and a reindexing migration. The profile is what keeps
        // that off the runs that do not want it, and this is that claim read off the registry rather
        // than off the compose file - the two agree because `--profile` is derived from `SERVICES`.
        let default_set = super::expected_services(&[]);
        assert!(
            !default_set.contains(&"datahub"),
            "the default set must not include the metadata platform: {default_set:?}"
        );

        let with_datahub = super::expected_services(&["datahub"]);
        assert!(with_datahub.contains(&"datahub"), "{with_datahub:?}");
        // And asking for one profile must not drag the other in: the two are independent costs.
        assert!(!with_datahub.contains(&"keycloak"), "{with_datahub:?}");
    }

    #[test]
    fn every_service_that_publishes_a_port_is_a_registered_service() {
        // THE seam this whole tier keys off, checked in the one direction nothing else covers.
        // `read_back_ports` walks `SERVICES` and asks docker for a host port; a compose service that
        // publishes a port and is NOT registered is an endpoint no discovery file ever carries, so a
        // test reaching for it gets a missing key rather than an address - and the provision reports
        // success, because nothing asked. The other direction fails loudly already: a registered
        // service publishing nothing fails the provision at the read-back.
        //
        // Written when the `DataHub` stack arrived, because that block is the first one where a
        // service publishing a port and a service merely EXISTING stopped being the same thing:
        // four of its five containers publish nothing on purpose.
        let Some(text) = compose_text() else { return };
        let registered: Vec<&str> = sutura_dev::scope::SERVICES
            .iter()
            .map(sutura_dev::scope::Service::name)
            .collect();

        // A service block is a two-space key under `services:`; a `ports:` key is four-space, so the
        // service a `ports:` belongs to is the last two-space key seen above it.
        let mut current: Option<&str> = None;
        let mut publishing: Vec<&str> = Vec::new();
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("  ")
                && !name.starts_with(' ')
                && !name.starts_with('#')
                && let Some(name) = name.strip_suffix(':')
                && !name.contains(' ')
            {
                current = Some(name);
            }
            if line.trim() == "ports:"
                && let Some(name) = current
                && !publishing.contains(&name)
            {
                publishing.push(name);
            }
        }

        assert!(
            !publishing.is_empty(),
            "no service in the tier publishes a port - this scan broke"
        );
        for name in &publishing {
            assert!(
                registered.contains(name),
                "`{name}` publishes a host port and is not in `sutura_dev::scope::SERVICES`, so \
                 nothing writes its address into the discovery file: {registered:?}"
            );
        }
    }

    #[test]
    fn no_service_in_the_tier_mounts_a_host_directory() {
        // A named volume is prefixed with the compose project name, so it belongs to one worktree. A
        // HOST PATH belongs to the machine, and is therefore shared by every worktree on it - which
        // is the one failure this whole tier exists to prevent, arriving through the volume list
        // instead of through a port.
        //
        // Not hypothetical, and that is why it is a check: upstream `DataHub`'s quickstart binds
        // `${HOME}/.datahub/plugins` and `${HOME}/.datahub/search` into two of its containers. Both
        // were dropped when that stack was reproduced here, and a check is what stops the next
        // faithful copy of an upstream compose file bringing them back.
        let Some(text) = compose_text() else { return };
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            let Some(entry) = trimmed.strip_prefix("- ") else { continue };
            let value = entry.trim().trim_matches('"');
            // A bind mount's source is a path: it starts with `/`, `.` or `~`, or interpolates one.
            let is_host_path = value.starts_with('/')
                || value.starts_with("./")
                || value.starts_with("../")
                || value.starts_with('~')
                || value.starts_with("${HOME")
                || value.starts_with("${PWD");
            assert!(
                !(is_host_path && value.contains(':')),
                "{}:{}: `{value}` mounts a host path, which every worktree on this machine shares \
                 - use a named volume, which compose prefixes with the project name",
                super::docker::COMPOSE_FILE,
                index + 1
            );
        }
    }
}
