//! What every handler is handed.
//!
//! Two things behind an `Arc` each, so cloning the state per connection is two pointer bumps: the
//! [`Surface`] the question goes to, and the [`Settings`] the token gate and the assembled router
//! were built from.
//!
//! The settings are kept rather than read once at assembly time because the token gate needs them
//! per request. Nothing else does - the layers were all decided at startup - and that is
//! deliberate: a value a handler can read is a value a handler can branch on, and the posture
//! decisions in this service are supposed to be settled before the first request arrives.

use std::sync::Arc;

use sutura_config::Settings;

use crate::surface::Surface;

/// The request state.
#[derive(Clone)]
pub struct ServiceState {
    surface: Arc<dyn Surface>,
    settings: Arc<Settings>,
}

impl ServiceState {
    /// Builds the state from a started service and the settings it was started under.
    ///
    /// Takes the surface already behind an `Arc`, because the composition root owns it: the same
    /// service may be handed to a second transport later, and this crate must not be the one that
    /// decides there is only ever one.
    #[must_use]
    pub fn new(surface: Arc<dyn Surface>, settings: Arc<Settings>) -> Self {
        Self { surface, settings }
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
}

impl core::fmt::Debug for ServiceState {
    /// Hand-written because a `Surface` is not `Debug` and because the settings are printed once at
    /// startup rather than on every line that happens to include the state.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServiceState")
            .field("environment", &self.settings.environment())
            .finish_non_exhaustive()
    }
}
