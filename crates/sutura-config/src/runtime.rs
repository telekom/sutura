//! How much work may be in flight at once, how wide the engine runs, and how long stopping may
//! take.
//!
//! # Why this is its own group and not four more `server` keys
//!
//! Because it bounds a different thing from anything in [`crate::server`]. Those keys are about one
//! request: where it arrives, how big it may be, how long the caller waits for a reply. These are
//! about the *process*: how many questions execute at once, how wide the engine that executes them
//! is, and what the budget for winding down is.
//!
//! The distinction matters most for the timeout. `server.request_timeout_seconds` is a deadline on
//! the **reply**, not a bound on the **work**. When it expires the caller is answered `408` and the
//! handler future is dropped - and a started `tokio::task::spawn_blocking` task cannot be aborted,
//! so the question keeps running. Without something else in the picture, a caller asking questions
//! that cost more than the timeout gets a fast turnaround while the deployment keeps the whole
//! cost, and in-flight work accumulates at the rate limit with nothing shedding it. The blocking
//! pool defaults to 512 threads with an unbounded queue, so that backlog is bounded by memory.
//!
//! [`QueryConcurrency`] is the bound on the work. It is the number of questions that may be
//! *executing*, and the permit is held by the blocking task rather than by the handler future - so
//! a timed-out request does not hand its slot back until the work it started actually finishes.
//! That is what makes the backlog a number somebody chose.
//!
//! [`AdmissionTimeout`] stops the queue in front of that bound from being a second unbounded thing.
//! A question that cannot get a slot inside it is refused rather than left waiting, and a refused
//! waiter costs a dropped future rather than a thread.
//!
//! [`EngineWorkers`] is not a bound at all - it is a width. The in-process engine drives its own
//! runtime and blocks on it, so a single-threaded one is a contention point every concurrent
//! question shares. See `sutura_exec_datafusion::DataFusionWarehouse`.
//!
//! [`WorkingSetCeiling`] is the bound that did not exist. Nothing built a `RuntimeEnv`, so the engine
//! installed its unbounded memory pool - and under `panic = "abort"` a hash join wide enough to
//! outgrow the machine is the process ending for every caller in flight rather than an error for the
//! one who asked. It is a **query-wide** value with no per-source override, and the reason it is not
//! symmetric with the deadline is on the type.
//!
//! [`ShutdownGrace`] is the budget for stopping, and it covers the whole of stopping rather than
//! the connection drain alone.
//!
//! Every type here has the shape of [`crate::server::RequestTimeout`], which is the pattern this
//! crate already had: a private field, one `parse` that is the only constructor, a ceiling the type
//! declares, and a zero refused rather than read as `no limit`. They share
//! [`InvalidBound`] rather than growing a second error, so a bad value here lands in the same
//! `SettingsError` variant a bad server bound does.

use std::time::Duration;

use crate::server::InvalidBound;

/// How many questions may be executing at once.
///
/// Not how many may be *in flight*: a request waiting for a slot, parsing a body or writing a
/// response is not counted. This is the number of questions holding a blocking-pool thread and a
/// data system, which is the resource that runs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryConcurrency(usize);

/// How long a question may wait for one of those slots.
///
/// Bounded because the alternative is a queue nothing empties. Shorter than the request timeout by
/// default, on purpose: a caller who has waited five seconds for a slot is better served by a
/// `503` they can retry than by a `408` twenty-five seconds later that says the same thing less
/// clearly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionTimeout(Duration);

/// How many threads the in-process engine's own runtime gets.
///
/// Resolved to a number at load time rather than kept as "whatever the machine has", so the value
/// in the startup log is the value in effect. An absent key follows
/// `std::thread::available_parallelism`; a present one wins, which is what a container with a CPU
/// quota needs - `available_parallelism` reports what the kernel exposes, and on most container
/// runtimes that is the host's core count rather than the cgroup's share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineWorkers {
    count: usize,
    chosen: bool,
}

/// How many bytes the engine's operators may reserve at once, across the whole process.
///
/// **This is the bound that did not exist, and its absence was process death.** Nothing constructed
/// a `RuntimeEnv`, so the engine installed its unbounded memory pool: a hash join or an aggregate
/// wide enough to outgrow the machine allocated until the allocator failed, and shipped profiles
/// compile `panic = "abort"`, so that is not an error for the caller who asked - it is the process
/// ending for every caller in flight. A bounded pool turns it into a reservation that fails, which
/// leaves as `RefusalReason::ResourcesExhausted`.
///
/// # What it counts, and what it does not
///
/// The pool counts what the engine's own operators reserve - a hash-join build side, aggregate
/// state, a sort - and nothing else. **Not** what a driver buffers before conversion, **not**
/// `collect()` materialising every batch, **not** the row set built while a result is converted into
/// domain rows. So this is not a bound on the process's memory and must not be read as one: a
/// question large enough to end the process on one of those paths still ends it. The bound that
/// reaches those is a byte budget applied as rows are converted, which
/// `docs/adr/0009-the-plan-from-one-source-to-many.md` puts with the execution boundary rather than
/// here.
///
/// # Global, and no per-source override
///
/// There is one combiner and one working set, so a per-source ceiling would be a number with nothing
/// to bound - and 0009 decides that a source declaration carrying one is **refused at parse rather
/// than ignored**, because a setting that silently does nothing is worse than a missing one.
/// `deny_unknown_fields` on every on-disk shape is the mechanism, and the test that provokes it is in
/// [`crate::settings`]. **The deadline is the bound that takes a per-source override; this one does
/// not**, and the two are decided separately on purpose.
///
/// # Never spill
///
/// Decided rather than defaulted, and the second reason is what settles it. A refusal the caller sees
/// beats a degraded answer it cannot; and spilling writes the *asking subject's rows* to the pod's
/// local disk, an ungoverned data-at-rest surface, on the one path whose whole purpose is that a
/// query runs as the person who asked. So no spill directory and no disk sizing - the adapter builds
/// its runtime with temporary files disabled, and `sutura_exec_datafusion::WorkingSet` is where that
/// is written down.
///
/// # A provisional number
///
/// [`Self::DEFAULT_BYTES`] is a gibibyte and nobody has measured it. It is a starting point recorded
/// as one, not a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkingSetCeiling {
    bytes: core::num::NonZeroUsize,
    /// What the ceiling was compared against at boot, when the platform would say.
    ///
    /// Recorded rather than recomputed, for the reason `EngineWorkers` records `chosen`: the check is
    /// not reversible once the value is stored, and an operator reading a ceiling in the startup log
    /// needs to know whether anything verified it. `None` is a platform that would not answer - see
    /// [`available_memory_bytes`], which says which ones do.
    checked_against: Option<u64>,
}

/// How long stopping may take, once stopping has been asked for.
///
/// Chosen against the deadline on the other side rather than as a round number. An orchestrator
/// sends a termination signal and starts a kill timer - the usual window is thirty seconds - and a
/// process still running when that expires is killed mid-answer, so whatever it would have done on
/// the way out does not happen. Fifteen seconds leaves room for the exit itself.
///
/// It bounds the *whole* of stopping and not the connection drain alone. The drain gets the budget
/// first; what is left of it is what the runtime will wait for a blocking task it cannot cancel.
/// See `sutura_runtime::Shutdown::remaining_grace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownGrace(Duration);

impl QueryConcurrency {
    /// The ceiling. `tokio`'s blocking pool defaults to 512 threads, so a bound above that is a
    /// number with nothing behind it: the pool would be the real limit while this key read as
    /// though it were.
    pub const MAX: usize = 512;

    /// Reads a concurrency bound.
    pub const fn parse(count: usize) -> Result<Self, InvalidBound> {
        if count == 0 {
            return Err(InvalidBound::Zero {
                name: "runtime.max_concurrent_queries",
            });
        }
        if count > Self::MAX {
            return Err(InvalidBound::TooLarge {
                name: "runtime.max_concurrent_queries",
                found: count as u64,
                limit: Self::MAX as u64,
            });
        }
        Ok(Self(count))
    }

    #[inline]
    pub const fn count(self) -> usize {
        self.0
    }
}

impl AdmissionTimeout {
    /// The ceiling, in seconds. The same five minutes
    /// [`RequestTimeout`](crate::server::RequestTimeout) allows, because a wait longer than the
    /// request timeout cannot be observed - the timeout layer answers first and this key stops
    /// doing anything. The overlap is documented rather than refused: which of the two answers is a
    /// deployment's own choice, and a cross-group refusal would take that choice away.
    pub const MAX_SECONDS: u64 = 300;

    /// Reads a wait in whole seconds.
    pub const fn parse(seconds: u64) -> Result<Self, InvalidBound> {
        if seconds == 0 {
            return Err(InvalidBound::Zero {
                name: "runtime.admission_timeout_seconds",
            });
        }
        if seconds > Self::MAX_SECONDS {
            return Err(InvalidBound::TooLarge {
                name: "runtime.admission_timeout_seconds",
                found: seconds,
                limit: Self::MAX_SECONDS,
            });
        }
        Ok(Self(Duration::from_secs(seconds)))
    }

    #[inline]
    pub const fn duration(self) -> Duration {
        self.0
    }

    #[inline]
    pub const fn seconds(self) -> u64 {
        self.0.as_secs()
    }
}

impl EngineWorkers {
    /// The ceiling. Well past any machine this runs on, and low enough that a mistyped key is a
    /// refusal rather than a thousand threads.
    pub const MAX: usize = 256;

    /// Reads a worker count, or resolves the absent one.
    ///
    /// Not a `const fn`, unlike its neighbours, because the absent case asks the operating system
    /// how many threads this machine can run at once. A machine that will not answer is treated as
    /// one, which is the behaviour this adapter had before the key existed rather than a failure to
    /// start.
    pub fn parse(configured: Option<usize>) -> Result<Self, InvalidBound> {
        let Some(count) = configured else {
            return Ok(Self {
                count: std::thread::available_parallelism().map_or(1, core::num::NonZeroUsize::get),
                chosen: false,
            });
        };
        if count == 0 {
            return Err(InvalidBound::Zero {
                name: "runtime.engine_worker_threads",
            });
        }
        if count > Self::MAX {
            return Err(InvalidBound::TooLarge {
                name: "runtime.engine_worker_threads",
                found: count as u64,
                limit: Self::MAX as u64,
            });
        }
        Ok(Self { count, chosen: true })
    }

    #[inline]
    pub const fn count(self) -> usize {
        self.count
    }

    /// Did an operator write this number, or did the machine?
    ///
    /// For the startup log, and for the same reason `api.docs` records it: an operator reading
    /// `engine_worker_threads = 8` needs to know whether that was their decision.
    #[inline]
    pub const fn was_chosen(self) -> bool {
        self.chosen
    }
}

impl WorkingSetCeiling {
    /// The configuration key this value comes from. Named once, so the error and the message agree.
    const KEY: &'static str = "runtime.working_set_max_bytes";

    /// The provisional default: one gibibyte.
    ///
    /// **Provisional is the operative word.** Nobody has measured it against a corpus, so it is a
    /// starting point rather than a finding, and `docs/adr/0009` records it as one on purpose - to
    /// stop a number nobody measured from hardening into a decision by being written down.
    ///
    /// It is also what `defaults.yaml` writes, and a test asserts the two are the same number rather
    /// than trusting that nobody edited one of them.
    pub const DEFAULT_BYTES: u64 = 1024 * 1024 * 1024;

    /// The ceiling this type declares, in bytes. One tebibyte.
    ///
    /// Well past any machine this runs on, which is the same reasoning [`EngineWorkers::MAX`] uses:
    /// the type's job here is to catch a mistyped key rather than to size a deployment. The bound
    /// that actually protects the process is the comparison against
    /// [`available_memory_bytes`], because a value under this ceiling and over the container's limit
    /// is exactly the mistake that ends the process.
    pub const MAX_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

    /// Reads a ceiling in bytes, against what the process can actually reach.
    ///
    /// `available` is passed in rather than probed here, and that is deliberate twice over: it makes
    /// this function total and testable - the interesting case is a machine nobody has - and it keeps
    /// the one place that reads `/proc` and `/sys` separate from the one that decides. `None` means
    /// the platform would not say, and then the comparison is not made; [`available_memory_bytes`]
    /// says which platforms those are.
    ///
    /// Bytes and not a suffixed string, for the reason [`crate::server::RequestTimeout`] takes whole
    /// seconds: a parser for `1GiB` is a second grammar for a single value, and the two spellings of
    /// a gibibyte that differ by 7% are exactly the confusion it would introduce.
    ///
    /// Not a `const fn`, unlike most of its neighbours and for the same shape of reason
    /// [`EngineWorkers::parse`] is not: the width check below is a `TryFrom`, which is not yet const.
    /// An `as` cast would be const and would truncate silently on the one target where the check
    /// matters, which is the wrong direction for a bound.
    pub fn parse(bytes: u64, available: Option<u64>) -> Result<Self, InvalidBound> {
        // Zero first, because it is the value that reads as `no limit` and is the opposite - the same
        // rule every bound in this crate is held to.
        let Some(non_zero) = core::num::NonZeroU64::new(bytes) else {
            return Err(InvalidBound::Zero { name: Self::KEY });
        };
        if non_zero.get() > Self::MAX_BYTES {
            return Err(InvalidBound::TooLarge {
                name: Self::KEY,
                found: bytes,
                limit: Self::MAX_BYTES,
            });
        }
        // The type's ceiling is a tebibyte, which does not fit a 32-bit `usize`, and the pool this
        // becomes is sized in `usize`. Reported as too large rather than clamped: clamping would
        // silently install a bound of `usize::MAX`, which is the whole address space and therefore
        // no bound at all. Unreachable on the targets this ships to - all four are 64-bit - and
        // written as a branch because the wrong direction here is the one that opens the control.
        //
        // `NonZeroUsize::try_from` rather than `usize::try_from` and a second non-zero check: the
        // width conversion and the non-zero carry are one question, and asking twice would leave a
        // branch that cannot be reached and therefore cannot be tested.
        let Ok(fits) = core::num::NonZeroUsize::try_from(non_zero) else {
            return Err(InvalidBound::TooLarge {
                name: Self::KEY,
                found: bytes,
                limit: usize::MAX as u64,
            });
        };
        // Last, because it is the only check whose answer depends on where this is running. An
        // operator who wrote ten tebibytes should be told the key does not go that high, not that
        // their laptop is small.
        if let Some(limit) = available
            && non_zero.get() > limit
        {
            return Err(InvalidBound::AboveAvailableMemory {
                name: Self::KEY,
                found: bytes,
                available: limit,
            });
        }
        Ok(Self {
            bytes: fits,
            checked_against: available,
        })
    }

    /// The ceiling, for whatever builds the pool.
    #[inline]
    pub const fn bytes(self) -> core::num::NonZeroUsize {
        self.bytes
    }

    /// What this ceiling was compared against at boot, if the platform would say.
    ///
    /// For the startup log, and it is the honest half of the claim: a `None` here means nothing
    /// verified that the configured ceiling is reachable, so an over-configured deployment on such a
    /// platform starts and dies later rather than refusing now.
    #[inline]
    pub const fn checked_against(self) -> Option<u64> {
        self.checked_against
    }
}

/// How many bytes this process can actually reach, when the platform will say.
///
/// **Three sources, smallest wins**, because they answer three different questions and the binding
/// one is whichever is tightest:
///
/// 1. `/sys/fs/cgroup/memory.max` - the cgroup v2 limit, and the number that matters in a container.
/// 2. `/sys/fs/cgroup/memory/memory.limit_in_bytes` - the same under cgroup v1, where "unlimited" is
///    a sentinel near `u64::MAX` rather than a word, which is why taking the minimum with the
///    machine total is what disarms it rather than a comparison against the sentinel.
/// 3. `MemTotal` in `/proc/meminfo` - the machine, for a process with no cgroup limit.
///
/// **`None` on any platform that has none of these, and that is a limit on the claim rather than a
/// fallback.** macOS is such a platform: nothing here reads `sysctl`, so a laptop makes no boot check
/// at all and an over-configured ceiling there starts and dies later. The shipped artifacts are Linux,
/// which is where the check has to hold - and [`WorkingSetCeiling::checked_against`] is what lets the
/// startup log say which of the two happened rather than implying the check was made.
///
/// It reads files and cannot fail: an unreadable or unparsable source contributes nothing rather than
/// refusing to start, because the expensive failure here is a deployment that will not boot on a
/// kernel laid out differently, and the cheap one is a boot check that did not run and said so.
#[must_use]
pub fn available_memory_bytes() -> Option<u64> {
    let sources = [
        std::fs::read_to_string("/sys/fs/cgroup/memory.max")
            .ok()
            .as_deref()
            .and_then(cgroup_bytes),
        std::fs::read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes")
            .ok()
            .as_deref()
            .and_then(cgroup_bytes),
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .as_deref()
            .and_then(mem_total_bytes),
    ];
    sources.into_iter().flatten().min()
}

/// One cgroup memory limit file's contents, as bytes.
///
/// `max` is cgroup v2's word for "no limit" and answers `None`. So does a zero: a memory limit of
/// zero bytes is not a limit anybody configured, and treating it as one would refuse every ceiling and
/// leave a deployment unable to start for a reason nothing explains.
fn cgroup_bytes(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    if trimmed == "max" {
        return None;
    }
    trimmed.parse::<u64>().ok().filter(|bytes| *bytes > 0)
}

/// `MemTotal` out of `/proc/meminfo`, as bytes.
///
/// The file states it in kibibytes - `MemTotal:       16305236 kB` - so the number is scaled here.
/// `saturating_mul` rather than `*`, because the overflow lints are denied and a kernel reporting an
/// absurd total should produce the largest number this type can hold rather than a panic.
fn mem_total_bytes(text: &str) -> Option<u64> {
    let kibibytes: u64 = text
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kibibytes.saturating_mul(1024)).filter(|bytes| *bytes > 0)
}

impl ShutdownGrace {
    /// The ceiling, in seconds. Five minutes, matching
    /// [`RequestTimeout::MAX_SECONDS`](crate::server::RequestTimeout::MAX_SECONDS): a grace period
    /// longer than the orchestrator's own window buys nothing, and the value worth refusing is the
    /// one that is obviously out of range rather than the one that is merely optimistic.
    pub const MAX_SECONDS: u64 = 300;

    /// Reads a grace period in whole seconds.
    pub const fn parse(seconds: u64) -> Result<Self, InvalidBound> {
        if seconds == 0 {
            return Err(InvalidBound::Zero {
                name: "runtime.shutdown_grace_seconds",
            });
        }
        if seconds > Self::MAX_SECONDS {
            return Err(InvalidBound::TooLarge {
                name: "runtime.shutdown_grace_seconds",
                found: seconds,
                limit: Self::MAX_SECONDS,
            });
        }
        Ok(Self(Duration::from_secs(seconds)))
    }

    #[inline]
    pub const fn duration(self) -> Duration {
        self.0
    }

    #[inline]
    pub const fn seconds(self) -> u64 {
        self.0.as_secs()
    }
}

/// Everything about how much runs at once and how the process stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeSettings {
    max_concurrent_queries: QueryConcurrency,
    admission_timeout: AdmissionTimeout,
    engine_workers: EngineWorkers,
    working_set: WorkingSetCeiling,
    shutdown_grace: ShutdownGrace,
}

impl RuntimeSettings {
    /// Assembles the group from parts that have each already been parsed.
    ///
    /// Infallible, like [`crate::server::ServerSettings::new`] and for the same reason: there is no
    /// cross-field rule inside this group. The one relationship worth knowing - that an admission
    /// timeout above the request timeout is never reached - spans two groups and is documented on
    /// [`AdmissionTimeout::MAX_SECONDS`] rather than refused.
    #[inline]
    pub const fn new(
        max_concurrent_queries: QueryConcurrency,
        admission_timeout: AdmissionTimeout,
        engine_workers: EngineWorkers,
        working_set: WorkingSetCeiling,
        shutdown_grace: ShutdownGrace,
    ) -> Self {
        Self {
            max_concurrent_queries,
            admission_timeout,
            engine_workers,
            working_set,
            shutdown_grace,
        }
    }

    #[inline]
    pub const fn max_concurrent_queries(self) -> QueryConcurrency {
        self.max_concurrent_queries
    }

    #[inline]
    pub const fn admission_timeout(self) -> AdmissionTimeout {
        self.admission_timeout
    }

    #[inline]
    pub const fn engine_workers(self) -> EngineWorkers {
        self.engine_workers
    }

    /// How many bytes the engine's operators may reserve at once.
    #[inline]
    pub const fn working_set(self) -> WorkingSetCeiling {
        self.working_set
    }

    #[inline]
    pub const fn shutdown_grace(self) -> ShutdownGrace {
        self.shutdown_grace
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdmissionTimeout, EngineWorkers, QueryConcurrency, ShutdownGrace, WorkingSetCeiling, cgroup_bytes, mem_total_bytes,
    };
    use crate::server::InvalidBound;

    /// A ceiling parsed with no machine to check it against. For the cases that are about the key.
    fn unchecked(bytes: u64) -> Result<WorkingSetCeiling, InvalidBound> {
        WorkingSetCeiling::parse(bytes, None)
    }

    #[test]
    fn a_ceiling_of_zero_is_refused_at_parse() {
        // The same rule every bound in this crate is held to, and it bites hardest here: a zero-byte
        // pool would refuse every question, which looks exactly like a configured deployment that
        // answers nothing. `enabled: false` is how a control is turned off; a zero is not.
        assert_eq!(
            unchecked(0),
            Err(InvalidBound::Zero {
                name: "runtime.working_set_max_bytes"
            })
        );
    }

    #[test]
    fn a_ceiling_above_the_available_memory_refuses_at_boot() {
        // THE check this bound exists for. A ceiling above what the process can reach is not a
        // generous bound - it is the unbounded case with a number written next to it, and under
        // `panic = "abort"` the allocation it permits ends the process for every caller in flight.
        //
        // The machine's number is passed in rather than probed, which is what makes the interesting
        // case testable: one gibibyte configured against a container held to 512 mebibytes.
        let error = WorkingSetCeiling::parse(1024 * 1024 * 1024, Some(512 * 1024 * 1024))
            .expect_err("a ceiling above the container's memory is not a ceiling");
        assert_eq!(
            error,
            InvalidBound::AboveAvailableMemory {
                name: "runtime.working_set_max_bytes",
                found: 1024 * 1024 * 1024,
                available: 512 * 1024 * 1024,
            }
        );
        // A DISTINCT variant rather than `TooLarge` with the machine's number in it, because the two
        // send an operator to different places - a key to lower against a container to resize - and
        // a message saying only "the maximum is N" cannot tell which produced it.
        assert!(!matches!(error, InvalidBound::TooLarge { .. }));

        // The boundary, a byte either side: equal to what is available is accepted, one over is not.
        assert_eq!(
            WorkingSetCeiling::parse(512, Some(512))
                .expect("a ceiling equal to what is available is a ceiling")
                .bytes()
                .get(),
            512
        );
        assert!(matches!(
            WorkingSetCeiling::parse(513, Some(512)),
            Err(InvalidBound::AboveAvailableMemory { .. })
        ));
    }

    #[test]
    fn the_types_own_ceiling_is_reported_before_the_machines() {
        // Order matters for the diagnostic and for nothing else, so it is asserted rather than
        // described: an operator who wrote ten tebibytes should be told the key does not go that
        // high, not that their laptop is small. Both checks would fire on this value.
        let error = WorkingSetCeiling::parse(WorkingSetCeiling::MAX_BYTES.saturating_add(1), Some(1024))
            .expect_err("above the type's ceiling is not a ceiling");
        assert!(
            matches!(
                error,
                InvalidBound::TooLarge {
                    limit: WorkingSetCeiling::MAX_BYTES,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_ceiling_records_whether_anything_checked_it() {
        // The honest half of the claim, and it is a value rather than a comment because the startup
        // log has to be able to say which of the two happened. `None` means no boot check was made -
        // macOS is such a platform - so an over-configured deployment there starts and dies later.
        let checked = WorkingSetCeiling::parse(1024, Some(4096)).expect("a kibibyte under four is a ceiling");
        assert_eq!(checked.checked_against(), Some(4096));
        assert_eq!(checked.bytes().get(), 1024);

        let unverified = unchecked(1024).expect("a kibibyte is a ceiling");
        assert_eq!(unverified.checked_against(), None);
    }

    #[test]
    fn a_cgroup_file_that_says_max_or_zero_is_not_a_limit() {
        // Two spellings of "no limit", and neither may become a bound. `max` is cgroup v2's word for
        // it; a zero is not something anybody configures, and treating it as a limit would refuse
        // every ceiling and leave a deployment unable to start for a reason nothing explains.
        assert_eq!(cgroup_bytes("max\n"), None);
        assert_eq!(cgroup_bytes("0\n"), None);
        assert_eq!(cgroup_bytes("536870912\n"), Some(512 * 1024 * 1024));
        // cgroup v1's unlimited is a sentinel near `u64::MAX` rather than a word, so it parses as a
        // number - which is why the minimum against the machine total is what disarms it, and why
        // nothing here compares against the sentinel.
        assert_eq!(cgroup_bytes("9223372036854771712"), Some(0x7FFF_FFFF_FFFF_F000));
        assert_eq!(cgroup_bytes("not a number"), None);
    }

    #[test]
    fn meminfo_is_read_in_kibibytes_and_answered_in_bytes() {
        // The scaling, which is the one thing about this file that is easy to get wrong by a factor
        // of 1024 - and a ceiling compared against a number 1024 times too small refuses every
        // deployment, while one 1024 times too large checks nothing.
        let meminfo = "MemTotal:       16305236 kB\nMemFree:         1234567 kB\n";
        assert_eq!(mem_total_bytes(meminfo), Some(16_696_561_664));
        assert_eq!(mem_total_bytes("MemFree: 100 kB\n"), None);
        assert_eq!(mem_total_bytes("MemTotal:  not a number kB\n"), None);
    }

    #[test]
    fn a_zero_bound_is_refused_rather_than_read_as_unlimited() {
        // The same rule the server bounds are held to, and it matters more here: a zero
        // concurrency bound is a permit set nobody can ever enter, which is a service that refuses
        // every question and still looks configured.
        assert_eq!(
            QueryConcurrency::parse(0),
            Err(InvalidBound::Zero {
                name: "runtime.max_concurrent_queries"
            })
        );
        assert_eq!(
            AdmissionTimeout::parse(0),
            Err(InvalidBound::Zero {
                name: "runtime.admission_timeout_seconds"
            })
        );
        assert_eq!(
            EngineWorkers::parse(Some(0)),
            Err(InvalidBound::Zero {
                name: "runtime.engine_worker_threads"
            })
        );
        assert_eq!(
            ShutdownGrace::parse(0),
            Err(InvalidBound::Zero {
                name: "runtime.shutdown_grace_seconds"
            })
        );
    }

    #[test]
    fn a_bound_above_the_ceiling_is_refused_and_names_both_numbers() {
        let error = QueryConcurrency::parse(QueryConcurrency::MAX.saturating_add(1))
            .expect_err("above the ceiling is not a concurrency bound");
        let InvalidBound::TooLarge { found, limit, .. } = error else {
            panic!("above the ceiling is a too-large bound, not {error:?}");
        };
        assert_eq!((found, limit), (513, 512));
        assert!(matches!(
            AdmissionTimeout::parse(AdmissionTimeout::MAX_SECONDS.saturating_add(1)),
            Err(InvalidBound::TooLarge { .. })
        ));
        assert!(matches!(
            EngineWorkers::parse(Some(EngineWorkers::MAX.saturating_add(1))),
            Err(InvalidBound::TooLarge { .. })
        ));
        assert!(matches!(
            ShutdownGrace::parse(ShutdownGrace::MAX_SECONDS.saturating_add(1)),
            Err(InvalidBound::TooLarge { .. })
        ));
    }

    #[test]
    fn the_boundary_values_themselves_are_accepted() {
        // The other side of the assertion above, a step away, so a `>=` written where `>` belongs
        // fails one of them.
        assert_eq!(
            QueryConcurrency::parse(QueryConcurrency::MAX)
                .expect("the ceiling is a bound")
                .count(),
            QueryConcurrency::MAX
        );
        assert_eq!(QueryConcurrency::parse(1).expect("one is a bound").count(), 1);
        assert_eq!(
            AdmissionTimeout::parse(AdmissionTimeout::MAX_SECONDS)
                .expect("the ceiling is a wait")
                .seconds(),
            AdmissionTimeout::MAX_SECONDS
        );
        assert_eq!(
            ShutdownGrace::parse(ShutdownGrace::MAX_SECONDS)
                .expect("the ceiling is a grace period")
                .seconds(),
            ShutdownGrace::MAX_SECONDS
        );
    }

    #[test]
    fn an_absent_worker_count_is_resolved_from_the_machine_and_says_it_was_not_chosen() {
        // Resolved at load time rather than left as "whatever the machine has", so the number in
        // the startup log is the number in effect. `was_chosen` is what tells an operator whether
        // the number they are reading is theirs.
        let resolved = EngineWorkers::parse(None).expect("an absent count resolves");
        assert!(resolved.count() >= 1, "a machine runs at least one thread");
        assert!(!resolved.was_chosen());

        let explicit = EngineWorkers::parse(Some(3)).expect("three is a worker count");
        assert_eq!(explicit.count(), 3);
        assert!(explicit.was_chosen());
    }
}
