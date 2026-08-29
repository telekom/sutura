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
    /// The data systems this deployment declares, keyed by the alias a model's `source:` names.
    ///
    /// **A map and not a list**, so the key IS the alias and there is one place a source is named. The
    /// duplicate-alias refusal is still real and still needed: a source name is trimmed when it is
    /// parsed, so `local` and `" local"` are two keys in a YAML mapping and one `SourceName`.
    ///
    /// Defaults to empty, and an empty registry is not a refusal here - see
    /// `crate::sources::SourceRegistry`. What refuses a deployment that declared nothing is the
    /// composition root, on the catalog naming a source it has no declaration for.
    #[serde(default)]
    pub(crate) sources: std::collections::BTreeMap<String, RawSource>,
}

/// One source's entry.
///
/// `posture` is REQUIRED and has no default, which is the one thing this shape does that a comment
/// could not: identity has no bind address that makes "one identity for every caller" safe to assume,
/// so an entry that declares no posture is a deserialization error naming the key rather than a value
/// somebody has to remember to check.
///
/// `working_set_max_bytes` is deliberately absent, and `deny_unknown_fields` is what makes that a
/// mechanism: the query-wide ceiling lives under `runtime` and nowhere else, so a per-source override
/// is an unknown-key error rather than a number with nothing to bound.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSource {
    /// What kind of data system this is, which decides which adapter opens it.
    ///
    /// Required, with no default. It replaced a comparison against a hard-coded source NAME in the
    /// composition root - see `crate::sources::SourceKind` for why that comparison was right while the
    /// catalog was the only signal and stops being right once the deployment declares each source.
    pub(crate) kind: String,
    /// Where the files behind this source's models live. Required, and absolute.
    #[serde(default)]
    pub(crate) data_dir: Option<String>,
    /// `shared-service-user` or `impersonation-at-source`. No default.
    pub(crate) posture: String,
    /// The operator's reason for serving this source under one identity for everybody.
    #[serde(default)]
    pub(crate) acknowledged_because: Option<String>,
    /// The identity this source's anchors re-run under. Only for an impersonating source: on a shared
    /// one the verification identity IS the shared identity, and a name here would be read by nothing.
    #[serde(default)]
    pub(crate) verification_identity: Option<String>,
}

/// How much runs at once, how wide the engine is, and how long stopping may take.
///
/// `engine_worker_threads` is the one optional field: absent means "as many threads as this machine
/// can run", resolved to a number at load time so the startup log prints what is in effect. The
/// other four are required, because a bound nobody wrote down is a bound nobody chose.
///
/// `working_set_max_bytes` is here and **nowhere else**, which is what makes 0009's "no per-source
/// override" a mechanism rather than a sentence: `deny_unknown_fields` sits on every shape in this
/// file, so the key written under any other group is an error naming it.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRuntime {
    pub(crate) max_concurrent_queries: usize,
    pub(crate) admission_timeout_seconds: u64,
    pub(crate) working_set_max_bytes: u64,
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
    /// `single-user` or `multi-user`. **Absent means absent**, not a default: this is the one key in
    /// this tree whose absence is a refusal rather than a value, because no bind address and no
    /// combination of source postures makes either mode safe to assume. `defaults.yaml` deliberately
    /// does not write it.
    #[serde(default)]
    pub(crate) identity: Option<String>,
    /// Why single-user mode is correct here. Required with `single-user`, refused with `multi-user`.
    #[serde(default)]
    pub(crate) single_user_because: Option<String>,
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
