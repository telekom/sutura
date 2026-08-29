//! What every handler is handed.
//!
//! Three things, each cheap to clone, so cloning the state per connection is a few pointer bumps:
//! the [`Surface`] the question goes to, the [`Settings`] the token gate and the assembled router
//! were built from, and the [`Admission`] bound the query handler takes a slot from.
//!
//! The settings are kept rather than read once at assembly time because the token gate needs them
//! per request. Nothing else does - the layers were all decided at startup - and that is
//! deliberate: a value a handler can read is a value a handler can branch on, and the posture
//! decisions in this service are supposed to be settled before the first request arrives.
//!
//! # The admission bound is built here, from the settings this state was given
//!
//! And that is the whole of why nothing else had to change to install it. `Admission` is a bound,
//! so it has to be *one* value shared by every request - a per-request copy would read like a limit
//! and bound nothing - and the only place that can be true without a second constructor argument is
//! beside the settings it is derived from. [`ServiceState::new`] therefore takes exactly what it
//! took before, and every caller of it, production and test alike, gets the configured bound rather
//! than having to remember to pass one.
//!
//! `Clone` on this type shares that bound rather than duplicating it, because the field is an
//! `Admission` whose own `Clone` shares one permit set. That is the property the whole control
//! rests on, and it is the one a future field here must not break.

use std::sync::Arc;

use sutura_config::Settings;
use sutura_runtime::Admission;

use crate::surface::Surface;

/// The request state.
#[derive(Clone)]
pub struct ServiceState {
    surface: Arc<dyn Surface>,
    settings: Arc<Settings>,
    admission: Admission,
    /// Leg 1, when a deployment declares one. `None` is the shape that ships today.
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
    /// The admission bound is derived from the settings rather than passed in beside them. See the
    /// module documentation: a bound that a caller supplies is a bound a caller can forget.
    #[must_use]
    pub fn new(surface: Arc<dyn Surface>, settings: Arc<Settings>) -> Self {
        let admission = Admission::from_settings(settings.runtime());
        Self {
            surface,
            settings,
            admission,
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

    /// The bound on how many questions execute at once.
    ///
    /// Borrowed rather than cloned, so a handler takes a slot from *this* bound. A clone would be
    /// correct too - `Admission` shares its permit set - and a borrow says so at the call site.
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
