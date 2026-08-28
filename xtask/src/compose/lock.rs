//! The lock provisioning holds, and the identity check that decides when one is stale.
//!
//! Rule 2 of the teardown contract in `sutura_dev::scope`: **eligibility is re-checked at destroy
//! time, under a lock held across the destroy.** Deciding a container is stale and removing it are
//! two moments, and another worktree can start between them - so the lock is taken once and held
//! for the whole operation rather than per item, because the window is what is being closed.
//!
//! # A PID is not an identity
//!
//! A lock file names the process that wrote it, and a PID is reused. So the recorded number alone
//! cannot say whether anybody still holds the lock, and treating it as if it could is how a tool
//! decides a stranger's process is its own.
//!
//! The check is the process's **working directory, resolved and compared against this
//! repository's root** - the one property a colliding stranger cannot accidentally have. It runs
//! BEFORE anything is done to the process, which is the ordering that matters rather than the check
//! existing.
//!
//! **And nothing here signals anything.** Liveness comes from `ps`, which asks the process table
//! rather than poking the process, so there is no signal to get wrong. The identity check is
//! implemented and gates the decision anyway, because it is the guard the day somebody does need to
//! send one - a guard added afterwards is a guard added after the incident.

use std::path::{Path, PathBuf};
use std::process::Command;

use sutura_dev::scope::Scope;

/// The lock file, inside the worktree's own state directory.
const FILE: &str = "compose.lock";

/// An acquired lock. Released when it is dropped, which is what "held across the destroy" means:
/// the value lives as long as the operation and nothing has to remember to release it.
pub(crate) struct Held {
    /// The file to remove on release.
    path: PathBuf,
}

impl Held {
    /// Where the lock lives. Printed, so a reader who has to break one knows what to remove.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // A failed release is not worth aborting a completed destroy over, and `panic = "abort"`
        // makes a panic in a destructor process death. The next run classifies it as stale.
        drop(std::fs::remove_file(&self.path));
    }
}

/// Why a lock could not be taken.
#[derive(Debug)]
pub(crate) enum LockError {
    /// Somebody is provisioning this worktree right now.
    Live {
        /// The process holding it.
        pid: u32,
        /// Where the lock file is, so a reader can look at it.
        path: PathBuf,
    },
    /// A lock file exists and this tool cannot establish whether its holder is alive.
    ///
    /// **Refused rather than guessed**, and that direction is deliberate: this gates a destructive
    /// operation, so the expensive mistake is taking a lock somebody holds. A reader who knows
    /// better removes the file, which is a decision with a name on it.
    Undecidable {
        /// The process the file names.
        pid: u32,
        /// The lock file.
        path: PathBuf,
    },
    /// The state directory or the lock file could not be written.
    Unwritable {
        /// What was being written.
        path: PathBuf,
        /// What the filesystem said.
        cause: std::io::Error,
    },
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Live { pid, ref path } => write!(f, "process {pid} is provisioning this worktree already ({})", path.display()),
            Self::Undecidable { pid, ref path } => write!(
                f,
                "{} names process {pid} and this host cannot say whether it is alive - remove the \
                 file if you are sure nothing is provisioning",
                path.display()
            ),
            Self::Unwritable { ref path, .. } => write!(f, "could not write {}", path.display()),
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::Unwritable { ref cause, .. } => Some(cause),
            Self::Live { .. } | Self::Undecidable { .. } => None,
        }
    }
}

/// What the process table and the filesystem say about a recorded PID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Liveness {
    /// The process exists.
    Alive,
    /// It does not.
    Dead,
    /// This host could not be asked.
    Unknown,
}

/// What a lock file's recorded holder turns out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Claim {
    /// A live process whose working directory is under this repository. Somebody holds the lock.
    Live,
    /// Provably not a holder: dead, or alive with a working directory outside this repository,
    /// which means the number was reused. Safe to take.
    Stale,
    /// Cannot be decided on this host. Refused rather than taken.
    Undecidable,
}

/// The decision, as a pure function, so both directions are tested without a second process.
///
/// **Ordering is the point.** A working directory outside this repository settles the question
/// before anything else is considered: a stranger that happens to hold a number we wrote down is
/// not the holder of our lock, whatever else is true of it.
pub(crate) fn classify(liveness: Liveness, working_dir: Option<&Path>, repo_root: &Path) -> Claim {
    match liveness {
        Liveness::Dead => Claim::Stale,
        Liveness::Unknown => Claim::Undecidable,
        Liveness::Alive => match working_dir {
            // The one property a colliding stranger cannot accidentally have.
            Some(dir) if dir.starts_with(repo_root) => Claim::Live,
            Some(_) => Claim::Stale,
            // Alive, and this host will not say where it is working. Not enough to claim identity.
            None => Claim::Undecidable,
        },
    }
}

/// Does this process exist? Asked of the process table, so no signal is sent.
fn liveness(pid: u32) -> Liveness {
    let out = Command::new("ps").args(["-o", "pid=", "-p", &pid.to_string()]).output();
    match out {
        Err(_ignored) => Liveness::Unknown,
        Ok(out) if out.status.success() && !out.stdout.is_empty() => Liveness::Alive,
        Ok(_) => Liveness::Dead,
    }
}

/// A process's working directory, resolved, or `None` when this host will not say.
///
/// Two implementations because there are two ways to ask, and the answer is load-bearing: it is
/// what distinguishes our own holder from a stranger holding a reused number.
fn working_dir(pid: u32) -> Option<PathBuf> {
    // Linux: the kernel exposes it directly.
    let proc_link = PathBuf::from(format!("/proc/{pid}/cwd"));
    if let Ok(resolved) = std::fs::read_link(&proc_link) {
        return Some(resolved);
    }
    // macOS and the BSDs: `lsof` prints it, one field per line, the path prefixed with `n`.
    let out = Command::new("lsof")
        .args(["-a", "-d", "cwd", "-p", &pid.to_string(), "-F", "n"])
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    let printed = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .map(PathBuf::from)
        .find(|path| path.is_absolute())?;
    std::fs::canonicalize(printed).ok()
}

/// The PID a lock file names, if it names one.
fn recorded_pid(text: &str) -> Option<u32> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("pid="))
        .find_map(|value| value.trim().parse().ok())
}

/// Take this worktree's provisioning lock, or say who has it.
///
/// Not atomic against a concurrent breaker of a stale lock, and that is stated rather than implied:
/// two processes that both classify one lock as stale in the same instant can both take it. What
/// this closes is the window rule 2 is about - a live neighbour losing its containers to somebody
/// else's teardown - which is a different and much more expensive race.
pub(crate) fn acquire(scope: &Scope) -> Result<Held, LockError> {
    // The two roots coincide for the tool - a worktree IS the repository it is a worktree of - and
    // they are separate parameters because they answer separate questions: where state lives, and
    // what counts as "under this repository" when a PID's working directory is checked.
    acquire_under(scope, scope.root())
}

fn acquire_under(scope: &Scope, repository: &Path) -> Result<Held, LockError> {
    let dir = scope.state_dir();
    let path = dir.join(FILE);
    std::fs::create_dir_all(&dir).map_err(|cause| LockError::Unwritable {
        path: path.clone(),
        cause,
    })?;

    if let Ok(existing) = std::fs::read_to_string(&path)
        && let Some(pid) = recorded_pid(&existing)
    {
        // The identity check runs before anything is done with the number.
        match classify(liveness(pid), working_dir(pid).as_deref(), repository) {
            Claim::Live => return Err(LockError::Live { pid, path }),
            Claim::Undecidable => return Err(LockError::Undecidable { pid, path }),
            Claim::Stale => drop(std::fs::remove_file(&path)),
        }
    }

    let note = format!("pid={}\nroot={}\n", std::process::id(), scope.root().display());
    // `create_new`: whoever loses this race gets the error rather than both believing they won.
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            use std::io::Write as _;
            file.write_all(note.as_bytes()).map_err(|cause| LockError::Unwritable {
                path: path.clone(),
                cause,
            })?;
            Ok(Held { path })
        }
        Err(cause) if cause.kind() == std::io::ErrorKind::AlreadyExists => {
            let pid = std::fs::read_to_string(&path)
                .ok()
                .as_deref()
                .and_then(recorded_pid)
                .unwrap_or_default();
            Err(LockError::Live { pid, path })
        }
        Err(cause) => Err(LockError::Unwritable { path, cause }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_dev::scope::Scope;

    use super::{Claim, Liveness, LockError, acquire, acquire_under, classify, liveness, recorded_pid, working_dir};

    fn temp_worktree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-lock-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dirs are creatable");
        dir
    }

    #[test]
    fn a_process_outside_this_repository_is_not_the_holder_of_our_lock() {
        // The neighbour-killing case turned inside out: a reused PID belonging to a stranger must
        // not read as "somebody is provisioning", and it must not be signalled either. A port or a
        // number is not an identity; a working directory under this repository is.
        let root = Path::new("/repo/worktree");
        assert_eq!(
            classify(Liveness::Alive, Some(Path::new("/elsewhere/entirely")), root),
            Claim::Stale
        );
        assert_eq!(
            classify(Liveness::Alive, Some(Path::new("/repo/worktree/crates")), root),
            Claim::Live
        );
    }

    #[test]
    fn a_holder_this_host_cannot_identify_is_refused_rather_than_overridden() {
        // Fail closed, because this gates a destructive operation: the expensive mistake is taking
        // a lock somebody holds, not refusing one nobody does.
        let root = Path::new("/repo/worktree");
        assert_eq!(classify(Liveness::Unknown, None, root), Claim::Undecidable);
        assert_eq!(classify(Liveness::Alive, None, root), Claim::Undecidable);
        // Dead settles it without needing a directory at all.
        assert_eq!(classify(Liveness::Dead, None, root), Claim::Stale);
    }

    #[test]
    fn a_lock_is_exclusive_and_released_on_drop() {
        let dir = temp_worktree("exclusive");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        // The identity root is this test process's own working directory, because the holder the
        // second attempt finds IS this process. Under the tool the two coincide; separating them
        // here is what lets the live branch be exercised rather than described.
        let repository = std::env::current_dir().expect("a working directory");

        let held = acquire_under(&scope, &repository).expect("first acquisition");
        assert!(held.path().is_file());
        assert!(matches!(acquire_under(&scope, &repository), Err(LockError::Live { .. })));

        let path = held.path().to_path_buf();
        drop(held);
        assert!(!path.exists(), "a dropped lock is released");
        drop(acquire_under(&scope, &repository).expect("released"));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_lock_naming_a_dead_process_is_stale_and_taken() {
        let dir = temp_worktree("stale");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        std::fs::create_dir_all(scope.state_dir()).expect("state dir");
        // PID 0 is never a user process, so the process table says dead on every platform here.
        std::fs::write(scope.state_dir().join("compose.lock"), "pid=0\nroot=/gone\n").expect("write");
        let held = acquire(&scope).expect("a dead holder is not a holder");
        drop(held);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_lock_file_without_a_pid_is_not_read_as_one() {
        assert_eq!(recorded_pid("pid=4321\nroot=/x\n"), Some(4321));
        assert_eq!(recorded_pid("root=/x\n"), None);
        assert_eq!(recorded_pid("pid=not-a-number\n"), None);
        assert_eq!(recorded_pid(""), None);
    }

    #[test]
    fn this_process_is_alive_and_working_here() {
        // The two host probes, exercised against the one process whose answers are known. Without
        // this the `classify` tests above would be pinning a decision over inputs nothing produces.
        let me = std::process::id();
        assert_eq!(liveness(me), Liveness::Alive);
        let dir = working_dir(me);
        assert!(dir.is_some_and(|d| d.is_dir()), "this process has a working directory");
    }
}
