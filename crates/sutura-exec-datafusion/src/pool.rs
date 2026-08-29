//! The working-set ceiling.
//!
//! The pool the engine's operators reserve against, and how a refused reservation is recognised on
//! the way back out.
//!
//! # Why this file exists at all
//!
//! **There was no memory pool.** Nothing in the workspace constructed a `RuntimeEnv`, so
//! `DataFusion` installed its `UnboundedMemoryPool` at both `SessionContext` construction sites -
//! and shipped profiles compile `panic = "abort"`, so a hash join or an aggregate wide enough to
//! outgrow the machine was not an error for the caller who asked. It was the process ending for every
//! caller in flight. A bounded pool turns that into a reservation that fails, which
//! `sutura_app::answer` turns into `RefusalReason::ResourcesExhausted`.
//!
//! # What the pool counts, and what it does not
//!
//! It counts what the engine's own operators reserve: a hash-join build side, aggregate state, a
//! sort. **It counts nothing else.** Not what a driver buffers, not `collect()` materialising every
//! batch into memory at once, not the `Vec<Vec<Value>>` built while a result is converted into domain
//! rows - all three of which are on the path a question takes through this crate. So this is not a
//! bound on the process's memory and must not be alerted on as one: a question large enough to end
//! the process on one of those paths still ends it. `docs/adr/0009` puts the bound that reaches them
//! - a byte budget applied as rows are converted - with the execution boundary rather than here, and
//! says so rather than letting this one be read as wider than it is.
//!
//! # Greedy, and never spilling
//!
//! [`GreedyMemoryPool`] rather than `FairSpillPool`: first come, first served, and a reservation over
//! the ceiling fails immediately. `docs/adr/0009` Decision 3 decides the policy and the second of its
//! two reasons is what settles it - spilling writes the **asking subject's rows** to the pod's local
//! disk, a data-at-rest surface nothing in this design governs, on the one path whose whole purpose
//! is that a query executes as the person who asked. A bound that protects memory by making an
//! ungoverned copy of the data has not protected anything.
//!
//! So temporary files are **disabled** rather than left at the engine's default of an OS temporary
//! directory. That is belt and braces on purpose: the pool alone would still let a spilling operator
//! react to a refused reservation by writing, and `DiskManagerMode::Disabled` is what makes there be
//! nowhere to write to. No spill directory, no disk sizing, and a refusal that does not depend on
//! disk state.
//!
//! Not wrapped in `TrackConsumersPool` either, though it would improve the engine's own message: what
//! reaches a caller is [`sutura_domain::query::RefusalReason::ResourcesExhausted`],
//! which carries the configured ceiling and deliberately nothing about what the question demanded.

use std::sync::Arc;

use datafusion::execution::disk_manager::{DiskManagerBuilder, DiskManagerMode};
use datafusion::execution::memory_pool::{GreedyMemoryPool, MemoryPool};
use datafusion::execution::runtime_env::{RuntimeEnv, RuntimeEnvBuilder};

use crate::DataFusionError;

/// How many bytes the engine's operators may reserve at once.
///
/// **A newtype for the unit rather than for a range**, and that is the whole of its job:
/// [`DataFusionWarehouse::with_worker_threads`](crate::DataFusionWarehouse::with_worker_threads)
/// already takes a `NonZeroUsize` for a thread count, so a second bare `NonZeroUsize` beside it would
/// be two arguments of one type whose meanings are a width and a quantity of memory. Swapping them
/// compiles and installs a three-byte pool. Wrapped, the swap does not build.
///
/// It parses nothing beyond non-zero, which the inner type already carries - the range that matters is
/// parsed once, in `sutura_config::WorkingSetCeiling`, against the memory the process can actually
/// reach. This crate does not depend on that one and must not: an adapter does not call another
/// adapter, so the composition root converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkingSet(core::num::NonZeroUsize);

impl WorkingSet {
    /// The ceiling, in bytes.
    ///
    /// The one constructor, named for the unit so a call site reads as bytes at the point of the call
    /// rather than at the declaration it came from.
    #[inline]
    #[must_use]
    pub const fn of_bytes(bytes: core::num::NonZeroUsize) -> Self {
        Self(bytes)
    }

    /// The ceiling, for whatever sizes the pool.
    #[inline]
    #[must_use]
    pub const fn bytes(self) -> usize {
        self.0.get()
    }
}

/// What [`environment`] hands back: the environment a session is built with, and the pool inside it.
///
/// Named rather than written out at the signature, which is the trade `clippy::type_complexity` asks
/// for and the better half of it here: the tuple is two `Arc`s of unrelated things, and a reader
/// needs the words more than the shapes.
type Bounded = (Arc<RuntimeEnv>, Arc<dyn MemoryPool>);

/// The execution environment one session runs in: a bounded pool, and nowhere to spill.
///
/// Returned as an `Arc` because that is what a `SessionContext` takes, and the pool inside it is
/// handed back separately so the adapter can retain it - `RuntimeEnv::memory_pool` is a public field
/// and reading it back would be a second path to the same value.
pub(crate) fn environment(working_set: WorkingSet) -> Result<Bounded, DataFusionError> {
    let pool: Arc<dyn MemoryPool> = Arc::new(GreedyMemoryPool::new(working_set.bytes()));
    let environment = RuntimeEnvBuilder::new()
        .with_memory_pool(Arc::clone(&pool))
        // The other half of "never spill". Without it the engine keeps its default of an OS temporary
        // directory, and a spilling operator answers a refused reservation by writing the asking
        // subject's rows to the pod's local disk.
        .with_disk_manager_builder(DiskManagerBuilder::default().with_mode(DiskManagerMode::Disabled))
        .build_arc()
        .map_err(|cause| DataFusionError::Environment { cause })?;
    Ok((environment, pool))
}

/// Was this the pool refusing a reservation?
///
/// **`find_root` and not a match on the outermost variant**, because a reservation is refused deep
/// inside an operator and the error arrives wrapped: the engine adds `Context`, and a plan executed
/// across partitions can hand it back as `External` or `Shared`. Matching the outermost variant would
/// classify almost every real exhaustion as something else, and the direction that mistake falls in
/// is the one that reports a governance refusal as a transport failure.
///
/// One variant and no string matching. The engine has a `ResourcesExhausted` variant and the pool
/// raises exactly it, so nothing here reads a message - a text test would be this function depending
/// on wording the engine is free to change, and it would also match a message that merely mentions
/// memory.
pub(crate) fn exhausted(error: &datafusion::error::DataFusionError) -> bool {
    matches!(error.find_root(), datafusion::error::DataFusionError::ResourcesExhausted(_))
}

/// Was this adapter's failure the pool refusing a reservation?
///
/// **Exhaustive, with no `_` arm.** A variant added to [`DataFusionError`] would otherwise be
/// classified silently, and the direction that mistake falls in is the one that tells a caller not to
/// retry something a retry would have answered.
///
/// [`DataFusionError::Analyze`] is deliberately in the `false` group even though it wraps the same
/// engine error type: it is raised where a plan is resolved, before any operator has reserved
/// anything, so an exhaustion arriving through it would be an engine defect rather than this bound
/// biting. [`DataFusionError::Environment`] likewise - that is the pool failing to be *built*, which
/// is a deployment that never started rather than a question that was refused.
pub(crate) fn refused_a_reservation(error: &DataFusionError) -> bool {
    match *error {
        DataFusionError::Execute { ref cause } => exhausted(cause),
        DataFusionError::Runtime { .. }
        | DataFusionError::Environment { .. }
        | DataFusionError::Attach { .. }
        | DataFusionError::Build { .. }
        | DataFusionError::Analyze { .. }
        | DataFusionError::UnsupportedType { .. }
        | DataFusionError::Downcast { .. }
        | DataFusionError::NotFinite { .. }
        | DataFusionError::NotADate { .. }
        | DataFusionError::Shape { .. }
        | DataFusionError::SchemaMismatch { .. }
        | DataFusionError::MissingParam { .. }
        | DataFusionError::NoPredicate
        // A leg handed to an adapter with nothing above it is a composition fault, not a ceiling
        // refusing a reservation: no operator ran, so no reservation was made.
        | DataFusionError::LegWithoutCombiner { .. }
        // The same shape one step earlier: credential material this adapter cannot use is refused
        // before anything is planned, so nothing reserved anything.
        | DataFusionError::NoPlaceForASubject { .. } => false,
    }
}

/// The bound, biting on a real operator rather than on the pool alone. Its own file for the reason
/// `width_tests.rs` is: `lib.rs` is at the length gate and needs the room more than this does.
#[cfg(test)]
mod ceiling_tests;

#[cfg(test)]
mod tests {
    use super::{WorkingSet, environment, exhausted};
    use datafusion::error::DataFusionError as EngineError;
    use datafusion::execution::memory_pool::{MemoryConsumer, MemoryLimit};

    fn bytes(count: usize) -> WorkingSet {
        WorkingSet::of_bytes(core::num::NonZeroUsize::new(count).expect("a test ceiling is positive"))
    }

    #[test]
    fn a_reservation_over_the_ceiling_is_refused_and_one_under_it_is_not() {
        // The pool itself, at the boundary, without an engine in the way. What the adapter's own
        // suite asserts is that a real operator reaches this; what this asserts is that the number
        // configured is the number that bites, one byte either side of it.
        let (runtime, pool) = environment(bytes(1024)).expect("a bounded environment builds");
        // `MemoryLimit` implements neither `PartialEq` nor `Debug`, so this is a match rather than an
        // equality - and it is asserted at all because the alternative pool answers `Unknown` here:
        // a `Finite` reading is what says a bound was installed rather than defaulted away.
        assert!(
            matches!(pool.memory_limit(), MemoryLimit::Finite(1024)),
            "the pool reports no finite ceiling, so nothing was bounded"
        );

        let held = MemoryConsumer::new("a test operator").register(&runtime.memory_pool);
        held.try_grow(1024).expect("the ceiling itself is reservable");
        assert_eq!(pool.reserved(), 1024);
        let refused = held.try_grow(1).expect_err("one byte past the ceiling is not reservable");
        assert!(matches!(refused, EngineError::ResourcesExhausted(_)), "{refused:?}");
        // And the refusal did not take the reservation with it: the pool still holds what was
        // granted, so a caller whose question was refused has not disturbed one that was not.
        assert_eq!(pool.reserved(), 1024);
    }

    #[test]
    fn there_is_nowhere_to_spill() {
        // The policy, asserted rather than described. `DiskManagerMode::Disabled` is what makes the
        // never-spill decision a mechanism: with the engine's default an operator answers a refused
        // reservation by writing the asking subject's rows to the pod's local disk, which is the
        // ungoverned data-at-rest surface 0009 refuses.
        let (runtime, _) = environment(bytes(1024)).expect("a bounded environment builds");
        assert!(
            !runtime.disk_manager.tmp_files_enabled(),
            "a spill directory exists, so a refused reservation can write the caller's rows to disk"
        );
    }

    #[test]
    fn exhaustion_is_recognised_through_the_wrapping_the_engine_adds() {
        // The classification, and the case that makes `find_root` necessary rather than tidy: a
        // reservation is refused inside an operator and the error comes back wrapped. Matching the
        // outermost variant would report a governance refusal as a transport failure, which is the
        // defect this whole step exists to fix - so the wrapped forms are asserted, not just the
        // bare one.
        let bare = EngineError::ResourcesExhausted(String::from("a pool refused"));
        assert!(exhausted(&bare));
        assert!(exhausted(
            &EngineError::ResourcesExhausted(String::from("x")).context("while joining")
        ));
        assert!(exhausted(&EngineError::External(Box::new(EngineError::ResourcesExhausted(
            String::from("x")
        )))));

        // And the other direction, which is what stops this from classifying everything: a plan the
        // engine would not build is not the ceiling refusing anything, and a caller told not to
        // retry it would be told the wrong thing.
        assert!(!exhausted(&EngineError::Execution(String::from("something else"))));
        assert!(!exhausted(&EngineError::Plan(String::from(
            "a message that mentions memory and is not an exhaustion"
        ))));
    }
}
