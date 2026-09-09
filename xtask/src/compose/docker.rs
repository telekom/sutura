//! The container-runtime surface, and the pure functions that read what it printed.
//!
//! Everything that shells out is one thin function; everything that DECIDES is a pure function
//! beside it, so the deciding half is unit-tested without docker. That split is not tidiness: the
//! interesting failures here - a service that never becomes healthy, a published address that is a
//! bind address rather than a connectable one, a project listing that contains a neighbour's
//! worktree - are all decisions about text, and a test that needed a daemon to reach them would not
//! be written.
//!
//! **Docker is a host dependency this repository deliberately does not pin with nix**, so nothing
//! here may assume a version. What it assumes is the Compose v2 CLI: `docker compose`, `--format
//! json`, and `compose ps` emitting either a JSON array or one object per line - both spellings are
//! accepted below, because which one you get depends on the version somebody has installed.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Budgets by kind of call, the one bounded wait, and the capture that makes bounding possible.
/// Its own module because a per-subcommand budget is a separable concern from what the runtime is
/// asked, and because this file is near its line budget.
mod bounded;

/// The floor under every budget, reached by the sibling whose deadline goes through
/// [`budget_from_env`] so that its clamp test asserts the constant instead of a copy of the number.
/// Test-only because production has no reason to name it - the clamp is `budget_from_env`'s job.
#[cfg(test)]
pub(in crate::compose) use bounded::TIMEOUT_MIN_SECS;
pub(in crate::compose) use bounded::budget_from_env;
use bounded::{ANSWER_TIMEOUT_MAX_SECS, WEDGED_DAEMON_REMEDY, run, waited};
pub(crate) use bounded::{Budget, Failed, Output};

/// The compose file, relative to the repository root. One file; per-worktree values come from the
/// project name and from ephemeral publishing, never from a second file.
///
/// **Not `compose.dev.yaml`**, which is the dev-CONTAINER wrapper and has nothing to do with
/// services - `docs/adr/0007` records that this repository had no compose file for services and
/// that standing one up is this tier's work.
pub(crate) const COMPOSE_FILE: &str = "compose.services.yaml";

/// What is missing before anything can be provisioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Missing {
    /// No `docker` on `PATH`.
    Cli,
    /// `docker` is there; the Compose v2 plugin is not.
    ComposePlugin,
    /// Both are there and the daemon REFUSED. A stopped Docker Desktop is this case.
    Daemon,
    /// Both are there and the daemon never answered at all - it accepted the probe and returned
    /// nothing inside its budget.
    ///
    /// Separate from [`Self::Daemon`] because the two send a reader to opposite actions: a stopped
    /// daemon needs starting, and one that is already running needs RESTARTING. Telling somebody on
    /// a wedged host to "start the docker daemon" is advice they have already taken. It is also the
    /// only variant that means *this machine has the tier and it is faulty* rather than *this
    /// machine does not have the tier*, which is why [`super::absent`] refuses to skip on it.
    WedgedDaemon,
}

impl Missing {
    /// What a reader has to install, start or restart, in the words its own documentation uses.
    pub(crate) const fn remedy(self) -> &'static str {
        match self {
            Self::Cli => "install docker (a host dependency; nix deliberately does not pin it)",
            Self::ComposePlugin => "install the Compose v2 plugin - `docker compose version` must work",
            Self::Daemon => "start the docker daemon - `docker info` must answer",
            // The same sentence a status query that ran out of budget prints, from the module that
            // detects both: one condition, one wording.
            Self::WedgedDaemon => WEDGED_DAEMON_REMEDY,
        }
    }

    /// May a machine class that treats an absent runtime as optional skip on this?
    ///
    /// No for exactly one variant. The optional direction exists for a machine that does not have
    /// docker at all - nix deliberately does not pin it, so that is a legitimate configuration and
    /// skipping is right. A daemon that is installed, running and not answering is not that machine:
    /// it is a FAULT on a machine that does have the tier, and skipping it produces the outcome the
    /// tier exists to prevent - a green run that provisioned nothing.
    pub(crate) const fn may_be_skipped(self) -> bool {
        !matches!(self, Self::WedgedDaemon)
    }
}

/// How long ONE probe may wait for an answer before the runtime counts as absent.
///
/// A WEDGED daemon is the case this exists for, and it is not hypothetical. Docker Desktop can go on
/// accepting on its socket while never answering, and the CLI then waits forever because it has no
/// timeout of its own. Measured on a developer machine here: `docker version` and `docker info`
/// were both still running when a 25s external timeout killed them, twice, fifty minutes apart.
///
/// What that cost is the reason this is a constant and not a nicety. [`presence`] is the first thing
/// every gate that touches a service calls, so an unbounded probe does not FAIL a gate - it HANGS
/// one, with no output naming the cause. On that machine `just test`, `just causality`,
/// `just ship-check` and every pre-commit tier blocked indefinitely, all of them behind the single
/// unit test that calls this function.
///
/// **What this does NOT bound.** Bounding [`presence`] closes the door a gate opens FIRST, and that
/// is not the only door: a daemon that wedges *after* the pre-flight passed is a different failure,
/// closed by [`Budget`] rather than by this constant, because one budget cannot serve every
/// subcommand. What neither of them reaches is the descendant limit [`bounded::waited`] states.
///
/// Ten seconds because the question is "is a daemon answering at all", not "is it quick": a busy
/// daemon on a cold start answers in a second or two, and nothing here needs to tell slow from dead
/// more finely than that. Three probes, so a fully wedged host costs 30s once instead of forever.
const PROBE_TIMEOUT_SECS: u64 = 10;

/// The per-probe budget, overridable for a host where ten seconds is genuinely too few.
fn probe_budget() -> Duration {
    budget_from_env(
        "SUTURA_DOCKER_PROBE_TIMEOUT_SECS",
        PROBE_TIMEOUT_SECS,
        ANSWER_TIMEOUT_MAX_SECS,
    )
}

/// Is there a usable container runtime, and if not, which part is absent?
///
/// Three probes rather than one, because "docker is missing" and "docker is not running" send a
/// reader to different places and the difference costs nothing to report.
///
/// Every probe is bounded (see [`PROBE_TIMEOUT_SECS`]), so this ANSWERS on a wedged host rather than
/// blocking its caller, and a daemon that went SILENT is reported apart from one that REFUSED -
/// [`Missing::WedgedDaemon`] against [`Missing::Daemon`] - because the remedy and the skip direction
/// both differ.
///
/// The limit worth stating: the distinction is drawn only for the daemon. A `docker` binary that
/// hangs on `--version` is reported as no CLI at all, which is approximate. It stays approximate on
/// purpose - a hanging `--version` is a broken installation rather than a running service, so the
/// remedy still points at the right half of the problem, and the two remaining variants would each
/// need a caller that acted on them differently to be worth carrying.
pub(crate) fn presence() -> Result<(), Missing> {
    let budget = probe_budget();
    if probed(Command::new("docker").arg("--version"), budget) != Probe::Answered {
        return Err(Missing::Cli);
    }
    if probed(Command::new("docker").args(["compose", "version"]), budget) != Probe::Answered {
        return Err(Missing::ComposePlugin);
    }
    match probed(
        Command::new("docker").args(["info", "--format", "{{.ServerVersion}}"]),
        budget,
    ) {
        Probe::Answered => Ok(()),
        Probe::Silent => Err(Missing::WedgedDaemon),
        Probe::Refused => Err(Missing::Daemon),
    }
}

/// What a probe did. Three outcomes rather than a `bool`, because the difference between *said no*
/// and *said nothing* is the one that decides whether a gate may skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe {
    /// It ran and exited zero.
    Answered,
    /// It ran and exited non-zero, or could not be started at all.
    Refused,
    /// It never answered inside its budget.
    Silent,
}

/// The bounded wait, with the budget passed in so the deciding half is testable without a daemon
/// and without spending ten seconds to watch it work.
///
/// **Fails closed at the probe.** A command that has not answered inside its budget is reported as
/// [`Probe::Silent`] rather than waited on, so a wedged daemon reaches a caller that can act on it
/// instead of a caller that never returns. Answering the other way would be worse than the hang it
/// replaces: it would report a runtime that cannot serve a container as present.
///
/// The limit this one adds to [`bounded::waited`]'s: a spawn that FAILS is [`Probe::Refused`], not
/// [`Probe::Silent`]. That is the right way round - a binary that is not there refused - but it
/// means `Silent` is specifically *started and did not answer*, which is what makes it safe for
/// [`Missing::may_be_skipped`] to key on.
fn probed(command: &mut Command, budget: Duration) -> Probe {
    let spawned = command
        // stdin included: a probe inheriting a terminal can consume a keystroke meant for the hook
        // that ran it, and nothing here reads input.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else { return Probe::Refused };

    match waited(&mut child, budget) {
        Ok(Some(status)) if status.success() => Probe::Answered,
        // A non-zero exit and a wait that errors are the same answer: it will not report success,
        // and neither is the silence that vetoes a skip.
        Ok(Some(_)) | Err(_) => Probe::Refused,
        Ok(None) => Probe::Silent,
    }
}

/// The arguments that scope every compose invocation to one worktree.
///
/// **One function, used by start and by stop alike.** Rule 3 of the teardown contract in
/// `sutura_dev::scope` is about exactly this: a project passed only as a flag does not populate the
/// variable an override file interpolates, so a compose file that puts the project name into a
/// network or a volume resolves it from an unset variable at destroy time - and the destroy targets
/// the wrong network while reporting success. So the project goes in twice, here and in
/// [`environment`], and both come from one place.
///
/// `profiles` are the compose profiles to activate. They go BEFORE the subcommand, because
/// `--profile` is a flag on `docker compose` and not on `up` - and a service in an inactive profile
/// is invisible to every subcommand, `down` included, which is why teardown passes them all.
pub(crate) fn scoped_args(root: &Path, project: &str, profiles: &[&str]) -> Vec<String> {
    let mut args = vec![
        String::from("compose"),
        String::from("--file"),
        root.join(COMPOSE_FILE).to_string_lossy().into_owned(),
        String::from("--project-directory"),
        root.to_string_lossy().into_owned(),
        String::from("--project-name"),
        String::from(project),
    ];
    for profile in profiles {
        args.push(String::from("--profile"));
        args.push(String::from(*profile));
    }
    args
}

/// The environment every compose invocation carries. The other half of rule 3.
pub(crate) fn environment(project: &str) -> Vec<(&'static str, String)> {
    vec![("COMPOSE_PROJECT_NAME", String::from(project))]
}

/// Run one compose subcommand, scoped to this worktree, inside the budget its class allows.
pub(crate) fn compose(root: &Path, project: &str, profiles: &[&str], extra: &[&str]) -> Result<Output, Failed> {
    let mut command = Command::new("docker");
    command.current_dir(root);
    command.args(scoped_args(root, project, profiles));
    command.args(extra);
    for (name, value) in environment(project) {
        command.env(name, value);
    }
    run(&mut command, Budget::of(extra))
}

/// The published address to record, out of what `docker compose port` printed.
///
/// It can print more than one line - an IPv4 binding and an IPv6 one - and reading the whole output
/// as a single address produces a host of `0.0.0.0:32768\n[::]` that parses as neither. The IPv4
/// line is preferred where there is one, because it is the form every client library here connects
/// to without configuration.
pub(crate) fn first_published(stdout: &str) -> Option<&str> {
    let mut fallback = None;
    for line in stdout.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !line.starts_with('[') {
            return Some(line);
        }
        fallback = fallback.or(Some(line));
    }
    fallback
}

/// One service, as `docker compose ps` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reported {
    /// The compose service name.
    pub(crate) service: String,
    /// `running`, `exited`, `restarting`, and so on.
    pub(crate) state: String,
    /// `healthy`, `starting`, `unhealthy`, or empty when the service declares no health check.
    pub(crate) health: String,
}

/// Read `docker compose ps --format json`.
///
/// Two shapes are accepted because two shapes exist in the wild: a JSON array, and one object per
/// line. Guessing wrong reads as "no services are up", which would turn a readiness gate into a
/// timeout with no diagnostic - so both are parsed rather than one being assumed.
pub(crate) fn parse_ps(text: &str) -> Vec<Reported> {
    if let Ok(serde_json::Value::Array(rows)) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        return rows.iter().filter_map(reported).collect();
    }
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| reported(&value))
        .collect()
}

/// One `ps` row, if it names a service.
fn reported(value: &serde_json::Value) -> Option<Reported> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_lowercase()
    };
    let service = text("Service");
    if service.is_empty() {
        return None;
    }
    Some(Reported {
        service,
        state: text("State"),
        health: text("Health"),
    })
}

/// Whether every expected service is ready, and if not, why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Readiness {
    /// Every expected service is up and, where it declares a health check, healthy.
    Ready,
    /// Still coming up. Named, so a timeout says which service never arrived.
    Waiting(Vec<String>),
    /// One of them will not arrive: exited, or reported unhealthy.
    Failed(Vec<String>),
}

/// The health gate, as a pure function over what `ps` reported.
///
/// **A gate rather than a sleep**, and the difference is what it does when it is wrong: a sleep
/// that is too short produces a connection refused inside a test, attributed to whatever the test
/// was doing. This says which service is not ready and stops.
///
/// A service reporting `unhealthy` or a dead state is [`Readiness::Failed`] rather than
/// [`Readiness::Waiting`], because waiting for something that has already lost is how a two-minute
/// timeout gets spent on a container whose log said what was wrong in the first second.
pub(crate) fn readiness(reported: &[Reported], expected: &[&str]) -> Readiness {
    let mut waiting = Vec::new();
    let mut failed = Vec::new();

    for name in expected {
        match reported.iter().find(|row| row.service == *name) {
            None => waiting.push(format!("{name}: no container yet")),
            Some(row) if row.health == "unhealthy" => failed.push(format!("{name}: unhealthy")),
            Some(row) if row.state == "exited" || row.state == "dead" => {
                failed.push(format!("{name}: {}", row.state));
            }
            Some(row) if row.state != "running" => waiting.push(format!("{name}: {}", row.state)),
            // Empty health means the service declares no health check. Running is all there is to
            // know, and treating that as "waiting" would hang forever on a service nobody probed.
            Some(row) if row.health.is_empty() || row.health == "healthy" => {}
            Some(row) => waiting.push(format!("{name}: {}", row.health)),
        }
    }

    if !failed.is_empty() {
        return Readiness::Failed(failed);
    }
    if waiting.is_empty() {
        return Readiness::Ready;
    }
    Readiness::Waiting(waiting)
}

/// Every compose project the runtime knows about, from `docker compose ls --all --format json`.
///
/// Read for teardown, so it can report a neighbour it deliberately did NOT touch. A `BTreeSet` for
/// stable output: this list reaches a reader who is deciding whether the tool is about to do
/// something to somebody else's containers.
pub(crate) fn parse_projects(text: &str) -> BTreeSet<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return BTreeSet::new();
    };
    let rows = match value {
        serde_json::Value::Array(rows) => rows,
        other => vec![other],
    };
    rows.iter()
        .filter_map(|row| row.get("Name").and_then(serde_json::Value::as_str))
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect()
}

/// What the runtime said when asked which compose projects exist.
///
/// **Two variants because emptiness and absence are different claims**, and collapsing them into
/// one `BTreeSet` made the tool state something no host had told it. An empty answer supports
/// "nothing else was running"; a host that refused or never answered supports nothing at all - and
/// it also does not support "this worktree has no project", which is how a destroy came to report
/// success having removed a tier that was still up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Listing {
    /// The runtime answered. Possibly with nothing, which is a legitimate answer.
    Answered(BTreeSet<String>),
    /// It refused, or never answered inside its budget.
    Unknown,
}

/// Ask the runtime what it knows. An unanswered listing does not stop a destroy, because what the
/// listing buys is the ability to report what was spared, not the authority to remove - but it is
/// carried as [`Listing::Unknown`] rather than as an empty answer, so nothing downstream reports a
/// spared neighbour, or an absent project, on a host that did not say.
pub(crate) fn projects(root: &Path) -> Listing {
    match unscoped(root, &["ls", "--all", "--format", "json"]) {
        Ok(out) if out.ok => Listing::Answered(parse_projects(&out.stdout)),
        Ok(out) => {
            eprintln!("xtask compose: `docker compose ls` failed - what else is running is unknown");
            eprint!("{}", out.stderr);
            Listing::Unknown
        }
        Err(cause) => {
            eprintln!("xtask compose: {cause} - what else is running is unknown");
            Listing::Unknown
        }
    }
}

/// Run one compose subcommand WITHOUT this worktree's scoping flags, inside its kind's budget.
///
/// One caller, and it earns a function rather than a comment: `compose ls` takes none of
/// [`scoped_args`]' flags - scoping the listing to one project is the opposite of what teardown
/// reads it for - and the budget has to be derived from the arguments that are actually run. A
/// `Budget::of` over a slice written beside the command line is a second place for *this call is an
/// `ls`* to live, and two places drift.
fn unscoped(root: &Path, extra: &[&str]) -> Result<Output, Failed> {
    let mut command = Command::new("docker");
    command.current_dir(root);
    command.arg("compose");
    command.args(extra);
    run(&mut command, Budget::of(extra))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::bounded::{ANSWER_TIMEOUT_MAX_SECS, TIMEOUT_MIN_SECS, answers_with, never_answers};
    use super::{
        Missing, Probe, Readiness, Reported, parse_projects, parse_ps, presence, probe_budget, probed, readiness, scoped_args,
    };

    fn row(service: &str, state: &str, health: &str) -> Reported {
        Reported {
            service: String::from(service),
            state: String::from(state),
            health: String::from(health),
        }
    }

    #[test]
    fn a_ps_listing_is_read_in_either_shape() {
        // Both are real: the shape depends on the compose version somebody has installed, and
        // guessing wrong would read as "nothing is up" and time out with no diagnostic.
        let array = r#"[{"Service":"postgres","State":"running","Health":"healthy"}]"#;
        let lines = "{\"Service\":\"postgres\",\"State\":\"running\",\"Health\":\"healthy\"}\n";
        let expected = vec![row("postgres", "running", "healthy")];
        assert_eq!(parse_ps(array), expected);
        assert_eq!(parse_ps(lines), expected);
        assert!(parse_ps("").is_empty(), "empty input parses to no rows");
        assert!(parse_ps("not json").is_empty(), "non-JSON input parses to no rows");
    }

    #[test]
    fn readiness_is_a_gate_and_not_a_sleep() {
        let expected = ["postgres", "clickhouse"];

        // Nothing yet: waiting, and it says which.
        assert_eq!(
            readiness(&[], &expected),
            Readiness::Waiting(vec![
                String::from("postgres: no container yet"),
                String::from("clickhouse: no container yet"),
            ])
        );

        // Running but still probing.
        assert_eq!(
            readiness(
                &[
                    row("postgres", "running", "starting"),
                    row("clickhouse", "running", "healthy")
                ],
                &expected
            ),
            Readiness::Waiting(vec![String::from("postgres: starting")])
        );

        // Both healthy.
        assert_eq!(
            readiness(
                &[row("postgres", "running", "healthy"), row("clickhouse", "running", "healthy")],
                &expected
            ),
            Readiness::Ready
        );

        // A service with no health check is ready when it is running - otherwise the gate would
        // wait forever for a probe nobody declared.
        assert_eq!(
            readiness(&[row("postgres", "running", ""), row("clickhouse", "running", "")], &expected),
            Readiness::Ready
        );
    }

    #[test]
    fn a_service_that_has_already_lost_fails_rather_than_being_waited_for() {
        // Waiting on a container that exited is how a two-minute timeout gets spent on a log line
        // that was available in the first second.
        let expected = ["postgres"];
        assert_eq!(
            readiness(&[row("postgres", "exited", "")], &expected),
            Readiness::Failed(vec![String::from("postgres: exited")])
        );
        assert_eq!(
            readiness(&[row("postgres", "running", "unhealthy")], &expected),
            Readiness::Failed(vec![String::from("postgres: unhealthy")])
        );
    }

    #[test]
    fn a_project_listing_is_read_in_either_shape() {
        let array = r#"[{"Name":"sutura-dev-aaaa1111"},{"Name":"something-else"}]"#;
        let found = parse_projects(array);
        assert!(found.contains("sutura-dev-aaaa1111"));
        assert!(found.contains("something-else"));
        assert_eq!(parse_projects(r#"{"Name":"sutura-dev-bbbb2222"}"#).len(), 1);
        assert!(parse_projects("nonsense").is_empty());
    }

    #[test]
    fn every_compose_invocation_names_one_project_and_one_file() {
        let args = scoped_args(Path::new("/repo"), "sutura-dev-aaaa1111", &[]);
        assert!(args.contains(&String::from("--project-name")));
        assert!(args.contains(&String::from("sutura-dev-aaaa1111")));
        assert!(args.iter().any(|a| a.ends_with("compose.services.yaml")));
        // The project directory is the worktree, so relative paths in the compose file and the
        // named volumes derived from the project both belong to this worktree and no other.
        assert!(args.contains(&String::from("/repo")));
    }

    #[test]
    fn a_two_line_published_address_is_read_as_one_address() {
        // `docker compose port` can print an IPv4 line and an IPv6 line. Reading the whole output
        // as one address gives a host of `0.0.0.0:32768\n[::]`, which is not an address at all -
        // and it would be written into the discovery file as if it were.
        assert_eq!(super::first_published("0.0.0.0:32768\n[::]:32768\n"), Some("0.0.0.0:32768"));
        assert_eq!(
            super::first_published("[::]:32768\n0.0.0.0:32768\n"),
            Some("0.0.0.0:32768"),
            "the IPv4 form is preferred wherever there is one"
        );
        assert_eq!(super::first_published("[::]:32768\n"), Some("[::]:32768"));
        assert_eq!(super::first_published("\n  \n"), None);
    }

    #[test]
    fn stop_is_given_the_project_the_same_way_start_is() {
        // Rule 3 of the teardown contract, as a property of the two functions rather than as a
        // sentence. The trap: a project passed only as a FLAG does not populate the variable an
        // override file interpolates, so a compose file that puts the project name into a network
        // or a volume resolves it from an unset variable at destroy time - and the destroy targets
        // the wrong network while reporting success. Both halves come from here, so they cannot
        // drift apart between the call that starts and the call that stops.
        let project = "sutura-dev-aaaa1111";
        assert!(scoped_args(Path::new("/repo"), project, &[]).contains(&String::from(project)));
        assert_eq!(
            super::environment(project),
            vec![("COMPOSE_PROJECT_NAME", String::from(project))]
        );
    }

    #[test]
    fn a_missing_part_says_what_to_do_about_it() {
        for missing in [Missing::Cli, Missing::ComposePlugin, Missing::Daemon, Missing::WedgedDaemon] {
            assert!(
                !missing.remedy().is_empty(),
                "every missing-part variant has a non-empty remedy"
            );
        }
        // The two daemon variants must not give the same advice. That is the whole reason they are
        // two: telling somebody whose daemon is running-but-silent to START it is advice they have
        // already taken, and this is what stops the pair collapsing back into one message.
        assert_ne!(Missing::Daemon.remedy(), Missing::WedgedDaemon.remedy());

        // Not an assertion about this machine: `presence` is allowed to say either thing. What is
        // asserted is that it answers within a BOUND - the half this test used to leave out. On a
        // host whose daemon had wedged it blocked here forever and took `just test` and every
        // pre-commit tier with it, so the test meant to prove the probe usable was the thing hanging.
        let started = Instant::now();
        let answer = presence();
        let waited = started.elapsed();
        // Three bounded probes plus slack for a loaded machine. Derived from the budget rather than
        // written as a number, so raising `SUTURA_DOCKER_PROBE_TIMEOUT_SECS` does not redden this;
        // the clamp in `probe_budget` is what keeps this multiplication from overflowing.
        let ceiling = probe_budget() * 3 + Duration::from_secs(10);
        assert!(waited < ceiling, "`presence` was not bounded: waited {waited:?}");
        // Whatever it decided, only a silent daemon may refuse to be skipped.
        if let Err(missing) = answer {
            assert_eq!(missing.may_be_skipped(), missing != Missing::WedgedDaemon);
        }
    }

    #[test]
    fn a_probe_that_never_answers_is_bounded_and_reported_silent() {
        // The wedged daemon, which is the whole reason a budget exists. Before it, this shape did not
        // fail a gate - it hung one, and a hang has no output, so it read as a slow machine.
        let budget = Duration::from_millis(250);
        let started = Instant::now();
        let outcome = probed(&mut never_answers(), budget);
        let waited = started.elapsed();

        assert_eq!(
            outcome,
            Probe::Silent,
            "a probe that never answered must be Silent, not Refused"
        );
        // THE ASSERTION THAT MAKES THIS TEST NON-VACUOUS. Without it every fast failure passes:
        // a missing interpreter, a missing `sleep`, a syntax error - each returns in about a
        // millisecond and satisfies both of the other two assertions. This one says the budget was
        // actually spent, so the test cannot go green without reaching the timeout branch.
        assert!(waited >= budget, "the budget was never consumed - waited only {waited:?}");
        // And still bounded. Generous on purpose: what is asserted is BOUNDED, not fast, so a loaded
        // CI runner may be slow without turning this into a flake.
        assert!(
            waited < Duration::from_secs(10),
            "the probe was not bounded: waited {waited:?}"
        );
    }

    #[test]
    fn a_probe_that_answers_is_read_rather_than_timed_out() {
        // The other direction, and the one that catches a budget so tight that a healthy daemon reads
        // as absent. A bound that fails closed on everything is not a bound, it is an outage that
        // still prints a reason.
        let budget = Duration::from_secs(30);
        assert_eq!(probed(&mut answers_with(0), budget), Probe::Answered);
        // A refusal is NOT silence: only silence may refuse to be skipped, so conflating them would
        // turn every machine without docker into a hard failure.
        assert_eq!(probed(&mut answers_with(1), budget), Probe::Refused);
    }

    #[test]
    fn only_a_silent_daemon_refuses_to_be_skipped() {
        // The direction the gate reads. A machine with no docker is a legitimate configuration and
        // skips; a daemon that is installed, running and not answering is a fault and must not.
        assert!(Missing::Cli.may_be_skipped());
        assert!(Missing::ComposePlugin.may_be_skipped());
        assert!(Missing::Daemon.may_be_skipped());
        assert!(!Missing::WedgedDaemon.may_be_skipped());
    }

    #[test]
    fn the_probe_budget_is_clamped_at_both_ends() {
        // Read through the public path so the clamp cannot be bypassed by a caller.
        let budget = probe_budget();
        assert!(budget >= Duration::from_secs(TIMEOUT_MIN_SECS), "{budget:?}");
        assert!(budget <= Duration::from_secs(ANSWER_TIMEOUT_MAX_SECS), "{budget:?}");
        // The multiplication the ceiling assertion above performs must not overflow at the extreme.
        let _ceiling: Duration = Duration::from_secs(ANSWER_TIMEOUT_MAX_SECS) * 3;
    }
}
