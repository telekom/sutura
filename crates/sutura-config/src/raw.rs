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
    /// How the identity of a CALLER reaches this deployment. Absent means it does not.
    ///
    /// `Option` and not `#[serde(default)]` on the struct, and the difference is the whole design
    /// `crate::inbound` documents: a deployment with no block is a single-player deployment and is
    /// unaffected by any of this, while a block with no `mode` is a deployment that meant to
    /// establish identity and did not say how - and that one does not start.
    #[serde(default)]
    pub(crate) inbound: Option<RawInbound>,
}

/// The inbound-identity declaration, as read.
///
/// **Flat, with every key optional, and the mode is what decides which are required.** The
/// alternative - a tagged enum in serde - reads better and diagnoses worse: `config` layers a variable
/// per key, so `SUTURA__SECURITY__INBOUND__RESOURCE` has to be settable without the layer below it
/// having to restate the mode. Every mode-dependent absence becomes a refusal naming the key, in
/// `crate::settings::parse_inbound`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawInbound {
    /// `direct` or `behind-gateway`. **Required**, and there is no default because both would be
    /// wrong - see `crate::inbound`.
    #[serde(default)]
    pub(crate) mode: Option<String>,
    /// `direct`: what this deployment calls itself when it validates an audience.
    #[serde(default)]
    pub(crate) resource: Option<String>,
    /// `direct`: where the tokens it accepts are minted.
    #[serde(default)]
    pub(crate) authorization_server: Option<String>,
    /// `behind-gateway`: the header the signed transit proof arrives in. Never a header holding a
    /// name - see `crate::inbound` on why this is a proof and not an assertion.
    #[serde(default)]
    pub(crate) transit_header: Option<String>,
    /// `behind-gateway`: the issuer that must have signed the proof.
    #[serde(default)]
    pub(crate) transit_issuer: Option<String>,
    /// `behind-gateway`: the audience the proof must carry.
    #[serde(default)]
    pub(crate) transit_audience: Option<String>,
    /// Both modes: where the signing keys are read from.
    #[serde(default)]
    pub(crate) key_set_file: Option<String>,
    /// Both modes: the algorithms this deployment will accept. Required and never defaulted, because
    /// pinning is the control and a default here would be this crate choosing it.
    #[serde(default)]
    pub(crate) algorithms: Vec<String>,
    /// `direct`: which class of token, out of the `typ` header. Absent means RFC 9068's `at+jwt`.
    ///
    /// **Absent is the SAFE value here, unlike `mode`**, which is why it has a default at all: the
    /// unsafe reading is `any`, and that is a word an operator writes and the startup log prints at
    /// `WARN`. Defaulting the other way would have made the check switchable by silence, which is the
    /// shape review found.
    #[serde(default)]
    pub(crate) token_type: Option<String>,
    /// `behind-gateway`: which class of token the component emits. **Required**, because a component's
    /// `typ` is a fact only the deployment knows - there is no value this crate could guess that does
    /// not either reject every request or check nothing. `any` is how a deployment says its component
    /// sets none.
    #[serde(default)]
    pub(crate) transit_token_type: Option<String>,
    /// `behind-gateway`: the longest lifetime a proof may declare, in seconds. Absent means
    /// `ProofLifetime::DEFAULT_SECONDS`.
    #[serde(default)]
    pub(crate) transit_max_lifetime_seconds: Option<u64>,
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
