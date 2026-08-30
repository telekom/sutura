//! The lock provisioning holds, and the identity check that describes who holds it.
//!
//! Rule 2 of the teardown contract in `sutura_dev::scope`: **eligibility is re-checked at destroy
//! time, under a lock held across the destroy.** Deciding a container is stale and removing it are
//! two moments, and another worktree can start between them - so the lock is taken once and held
//! for the whole operation rather than per item, because the window is what is being closed.
//!
//! # The kernel owns the lock, and that is a correction
//!
//! An earlier version of this module answered "does anybody still hold this?" by recording a PID in
//! the file and asking `ps` about it. That was wrong twice, and CI found the second one:
//!
//! * **It was not portable.** `ps` and `lsof` are external binaries, and the Nix build sandbox has
//!   neither on `PATH`. The probe answered "cannot tell", the fail-closed direction turned that into
//!   a refusal, and two tests that pass on a developer machine failed on `x86_64-linux` -
//!   `Undecidable { pid: 0 }` for a PID that is dead on every platform. A guard whose answer depends
//!   on what happens to be installed is not a guard.
//! * **It was a heuristic where a primitive exists.** `File::try_lock` takes an advisory lock the
//!   operating system releases when the holding process dies - so a crashed holder's lock is simply
//!   gone, and "is this stale?" stops being a judgement call. It also closes the steal race the old
//!   version documented and did not fix: two processes that both decided one lock was stale could
//!   both take it.
//!
//! **The lock file is therefore never deleted.** Unlinking a file while holding an advisory lock on
//! it is the classic way to lose exclusion: another process opens the same path, we unlink and
//! release, it locks a deleted inode while a third creates a fresh file and locks that. Two
//! processes, both convinced they hold the lock. The file stays; the kernel lock is the exclusion.
//!
//! # A PID is not an identity, and what that check does now
//!
//! The recorded PID and root survive, and they do exactly one job: **describing** the holder in the
//! refusal. Whether the lock is held is the kernel's answer, and no decision here depends on the
//! PID at all.
//!
//! That is a smaller job than the old version claimed and it is stated as the smaller one, because
//! a guard described as stronger than it is spends trust a reviewer needed elsewhere. What it buys
//! is a materially different diagnosis - "another provisioning run in this worktree, wait for it"
//! against "something outside this repository is holding the file" - and it is the guard that would
//! gate a signal the day one is sent. **Nothing here signals anything today**, so the rule "a PID is
//! signalled only if its working directory is under this repository" is honoured by there being no
//! signal, with the check already in place for when that changes.

use std::path::{Path, PathBuf};
use std::process::Command;

use sutura_dev::scope::Scope;

/// The lock file, inside the worktree's own state directory.
const FILE: &str = "compose.lock";

/// An acquired lock. Released when it is dropped, because dropping the file closes the descriptor
/// the operating system attached the lock to - which is also why a crash releases it.
///
/// `Debug` so a test that expected a refusal can say what it got instead. It prints the descriptor,
/// which is not a secret and not a path outside this repository.
#[derive(Debug)]
pub(crate) struct Held {
    /// The locked file. Held for the lifetime of the operation; not read again.
    ///
    /// It is the LOCK, not a handle to tidy up: the field exists so the descriptor outlives the
    /// destroy rather than being closed at the end of `acquire`.
    _file: std::fs::File,
    /// Where the lock lives, for the message.
    path: PathBuf,
}

impl Held {
    /// Where the lock lives. Printed, so a reader can see which file is involved.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// What the file says about the process holding the lock.
///
/// Three outcomes, and the third is not the absence of the other two: a host that will not say
/// where a process is working is a real state, reachable in a build sandbox with neither `/proc` nor
/// `lsof`, and it must not be reported as either "ours" or "a stranger".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holder {
    /// A process whose working directory is under this repository. Another provisioning run.
    Ours,
    /// A process working outside this repository. The number in the file was reused, or something
    /// unrelated is holding the path.
    Foreign,
    /// This host will not say where the process is working, so it is not claimed either way.
    Unidentified,
}

impl Holder {
    /// What a reader should do about it.
    pub(crate) const fn advice(self) -> &'static str {
        match self {
            Self::Ours => "another provisioning run in this worktree - wait for it to finish",
            Self::Foreign => {
                "that process is working outside this repository, so it is not one of ours - \
                 nothing was signalled, and nothing will be"
            }
            Self::Unidentified => {
                "this host will not say where that process is working, so it is not claimed either \
                 way - the lock is held regardless, which is the kernel's answer and not a guess"
            }
        }
    }
}

/// Which holder a working directory describes, as a pure function.
///
/// **Ordering is the point.** A working directory outside this repository settles the question: a
/// stranger that happens to hold a number we wrote down is not the holder of our lock, whatever
/// else is true of it. And `None` is its own answer rather than a default to either side.
pub(crate) fn holder(working_dir: Option<&Path>, repo_root: &Path) -> Holder {
    match working_dir {
        // The one property a colliding stranger cannot accidentally have.
        Some(dir) if dir.starts_with(repo_root) => Holder::Ours,
        Some(_) => Holder::Foreign,
        None => Holder::Unidentified,
    }
}

/// Why a lock could not be taken.
#[derive(Debug)]
pub(crate) enum LockError {
    /// Somebody is provisioning this worktree right now. The kernel says so; the PID and the
    /// [`Holder`] only say who.
    Held {
        /// The process the file names, or `None` when it names none - an unreadable or empty lock
        /// file is an unknown holder, not a fabricated PID 0.
        pid: Option<u32>,
        /// What that process turns out to be.
        holder: Holder,
        /// The lock file.
        path: PathBuf,
    },
    /// The state directory or the lock file could not be opened or written.
    Unusable {
        /// What was being opened or written.
        path: PathBuf,
        /// What the filesystem said.
        cause: std::io::Error,
    },
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Held { pid, holder, ref path } => {
                let who = pid.map_or_else(
                    || "a process the lock file does not name".to_owned(),
                    |pid| format!("process {pid}"),
                );
                write!(f, "{} is locked by {who}: {}", path.display(), holder.advice())
            }
            Self::Unusable { ref path, .. } => write!(f, "could not use {}", path.display()),
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::Unusable { ref cause, .. } => Some(cause),
            Self::Held { .. } => None,
        }
    }
}

/// A process's working directory, resolved, or `None` when this host will not say.
///
/// Best effort ON PURPOSE, and that is the whole change from the version CI rejected: **no decision
/// in this module depends on the answer.** It picks which of three descriptions a refusal carries,
/// so a host with neither `/proc` nor `lsof` - the Nix build sandbox is one - reports
/// [`Holder::Unidentified`] and everything else behaves identically.
fn working_dir(pid: u32) -> Option<PathBuf> {
    // Linux: the kernel exposes it directly, and the path simply does not exist elsewhere.
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
pub(crate) fn acquire(scope: &Scope) -> Result<Held, LockError> {
    // The two roots coincide for the tool - a worktree IS the repository it is a worktree of - and
    // they are separate parameters because they answer separate questions: where state lives, and
    // what counts as "under this repository" when a holder is described.
    acquire_under(scope, scope.root())
}

fn acquire_under(scope: &Scope, repository: &Path) -> Result<Held, LockError> {
    let dir = scope.state_dir();
    let path = dir.join(FILE);
    let unusable = |cause: std::io::Error| LockError::Unusable {
        path: path.clone(),
        cause,
    };

    std::fs::create_dir_all(&dir).map_err(unusable)?;
    // `create` and not `create_new`: the file persists between runs by design, because unlinking it
    // is how exclusion gets lost. The kernel lock below is what excludes, not the file's existence.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(unusable)?;

    // ONE call, matched exhaustively. `try_lock` is not idempotent to ask twice: an if/else-if
    // chain over two calls would take the lock in the first and re-ask in the second.
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            // Somebody holds it. The PID is only how the refusal names them, and a file that cannot
            // be read or names no PID is `None` rather than a fabricated 0 - an undetermined holder
            // silently becoming PID 0 is the shape this module stopped shipping, and probing
            // `working_dir(0)` for a process that does not exist is the same guess written out.
            let pid = std::fs::read_to_string(&path).ok().as_deref().and_then(recorded_pid);
            let who = pid.map_or(Holder::Unidentified, |pid| holder(working_dir(pid).as_deref(), repository));
            return Err(LockError::Held { pid, holder: who, path });
        }
        Err(std::fs::TryLockError::Error(cause)) => return Err(unusable(cause)),
    }

    // Ours. Record who, for the next run's refusal message. Written first and then truncated to the
    // written length, NOT truncated then written: between the two the file used to be observably
    // empty, and the refusal path reads it - so a concurrent acquirer landing in that window read
    // no holder and reported PID 0. The kernel lock was already taken here, so the reordering buys
    // the diagnostic without touching the exclusion.
    let note = format!("pid={}\nroot={}\n", std::process::id(), scope.root().display());
    {
        use std::io::Write as _;
        let mut writer = &file;
        writer.write_all(note.as_bytes()).map_err(unusable)?;
    }
    file.set_len(note.len() as u64).map_err(unusable)?;
    Ok(Held { _file: file, path })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_dev::scope::Scope;

    use super::{Holder, LockError, acquire, acquire_under, holder, recorded_pid, working_dir};

    fn temp_worktree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-lock-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dirs are creatable");
        dir
    }

    #[test]
    fn a_process_outside_this_repository_is_not_the_holder_of_our_lock() {
        // The neighbour-killing case turned inside out: a reused PID belonging to a stranger must
        // not read as "another provisioning run", and it must never be signalled. A number is not
        // an identity; a working directory under this repository is.
        //
        // Pure, so it asserts the same property on darwin and on linux. The version CI rejected
        // asserted this THROUGH `ps`, which is why it held on one platform and not the other.
        let root = Path::new("/repo/worktree");
        assert_eq!(holder(Some(Path::new("/elsewhere/entirely")), root), Holder::Foreign);
        assert_eq!(holder(Some(Path::new("/repo/worktree/crates")), root), Holder::Ours);
        assert!(
            Holder::Foreign.advice().contains("nothing was signalled"),
            "the refusal has to say the stranger was left alone"
        );
    }

    #[test]
    fn a_holder_this_host_cannot_identify_is_named_as_such_rather_than_guessed() {
        // The Nix build sandbox has neither `/proc` nor `lsof`, so this is a state a real host
        // reaches. It is its own answer: claiming it either way would be the guess.
        let root = Path::new("/repo/worktree");
        assert_eq!(holder(None, root), Holder::Unidentified);
        // And it changes nothing about whether the lock is held - which is the whole repair.
        assert!(Holder::Unidentified.advice().contains("held regardless"));
    }

    #[test]
    fn a_lock_is_exclusive_and_released_on_drop() {
        // Exclusion comes from the kernel, so this test depends on no external binary and asserts
        // the same thing on both platforms. Two `File`s on one path conflict within a process as
        // well as between processes, which is what makes it testable at all.
        let dir = temp_worktree("exclusive");
        let scope = Scope::from_root(&dir).expect("the directory exists");

        let held = acquire(&scope).expect("first acquisition");
        assert!(held.path().is_file());
        match acquire(&scope) {
            Err(LockError::Held { pid, .. }) => {
                assert_eq!(pid, Some(std::process::id()), "the refusal names the holder");
            }
            other => panic!("a second acquisition must be refused, got {other:?}"),
        }

        let path = held.path().to_path_buf();
        drop(held);
        assert!(path.is_file(), "the file persists - unlinking it is how exclusion gets lost");
        drop(acquire(&scope).expect("dropping the lock releases it"));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_lock_file_that_names_no_holder_is_not_read_as_pid_zero() {
        // The empty read the old truncate-first write opened, made deterministic: here the empty
        // file is written by hand while the kernel lock is held, and the refusal must report an
        // unidentifiable holder rather than a fabricated PID 0 - an undetermined value silently
        // becoming a definite one is the defect, and this pins its absence.
        let dir = temp_worktree("nameless");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        std::fs::create_dir_all(scope.state_dir()).expect("state dir");
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(scope.state_dir().join("compose.lock"))
            .expect("open the lock file");
        held.try_lock().expect("this test holds the kernel lock");

        match acquire(&scope) {
            Err(LockError::Held { pid, holder, .. }) => {
                assert_eq!(pid, None, "an unreadable holder is not fabricated as a pid");
                assert_eq!(holder, Holder::Unidentified, "an unreadable holder is not claimed");
            }
            other => panic!("a lock held by this process must be refused, got {other:?}"),
        }
        drop(held);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_lock_file_left_by_a_crashed_process_does_not_block_the_next_run() {
        // What "stale" now means, and why it needs no probe: the operating system released the lock
        // when that process died, so a leftover file naming a PID that is gone - or one that was
        // never alive - is not a holder. Deterministic on every platform, and it does not race
        // against reaping, because nothing is spawned or killed.
        let dir = temp_worktree("stale");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        std::fs::create_dir_all(scope.state_dir()).expect("state dir");
        std::fs::write(scope.state_dir().join("compose.lock"), "pid=0\nroot=/gone\n").expect("write");

        let held = acquire(&scope).expect("a crashed holder is not a holder");
        // And the record is replaced, so the NEXT refusal names this process rather than the ghost.
        let recorded = std::fs::read_to_string(held.path()).expect("readable");
        assert_eq!(recorded_pid(&recorded), Some(std::process::id()));
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
    fn the_working_directory_probe_either_answers_or_says_it_cannot() {
        // The impure half, and deliberately not asserted to succeed: on a host with `/proc` or
        // `lsof` it answers, and in a sandbox with neither it does not. What is asserted is that
        // BOTH outcomes are shaped correctly - a wrong path would be the silent failure - and the
        // pure `holder` tests above carry the property, which is what makes this one honest rather
        // than a platform coin-flip dressed as coverage.
        let me = std::process::id();
        match working_dir(me) {
            Some(dir) => {
                assert!(dir.is_dir(), "an answer has to be a real directory: {}", dir.display());
                let repository = std::env::current_dir().expect("a working directory");
                assert_eq!(
                    holder(Some(&dir), &repository),
                    Holder::Ours,
                    "this process is working inside this repository"
                );
            }
            None => assert_eq!(holder(None, Path::new("/anywhere")), Holder::Unidentified),
        }
    }

    #[test]
    fn the_identity_root_is_a_separate_question_from_where_state_lives() {
        // The two coincide under the tool. They are separate parameters because a holder working in
        // a directory that is not this repository is `Foreign` regardless of where the lock file
        // sits, and collapsing them would make the check read the wrong root.
        let dir = temp_worktree("roots");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        let held = acquire_under(&scope, Path::new("/definitely/not/here")).expect("first");
        match acquire_under(&scope, Path::new("/definitely/not/here")) {
            Err(LockError::Held { holder, .. }) => {
                // This process is not working under `/definitely/not/here`, so it is either a
                // stranger or unidentifiable - never `Ours`. Both are correct; claiming `Ours`
                // against a root the holder is not under would not be.
                assert_ne!(holder, Holder::Ours, "the identity root was ignored");
            }
            other => panic!("{other:?}"),
        }
        drop(held);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
