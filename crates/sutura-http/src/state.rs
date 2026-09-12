//! What every handler is handed.
//!
//! Each field is cheap to clone, so cloning the state per connection is a few pointer bumps: the
//! [`Surface`] the question goes to, the [`Settings`] the token gate and assembled router were built
//! from, the [`Admission`] bound the query handler takes a slot from, the metrics registry with the
//! transport handles that write it, and an optional inbound identity gate. See
//! [`ServiceState::new`] for why the registry is built here and frozen at construction.
//!
//! The settings are kept rather than read once at assembly time because the token gate needs them
//! per request. Nothing else does - the layers were all decided at startup - and that is
//! deliberate: a value a handler can read is a value a handler can branch on, and the posture
//! decisions in this service are supposed to be settled before the first request arrives.
//!
//! # The admission bound is TAKEN, and it used to be built here
//!
//! **`telekom/sutura#340`, and the sentence this section replaced is the defect.** It read *the
//! only place that can be true without a second constructor argument is beside the settings it is
//! derived from*, and it was wrong in the way that matters: `Admission::from_settings` inside
//! [`ServiceState::new`] made a second `ServiceState` a second permit set, and a process serving
//! this transport beside another would have held two semaphores each reporting a limit the other
//! can exceed. `sutura_runtime::admission`'s own module documentation calls that shape not-a-bound
//! and says the composition root builds one - and nothing held it.
//!
//! So the bound arrives as an argument. Three consequences worth naming, because the argument for
//! deriving it was that a caller can forget a bound:
//!
//! * **It cannot be forgotten**: the parameter has no default and no `Option`, so a state built
//!   without one does not compile - the same shape `sutura_app::surface::LocalService::start`
//!   gives its audit sink.
//! * **It can be SHARED**: one `Admission` handed to two states is one permit set, which is what
//!   makes the number a bound on the process rather than on a router.
//! * **A composition root builds exactly one**, held by `cargo xtask check-one-bound` in
//!   `just hygiene` rather than by this comment. That gate would fail this crate for building one
//!   at all.
//!
//! `Clone` on this type shares that bound rather than duplicating it, because the field is an
//! `Admission` whose own `Clone` shares one permit set. That is the property the whole control
//! rests on, and it is the one a future field here must not break.

use std::sync::Arc;

use sutura_config::Settings;
use sutura_runtime::{Admission, Registry, RegistryBuilder};

use crate::surface::Surface;

/// The request state.
#[derive(Clone)]
pub struct ServiceState {
    surface: Arc<dyn Surface>,
    settings: Arc<Settings>,
    admission: Admission,
    /// This state's metrics registry and the transport handles that write it.
    ///
    /// Built once per state by [`ServiceState::new`], which is the one place that has both the
    /// settings and the served bundle; the `/metrics` route renders it and every handler observes
    /// through the handles. The series set is frozen at construction, so a scrape reads atomics and
    /// fixed strings and takes no lock.
    registry: Arc<Registry>,
    metrics: crate::metrics::Metrics,
    ///
    /// **Attached by a builder rather than taken by [`ServiceState::new`]**, and the reason is that
    /// building it reads a file: a `new` that could not fail would have to swallow an unreadable key
    /// set or read it lazily on the first request, and both turn a refusal to start into a deployment
    /// that authenticates nobody. What keeps the builder from being forgettable is not discipline -
    /// `crate::router::assemble` refuses to build a router whose settings declare an inbound identity
    /// and whose state carries no gate.
    inbound: Option<Arc<crate::inbound::InboundGate>>,
}

impl ServiceState {
    /// Builds the state from a started service and the settings it was started under.
    ///
    /// Takes the surface already behind an `Arc`, because the composition root owns it: the same
    /// service may be handed to a second transport later, and this crate must not be the one that
    /// decides there is only ever one.
    ///
    /// **The admission bound is taken and not derived, which is `telekom/sutura#340`.** It is the
    /// same argument as the surface one line above it, one bound further: the permit set belongs to
    /// the process, so the only component that may decide there is one of it is the composition
    /// root. See the module documentation for what deriving it cost.
    ///
    /// **The registry is built here, and it is the one place that can see both halves of it.** The
    /// transport series come from [`crate::metrics::Metrics::install`]; the two deployment-wide
    /// numbers - the engine width and the catalog's governed coverage - come from the settings this
    /// state serves under and the pinned bundle its surface already holds. Registering them together
    /// is what makes `/metrics` show the deployment rather than only its requests, and the builder is
    /// consumed here so no later caller can add a series to a registry a scrape is already reading.
    #[must_use]
    pub fn new(surface: Arc<dyn Surface>, settings: Arc<Settings>, admission: Admission) -> Self {
        let mut builder = RegistryBuilder::default();
        let metrics = crate::metrics::Metrics::install(&mut builder);
        // The capacity denominator is a property of this state's bound, not of a question, so it is
        // reported once here rather than on the request path. `sutura_execution_slots_in_use` is the
        // number that moves; this is the number an operator divides it by.
        metrics.set_capacity(admission.bound());
        // `sutura_engine_worker_threads` is the width the engine was opened with - the same value
        // `sutura_serve::open_engine` passed to `with_worker_threads`, read from the settings rather
        // than guessed, because under a CPU quota the machine's own answer is the wrong one.
        let workers = builder.gauge("sutura_engine_worker_threads");
        workers.set(settings.runtime().engine_workers().count() as u64);
        // `sutura_catalog_metrics` is the governed coverage the SERVED bundle carries. Read once
        // from the pinned bundle the surface already holds - no load, no I/O, and the number a
        // coverage ramp is measured against.
        let coverage = builder.gauge("sutura_catalog_metrics");
        coverage.set(surface.definitions().definitions().metrics().len() as u64);
        let registry = Arc::new(builder.build());
        Self {
            surface,
            settings,
            admission,
            registry,
            metrics,
            inbound: None,
        }
    }

    /// The same state, with leg 1 attached.
    ///
    /// Called by the composition root, after it has read the key set the declaration names. A state
    /// whose settings declare an inbound identity and which has not been through this is a state
    /// `crate::router::assemble` refuses.
    #[must_use]
    pub fn with_inbound_identity(mut self, gate: Arc<crate::inbound::InboundGate>) -> Self {
        self.inbound = Some(gate);
        self
    }

    /// Leg 1, if this deployment has it.
    ///
    /// Read by `crate::router` to install the layer, and by nothing else - a handler must not be able
    /// to reach the validator, which is why the middleware takes the gate as its own state rather than
    /// reading it back out of this one.
    #[inline]
    #[must_use]
    pub const fn inbound_identity(&self) -> Option<&Arc<crate::inbound::InboundGate>> {
        self.inbound.as_ref()
    }

    /// The service, for a handler that is about to move the call onto the blocking pool.
    #[must_use]
    pub fn surface(&self) -> Arc<dyn Surface> {
        Arc::clone(&self.surface)
    }

    /// The service, borrowed, for a handler that only reads the pinned bundle.
    #[must_use]
    pub fn definitions(&self) -> &sutura_domain::pinned::PinnedDefinitions {
        self.surface.definitions()
    }

    #[inline]
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// This state's metrics registry, shared with its `/metrics` route.
    ///
    /// Handlers observe question outcomes through it; the metrics route renders it. Neither can
    /// mutate the series set at request time. The shipped composition root builds one service state,
    /// but this type does not enforce process-wide ownership: separately constructed states have
    /// separate registries.
    #[inline]
    #[must_use]
    pub fn registry(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }
    /// The transport's metrics observer: records question outcomes, admission and rate-limit
    /// events against the shared registry.
    #[inline]
    #[must_use]
    pub const fn metrics(&self) -> &crate::metrics::Metrics {
        &self.metrics
    }

    /// The bound on how many questions execute at once.
    #[inline]
    #[must_use]
    pub const fn admission(&self) -> &Admission {
        &self.admission
    }
}

impl core::fmt::Debug for ServiceState {
    /// Hand-written because a `Surface` is not `Debug` and because the settings are printed once at
    /// startup rather than on every line that happens to include the state.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServiceState")
            .field("environment", &self.settings.environment())
            // The bound and how much of it is free, because those are the two numbers worth having
            // on a line that happened to include the state.
            .field("max_concurrent_queries", &self.admission.bound())
            .field("slots_free", &self.admission.free())
            .finish_non_exhaustive()
    }
}
