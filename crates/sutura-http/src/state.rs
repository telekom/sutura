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
use sutura_runtime::{Admission, Gauge, Registry, RegistryBuilder};

#[cfg(feature = "agent")]
use crate::router::Ungoverned;
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
    /// `sutura_spend_headroom_bytes` - registered only when this deployment's surface reports a
    /// headroom value at all, i.e. only when `governance.per_replica_spend_ceiling` is configured.
    ///
    /// **`None` here means the series does not exist**, not that it reads zero - the same "absent
    /// rather than zero" discipline `docs/adr/0015` already applies to the engine memory pool
    /// series: a spend ceiling nobody configured is unlimited, and a zero-forever gauge would read
    /// as a deployment permanently one byte from refusing everything. Updated from the `POST
    /// /v1/query` route after every call it answers, never from `/metrics` itself - Decision 1 of
    /// that record is why the scrape handler's own state carries no [`crate::surface::Surface`] to
    /// poll.
    ///
    /// **Pushed from the served agent surface via the mount the composition root attaches.**
    /// `sutura-mcp` answers through the same `Surface::answer` and charges the same ledger, but it
    /// carries no dependency on this crate - and must not, since a transport does not link another
    /// transport - so nothing on that path can reach this field directly. `docs/adr/0015`'s
    /// amendment records that constraint as the reason the push had to be done this way: the
    /// composition root (`sutura-cli/src/serve::agent_mount`) hands a handle to this gauge across
    /// that boundary into the `Serving` wrapper it builds around the agent transport, so both
    /// surfaces drive one `sutura_spend_headroom_bytes` series rather than two that disagree.
    ///
    /// **The handoff is required by a type, not by the composition root remembering it.** The only
    /// route to the handle is [`SpendHeadroomPush::of`], and [`AgentMount::new`] cannot be called
    /// without a [`SpendHeadroomPush`] - so a mount attached with no handle is a mount whose caller
    /// wrote [`SpendHeadroomPush::NoCeilingConfigured`] on purpose, and
    /// `crate::router::agent_subtree` refuses to assemble that against a state which registered
    /// the series.
    spend_headroom: Option<Gauge>,
    ///
    /// **Attached by a builder rather than taken by [`ServiceState::new`]**, and the reason is that
    /// building it reads a file: a `new` that could not fail would have to swallow an unreadable key
    /// set or read it lazily on the first request, and both turn a refusal to start into a deployment
    /// that authenticates nobody. What keeps the builder from being forgettable is not discipline -
    /// `crate::router::assemble` refuses to build a router whose settings declare an inbound identity
    /// and whose state carries no gate.
    inbound: Option<Arc<crate::inbound::InboundGate>>,
    ///
    /// **The mounted agent transport, attached by a builder like [`ServiceState::with_inbound_identity`].**
    /// Only present when the composition root both compiled the `agent` feature and read
    /// `server.agent_surface.enabled: true`. It is carried OPAQUELY - this crate must not name
    /// `sutura_mcp`'s types (a transport never links another transport), so the transport arrives
    /// already boxed into [`AgentMount`] and this crate only has to nest it behind the same
    /// `establish_asked`/`inbound_layered` layers the versioned surface runs behind. `crate::router`
    /// refuses to assemble when this is `Some` and no `security.inbound` gateway was attached.
    #[cfg(feature = "agent")]
    agent: Option<AgentMount>,
}

/// The mounted agent transport, boxed so `sutura-http` can hold and nest it without naming the
/// `sutura-mcp` type a transport crate composes.
///
/// Built by the composition root from `sutura_mcp::http::service`. It is kept as an [`Ungoverned`]
/// value rather than a bare `axum::Router` (the transport nested at the router's own root, `Router`
/// rather than tower's `BoxCloneService` for the reason [`Self::new`] states) so the path
/// [`Ungoverned::mount`] was given travels with the router end to end - this type never unfuses the
/// two, so `crate::router::agent_subtree` cannot re-record the mount under a different literal path
/// than the one the transport actually answers on.
#[cfg(feature = "agent")]
#[derive(Clone)]
pub struct AgentMount {
    mount: Ungoverned,
    /// Which `sutura_spend_headroom_bytes` series the mounted transport pushes onto.
    ///
    /// Taken by [`Self::new`] and read by `crate::router::agent_subtree`, which refuses to
    /// assemble a mount whose declaration disagrees with the state it is attached to. The
    /// transport itself never reads this - the push happens in the composition root's own wrapper
    /// around the service, one crate out - so what rides here is the DECLARATION, and its whole job
    /// is to be checked against [`ServiceState`]'s own registration.
    spend: SpendHeadroomPush,
}

/// Where the served agent surface pushes this replica's spend headroom.
///
/// **A two-variant declaration rather than an `Option<Gauge>`, because at a call site an `Option`
/// makes forgetting and deciding look identical.** The same shape
/// `sutura_domain::source::ImpersonationCapability` uses for the same reason: the absence is a
/// variant with a name, so a deployment that genuinely has no ceiling says so and a caller that
/// simply did not think about it cannot compile.
///
/// **And the gauge is the state's own, held by the type rather than by the caller's care.**
/// [`Self::ThisReplicasGauge`] carries a [`ReplicaSpendGauge`], whose only constructor is
/// [`Self::of`] - so "this is the series `POST /v1/query` writes" is what an instance means, not
/// merely "someone chose a gauge". [`Self::gauge`] hands an `Option` back out, which is not the
/// `Option` this type replaces: the push site must branch, and that is the one place that should.
#[cfg(feature = "agent")]
#[derive(Debug, Clone)]
pub enum SpendHeadroomPush {
    /// The `sutura_spend_headroom_bytes` gauge [`ServiceState::new`] registered for this replica.
    ThisReplicasGauge(ReplicaSpendGauge),
    /// There is no such series, because `governance.per_replica_spend_ceiling` is not configured.
    ///
    /// Saying so explicitly is the point of the declaration - an unconfigured ceiling is unlimited
    /// rather than zero (see [`ServiceState`]'s `spend_headroom` field), so "nobody pushed
    /// anything" and "there is nothing to push" have to be different values or the second one is
    /// indistinguishable from the first.
    NoCeilingConfigured,
}

/// This replica's `sutura_spend_headroom_bytes` gauge, obtainable only from the [`ServiceState`]
/// that registered it.
///
/// **Only [`SpendHeadroomPush::of`] can make one, which is the whole point of the type existing.**
/// A [`Gauge`] cannot be constructed outside `sutura_runtime`'s registry, but any caller holding a
/// `RegistryBuilder` can mint an unrelated one - and a declaration carrying that would typecheck
/// while pushing onto a series no scrape of this deployment renders. A private field closes it. The
/// fence names the error code (`E0423`, a tuple struct with private fields) rather than a bare
/// `compile_fail`, so a change that made this fail for the WRONG reason - a rename, an unrelated
/// syntax error - would itself fail to compile:
///
/// ```compile_fail,E0423
/// fn _unrelated(gauge: sutura_runtime::Gauge) -> sutura_http::ReplicaSpendGauge {
///     sutura_http::ReplicaSpendGauge(gauge)
/// }
/// ```
///
/// The compiling twin, so the failure above is the privacy error it claims to be and not an
/// unresolved path: the same path, in the same crate, named rather than constructed.
///
/// ```
/// fn _reachable(_: sutura_http::ReplicaSpendGauge) {}
/// ```
///
/// The limit: this holds that the gauge came from *a* [`ServiceState`], not from the one the mount
/// is attached to. Two states in one process could cross their gauges, and what catches that is the
/// assembly refusal `crate::router::agent_subtree` raises on a declaration whose presence
/// disagrees with the attaching state's own registration - a presence check, not gauge identity.
#[cfg(feature = "agent")]
#[derive(Debug, Clone)]
pub struct ReplicaSpendGauge(Gauge);

#[cfg(feature = "agent")]
impl SpendHeadroomPush {
    /// This state's own declaration, whichever of the two it is.
    ///
    /// The only route to [`Self::ThisReplicasGauge`]. `Gauge` shares its storage by `Arc`, so the
    /// state and every holder of the returned declaration observe one series.
    #[must_use]
    pub fn of(state: &ServiceState) -> Self {
        state.spend_headroom.as_ref().map_or(Self::NoCeilingConfigured, |gauge| {
            Self::ThisReplicasGauge(ReplicaSpendGauge(gauge.clone()))
        })
    }

    /// The gauge to push onto, or `None` where this deployment has no ceiling at all.
    ///
    /// For the push site, which has to branch: a `None` reading leaves the gauge untouched rather
    /// than fabricating zero, and where there is no gauge there is nothing to leave untouched.
    #[inline]
    #[must_use]
    pub const fn gauge(&self) -> Option<&Gauge> {
        match self {
            Self::ThisReplicasGauge(ReplicaSpendGauge(gauge)) => Some(gauge),
            Self::NoCeilingConfigured => None,
        }
    }
}

#[cfg(feature = "agent")]
impl AgentMount {
    /// Wraps any service `nest_service` can mount, so `sutura-http` never names its concrete type.
    ///
    /// **`spend` is required, and that is the mechanism rather than a parameter.** The mounted
    /// transport answers through the same `Surface` and charges the same ledger as `POST /v1/query`,
    /// so a mount attached with no handle to this replica's `sutura_spend_headroom_bytes` gauge
    /// leaves that series frozen while the ledger drains. [`ServiceState::with_agent_surface`]
    /// cannot ask for it - it takes a mount that is already built - so the requirement sits here,
    /// where the mount is made, and rides inside it from there. Build it with
    /// [`SpendHeadroomPush::of`]; [`SpendHeadroomPush::NoCeilingConfigured`] is the deployment
    /// declaring it genuinely has no ceiling, and `crate::router::agent_subtree` refuses to
    /// assemble that against a state which registered the series.
    ///
    /// The transport is nested at this crate's own `AGENT_MOUNT_PATH` (this builder lives in
    /// `sutura-http`, so it may name it) - `axum::Router::nest_service` panics on the root path and
    /// requires `T::Response: IntoResponse` rather than `Response<Body>`, and the
    /// `StreamableHttpService` a transport crate hands over yields `Response<BoxBody<…>>`, so the
    /// wrapper must not pin the response body. `crate::router` applies the leg 1 and `establish_asked`
    /// layers around this router with [`Ungoverned::layered`]/[`Ungoverned::try_layered`], then
    /// `assemble` merges and records it in one call. Cloning the router shares one underlying
    /// transport the way `sutura_mcp`'s own `StreamableHttpService::clone` does.
    #[must_use]
    pub fn new<S>(service: S, spend: SpendHeadroomPush) -> Self
    where
        S: tower::Service<axum::http::Request<axum::body::Body>, Error = std::convert::Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Response: axum::response::IntoResponse,
        S::Future: Send + 'static,
    {
        // The one physical `nest_service` in this crate or `sutura-cli` lives inside
        // `Ungoverned::mount`, which is what makes an ungoverned mount and its allowlist row one
        // value (`xtask::boundaries::ungoverned` holds that it is the only call site). Kept as the
        // `Ungoverned` value itself, not unfused into a bare `Router` here - see the struct doc.
        Self {
            mount: Ungoverned::mount(crate::constants::AGENT_MOUNT_PATH, service),
            spend,
        }
    }

    /// What this mount declared about the spend-headroom series, for the assembly check.
    ///
    /// `pub(crate)`: only `crate::router::agent_subtree` reads it, to refuse a declaration that
    /// disagrees with the state the mount is being attached to.
    #[inline]
    pub(crate) const fn spend(&self) -> &SpendHeadroomPush {
        &self.spend
    }

    /// A clone of the fused mount, for `crate::router::agent_subtree` to layer and merge into the
    /// assembly.
    ///
    /// Hands back the whole [`Ungoverned`] value rather than its router, so a caller can only
    /// transform it via [`Ungoverned::layered`]/[`Ungoverned::try_layered`] (which carry `path`
    /// forward untouched) or extract it via [`Ungoverned::merge_into`] (which merges and records in
    /// one call) - there is no accessor here that hands back a bare, re-fusable `Router`.
    /// `axum::Router` clones share one underlying transport, so nesting several routers over one
    /// mount are one mounted transport - the same property `sutura_mcp::http::service`'s own clone
    /// carries.
    pub(crate) fn ungoverned(&self) -> Ungoverned {
        self.mount.clone()
    }
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
        // Registered only when the surface reports SOME headroom right now - which it does
        // exactly when a ceiling is configured, since an unconfigured ledger's own accessor
        // returns `None` unconditionally. A fresh ledger's initial reading is its own ceiling, so
        // this is also the correct first sample rather than a placeholder zero.
        let spend_headroom = surface.spend_headroom_bytes().map(|initial| {
            let gauge = builder.gauge("sutura_spend_headroom_bytes");
            gauge.set(initial);
            gauge
        });
        let registry = Arc::new(builder.build());
        Self {
            surface,
            settings,
            admission,
            registry,
            metrics,
            spend_headroom,
            inbound: None,
            #[cfg(feature = "agent")]
            agent: None,
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

    /// The same state, with the agent surface's transport attached.
    ///
    /// Called by the composition root, under the `agent` feature, when the deployment set
    /// `server.agent_surface.enabled: true`. A state carrying a mount but no inbound identity is a
    /// state `crate::router::assemble` refuses (`AgentSurfaceWithoutInboundIdentity`): the agent
    /// surface must never be reachable where no caller can be verified.
    #[cfg(feature = "agent")]
    #[must_use]
    pub fn with_agent_surface(mut self, mount: AgentMount) -> Self {
        self.agent = Some(mount);
        self
    }

    /// The mounted agent transport, if this build and deployment carry one.
    ///
    /// Read by `crate::router` to nest it behind the same `establish_asked`/`inbound_layered`
    /// layers the versioned surface runs behind, and to refuse assembly when it is present with no
    /// inbound identity attached. Nothing else may reach the transport.
    #[cfg(feature = "agent")]
    #[inline]
    #[must_use]
    pub const fn agent_surface(&self) -> Option<&AgentMount> {
        self.agent.as_ref()
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

    /// Whether this state registered `sutura_spend_headroom_bytes` at all.
    ///
    /// `pub(crate)`: `crate::router::agent_subtree` compares it against what an [`AgentMount`]
    /// declared, so a mount claiming there is no ceiling cannot be assembled onto a state that
    /// registered the series. Deliberately NOT a public accessor handing the [`Gauge`] out - the
    /// one route to the handle is [`SpendHeadroomPush::of`], which is what makes the declaration
    /// mean "this state's own gauge" rather than "a gauge".
    #[cfg(feature = "agent")]
    #[inline]
    pub(crate) const fn spend_headroom_registered(&self) -> bool {
        self.spend_headroom.is_some()
    }

    /// Pushes this replica's current spend headroom onto the gauge, if this deployment has a
    /// ceiling configured at all.
    ///
    /// Called by the `POST /v1/query` route after a call to [`crate::surface::Surface::answer`],
    /// never by the metrics route: a scrape must not poll the ledger itself, only read what a
    /// request already pushed. **No agent-surface route calls this** - see [`ServiceState`]'s
    /// `spend_headroom` field doc for the limit that leaves. A `None` reading - either no ceiling
    /// configured, or this deployment's own composition never attached one - leaves the gauge
    /// untouched rather than fabricating zero.
    pub(crate) fn record_spend_headroom(&self, headroom_bytes: Option<u64>) {
        if let (Some(gauge), Some(bytes)) = (&self.spend_headroom, headroom_bytes) {
            gauge.set(bytes);
        }
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
