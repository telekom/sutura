//! The shapes the configuration sources are read into, before anything is parsed.
//!
//! Private to this crate, and deliberately dull: every field is a primitive, nothing is
//! validated here, and no type in this module is re-exported. It exists so the layering can be
//! done by the `config` crate - which speaks in strings, numbers and booleans - and the parsing
//! can be done once, afterwards, by [`crate::Settings::parse`].
//!
//! Three things about this module are load-bearing rather than incidental.
//!
//! **No `Debug` derive, anywhere.** A raw shape holds the access token as a plain `String`, so a
//! `{:?}` on one would print it. The redaction lives in [`sutura_domain::identity::Secret`], and
//! the raw tree is converted into types that hold one before anything logs anything. Not deriving
//! `Debug` is what makes that ordering impossible to get wrong: there is nothing to print.
//!
//! **`deny_unknown_fields` at every depth.** A misspelled key is otherwise dropped in silence and
//! the service runs on the default the operator thought they had overridden - which is the same
//! failure the catalog documents use this attribute to prevent, arrived at from the other side. It
//! also means a stray `SUTURA__*` variable is an error naming the key rather than a value nobody
//! reads.
//!
//! **No `environment` field.** The environment decides *which file* is layered, so a file that
//! could set it would be self-referential. It comes from one place only - the `SUTURA_ENVIRONMENT`
//! variable - and because there is no field for it here, `SUTURA__ENVIRONMENT=production` is an
//! unknown-field error rather than a setting that silently does nothing.

/// The whole tree, as read.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSettings {
    pub(crate) server: RawServer,
    #[serde(default)]
    pub(crate) security: RawSecurity,
    pub(crate) rate_limit: RawRateLimit,
    pub(crate) telemetry: RawTelemetry,
    #[serde(default)]
    pub(crate) api: RawApi,
    pub(crate) catalog: RawCatalog,
    pub(crate) runtime: RawRuntime,
    #[serde(default)]
    pub(crate) prompt: RawPrompt,
}

/// How much runs at once, how wide the engine is, and how long stopping may take.
///
/// `engine_worker_threads` is the one optional field: absent means "as many threads as this machine
/// can run", resolved to a number at load time so the startup log prints what is in effect. The
/// other three are required, because a bound nobody wrote down is a bound nobody chose.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRuntime {
    pub(crate) max_concurrent_queries: usize,
    pub(crate) admission_timeout_seconds: u64,
    #[serde(default)]
    pub(crate) engine_worker_threads: Option<usize>,
    pub(crate) shutdown_grace_seconds: u64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawServer {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) request_timeout_seconds: u64,
    pub(crate) max_body_bytes: usize,
    /// Absent means "this process does not terminate TLS", which is the default.
    #[serde(default)]
    pub(crate) tls_certificate: Option<String>,
    #[serde(default)]
    pub(crate) tls_key: Option<String>,
}

/// Both fields default, because the safe posture is the one that needs no configuration: no token
/// and no TLS declaration is a loopback-only development service, which is what the refusals in
/// [`crate::Settings::parse`] then hold it to.
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSecurity {
    #[serde(default)]
    pub(crate) access_token: Option<String>,
    /// Where TLS is terminated, as a word. Absent means `none`.
    #[serde(default)]
    pub(crate) tls_termination: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRateLimit {
    /// Absent means "whatever this environment gets": off in development and test, on in
    /// production. An `Option` and not a `bool` with a default in `defaults.yaml`, for the reason
    /// `telemetry.format` and `api.docs` are: once the value is stored, a default nobody wrote down
    /// is indistinguishable from a decision somebody made, and the startup log has to tell them
    /// apart. `false` in production is refused whichever layer produced it.
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    pub(crate) probe_per_second: u32,
    pub(crate) probe_burst: u32,
    pub(crate) api_per_second: u32,
    pub(crate) api_burst: u32,
    /// Where the address a bucket is keyed on comes from. Absent means `peer`.
    #[serde(default)]
    pub(crate) client_address: Option<String>,
    /// The hops whose forwarded header is believed. Empty by default, which is what makes the
    /// default posture unspoofable.
    #[serde(default)]
    pub(crate) trusted_proxies: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTelemetry {
    pub(crate) service_name: String,
    pub(crate) filter: String,
    /// Absent means "whatever this environment gets", which is the whole point of the split.
    #[serde(default)]
    pub(crate) format: Option<String>,
}

/// Absent means "whatever this environment gets": on everywhere but production.
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawApi {
    #[serde(default)]
    pub(crate) docs: Option<bool>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCatalog {
    pub(crate) dir: String,
    pub(crate) data_dir: String,
    pub(crate) version: String,
}

/// What goes into the agent-facing prompt beyond the bundle and the tool list.
///
/// Both fields default, and they default to different KINDS of absent. `instructions_file` absent
/// means there is no operator section at all, which is the shape a deployment that wrote nothing
/// has. `catalog_prose` absent means `quoted`, which is also what `defaults.yaml` says - written
/// there rather than only here so the value in effect is readable in one file, the way
/// `rate_limit.client_address` is.
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPrompt {
    #[serde(default)]
    pub(crate) instructions_file: Option<String>,
    #[serde(default)]
    pub(crate) catalog_prose: Option<String>,
}
