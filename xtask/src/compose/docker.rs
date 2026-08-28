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
use std::process::Command;

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
    /// Both are there and no daemon answered. A stopped Docker Desktop is this case.
    Daemon,
}

impl Missing {
    /// What a reader has to install or start, in the words its own documentation uses.
    pub(crate) const fn remedy(self) -> &'static str {
        match self {
            Self::Cli => "install docker (a host dependency; nix deliberately does not pin it)",
            Self::ComposePlugin => "install the Compose v2 plugin - `docker compose version` must work",
            Self::Daemon => "start the docker daemon - `docker info` must answer",
        }
    }
}

/// Is there a usable container runtime, and if not, which part is absent?
///
/// Three probes rather than one, because "docker is missing" and "docker is not running" send a
/// reader to different places and the difference costs nothing to report.
pub(crate) fn presence() -> Result<(), Missing> {
    if !succeeds(Command::new("docker").arg("--version")) {
        return Err(Missing::Cli);
    }
    if !succeeds(Command::new("docker").args(["compose", "version"])) {
        return Err(Missing::ComposePlugin);
    }
    if !succeeds(Command::new("docker").args(["info", "--format", "{{.ServerVersion}}"])) {
        return Err(Missing::Daemon);
    }
    Ok(())
}

/// Did the command run and exit zero? Output discarded: this is a probe, not a query.
fn succeeds(command: &mut Command) -> bool {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The arguments that scope every compose invocation to one worktree.
///
/// **One function, used by start and by stop alike.** Rule 3 of the teardown contract in
/// `sutura_dev::scope` is about exactly this: a project passed only as a flag does not populate the
/// variable an override file interpolates, so a compose file that puts the project name into a
/// network or a volume resolves it from an unset variable at destroy time - and the destroy targets
/// the wrong network while reporting success. So the project goes in twice, here and in
/// [`environment`], and both come from one place.
pub(crate) fn scoped_args(root: &Path, project: &str) -> Vec<String> {
    vec![
        String::from("compose"),
        String::from("--file"),
        root.join(COMPOSE_FILE).to_string_lossy().into_owned(),
        String::from("--project-directory"),
        root.to_string_lossy().into_owned(),
        String::from("--project-name"),
        String::from(project),
    ]
}

/// The environment every compose invocation carries. The other half of rule 3.
pub(crate) fn environment(project: &str) -> Vec<(&'static str, String)> {
    vec![("COMPOSE_PROJECT_NAME", String::from(project))]
}

/// What a compose invocation produced.
pub(crate) struct Output {
    /// Standard output, as text.
    pub(crate) stdout: String,
    /// Standard error, as text. Printed on failure; a runtime's diagnostic is the useful one.
    pub(crate) stderr: String,
    /// Did it exit zero?
    pub(crate) ok: bool,
}

/// Run one compose subcommand, scoped to this worktree.
pub(crate) fn compose(root: &Path, project: &str, extra: &[&str]) -> Result<Output, std::io::Error> {
    let mut command = Command::new("docker");
    command.current_dir(root);
    command.args(scoped_args(root, project));
    command.args(extra);
    for (name, value) in environment(project) {
        command.env(name, value);
    }
    let out = command.output()?;
    Ok(Output {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        ok: out.status.success(),
    })
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

/// Ask the runtime what it knows. Failure is an empty listing rather than an error, because
/// teardown must still be able to remove THIS worktree's project when the listing is unavailable -
/// what the listing buys is the ability to report what was spared, not the authority to destroy.
pub(crate) fn projects(root: &Path) -> BTreeSet<String> {
    Command::new("docker")
        .current_dir(root)
        .args(["compose", "ls", "--all", "--format", "json"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| parse_projects(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Missing, Readiness, Reported, parse_projects, parse_ps, presence, readiness, scoped_args};

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
        assert!(parse_ps("").is_empty());
        assert!(parse_ps("not json").is_empty());
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
        let args = scoped_args(Path::new("/repo"), "sutura-dev-aaaa1111");
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
        assert!(scoped_args(Path::new("/repo"), project).contains(&String::from(project)));
        assert_eq!(
            super::environment(project),
            vec![("COMPOSE_PROJECT_NAME", String::from(project))]
        );
    }

    #[test]
    fn a_missing_part_says_what_to_do_about_it() {
        for missing in [Missing::Cli, Missing::ComposePlugin, Missing::Daemon] {
            assert!(!missing.remedy().is_empty());
        }
        // Not an assertion about this machine: `presence` is allowed to say either thing. What is
        // asserted is that it answers rather than panicking, on a host with docker and on one
        // without - which is the property every other test here depends on.
        let answer = presence();
        assert!(answer.is_ok() || answer.is_err());
    }
}
