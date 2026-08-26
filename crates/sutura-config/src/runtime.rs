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
        shutdown_grace: ShutdownGrace,
    ) -> Self {
        Self {
            max_concurrent_queries,
            admission_timeout,
            engine_workers,
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

    #[inline]
    pub const fn shutdown_grace(self) -> ShutdownGrace {
        self.shutdown_grace
    }
}

#[cfg(test)]
mod tests {
    use super::{AdmissionTimeout, EngineWorkers, QueryConcurrency, ShutdownGrace};
    use crate::server::InvalidBound;

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
