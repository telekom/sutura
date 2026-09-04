//! `sutura-dev` - the developer workflow that needs to know which worktree it is in.
//!
//! Why a separate binary rather than another `xtask` task: xtask holds *gates*, which answer
//! "does this repo violate a rule?" and are run by hooks and CI. This holds *actions* on a
//! developer's own tree - create a worktree, tell me where I am.
//!
//! **Starting a container is NOT one of them, and that is a change.** This header used to name
//! "start a database" as the example of an action that belongs here. Provisioning went to `xtask`
//! instead, because `xtask` already owns "what does this change require" and is the tool CI runs -
//! and a service tier that only a developer's binary can start is a tier CI cannot stand up. What
//! this binary keeps is the read-only half: which worktree is this, and what did provisioning bind.
//!
//! Why not part of the shipped `sutura` binary: the image contains one executable and the tool
//! surface is the governance boundary. Development machinery has no business being reachable
//! there.
//!
//! WHAT IT IS FOR. Several worktrees of this repo are open at once - that is the point of
//! stacked branches - and each needs its own Postgres, `ClickHouse` and an identity provider.
//! Two worktrees sharing one container is the worst available outcome: a test passes because
//! the *other* branch's migration ran. `sutura_dev::scope` derives every NAME per worktree so that
//! cannot happen, and `sutura_dev::discovery` reads back the ports docker allocated - which are
//! allocated rather than derived, because a hash into a port range cannot promise disjoint blocks
//! and check-then-bind is a race.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use sutura_dev::discovery::{self, Endpoints};
use sutura_dev::scope::{SERVICES, Scope};

/// A command: the name, the `--help` line, and the code it runs.
///
/// Same shape as xtask's table, for the same reason: dispatch derives from the table, so
/// `--help` cannot describe a command that does not exist.
/// What a command does. Same shape as xtask's `Gate`, and named differently on purpose: a
/// gate reports, an action changes your machine.
type Action = fn(&[String]) -> ExitCode;

struct Cmd {
    name: &'static str,
    description: &'static str,
    run: Action,
}

const COMMANDS: &[Cmd] = &[
    Cmd {
        name: "ports",
        description: "this worktree's compose project, and the endpoints provisioning bound",
        run: cmd_ports,
    },
    Cmd {
        name: "worktree",
        description: "create <branch> | list - isolated worktrees for stacked work",
        run: cmd_worktree,
    },
    Cmd {
        name: "doctor",
        description: "what is present, what is missing, what would fail",
        run: cmd_doctor,
    },
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.split_first().map(|(_, rest)| rest).unwrap_or_default();

    match args.first().map(String::as_str) {
        Some("--help" | "-h" | "help") | None => {
            usage();
            if args.is_empty() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            }
        }
        Some(requested) => COMMANDS.iter().find(|c| c.name == requested).map_or_else(
            || {
                eprintln!("sutura-dev: unknown command `{requested}`");
                usage();
                ExitCode::from(2)
            },
            |cmd| (cmd.run)(rest),
        ),
    }
}

fn usage() {
    eprintln!("usage: sutura-dev <command>");
    for cmd in COMMANDS {
        eprintln!("  {:<10} {}", cmd.name, cmd.description);
    }
}

/// The worktree we are in, from git rather than from the current directory.
fn worktree_root() -> Option<PathBuf> {
    let out = Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn require_root() -> Result<PathBuf, ExitCode> {
    worktree_root().ok_or_else(|| {
        eprintln!("sutura-dev: not inside a git worktree");
        ExitCode::FAILURE
    })
}

fn cmd_ports(_args: &[String]) -> ExitCode {
    let Ok(root) = require_root() else {
        return ExitCode::FAILURE;
    };
    let Some(scope) = require_scope(&root) else {
        return ExitCode::FAILURE;
    };

    println!("worktree   {}", root.display());
    println!("scope      {}", scope.digest());
    println!("compose    {}", scope.project());
    println!("discovery  {}", discovery::path_for(&scope).display());
    println!();

    match Endpoints::discover(&scope) {
        Ok(endpoints) => {
            println!("{:<12} endpoint", "service");
            for (name, endpoint) in endpoints.services() {
                println!("{name:<12} {endpoint}");
            }
            println!();
            println!("Read from the discovery file, which is the only place these exist. The ports");
            println!("were allocated by docker and the operating system, so they move between runs -");
            println!("which is what makes two worktrees provisioning at once safe rather than lucky.");
        }
        Err(problem) => {
            println!("nothing provisioned: {problem}");
            println!();
            println!("Declared services: {}", declared_services());
            println!("`just dev-up` provisions them and writes the discovery file.");
        }
    }
    ExitCode::SUCCESS
}

/// The service names this worktree could run, for a message. NOT their ports: there is no such
/// thing until docker has bound one, and printing a placeholder is how a constant gets copied.
fn declared_services() -> String {
    SERVICES
        .iter()
        .map(sutura_dev::scope::Service::name)
        .collect::<Vec<&str>>()
        .join(", ")
}

/// This worktree's scope, or a message saying why the path could not be resolved.
fn require_scope(root: &Path) -> Option<Scope> {
    match Scope::from_root(root) {
        Ok(scope) => Some(scope),
        Err(problem) => {
            eprintln!("sutura-dev: {problem}");
            None
        }
    }
}

fn cmd_worktree(args: &[String]) -> ExitCode {
    match args.split_first() {
        Some((verb, rest)) if verb == "create" => rest.first().map_or_else(
            || {
                eprintln!("sutura-dev worktree create <branch>");
                ExitCode::from(2)
            },
            |branch| worktree_create(branch),
        ),
        Some((verb, _)) if verb == "list" => worktree_list(),
        _ => {
            eprintln!("sutura-dev worktree create <branch> | list");
            ExitCode::from(2)
        }
    }
}

/// Variables that point git at a different repository. A git hook exports these, so a
/// `sutura-dev` invoked from one would create the worktree against the HOOK's repo. Stripped
/// before every git and stax call.
const GIT_LOCAL_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_PREFIX",
    "GIT_GRAFT_FILE",
    "GIT_SHALLOW_FILE",
];

fn strip_git_env(command: &mut Command) {
    for name in GIT_LOCAL_ENV {
        command.env_remove(name);
    }
}

/// The stax binary. It installs as `st`, and `stax` is the long form.
fn stax_binary() -> Option<&'static str> {
    ["st", "stax"].into_iter().find(|candidate| found(candidate))
}

/// `origin/main` -> `main`. stax wants the local branch name.
fn local_base(base: &str) -> &str {
    base.strip_prefix("origin/").unwrap_or(base)
}

fn worktree_create(branch: &str) -> ExitCode {
    let Ok(root) = require_root() else {
        return ExitCode::FAILURE;
    };

    // Delegate to stax rather than calling `git worktree add`. stax owns the stack, and a
    // branch created behind its back is a branch it will not rebase or submit - which is the
    // entire reason the stack tool is here. Refusing beats creating something half-managed.
    let Some(stax) = stax_binary() else {
        eprintln!("sutura-dev: stax is not on PATH, so the worktree would not join the stack.");
        eprintln!("  It is in the dev shell: `direnv allow`, or enter it with `devenv shell`.");
        eprintln!("  See the `stacked-branches` skill for what stax is for.");
        return ExitCode::FAILURE;
    };

    let base = local_base("origin/main");
    println!("creating worktree for {branch} from {base} via {stax}");

    let mut command = Command::new(stax);
    strip_git_env(&mut command);
    command
        .current_dir(&root)
        .args(["worktree", "create", branch, "--from", base]);

    match command.status() {
        Ok(status) if status.success() => {}
        Ok(_) => return ExitCode::FAILURE,
        Err(e) => {
            eprintln!("sutura-dev: could not run {stax}: {e}");
            return ExitCode::FAILURE;
        }
    }

    // stax chooses the directory, so ask git where it went rather than guessing.
    match created_worktree(&root, branch) {
        Some(path) => {
            println!();
            println!("{}", path.display());
            if let Some(scope) = require_scope(&path) {
                println!("scope {}   compose project {}", scope.digest(), scope.project());
            }
            println!(
                "services {} - `just dev-up` in the new worktree binds them",
                declared_services()
            );
            println!();
            println!("Next: cd {} && direnv allow", path.display());
            println!("direnv trust is per directory, so a new worktree needs it again.");
        }
        None => println!("worktree created; `sutura-dev worktree list` shows its scope"),
    }
    ExitCode::SUCCESS
}

/// The worktree whose checked-out branch is `branch`, according to git.
fn created_worktree(root: &Path, branch: &str) -> Option<PathBuf> {
    let mut command = Command::new("git");
    strip_git_env(&mut command);
    let out = command
        .current_dir(root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&out.stdout);
    let mut current: Option<&str> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(path);
        } else if let Some(head) = line.strip_prefix("branch ")
            && head.trim_start_matches("refs/heads/") == branch
        {
            return current.map(PathBuf::from);
        }
    }
    None
}

fn worktree_list() -> ExitCode {
    let Ok(root) = require_root() else {
        return ExitCode::FAILURE;
    };
    let out = Command::new("git")
        .current_dir(&root)
        .args(["worktree", "list", "--porcelain"])
        .output();
    let Ok(out) = out else {
        eprintln!("sutura-dev: could not run git worktree list");
        return ExitCode::FAILURE;
    };

    println!("{:<10} {:<14} path", "scope", "compose");
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            match Scope::from_root(Path::new(path)) {
                Ok(scope) => println!("{:<10} {:<14} {path}", scope.digest(), scope.project()),
                Err(problem) => println!("{:<10} {:<14} {path}  ({problem})", "?", "?"),
            }
        }
    }
    ExitCode::SUCCESS
}

/// The stages a hook must be installed for, and what each covers.
///
/// `commit-msg` and `pre-push` are distinct git hooks from `pre-commit`; installing one does
/// not install the others, and a missing one means that stage silently never runs.
const HOOK_STAGES: &[(&str, &str)] = &[
    ("pre-commit", "fmt, clippy, the structural gates"),
    ("commit-msg", "the conventional-commit subject check"),
    ("pre-push", "tests, doctests, supply chain, secrets"),
];

/// Are the hooks installed, and is git looking where they were installed?
///
/// Both halves were wrong here at once, which is why this is a check and not a sentence.
/// `core.hooksPath` pointed at a `.githooks/` directory that had since been deleted, so git
/// looked somewhere that did not exist - while `prek install` writes to `.git/hooks`, which git
/// was therefore ignoring. Every "enforced in the hooks" claim was false and nothing said so.
///
/// Asks git for the directory rather than assuming `.git/hooks`, because that assumption is
/// wrong inside a worktree and `sutura-dev worktree create` makes worktrees.
fn hook_problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();

    let configured = Command::new("git")
        .current_dir(root)
        .args(["config", "--local", "--get", "core.hooksPath"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from(String::from_utf8_lossy(&o.stdout).trim()))
        .filter(|value| !value.is_empty());

    if let Some(ref path) = configured
        && !root.join(path).is_dir()
    {
        problems.push(format!(
            "core.hooksPath is `{path}`, which does not exist - git looks there and finds nothing"
        ));
    }

    let hooks_dir = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--path-format=absolute", "--git-path", "hooks"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim().to_owned()));

    let Some(dir) = hooks_dir else {
        problems.push(String::from("could not ask git where its hooks directory is"));
        return problems;
    };

    for (stage, covers) in HOOK_STAGES {
        if !dir.join(stage).is_file() {
            problems.push(format!("the {stage} hook is not installed ({covers})"));
        }
    }
    problems
}

/// Is this tool on PATH?
///
/// A `PATH` walk and NOT a `--version` run, which is what this used to be. Three reasons, and the
/// first is a hang somebody hit:
///
/// 1. `.status()` has no timeout, so `found("docker")` against a wedged daemon blocked `just doctor`
///    forever - the same defect `xtask`'s container probe was bounded to remove, in the one command
///    whose whole job is to say what is wrong with this machine.
/// 2. It answers the question actually asked. A tool that is installed but hangs on `--version` IS
///    on `PATH`, and reporting it `MISSING` sends a reader to install something they already have.
/// 3. It executes nothing. `doctor` runs four of these, and spawning four processes to learn what a
///    directory listing already knows is a cost with no answer attached to it.
///
/// No timeout is needed because nothing is waited on. `X_OK` rather than merely existing, because a
/// file on `PATH` that cannot be executed is not a usable tool.
fn found(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| found_in(tool, &path))
}

/// The deciding half, with the search path passed in.
///
/// Split for the reason the container probe next door is split: the line that reads the ENVIRONMENT
/// is one line, and the decision is a function beside it that a test can call with a path it built
/// itself. The alternative is a test that mutates `PATH`, which in this edition needs an `unsafe`
/// block and makes the test order-dependent for a value the whole process shares.
fn found_in(tool: &str, path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|dir| {
        std::fs::metadata(dir.join(tool)).is_ok_and(|meta| {
            // Executable, not merely present: a file on `PATH` that cannot be run is not a tool.
            meta.is_file() && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0
        })
    })
}

fn cmd_doctor(_args: &[String]) -> ExitCode {
    let root = worktree_root();
    println!(
        "worktree   {}",
        root.as_ref()
            .map_or_else(|| String::from("NOT IN A GIT WORKTREE"), |r| r.display().to_string())
    );
    if let Some(ref root) = root
        && let Ok(scope) = Scope::from_root(root)
    {
        println!("scope      {}", scope.digest());
        println!("compose    {}", scope.project());
    }
    println!();

    // Required to work at all, versus needed once services exist. Reported separately so a
    // missing docker does not look like a broken setup today.
    let mut missing_required = false;
    for (tool, required) in [("git", true), ("cargo", true), ("nix", false), ("docker", false)] {
        let present = found(tool);
        if required && !present {
            missing_required = true;
        }
        println!(
            "{:<10} {}{}",
            tool,
            if present { "present" } else { "MISSING" },
            if required {
                ""
            } else {
                "  (`just dev-up` needs docker; everything else works without it)"
            }
        );
    }

    println!();
    let hooks = hook_problems(root.as_ref().map_or_else(|| Path::new("."), PathBuf::as_path));
    if hooks.is_empty() {
        println!("hooks      installed for all {} stage(s)", HOOK_STAGES.len());
    } else {
        for problem in &hooks {
            println!("hooks      {problem}");
        }
        println!();
        println!("An uninstalled hook does not complain - it never runs, so every gate it was");
        println!("meant to enforce is advisory. `just setup` installs them and repairs a");
        println!("dangling core.hooksPath.");
    }

    println!();
    if missing_required {
        println!("Something required is missing. See docs/getting-started.md.");
        return ExitCode::FAILURE;
    }
    if !hooks.is_empty() {
        return ExitCode::FAILURE;
    }
    println!("Ready. `just ports` shows this worktree's compose project and its endpoints.");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    /// `found_in` executes nothing, so a tool that would hang is still reported present.
    ///
    /// Against the previous shape this case did not fail, it never returned: `found` ran
    /// `--version` and waited with no timeout, which is what hung `just doctor` on a wedged daemon.
    #[test]
    fn a_tool_that_would_hang_is_still_found_because_nothing_is_executed() {
        let dir = std::env::temp_dir().join(format!("sutura-found-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let tool = dir.join("sutura-test-hangs");
        std::fs::write(&tool, "#!/bin/sh\nwhile :; do :; done\n").expect("write");
        std::fs::set_permissions(
            &tool,
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("chmod");
        std::fs::create_dir_all(dir.join("sutura-test-dir")).expect("subdir");

        let answer = super::found_in("sutura-test-hangs", dir.as_os_str());
        let absent = super::found_in("sutura-test-not-here", dir.as_os_str());
        // A directory is present and carries the execute bit, and is still not a tool.
        let directory = super::found_in("sutura-test-dir", dir.as_os_str());
        drop(std::fs::remove_dir_all(&dir));

        assert!(
            answer,
            "a tool on the path is found even though running it would never return"
        );
        assert!(!absent, "a tool that is not on the path is not found");
        assert!(!directory, "a directory is not an executable tool");
    }

    use super::{COMMANDS, local_base};

    #[test]
    fn a_remote_base_becomes_the_local_branch_name() {
        // stax wants `main`, not `origin/main`; passing the remote form makes it look for a
        // local branch that does not exist.
        assert_eq!(local_base("origin/main"), "main");
        assert_eq!(local_base("main"), "main");
    }

    #[test]
    fn command_names_are_unique_and_described() {
        let mut names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
        for cmd in COMMANDS {
            assert!(!cmd.description.is_empty(), "{} has no --help line", cmd.name);
        }
    }
}
