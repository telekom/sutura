//! `sutura-dev` - the developer workflow that needs to know which worktree it is in.
//!
//! Why a separate binary rather than another `xtask` task: xtask holds *gates*, which answer
//! "does this repo violate a rule?" and are run by hooks and CI. This holds *actions*, which
//! change your machine - create a worktree, start a database. Mixing them would mean CI runs a
//! binary that can start containers, and a developer runs a binary whose job is to say no.
//!
//! Why not part of the shipped `sutura` binary: the image contains one executable and the tool
//! surface is the governance boundary. Development machinery has no business being reachable
//! there.
//!
//! WHAT IT IS FOR. Several worktrees of this repo are open at once - that is the point of
//! stacked branches - and each will need its own Postgres, `ClickHouse` and an identity provider.
//! Two worktrees sharing one container is the worst available outcome: a test passes because
//! the *other* branch's migration ran. `scope` derives everything per worktree so that cannot
//! happen. The services are declared before they exist because the port scheme has to be
//! settled first; changing it later moves every developer's ports at once.

mod scope;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use scope::{SERVICES, Scope};

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
        description: "the ports this worktree's services use, and why",
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
    let scope = Scope::from_root(&root);

    println!("worktree   {}", root.display());
    println!("scope      {}", scope.digest);
    println!("compose    {}", scope.project());
    println!();
    println!("{:<12} {:>6}  override with", "service", "port");
    for service in SERVICES {
        println!("{:<12} {:>6}  {}", service.name, scope.port(service), service.port_env);
    }
    println!();
    println!("Derived from the worktree path, so they are stable across runs: a port that moves");
    println!("cannot go in a config file or a bug report. Another worktree gets other ports.");
    ExitCode::SUCCESS
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
            let scope = Scope::from_root(&path);
            println!();
            println!("{}", path.display());
            println!("scope {}   compose project {}", scope.digest, scope.project());
            for service in SERVICES {
                println!("  {:<12} {}", service.name, scope.port(service));
            }
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
            let scope = Scope::from_root(Path::new(path));
            println!("{:<10} {:<14} {path}", scope.digest, scope.project());
        }
    }
    ExitCode::SUCCESS
}

/// Is this tool on PATH?
fn found(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn cmd_doctor(_args: &[String]) -> ExitCode {
    let root = worktree_root();
    println!(
        "worktree   {}",
        root.as_ref()
            .map_or_else(|| String::from("NOT IN A GIT WORKTREE"), |r| r.display().to_string())
    );
    if let Some(ref root) = root {
        let scope = Scope::from_root(root);
        println!("scope      {}", scope.digest);
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
            if required { "" } else { "  (needed once dev services land)" }
        );
    }

    println!();
    if missing_required {
        println!("Something required is missing. See documentation/getting-started.md.");
        return ExitCode::FAILURE;
    }
    println!("Ready. `sutura-dev ports` shows this worktree's service ports.");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
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
