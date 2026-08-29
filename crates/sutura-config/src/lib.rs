//! The service's configuration: layered sources in, one typed tree out, and a refusal instead of
//! a permissive default.
//!
//! # What a caller does
//!
//! Read the environment, then load. Two steps rather than one, because the environment decides
//! which file is layered and which defaults apply, so it has to be known first.
//!
//! ```
//! use sutura_config::{Environment, Settings, Sources};
//!
//! // A test or an example supplies the environment directly; the binary reads it from the
//! // process with `sutura_config::environment_from_process`.
//! let settings = Settings::load(&Sources::defaults(Environment::Development))?;
//! assert!(settings.server().bind().is_loopback());
//! # Ok::<(), sutura_config::SettingsError>(())
//! ```
//!
//! # Precedence
//!
//! Later beats earlier:
//!
//! 1. the defaults embedded in this crate ([`Settings`] can always be built from them alone);
//! 2. `<dir>/base.yaml`, if a directory was given and the file is there;
//! 3. `<dir>/<environment>.yaml`, likewise;
//! 4. environment variables - `SUTURA__SERVER__PORT` sets `server.port`.
//!
//! The environment itself is chosen by `SUTURA_ENVIRONMENT` and by nothing else. It is
//! deliberately *not* a configuration key: it selects which file is layered, so a file that could
//! change it would be self-referential. Both `environment:` in a file and `SUTURA__ENVIRONMENT` in
//! the shell are therefore unknown-field errors rather than settings that quietly do nothing.
//!
//! Every layer is checked with `deny_unknown_fields`, at every depth. A misspelled key is an error
//! naming the key, not an override that silently did not happen.
//!
//! # There is no per-caller identity, and this crate says so out loud
//!
//! sutura has no request context, no credential broker and no way for a caller's identity to reach
//! the query path. `AGENTS.md` records "every query runs as the calling principal" as an
//! aspiration that is **not mechanised**, and `examples/multi-player/README.md` explains why
//! single-player makes it trivially true and worth nothing.
//!
//! That is a property of the runtime, so it is a property of every deployment this crate
//! configures. An [`AccessToken`](security::AccessToken) authenticates *the deployment*: a caller
//! who presents it proves they hold a secret an operator configured, and nothing more. It does not
//! say which caller, it cannot be scoped to a subset of the catalog, it does not reach the data
//! system, and every query still runs with whatever access the process already had.
//! [`SecuritySettings::describes_identity`](security::SecuritySettings::describes_identity) is the
//! function that answers this, it always answers `false`, and the startup log prints that answer
//! on every boot so an operator cannot deploy this believing otherwise.
//!
//! Rate limiting is not authentication either - see [`limits`] for what it does and does not buy.
//!
//! # What refuses to start
//!
//! [`NotFitToServe`] is the whole list, and each variant is a refusal rather than a warning. A
//! warning is read by whoever is looking at the log in the format the collector was configured
//! for; a process that does not start is read by everybody.
//!
//! - A bind address other hosts can reach with `security.tls_termination: none`. In *every*
//!   environment, including a laptop. **The bind itself is not refused** - an ingress controller or
//!   a sidecar terminating TLS in front of a plaintext pod-local listener is the normal
//!   arrangement - what is refused is not saying which of those it is, because that is what decides
//!   how far the bearer token travels in cleartext.
//! - No `security.access_token`, in production or on a non-loopback bind.
//! - `rate_limit.enabled: false` in production.
//! - `rate_limit.client_address: forwarded` with an empty `rate_limit.trusted_proxies`, or a
//!   non-empty list that nothing reads.
//! - `security.tls_termination: in-process` without a certificate and key, or in a binary built
//!   without the `tls` feature; or a certificate and key no declaration would ever read.
//! - `server.port: 0` in production.
//!
//! The checks read the *loaded* values, not any one file, because the variable layer is applied
//! last: a check against `production.yaml` would be checking something the process is not running
//! on.
//!
//! **A value out of range is a different refusal, through a different type, and one of them reads the
//! machine.** [`NotFitToServe`] is about a *combination* of settings that are each individually legal;
//! a single value the type will not accept is a [`SettingsError`] out of [`Settings::load`], so it
//! refuses to start too and is not in that list. The one worth naming here is
//! `runtime.working_set_max_bytes`: it is checked against the memory this process can actually reach -
//! a cgroup limit, or the machine - and refuses above it, because shipped profiles compile
//! `panic = "abort"` and a ceiling over what is reachable is the unbounded case with a number written
//! next to it. **On a platform that will not report that number, notably macOS, no check is made**,
//! and [`WorkingSetCeiling::checked_against`] is what lets the startup log say which of the two
//! happened rather than implying the check was run.

pub mod api;
pub mod catalog;
pub mod environment;
pub mod inbound;
pub mod limits;
pub mod prompt;
pub mod proxy;
pub mod runtime;
pub mod security;
pub mod server;
pub mod telemetry;

mod raw;
mod settings;

pub use crate::api::ApiSettings;
pub use crate::catalog::{CatalogSettings, InvalidCatalogSettings};
pub use crate::environment::{Environment, UnknownEnvironment};
pub use crate::inbound::{
    InboundIdentity, InvalidAlgorithms, InvalidInboundValue, IssuerUrl, KeyFamily, KeySetFile, PinnedAlgorithms, ProofHeader,
    ProofLifetime, RequiredTokenType, ResourceIdentifier, SigningAlgorithm, TokenLocation, TokenRequirement, TokenType,
    TransitProof,
};
pub use crate::limits::{InvalidQuota, Quota, RateLimitSettings};
pub use crate::prompt::{CatalogProse, InstructionsFile, InvalidPromptSettings, PromptSettings, UnknownCatalogProse};
pub use crate::proxy::{Cidr, ClientAddressSource, InvalidTrustedProxy, TrustedProxies, UnknownClientAddressSource};
pub use crate::runtime::{
    AdmissionTimeout, EngineWorkers, QueryConcurrency, RuntimeSettings, ShutdownGrace, WorkingSetCeiling, available_memory_bytes,
};
pub use crate::security::{AccessToken, InvalidAccessToken, SecuritySettings, TlsTermination, UnknownTlsTermination};
pub use crate::server::{
    BindAddress, BodyLimit, InvalidBindAddress, InvalidBound, InvalidTlsMaterial, RequestTimeout, ServerSettings, TlsMaterial,
};
pub use crate::settings::{
    ConfigLayers, ENVIRONMENT_VARIABLE, NotFitToServe, Settings, SettingsError, Sources, VARIABLE_PREFIX, VARIABLE_SEPARATOR,
    environment_from_process,
};
pub use crate::telemetry::{
    InvalidLogFilter, InvalidServiceName, LogFilter, LogFormat, ServiceName, TelemetrySettings, UnknownLogFormat,
};
