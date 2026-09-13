//! How long one call may take, and the single bounded wait every call here goes through.
//!
//! **The distinction this module exists for is that one budget cannot serve every subcommand.**
//! `docker compose` is reached through one function, and its subcommands differ by orders of
//! magnitude: an `up` that pulls a stack of images legitimately runs for minutes, a `ps` is a status
//! query that answers in well under a second. A single timeout is either too long to bound the query
//! - the call a readiness loop repeats, and the one that hung `just dev-up` when nothing bounded it
//! - or short enough to kill a legitimate pull halfway and leave containers behind.
//!
//! So the caller states the kind of call, the same way [`crate::compose::docker::probed`] takes a budget, and
//! everything that waits on a process does it in [`waited`] and nowhere else.
//!
//! **The limit, stated because it is the one a reader would assume away: nothing MAKES a future
//! call come through here.** One wait loop is a shape, not a mechanism - a `.output()` written into
//! the parent module would be unbounded again and every test in the tree would still pass. Two
//! mechanisms were weighed. `clippy.toml`'s `disallowed-methods` is out: an entry is workspace-wide,
//! and `Command::output` / `Command::status` have dozens of legitimate call sites in `xtask` driving
//! other tools, which wait without a bound on purpose. A path-scoped xtask gate over this directory
//! - the shape `check-newtype-leaks` and `check-boot-order` already use, blanking comments and
//! string interiors first, because the prose here writes `.output()` while explaining the defect -
//! WOULD work, starts green, and is issue 274. What such a gate holds is narrow enough to be worth
//! saying now: *no second wait loop was written in this directory*, never *every docker child is
//! bounded*, because a wait laundered through a helper is invisible to a text scan.

use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How often a child still running is checked for having finished.
///
/// Short enough that the ordinary case - a call that answers at once - is not measurably delayed
/// by the polling, long enough that waiting does not become a spin.
const WAIT_POLL_MILLIS: u64 = 25;

/// The floor under every budget here. See [`budget_from_env`] for why it is not cosmetic.
pub(in crate::compose) const TIMEOUT_MIN_SECS: u64 = 1;
/// Ten minutes: long enough that no real daemon needs more to ANSWER a question, short enough to
/// stay a bound. The ceiling for a probe and for a status query alike, because both are questions.
pub(super) const ANSWER_TIMEOUT_MAX_SECS: u64 = 600;
/// Six hours. A pull that has not finished by then is a broken network rather than a slow one.
const PROVISION_TIMEOUT_MAX_SECS: u64 = 21_600;

/// What a reader does about a daemon that accepted a call and never answered.
///
/// **One sentence, here, because two conditions share it and this is the module that detects both.**
/// The pre-flight's [`super::Missing::WedgedDaemon`] and a status query that ran out of budget are
/// the same fault, so a second wording would be a second thing to keep true. It lives here rather
/// than beside `Missing` so the dependency points inward: this module knows nothing about the
/// pre-flight's vocabulary, and the pre-flight reads down into it.
///
/// **Why it names free space as well.** Silence is a SYMPTOM, and this message used to offer one
/// cause for it. A restart cannot fix a host that has run out of disk, so a reader who takes the
/// only advice on offer has spent their one idea on the wrong thing - and the second clause is a
/// conditional, not a diagnosis: it says what a restart will not achieve, which is true whatever
/// wedged the daemon. Whether an out-of-space host actually produces this silence is NOT claimed
/// here, because nobody filled a disk to find out.
pub(super) const WEDGED_DAEMON_REMEDY: &str = "RESTART the docker daemon - it is running but `docker info` never answered. A restart cannot fix a full disk, so check free space too";

/// What one kind of call is allowed: its default, the variable that changes it, and how far.
///
/// A value rather than three constants per kind, because the thing worth reading here is the
/// CONTRAST - two allowances side by side, orders of magnitude apart - and six separately named
/// constants hide it.
struct Allowance {
    /// The environment variable that overrides the default on a host where it is wrong.
    variable: &'static str,
    /// Seconds, where nothing overrides it.
    default_secs: u64,
    /// The ceiling an override is clamped to. A budget settable arbitrarily high is the unbounded
    /// wait again, wearing a number.
    max_secs: u64,
}

/// Which kind of compose subcommand this is - the distinction this module's header argues for.
///
/// Two kinds, because two is the smallest number that is not one. A third would need a caller that
/// treated it differently from both of these, and there is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    /// A subcommand that only ASKS. It reads what the runtime already knows, so anything slower
    /// than a few seconds means the daemon has stopped answering rather than that the work is big.
    Query,
    /// A subcommand that CHANGES something - pulls, creates, starts, destroys. Its duration is a
    /// function of images, containers and this host's network, not of the daemon's responsiveness.
    Provision,
}

/// The subcommands this tier issues that only ask a question.
///
/// Membership is [`Call::Query`] and nothing else is in it, so anything unrecognised takes the
/// provisioning budget. That is the direction whose wrong answer is recoverable: a query
/// misclassified as a provision is still bounded, only later than it should be, while a provision
/// misclassified as a query is a pull killed halfway. Both are bounded, which is the property.
///
/// **What that costs, stated because the fail-safe direction HIDES it:** a query subcommand this
/// list does not name takes the provisioning budget silently. Nothing - not the compiler, not a
/// test, not a gate - reports it, because a slice of strings is exhaustive over nothing. Making it
/// exhaustive means a `Subcommand` type the four call sites construct instead of writing arguments,
/// which is a bigger change than this one and belongs on its own.
const QUERIES: [&str; 3] = ["ps", "port", "ls"];

impl Call {
    /// Which kind a compose invocation's arguments describe.
    ///
    /// The subcommand is `extra`'s first element by construction: [`super::scoped_args`] puts every
    /// global flag ahead of it, and `docker compose` refuses a subcommand that arrives after one -
    /// so a misordered call fails at the runtime rather than being quietly misclassified here.
    fn of(extra: &[&str]) -> Self {
        match extra.first() {
            Some(subcommand) if QUERIES.contains(subcommand) => Self::Query,
            _ => Self::Provision,
        }
    }

    /// What this kind gets, and what changes it.
    const fn allowance(self) -> Allowance {
        match self {
            // Thirty seconds: far above what a healthy daemon needs to read its own state, so a
            // loaded host is not reported as a wedged one, and low enough that a readiness loop
            // noticing mid-provision costs seconds rather than the whole readiness deadline.
            Self::Query => Allowance {
                variable: "SUTURA_DOCKER_QUERY_TIMEOUT_SECS",
                default_secs: 30,
                max_secs: ANSWER_TIMEOUT_MAX_SECS,
            },
            // Thirty minutes, and generous on purpose, because this budget's wrong answer destroys
            // work: a cold pull of the metadata platform's stack is nine images and hundreds of
            // megabytes, and cutting it off leaves containers running that nothing then removes.
            Self::Provision => Allowance {
                variable: "SUTURA_DOCKER_PROVISION_TIMEOUT_SECS",
                default_secs: 1800,
                max_secs: PROVISION_TIMEOUT_MAX_SECS,
            },
        }
    }

    /// What a call of this kind is, in a sentence a person reads.
    const fn what(self) -> &'static str {
        match self {
            Self::Query => "a docker status query",
            Self::Provision => "a docker provisioning call",
        }
    }

    /// What a reader does about one that never answered.
    const fn remedy(self) -> &'static str {
        match self {
            // A status query that did not answer IS the wedged daemon: one condition, one wording.
            Self::Query => WEDGED_DAEMON_REMEDY,
            // A provisioning call is the case where silence is genuinely ambiguous - a wedged daemon
            // and a slow pull look identical from here - so the remedy names both readings.
            Self::Provision => {
                "RESTART the docker daemon if it has stopped answering, or raise this budget if a \
                 pull on this host is honestly slower than it"
            }
        }
    }
}

/// How long ONE compose subcommand may take, carrying the kind of call that spent it.
///
/// The kind travels with the duration rather than being re-derived where the failure is REPORTED,
/// because re-reading the environment there can print a number that was never spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    /// The kind of call this budget was set for.
    call: Call,
    /// How long it may take on this host.
    allowed: Duration,
}

impl Budget {
    /// The short answer budget for a Docker command that only reads daemon state.
    pub(crate) fn query() -> Self {
        Self::for_call(Call::Query)
    }

    /// The long provisioning budget for a Docker command that changes daemon state.
    pub(crate) fn provision() -> Self {
        Self::for_call(Call::Provision)
    }

    /// The budget a compose invocation's arguments call for, on this host.
    ///
    /// **Resolved once per KIND and remembered, and the reason is the readiness loop.** It issues a
    /// `ps` every 500ms for up to a 900s deadline, so a budget re-read per invocation is ~1800
    /// environment reads per `dev-up` - and, now that an unusable override says so, ~1800 identical
    /// warnings burying the one report a reader needs. The environment cannot change under this:
    /// `set_var` is `unsafe` and the workspace forbids `unsafe_code`, so nothing in this process
    /// writes one.
    pub(crate) fn of(extra: &[&str]) -> Self {
        Self::for_call(Call::of(extra))
    }

    /// Resolve and remember one allowance, including for non-Compose Docker commands whose first
    /// argument is `volume` rather than the operation that determines their budget.
    fn for_call(call: Call) -> Self {
        static QUERY: OnceLock<Duration> = OnceLock::new();
        static PROVISION: OnceLock<Duration> = OnceLock::new();

        let remembered = match call {
            Call::Query => &QUERY,
            Call::Provision => &PROVISION,
        };
        let allowance = call.allowance();
        Self {
            call,
            allowed: *remembered.get_or_init(|| budget_from_env(allowance.variable, allowance.default_secs, allowance.max_secs)),
        }
    }
}

/// One budget, read from the environment and clamped.
///
/// **Every overridable budget in this tier comes through here** - the probe's, both compose kinds',
/// and the readiness deadline in `super::super::health` - so the policy is written once. An absent
/// or unparseable value takes the default rather than refusing, because a timeout helper is the
/// wrong place to fail a startup over a malformed number.
///
/// CLAMPED, and both ends are load-bearing. `0` would report every call unanswered on a healthy
/// host, and a bound that fails closed on everything is not a bound - it is an outage that still
/// prints a reason. The ceiling keeps a very large value from restoring the unbounded wait this
/// exists to remove, and keeps the `probe_budget() * 3` the probe's own test computes from
/// overflowing a `Duration`.
///
/// A value that was SET and cannot be used as written is said out loud, which is the whole reason
/// [`Override`] is a value: defaulting silently is right for a timeout helper, and leaves a
/// mistyped knob looking like it worked.
pub(in crate::compose) fn budget_from_env(variable: &str, default_secs: u64, max_secs: u64) -> Duration {
    let resolved = Override::of(std::env::var(variable).ok().as_deref(), default_secs, max_secs);
    if let Override::Unusable { asked, used } = resolved {
        match asked {
            Some(asked) => eprintln!("xtask compose: {variable}={asked} is outside 1..={max_secs} - using {used}s"),
            None => eprintln!("xtask compose: {variable} is not a whole number of seconds - using {used}s"),
        }
    }
    Duration::from_secs(resolved.seconds())
}

/// What an override resolved to, and whether the value given was usable as written.
///
/// **Pure, so the clamp is asserted rather than reasoned about.** Every budget in this tier comes
/// through [`budget_from_env`], including the readiness deadline - which is where the missing floor
/// that degraded the gate to a single poll came from - and a clamp nothing tests is a clamp that
/// can be lost in an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Override {
    /// The value in force: the default where nothing was set, or an override used as written.
    Used(u64),
    /// A value was set and cannot be used as written: not a number, or outside the clamp.
    Unusable {
        /// What was asked for, where it parsed at all.
        asked: Option<u64>,
        /// What is used instead.
        used: u64,
    },
}

impl Override {
    /// Resolve one override against its default and ceiling.
    fn of(value: Option<&str>, default_secs: u64, max_secs: u64) -> Self {
        let clamp = |secs: u64| secs.clamp(TIMEOUT_MIN_SECS, max_secs);
        let Some(value) = value else {
            return Self::Used(clamp(default_secs));
        };
        let Ok(asked) = value.parse::<u64>() else {
            return Self::Unusable {
                asked: None,
                used: clamp(default_secs),
            };
        };
        let used = clamp(asked);
        if used == asked {
            Self::Used(used)
        } else {
            Self::Unusable {
                asked: Some(asked),
                used,
            }
        }
    }

    /// The budget in seconds, whichever way it was reached.
    const fn seconds(self) -> u64 {
        match self {
            Self::Used(secs) | Self::Unusable { used: secs, .. } => secs,
        }
    }
}

/// Wait for a spawned child, bounded. `Ok(None)` is the budget having expired.
///
/// **The only place in this module that waits on a process**, and that is the property worth having
/// rather than tidiness: a second wait loop is a second chance to write an unbounded one, and this
/// surface has now been through that defect twice.
///
/// A child this function will not report a status for is killed AND reaped before it returns, and
/// that is **both** non-answering paths: the budget expiring, and the wait itself failing. One left
/// unreaped would put a zombie behind every gate that runs, and one left un-killed is the unbounded
/// wait again with nobody waiting on it - the case that reaches the second arm, an `ECHILD` from a
/// `SIGCHLD` disposition this process did not choose, is exactly the one where nothing else will.
///
/// Two limits, and both callers rely on them:
///
/// 1. It kills the child it spawned, NOT that child's own descendants. `docker` runs CLI plugins as
///    separate processes, and one can outlive the kill and stay blocked on the same socket. Nothing
///    here waits on them, so it costs this function nothing - but a wedged daemon does leave them
///    behind until it recovers, and somebody counting stray processes should know they are looking at
///    that rather than at a leak in this loop.
/// 2. It says nothing about what the child WROTE. A caller that needs the output has to arrange for
///    it before spawning, and [`run`] is where the reason that arrangement is files rather than
///    pipes is written down.
pub(super) fn waited(child: &mut Child, budget: Duration) -> Result<Option<ExitStatus>, std::io::Error> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Err(cause) => {
                abandon(child);
                return Err(cause);
            }
            Ok(None) => {}
        }
        if started.elapsed() >= budget {
            abandon(child);
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(WAIT_POLL_MILLIS));
    }
}

/// Stop caring about a child, without leaving it running or unreaped.
///
/// One function so the two non-answering arms of [`waited`] cannot drift into doing different
/// things, which they did: the wait-error arm returned with the `docker` process still running and
/// nothing left to reap it.
///
/// The reap is itself an unbounded `wait`, and that is deliberate rather than overlooked: the
/// process has just been sent `SIGKILL`, so it is already gone or about to be, and a bound here
/// would be a bound on the kernel. The descendant limit [`waited`] states is unchanged - the kill
/// reaches `docker`, not the CLI plugins it spawned.
fn abandon(child: &mut Child) {
    drop(child.kill());
    drop(child.wait());
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

/// Why a compose invocation produced nothing to read.
///
/// Not the same thing as a subcommand that FAILED: a non-zero exit is an answer, and it arrives as
/// [`Output::ok`] being false with the runtime's own diagnostic in [`Output::stderr`]. This is the
/// absence of an answer, and the two variants are the two ways to get one.
#[derive(Debug)]
pub(crate) enum Failed {
    /// **Nothing was started.** A capture that could not be opened, or a spawn that failed.
    ///
    /// `doing` says which half broke, because the two send a reader to different places: a spawn
    /// that failed is about docker, and a capture that could not be opened is about this host's
    /// temporary directory.
    Broken {
        /// What could not be done, in the words the message uses.
        doing: &'static str,
        /// Why not.
        cause: std::io::Error,
    },
    /// **It was started and this process lost track of it**: the wait itself failed, so whether it
    /// finished is unknown. Killed and reaped on the way out, exactly as a timeout is.
    ///
    /// Separate from [`Self::Broken`] because [`Self::left_running`] answers differently on the
    /// two, and getting that wrong is the expensive direction: a provisioning call that was
    /// SPAWNED may have started containers, and reporting it as having started nothing sends the
    /// reader past a tier that is half up. The budget travels with it so the answer comes off the
    /// kind of call, the same way a timeout's does.
    Lost {
        /// The budget the lost call was running under, and therefore its kind.
        budget: Budget,
        /// Why the wait failed. `ECHILD` is the case this exists for: something else reaped the
        /// child - a `SIG_IGN` disposition for `SIGCHLD`, or a reaper in the process - and
        /// `waitpid` then answers at once with no status to report.
        cause: std::io::Error,
    },
    /// It was still running when its budget expired, and has been killed. The budget carries how long
    /// was spent and which kind of call spent it, so the message needs no second copy of either.
    Silent(Budget),
}

impl Failed {
    /// Did the call leave containers running?
    ///
    /// **Decided here rather than at a call site**, because the cause is [`waited`]'s kill: killing
    /// the `docker` process does not stop what it had already started, and only a PROVISIONING call
    /// starts anything. A caller that remembered to ask for itself is a caller the next one forgets
    /// to copy - `dev-down` issues a provisioning call too - so the value answers instead.
    ///
    /// **The limit, and it is narrower than the name suggests: this answers for the CALL, not for
    /// the run.** A timed-out `ps` started nothing, so this says `false` - but a readiness `ps`
    /// runs after `up --detach` has already returned, and that run's containers are up. The caller
    /// that knows a provision preceded it says so itself; see `super::super::health`.
    pub(crate) const fn left_running(&self) -> bool {
        match *self {
            Self::Silent(ref budget) | Self::Lost { ref budget, .. } => matches!(budget.call, Call::Provision),
            Self::Broken { .. } => false,
        }
    }
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Broken { doing, cause } => write!(f, "could not {doing}: {cause}"),
            Self::Lost { budget, cause } => write!(
                f,
                "could not wait for {}: {cause} - it was started, so whether it finished is unknown",
                budget.call.what()
            ),
            Self::Silent(budget) => write!(
                f,
                "{} never answered within {}s ({}) - {}",
                budget.call.what(),
                budget.allowed.as_secs(),
                budget.call.allowance().variable,
                budget.call.remedy()
            ),
        }
    }
}

impl std::error::Error for Failed {
    /// The cause chain, which the sibling `super::super::lock`'s error also carries: a `Display`
    /// that flattened the `io::Error` into a string would be the end of the chain.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Broken { cause, .. } | Self::Lost { cause, .. } => Some(cause),
            Self::Silent(_) => None,
        }
    }
}

/// Where a bounded call's output is collected, and the reason it is not a pipe.
///
/// **Files, because a pipe puts the hang back.** `Command::output()` reads the pipes and waits in one
/// operation, which is exactly what could not be bounded; polling for the exit while the child writes
/// into a pipe deadlocks the moment the pipe buffer fills, and draining it needs a reader thread per
/// stream whose join is unbounded for the reason [`waited`] gives - `docker` spawns CLI plugins that
/// inherit the write end, so one can outlive the kill, hold the pipe open, and leave the reader
/// blocked forever. A file has no reader to block and no buffer to fill.
///
/// Removed on drop, so an early return does not leave two files per call behind.
struct Captured {
    /// Where the child's standard output goes.
    stdout: PathBuf,
    /// Where its standard error goes.
    stderr: PathBuf,
}

/// The two handles a bounded call hands its child, or the reason they could not be opened.
type Handles = Result<(Stdio, Stdio), std::io::Error>;

impl Captured {
    /// Two paths in the platform's temporary directory, named by process, clock and counter so that
    /// two calls in one run - and two runs at once - cannot collect into the same file.
    ///
    /// The clock is in the name because the process id alone is not unique over time: a run killed
    /// before [`Drop`] leaves its files behind, and `create_new` in [`Self::handles`] would then
    /// FAIL a later call that reused the id rather than silently truncate. Unpredictable is not
    /// claimed - the guarantee is [`Self::handles`]'s, not this name's.
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let unique = format!(
            "sutura-compose-{}-{}-{}",
            std::process::id(),
            since_epoch.as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        // THE KEY IS IN THE SAME STATEMENT AS THE TAKING, deliberately: `cargo xtask
        // check-worktree-state` reads a shared-root taking and the segment it narrows to, and a
        // bare `let at = std::env::temp_dir();` narrowed by a `join` ten lines away is a taking it
        // cannot attribute. Nothing about the paths changed - `unique` carries no dot, so the two
        // extensions land exactly where the two `join`s put them.
        let at = std::env::temp_dir().join(unique);
        Self {
            stdout: at.with_extension("out"),
            stderr: at.with_extension("err"),
        }
    }

    /// The two handles to hand the child.
    ///
    /// **`create_new`, so an existing name is refused rather than written through.** The platform's
    /// temporary directory is world-writable and the name is derivable, so `File::create` - which
    /// truncates, and follows a symlink to do it - lets any local user pre-create one of these as a
    /// link and have the target overwritten as the invoking user. On a `port` call it is worse than
    /// destructive: the attacker chooses the bytes [`super::first_published`] reads back, and those
    /// land in the discovery file a harness connects to. `O_CREAT | O_EXCL` fails on an existing
    /// path, symlink included, so that call fails closed as [`Failed::Broken`] instead.
    ///
    /// A sticky bit on the directory does not help: it stops deleting somebody else's file, not
    /// creating a new name.
    fn handles(&self) -> Handles {
        let fresh = |path: &PathBuf| std::fs::File::options().write(true).create_new(true).open(path);
        Ok((Stdio::from(fresh(&self.stdout)?), Stdio::from(fresh(&self.stderr)?)))
    }

    /// What the child wrote, as (stdout, stderr).
    ///
    /// A stream that cannot be read back is empty rather than an error: the call's own exit is the
    /// thing a caller acts on, and failing a provision because a temporary file went missing would
    /// replace a diagnostic with a second failure.
    fn read(&self) -> (String, String) {
        let text = |path: &PathBuf| {
            std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default()
        };
        (text(&self.stdout), text(&self.stderr))
    }
}

impl Drop for Captured {
    fn drop(&mut self) {
        drop(std::fs::remove_file(&self.stdout));
        drop(std::fs::remove_file(&self.stderr));
    }
}

/// Run one command inside the budget its kind allows, and collect what it printed.
pub(super) fn run(command: &mut Command, budget: Budget) -> Result<Output, Failed> {
    let broken = |doing: &'static str| move |cause| Failed::Broken { doing, cause };
    let captured = Captured::new();
    let (stdout, stderr) = captured.handles().map_err(broken("capture docker's output"))?;
    let spawned = command
        // stdin null for the reason `super::probed` gives: nothing here reads input, and a child
        // that inherited the terminal can eat a keystroke meant for the hook that ran it.
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn();
    let mut child = spawned.map_err(broken("run docker"))?;
    let status = answered(waited(&mut child, budget.allowed), budget)?;
    let (stdout, stderr) = captured.read();
    Ok(Output {
        stdout,
        stderr,
        ok: status.success(),
    })
}

/// What a bounded wait's outcome means, given the budget it was spent under.
///
/// **Its own function because the wait-ERROR arm cannot be produced on demand.** `try_wait` fails
/// when `waitpid` answers `ECHILD`, which needs a `SIGCHLD` disposition of `SIG_IGN` in this
/// process - `unsafe` to arrange, and this workspace forbids `unsafe_code`. Taking the wait's
/// result as a value makes the DECISION assertable without arranging the condition, which is what
/// stopped a spawned call from being reported as one that started nothing.
///
/// PURE, and deliberately: it decides which failure this is and returns the status, so what the
/// child wrote is read by [`run`]. A version that also took the capture made its test build a
/// temporary directory to assert a branch that never touches one.
fn answered(waited: Result<Option<ExitStatus>, std::io::Error>, budget: Budget) -> Result<ExitStatus, Failed> {
    match waited {
        Ok(Some(status)) => Ok(status),
        Ok(None) => Err(Failed::Silent(budget)),
        // NOT `Broken`: the child was spawned, so a provisioning call may have started containers,
        // and the budget is what lets `left_running` say so.
        Err(cause) => Err(Failed::Lost { budget, cause }),
    }
}

/// A command that never exits, built from SHELL BUILTINS ONLY.
///
/// Not `sleep 60`, and the reason is the bug the first version of this test had. These run inside
/// a nix check sandbox whose `PATH` is the derivation's and not the host's, so where `sleep` is
/// absent `/bin/sh -c 'sleep 60'` exits 127 in about a millisecond - and a test asserting only
/// "did not answer" then passed without ever reaching the timeout it exists for. `:` is a special
/// builtin that no `PATH` can take away, so this blocks on every host or not at all.
///
/// It spins rather than sleeps, which is fine at these budgets and buys something: the shell is
/// the process that blocks, so the kill lands on it directly and leaves no descendant behind -
/// which `sleep 60` did, once per run, for a minute.
#[cfg(test)]
pub(super) fn never_answers() -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "while :; do :; done"]);
    command
}

/// A command that answers at once with the given exit status.
#[cfg(test)]
pub(super) fn answers_with(code: u8) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", &format!("exit {code}")]);
    command
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::{Budget, Call, Failed, PROVISION_TIMEOUT_MAX_SECS, TIMEOUT_MIN_SECS, answers_with, never_answers, run};

    /// A budget of a stated length, so a test spends milliseconds rather than the real allowance.
    ///
    /// Constructed field-wise on purpose: `Budget::of` reads the environment, and the smallest value
    /// the clamp there permits is a whole second - which is a second per assertion, for no gain.
    /// Mutating the environment is not the alternative: `set_var` is `unsafe`, and this workspace
    /// FORBIDS `unsafe_code`.
    const fn budget_of(call: Call, millis: u64) -> Budget {
        Budget {
            call,
            allowed: Duration::from_millis(millis),
        }
    }

    #[test]
    fn a_status_query_and_a_provisioning_call_do_not_share_one_budget() {
        // The decision the whole change is about. One budget over `compose` is either too long to
        // bound the `ps` a readiness loop repeats - the call that hung `just dev-up` - or short
        // enough to kill a legitimate pull halfway and leave containers behind.
        let query = Budget::of(&["ps", "--all", "--format", "json"]);
        let provision = Budget::of(&["up", "--detach", "--remove-orphans"]);
        assert_eq!(query.call, Call::Query);
        assert_eq!(provision.call, Call::Provision);
        // Not merely different: an order of magnitude apart, which is the property that makes two
        // classes worth having. A pair of budgets a few seconds apart would be one budget.
        //
        // Over the DEFAULTS, which are pure, and not over `Budget::of`, which reads the
        // environment. Asserted there this went red on a configuration the change itself invites -
        // `SUTURA_DOCKER_PROVISION_TIMEOUT_SECS=300` makes 30s x 10 exactly 300s - and a check
        // that a supported setting turns red is a check on its way to being deleted.
        let (query_default, provision_default) = (Call::Query.allowance().default_secs, Call::Provision.allowance().default_secs);
        assert!(
            query_default * 10 < provision_default,
            "{query_default}s and {provision_default}s are not an order of magnitude apart"
        );

        // Every call site this tier has, classified.
        assert_eq!(Budget::of(&["port", "postgres", "5432"]).call, Call::Query);
        assert_eq!(Budget::of(&["ls", "--all", "--format", "json"]).call, Call::Query);
        assert_eq!(Budget::of(&["down", "--volumes", "--remove-orphans"]).call, Call::Provision);

        // And the fail-safe direction: unrecognised takes the PROVISIONING budget. A query given
        // too long is bounded, only later than it should be; a pull given a query's budget is
        // killed mid-pull, which is the unrecoverable half.
        assert_eq!(Budget::of(&["pull"]).call, Call::Provision);
        assert_eq!(Budget::of(&[]).call, Call::Provision);

        // Both are bounds. Neither is zero, which would report a healthy daemon as silent, and
        // neither is open-ended, which is the wait this module exists to remove.
        for budget in [query, provision] {
            assert!(budget.allowed >= Duration::from_secs(TIMEOUT_MIN_SECS), "{budget:?}");
            assert!(
                budget.allowed <= Duration::from_secs(PROVISION_TIMEOUT_MAX_SECS),
                "{budget:?}"
            );
        }
    }

    #[test]
    fn a_compose_call_that_never_answers_is_bounded_and_reported_silent() {
        // The defect this closes: `compose` waited with `Command::output()`, which has no timeout,
        // so a daemon that wedged AFTER the pre-flight passed hung `just dev-up` forever.
        let budget = budget_of(Call::Query, 250);
        let started = Instant::now();
        let outcome = run(&mut never_answers(), budget);
        let waited = started.elapsed();

        let Err(Failed::Silent(spent)) = outcome else {
            panic!("a call that never answered must be Silent");
        };
        assert_eq!(spent.allowed, budget.allowed);
        // THE ASSERTION THAT MAKES THIS NON-VACUOUS, and it is the one that was missing on the
        // probe's first version: without it every fast failure passes - a missing interpreter, a
        // missing `sleep`, a syntax error - because each returns in about a millisecond and is
        // still not an answer. This says the budget was actually spent.
        assert!(
            waited >= budget.allowed,
            "the budget was never consumed - waited only {waited:?}"
        );
        // And still bounded. Generous on purpose: what is asserted is BOUNDED, not fast.
        assert!(
            waited < Duration::from_secs(10),
            "the call was not bounded: waited {waited:?}"
        );

        // The message a person gets has to name the budget that was spent and the variable that
        // raises it, or the only remedy on offer is to run it again and wait as long.
        let reported = Failed::Silent(spent).to_string();
        assert!(reported.contains("SUTURA_DOCKER_QUERY_TIMEOUT_SECS"), "{reported}");
        assert!(reported.contains("RESTART"), "{reported}");
        // Both remedies, because a reader who only restarts has spent their one idea if the host
        // is out of space - and the two conditions are indistinguishable from a timed-out call.
        assert!(reported.contains("free space"), "{reported}");
    }

    #[test]
    fn a_compose_call_that_answers_is_read_rather_than_timed_out() {
        // The other direction, and the one that catches a budget so tight that a healthy daemon
        // reads as wedged. A bound that fails closed on everything is not a bound.
        let budget = budget_of(Call::Query, 30_000);
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf answered; printf complained >&2; exit 0"]);
        let out = run(&mut command, budget).expect("a command that exits must produce output");
        assert!(out.ok);
        assert_eq!(out.stdout, "answered");
        assert_eq!(out.stderr, "complained");

        // A non-zero exit is an ANSWER, not a failure of this function: the runtime said something
        // and its own diagnostic is the useful one. Conflating the two would turn every compose
        // error into "could not run docker".
        let failed = run(&mut answers_with(1), budget).expect("a non-zero exit is still an answer");
        assert!(!failed.ok);
    }

    #[test]
    fn output_past_a_pipe_buffer_is_captured_rather_than_deadlocking() {
        // Why the capture is files and not pipes. Polling for the exit while the child writes into
        // a pipe deadlocks as soon as the pipe buffer fills - the child blocks on the write, the
        // poll never sees an exit, and the budget then reports a healthy command as silent. This is
        // several times a typical 64 KiB buffer, written by a shell builtin so no `PATH` is needed.
        let line = "0123456789012345678901234567890123456789";
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &format!("i=0; while [ $i -lt 8000 ]; do echo {line}; i=$((i+1)); done")]);
        let out = run(&mut command, budget_of(Call::Provision, 60_000)).expect("it exits");
        assert!(out.ok, "{}", out.stderr);
        assert_eq!(out.stdout.lines().count(), 8000);
        assert!(out.stdout.len() > 300_000, "{} bytes", out.stdout.len());
    }

    #[test]
    fn a_call_that_cannot_be_started_is_broken_rather_than_silent() {
        // The two ways to get no answer are not the same answer. A binary that is not there
        // refused; only silence is the wedged daemon, and only silence names a budget.
        let mut command = Command::new("/nonexistent/sutura-not-a-binary");
        let outcome = run(&mut command, budget_of(Call::Query, 30_000));
        let Err(failed @ Failed::Broken { .. }) = outcome else {
            panic!("a command that cannot be spawned must be Broken");
        };
        assert!(failed.to_string().contains("could not run docker"), "{failed}");
        // Nothing was started, so nothing was left running - which is the half a caller acts on.
        assert!(!failed.left_running());
    }

    #[test]
    fn only_a_timed_out_provisioning_call_leaves_containers_running() {
        // The consequence of the kill, answered by the layer that did the killing rather than by a
        // `matches!` each caller has to remember: `dev-up` and `dev-down` both issue a provisioning
        // call, and a third caller would forget to copy the guard.
        assert!(Failed::Silent(Budget::of(&["up", "--detach"])).left_running());
        assert!(Failed::Silent(Budget::of(&["down", "--volumes"])).left_running());
        // A status query starts nothing, so a timed-out `ps` leaves nothing behind and must not
        // print a teardown remedy - that would send a reader to remove a tier that is coming up.
        assert!(!Failed::Silent(Budget::of(&["ps", "--all"])).left_running());
    }

    #[test]
    fn a_call_whose_wait_failed_is_lost_rather_than_never_started() {
        // The state `Broken` used to absorb, and the one where absorbing it costs containers: the
        // child WAS spawned and then `try_wait` failed, so what a provisioning call had started is
        // unaccounted for. Reported as having started nothing, a `dev-up` here prints no
        // `just dev-down` and the reader walks away from a tier that is half up.
        //
        // Asserted through `answered`, which is the function `run` uses, so this is the wiring and
        // not a hand-built value: the arm cannot be reached on demand, because `try_wait` fails on
        // `ECHILD` and arranging that needs `unsafe`, which this workspace forbids.
        let echild = || std::io::Error::from(std::io::ErrorKind::NotFound);

        let Err(lost @ Failed::Lost { .. }) = super::answered(Err(echild()), Budget::of(&["up", "--detach"])) else {
            panic!("a wait that failed on a spawned child must be Lost, not Broken");
        };
        assert!(
            lost.left_running(),
            "a provisioning call that was started must not report that it started nothing"
        );
        // It says the outcome is UNKNOWN. "could not wait for docker" alone reads as a tool
        // failure, and sends the reader at the daemon rather than at their containers.
        let reported = lost.to_string();
        assert!(reported.contains("unknown"), "{reported}");
        assert!(
            std::error::Error::source(&lost).is_some(),
            "the cause chain is the diagnostic"
        );

        // And a lost STATUS query still started nothing, so the two kinds stay distinguishable.
        let Err(query) = super::answered(Err(echild()), Budget::of(&["ps", "--all"])) else {
            panic!("Lost");
        };
        assert!(!query.left_running());
    }

    #[test]
    fn an_override_that_cannot_be_used_as_written_is_not_silently_the_default() {
        // The clamp, as a value rather than as a Duration nothing can see into - which is where the
        // readiness deadline's MISSING FLOOR came from: `SUTURA_DEV_READY_TIMEOUT_SECS=0` degraded
        // the gate to a single poll, and nothing anywhere asserted that it could not.
        let ceiling = 600;
        let of = |value: Option<&str>| super::Override::of(value, 30, ceiling);

        assert_eq!(of(None), super::Override::Used(30), "nothing set means the default");
        assert_eq!(of(Some("45")), super::Override::Used(45));

        // Both ends of the clamp, and both are load-bearing. `0` would report every call on a
        // healthy host as unanswered, and a value past the ceiling restores the unbounded wait.
        assert_eq!(
            of(Some("0")),
            super::Override::Unusable {
                asked: Some(0),
                used: TIMEOUT_MIN_SECS
            }
        );
        assert_eq!(
            of(Some("5000")),
            super::Override::Unusable {
                asked: Some(5000),
                used: ceiling
            }
        );
        // Not a number at all. `asked` is `None`, which is what makes the two messages different:
        // one says a value was out of range, the other that it was not a number.
        assert_eq!(of(Some("abc")), super::Override::Unusable { asked: None, used: 30 });

        // Whichever way it was reached, a budget comes out - the helper never refuses a startup
        // over a malformed number, which is the direction a timeout helper has to take.
        for value in [None, Some("45"), Some("0"), Some("5000"), Some("abc"), Some("")] {
            let seconds = of(value).seconds();
            assert!((TIMEOUT_MIN_SECS..=ceiling).contains(&seconds), "{value:?} -> {seconds}");
        }
    }
}
