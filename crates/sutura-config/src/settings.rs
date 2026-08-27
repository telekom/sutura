//! The resolved configuration, and the combinations this service refuses to start with.
//!
//! Two halves, and keeping them apart is the design. [`Settings::parse`] turns a raw tree into
//! typed values, so a malformed value is a parse error naming the key. [`Settings::refusals`] then
//! asks a different question - not "is this value well formed" but "is this deployment one we are
//! willing to serve" - and its answers are the security posture in [`NotFitToServe`].
//!
//! The second half exists because the first cannot express it. Every field of a refused
//! configuration is individually valid: `0.0.0.0` is an address, `false` is a boolean, an absent
//! token is an absent token. What is wrong is the *combination*, and a combination has no
//! constructor to hide behind.
//!
//! **The refusals are checked against the loaded value and not against a file.** The environment
//! layer is applied last, so a variable beats whatever the production file said - which means a
//! check that read the file would be checking something the process is not running on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sutura_domain::pinned::{DefinitionVersion, InvalidVersion};

use crate::api::ApiSettings;
use crate::catalog::{CatalogSettings, InvalidCatalogSettings};
use crate::environment::{Environment, UnknownEnvironment};
use crate::limits::{InvalidQuota, Quota, RateLimitSettings};
use crate::prompt::{CatalogProse, InstructionsFile, InvalidPromptSettings, PromptSettings, UnknownCatalogProse};
use crate::proxy::{ClientAddressSource, InvalidTrustedProxy, TrustedProxies, UnknownClientAddressSource};
use crate::raw::RawSettings;
use crate::runtime::{AdmissionTimeout, EngineWorkers, QueryConcurrency, RuntimeSettings, ShutdownGrace};
use crate::security::{AccessToken, InvalidAccessToken, SecuritySettings, TlsTermination, UnknownTlsTermination};
use crate::server::{
    BindAddress, BodyLimit, InvalidBindAddress, InvalidBound, InvalidTlsMaterial, RequestTimeout, ServerSettings,
};
use crate::telemetry::{
    InvalidLogFilter, InvalidServiceName, LogFilter, LogFormat, ServiceName, TelemetrySettings, UnknownLogFormat,
};

/// The variable that chooses the deployment environment.
///
/// One name, exported so a startup message and the documentation cannot disagree about it.
pub const ENVIRONMENT_VARIABLE: &str = "SUTURA_ENVIRONMENT";

/// The prefix every configuration variable carries, and the separator between key segments.
///
/// `SUTURA__SERVER__PORT` sets `server.port`. Two underscores for both, so a key segment that
/// itself contains an underscore - `access_token`, `max_body_bytes` - needs no escaping.
pub const VARIABLE_PREFIX: &str = "SUTURA";
/// The separator between nested key segments in a configuration variable name.
pub const VARIABLE_SEPARATOR: &str = "__";

/// The built-in defaults, embedded so a missing file cannot become a posture nobody chose.
const DEFAULTS: &str = include_str!("defaults.yaml");

/// Where a load reads from.
///
/// A value rather than a set of arguments, for one reason: the process environment is global, and
/// `std::env::set_var` is `unsafe` in this edition - so a test that wanted to exercise the variable
/// layer by setting variables could not be written under `unsafe_code = "forbid"`. Supplying the
/// variables as a map makes that layer a pure function of its input, and
/// [`Sources::from_process_environment`] is the one place that reads the real environment.
#[derive(Debug, Clone)]
pub struct Sources {
    environment: Environment,
    directory: Option<PathBuf>,
    overlay: Option<String>,
    variables: Option<BTreeMap<String, String>>,
}

impl Sources {
    /// The defaults for `environment`, and nothing else.
    ///
    /// No files and no variables. This is what a test starts from, and what the documentation
    /// examples describe.
    #[inline]
    pub const fn defaults(environment: Environment) -> Self {
        Self {
            environment,
            directory: None,
            overlay: None,
            variables: Some(BTreeMap::new()),
        }
    }

    /// The defaults, a configuration directory, and the real process environment.
    ///
    /// What the binary calls.
    #[inline]
    pub const fn from_process_environment(environment: Environment, directory: Option<PathBuf>) -> Self {
        Self {
            environment,
            directory,
            overlay: None,
            variables: None,
        }
    }

    /// Layers a YAML document on top of the defaults.
    ///
    /// The same position a deployment's own file occupies, supplied as text. This is how the
    /// refusal tests build a production configuration without a filesystem.
    #[must_use]
    pub fn with_overlay(mut self, yaml: impl Into<String>) -> Self {
        self.overlay = Some(yaml.into());
        self
    }

    /// Layers explicit variables instead of the process environment.
    #[must_use]
    pub fn with_variables(mut self, variables: BTreeMap<String, String>) -> Self {
        self.variables = Some(variables);
        self
    }

    #[inline]
    pub const fn environment(&self) -> Environment {
        self.environment
    }
}

/// Reads the deployment environment from the process.
///
/// Absent means [`Environment::Development`]: a developer running the binary with no environment
/// set is on a laptop, and the permissive default is safe there precisely because the other
/// defaults are loopback-only. An environment that is *present and unrecognised* is an error and
/// never falls back, because falling back would select the permissive branch of five decisions.
pub fn environment_from_process() -> Result<Environment, SettingsError> {
    match std::env::var(ENVIRONMENT_VARIABLE) {
        Ok(raw) => Environment::parse(raw).map_err(|cause| SettingsError::Environment { cause }),
        Err(std::env::VarError::NotPresent) => Ok(Environment::Development),
        Err(std::env::VarError::NotUnicode(_)) => Err(SettingsError::EnvironmentNotUnicode),
    }
}

/// Why a configuration could not be turned into settings.
///
/// One variant per thing that can be wrong, each keeping the typed cause underneath it. The
/// message names the concern and the `#[source]` chain names the value, so an operator reading
/// stderr gets both without either being formatted into the other.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// A source could not be read, or a value could not be deserialized into the raw tree. This is
    /// where an unknown key lands, because `deny_unknown_fields` is a deserialization error.
    ///
    /// Boxed: the `config` crate's error is large, and an error type that dwarfs the success value
    /// makes every `Result` in this crate expensive.
    #[error("the configuration sources could not be read")]
    Source {
        #[source]
        cause: Box<config::ConfigError>,
    },
    #[error("`{ENVIRONMENT_VARIABLE}` does not name a deployment environment")]
    Environment {
        #[source]
        cause: UnknownEnvironment,
    },
    #[error("`{ENVIRONMENT_VARIABLE}` is not valid Unicode")]
    EnvironmentNotUnicode,
    #[error("`server.host` and `server.port` are not an address to listen on")]
    Bind {
        #[source]
        cause: InvalidBindAddress,
    },
    #[error("a server bound is out of range")]
    Bound {
        #[source]
        cause: InvalidBound,
    },
    #[error("`security.access_token` is not usable as a token")]
    AccessToken {
        #[source]
        cause: InvalidAccessToken,
    },
    #[error("`security.tls_termination` does not name where TLS is terminated")]
    TlsTermination {
        #[source]
        cause: UnknownTlsTermination,
    },
    #[error("`server.tls_certificate` and `server.tls_key` are not a usable pair")]
    TlsMaterial {
        #[source]
        cause: InvalidTlsMaterial,
    },
    #[error("a rate limit tier is not a quota")]
    Quota {
        #[source]
        cause: InvalidQuota,
    },
    #[error("`rate_limit.client_address` does not name a source")]
    ClientAddress {
        #[source]
        cause: UnknownClientAddressSource,
    },
    #[error("an entry in `rate_limit.trusted_proxies` is not an address or an address block")]
    TrustedProxy {
        #[source]
        cause: InvalidTrustedProxy,
    },
    #[error("`telemetry.format` is not a log format")]
    Format {
        #[source]
        cause: UnknownLogFormat,
    },
    #[error("`telemetry.filter` is not a filter directive")]
    Filter {
        #[source]
        cause: InvalidLogFilter,
    },
    #[error("`telemetry.service_name` is not a service name")]
    Service {
        #[source]
        cause: InvalidServiceName,
    },
    #[error("`catalog.version` is not a definition version")]
    Version {
        #[source]
        cause: InvalidVersion,
    },
    #[error("the catalog configuration is not usable")]
    Catalog {
        #[source]
        cause: InvalidCatalogSettings,
    },
    #[error("`prompt.catalog_prose` does not say how catalog prose reaches an agent")]
    CatalogProse {
        #[source]
        cause: UnknownCatalogProse,
    },
    #[error("the prompt configuration is not usable")]
    Prompt {
        #[source]
        cause: InvalidPromptSettings,
    },
    /// The values are all well formed and the deployment they describe is one this service will
    /// not serve.
    ///
    /// **Every refusal is in the message, not just the first, and not behind a `#[source]`.** A
    /// `Vec` has no single cause to chain, so a variant that only said "not fit to serve" would
    /// leave the operator to guess which control was missing - and then discover the second one on
    /// the next restart. The format expression is what puts all of them in front of them at once.
    #[error(
        "this configuration is not fit to serve:\n  - {}",
        refusals.iter().map(ToString::to_string).collect::<Vec<String>>().join("\n  - ")
    )]
    NotFitToServe { refusals: Vec<NotFitToServe> },
}

/// A deployment this service refuses to start as.
///
/// **These are the security posture, and each one is a refusal rather than a warning on purpose.**
/// The thing being guarded against is not an operator who ignores a log line - it is an operator
/// who never sees one, because the line was emitted in a format nothing was collecting, on a
/// process that went on to serve traffic. A process that does not start is noticed.
///
/// Every variant names the key to change, because a refusal that does not say what to do is a
/// support request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotFitToServe {
    /// The bind address is reachable from other hosts and nobody said what protects the path to
    /// it.
    ///
    /// **This is not a refusal of the bind, and the previous version of this row was.** A
    /// plaintext listener on a pod network with an ingress controller or a sidecar terminating TLS
    /// in front of it is the normal arrangement, and refusing it would refuse the deployment this
    /// service is built for. What is refused is *silence*: the bearer token crosses whatever sits
    /// between the terminator and this process in cleartext, and how far that reaches - a loopback
    /// hop inside a pod, or the pod network - is a fact about the deployment that only the operator
    /// knows. Naming it makes it a stated fact that the startup log can print, and a declaration
    /// cannot be satisfied by agreeing that off-host was intended.
    #[error(
        "server.host is {bind}, which is reachable from other hosts, and security.tls_termination \
         is `none`. Say where TLS is terminated - one of: sidecar, ingress, in-process - or bind \
         127.0.0.1. The declaration does not encrypt anything: it records which cleartext hop \
         this bearer token crosses, which is a fact only this deployment knows"
    )]
    TlsTerminationUndeclared { bind: String },
    /// Something is reachable off-host, or this is production, and there is no token.
    ///
    /// Not authentication - see [`crate::security`] - but the difference between a bearer secret
    /// and nothing at all is the difference between a configured reader and anyone who can route
    /// a packet.
    #[error(
        "{because}, so security.access_token must be set. It authenticates the DEPLOYMENT and not \
         the caller: sutura has no per-caller identity, so every query still runs with whatever \
         access this process already had"
    )]
    AccessTokenRequired { because: &'static str },
    /// Production with the limiter switched off.
    #[error(
        "rate_limit.enabled is false in production. A question here is an aggregate over up to ten \
         years of history, so an unbounded caller is an unbounded load on the data system"
    )]
    RateLimitingDisabledInProduction,
    /// Production asking the kernel to choose the port.
    #[error(
        "server.port is 0 in production, which asks the kernel for an ephemeral port. Nothing can \
         then be configured to reach this service; port 0 is for a test that reads the port back"
    )]
    EphemeralPortInProduction,
    /// A forwarded header would be believed with nobody named as the hop it may come from.
    ///
    /// **The one refusal in this list that exists because the permissive branch is worse than the
    /// restrictive one in both directions.** With no trusted hop, `X-Forwarded-For` is a value any
    /// caller writes, so every bucket becomes the caller's to choose - a limiter that reports a
    /// configured limit and bounds nothing at all, which is strictly worse than the one shared
    /// bucket that peer keying gives behind a proxy.
    #[error(
        "rate_limit.client_address is `forwarded` and rate_limit.trusted_proxies is empty. A \
         forwarded header is a value any caller can write, so with no hop named it would let every \
         caller pick their own rate-limit bucket. List the proxy addresses or blocks, or set \
         client_address: peer"
    )]
    ForwardedWithoutTrustedProxies,
    /// Trusted proxies were listed and nothing reads them.
    ///
    /// Refused rather than ignored, for the reason every unknown key here is an error: a list that
    /// does nothing reads as a control that is in place.
    #[error(
        "rate_limit.trusted_proxies names {count} hop(s) and rate_limit.client_address is `peer`, \
         which reads no header - so the list has no effect. Set client_address: forwarded, or \
         remove the list"
    )]
    TrustedProxiesWithoutForwarding { count: usize },
    /// TLS termination was declared as in-process and no certificate and key were given.
    #[error(
        "security.tls_termination is `in-process` and no server.tls_certificate and server.tls_key \
         are set. This process cannot terminate TLS without them, and it will not fall back to \
         plaintext on a port that was configured to be encrypted"
    )]
    InProcessTlsWithoutMaterial,
    /// A certificate and key were given and nothing will use them.
    #[error(
        "server.tls_certificate and server.tls_key are set and security.tls_termination is \
         `{declared}`, so this process serves plaintext and the material is never read. Set \
         tls_termination: in-process, or remove the paths"
    )]
    TlsMaterialWithoutInProcessTermination { declared: &'static str },
    /// TLS termination was declared as in-process and this binary cannot do it.
    ///
    /// **The loud failure the requirement asks for.** A binary built without the `tls` feature has
    /// no TLS implementation linked in at all, so the alternative to refusing is serving plaintext
    /// on a port an operator configured to be encrypted - which is the one failure mode that must
    /// never be quiet.
    #[error(
        "security.tls_termination is `in-process` and this binary was built without the `tls` \
         feature, so it has no TLS implementation linked in. Rebuild with `--features tls`, or \
         terminate TLS in front of this process and declare `sidecar` or `ingress`"
    )]
    InProcessTlsNotCompiledIn,
}

/// The whole resolved configuration.
///
/// `Clone` because it is held in the request state, and every field is either `Copy` or a small
/// owned value. `Debug` is safe to log in full: the only credential-shaped field is held in
/// [`sutura_domain::identity::Secret`], whose `Debug` redacts, and a test in [`crate::security`]
/// asserts that at struct depth.
#[derive(Debug, Clone)]
pub struct Settings {
    environment: Environment,
    server: ServerSettings,
    security: SecuritySettings,
    rate_limit: RateLimitSettings,
    telemetry: TelemetrySettings,
    api: ApiSettings,
    catalog: CatalogSettings,
    runtime: RuntimeSettings,
    prompt: PromptSettings,
}

impl Settings {
    /// Reads every source, parses the result, and refuses a deployment it will not serve.
    ///
    /// The order is the contract: defaults, then `<dir>/base.yaml`, then
    /// `<dir>/<environment>.yaml`, then the variables. Later beats earlier, so a variable is the
    /// last word - which is exactly why [`Self::refusals`] runs on the parsed result rather than
    /// on any one layer.
    pub fn load(sources: &Sources) -> Result<Self, SettingsError> {
        let raw = read(sources)?;
        let settings = Self::parse(&raw, sources.environment)?;
        let refusals = settings.refusals();
        if refusals.is_empty() {
            Ok(settings)
        } else {
            Err(SettingsError::NotFitToServe { refusals })
        }
    }

    /// Turns a raw tree into typed values.
    ///
    /// Private: the raw shapes are private, so there is no way to call this with anything other
    /// than what [`read`] produced. That is what makes [`Self::load`] the only door in, and
    /// therefore what makes the refusal check unskippable.
    fn parse(raw: &RawSettings, environment: Environment) -> Result<Self, SettingsError> {
        Ok(Self {
            environment,
            server: parse_server(raw)?,
            security: parse_security(raw)?,
            rate_limit: parse_rate_limit(raw, environment)?,
            telemetry: parse_telemetry(raw, environment)?,
            api: ApiSettings::new(
                raw.api.docs.unwrap_or_else(|| ApiSettings::docs_default_for(environment)),
                raw.api.docs.is_some(),
            ),
            catalog: parse_catalog(raw)?,
            runtime: parse_runtime(raw)?,
            prompt: parse_prompt(raw)?,
        })
    }

    /// Every reason this deployment will not be served, or an empty list.
    ///
    /// Public and side-effect-free, so a test can assert on the set and a startup message can
    /// print all of it. It is called by [`Self::load`], so a caller cannot obtain a `Settings`
    /// that has not been through it - the method is exposed for inspection, not as the gate.
    #[must_use]
    pub fn refusals(&self) -> Vec<NotFitToServe> {
        let mut refusals = Vec::new();
        let bind = self.server.bind();
        let off_host = !bind.is_loopback();

        if off_host && !self.security.tls_termination().is_declared() {
            refusals.push(NotFitToServe::TlsTerminationUndeclared { bind: bind.to_string() });
        }
        refusals.extend(self.tls_refusals());
        refusals.extend(self.keying_refusals());
        if self.security.access_token().is_none() {
            // Two different reasons, and the message says which: an operator whose production
            // deployment refuses should not have to work out whether it was the bind or the
            // environment that asked for the token.
            if self.environment.is_production() {
                refusals.push(NotFitToServe::AccessTokenRequired {
                    because: "this is a production deployment",
                });
            } else if off_host {
                refusals.push(NotFitToServe::AccessTokenRequired {
                    because: "this service is bound where other hosts can reach it",
                });
            }
        }
        if self.environment.is_production() {
            if !self.rate_limit.enabled() {
                refusals.push(NotFitToServe::RateLimitingDisabledInProduction);
            }
            if bind.port() == 0 {
                refusals.push(NotFitToServe::EphemeralPortInProduction);
            }
        }
        refusals
    }

    /// Everything wrong with the TLS declaration and the material that goes with it.
    ///
    /// Split out of [`Self::refusals`] so each half stays under the complexity threshold, and
    /// because these three are one question asked three ways: does the declaration, the material
    /// and the binary agree about who terminates the connection?
    fn tls_refusals(&self) -> Vec<NotFitToServe> {
        let mut refusals = Vec::new();
        let declared = self.security.tls_termination();
        let material = self.server.tls();
        if declared.terminates_here() {
            if material.is_none() {
                refusals.push(NotFitToServe::InProcessTlsWithoutMaterial);
            }
            // `cfg!` rather than `#[cfg]`, so both arms are compiled and neither can rot: the
            // refusal is a value either way and only the boolean changes.
            if !cfg!(feature = "tls") {
                refusals.push(NotFitToServe::InProcessTlsNotCompiledIn);
            }
        } else if material.is_some() {
            refusals.push(NotFitToServe::TlsMaterialWithoutInProcessTermination {
                declared: declared.as_str(),
            });
        }
        refusals
    }

    /// Everything wrong with what a rate-limit bucket would be keyed on.
    fn keying_refusals(&self) -> Vec<NotFitToServe> {
        let mut refusals = Vec::new();
        let proxies = self.rate_limit.trusted_proxies();
        if self.rate_limit.client_address().reads_a_header() {
            if proxies.is_empty() {
                refusals.push(NotFitToServe::ForwardedWithoutTrustedProxies);
            }
        } else if !proxies.is_empty() {
            refusals.push(NotFitToServe::TrustedProxiesWithoutForwarding { count: proxies.len() });
        }
        refusals
    }

    #[inline]
    pub const fn environment(&self) -> Environment {
        self.environment
    }

    #[inline]
    pub const fn server(&self) -> &ServerSettings {
        &self.server
    }

    #[inline]
    pub const fn security(&self) -> &SecuritySettings {
        &self.security
    }

    #[inline]
    pub const fn rate_limit(&self) -> &RateLimitSettings {
        &self.rate_limit
    }

    #[inline]
    pub const fn telemetry(&self) -> &TelemetrySettings {
        &self.telemetry
    }

    #[inline]
    pub const fn api(&self) -> ApiSettings {
        self.api
    }

    #[inline]
    pub const fn catalog(&self) -> &CatalogSettings {
        &self.catalog
    }

    /// How much runs at once, how wide the engine is, and how long stopping may take.
    ///
    /// `Copy`, unlike the sections above it: every field is a bound rather than a string, so
    /// returning it by value costs nothing and a caller cannot hold a borrow of the settings for
    /// the life of a query.
    #[inline]
    pub const fn runtime(&self) -> RuntimeSettings {
        self.runtime
    }

    /// What goes into the agent-facing system prompt beyond the pinned bundle and the tool list.
    ///
    /// Read by the `prompt` command in `sutura-cli`, which renders what this deployment would hand
    /// an agent. A borrow rather than a copy, unlike [`Self::runtime`]: the operator's path is an
    /// owned value, and nothing in this group is a bound.
    #[inline]
    pub const fn prompt(&self) -> &PromptSettings {
        &self.prompt
    }
}

/// Builds the layered configuration and deserializes it into the raw tree.
fn read(sources: &Sources) -> Result<RawSettings, SettingsError> {
    let mut builder = config::Config::builder().add_source(config::File::from_str(DEFAULTS, config::FileFormat::Yaml));

    if let Some(directory) = sources.directory.as_deref() {
        builder = builder
            .add_source(optional_file(directory, "base"))
            .add_source(optional_file(directory, sources.environment.as_str()));
    }
    if let Some(overlay) = sources.overlay.as_deref() {
        builder = builder.add_source(config::File::from_str(overlay, config::FileFormat::Yaml));
    }

    let mut variables = config::Environment::with_prefix(VARIABLE_PREFIX)
        .prefix_separator(VARIABLE_SEPARATOR)
        .separator(VARIABLE_SEPARATOR);
    if let Some(ref supplied) = sources.variables {
        // An explicit map, including an EMPTY one, replaces the process environment. That is what
        // makes a test hermetic: without it, a `SUTURA__*` variable in the developer's shell would
        // change what the test asserts.
        variables = variables.source(Some(supplied.clone().into_iter().collect()));
    }
    builder = builder.add_source(variables);

    builder
        .build()
        .and_then(config::Config::try_deserialize)
        .map_err(|cause| SettingsError::Source { cause: Box::new(cause) })
}

/// A `<stem>.yaml` in `directory`, if it is there.
///
/// Optional rather than required, which is the opposite of what the reference project does, and
/// deliberately: the defaults here are embedded and complete, so a missing file means "nothing to
/// override" rather than "the deployment is half-configured". A required file would make the
/// binary unable to start without a filesystem it does not otherwise need.
fn optional_file(directory: &Path, stem: &str) -> config::File<config::FileSourceFile, config::FileFormat> {
    config::File::from(directory.join(format!("{stem}.yaml"))).required(false)
}

fn parse_server(raw: &RawSettings) -> Result<ServerSettings, SettingsError> {
    let bind = BindAddress::parse(&raw.server.host, raw.server.port).map_err(|cause| SettingsError::Bind { cause })?;
    let timeout = RequestTimeout::parse(raw.server.request_timeout_seconds).map_err(|cause| SettingsError::Bound { cause })?;
    let body = BodyLimit::parse(raw.server.max_body_bytes).map_err(|cause| SettingsError::Bound { cause })?;
    let tls = crate::server::TlsMaterial::parse(raw.server.tls_certificate.as_deref(), raw.server.tls_key.as_deref())
        .map_err(|cause| SettingsError::TlsMaterial { cause })?;
    Ok(ServerSettings::new(bind, timeout, body, tls))
}

fn parse_security(raw: &RawSettings) -> Result<SecuritySettings, SettingsError> {
    let token = match raw.security.access_token.as_deref() {
        // An empty string is the shape an unset variable takes in a shell, and treating it as a
        // configured token would give every request a 401 for a reason nothing explains.
        None | Some("") => None,
        Some(value) => Some(AccessToken::parse(value).map_err(|cause| SettingsError::AccessToken { cause })?),
    };
    let termination = match raw.security.tls_termination.as_deref() {
        None | Some("") => TlsTermination::default(),
        Some(value) => TlsTermination::parse(value).map_err(|cause| SettingsError::TlsTermination { cause })?,
    };
    Ok(SecuritySettings::new(token, termination))
}

fn parse_rate_limit(raw: &RawSettings, environment: Environment) -> Result<RateLimitSettings, SettingsError> {
    let probe = Quota::parse(
        "rate_limit.probe",
        raw.rate_limit.probe_per_second,
        raw.rate_limit.probe_burst,
    )
    .map_err(|cause| SettingsError::Quota { cause })?;
    let api = Quota::parse("rate_limit.api", raw.rate_limit.api_per_second, raw.rate_limit.api_burst)
        .map_err(|cause| SettingsError::Quota { cause })?;
    let client_address = match raw.rate_limit.client_address.as_deref() {
        None | Some("") => ClientAddressSource::default(),
        Some(value) => ClientAddressSource::parse(value).map_err(|cause| SettingsError::ClientAddress { cause })?,
    };
    let proxies =
        TrustedProxies::parse(&raw.rate_limit.trusted_proxies).map_err(|cause| SettingsError::TrustedProxy { cause })?;
    Ok(RateLimitSettings::new(
        // The same shape as `api.docs` above: the value, then whether anybody wrote it down. The
        // second is not derivable from the first once it is stored, and the startup log needs both.
        raw.rate_limit
            .enabled
            .unwrap_or_else(|| RateLimitSettings::enabled_default_for(environment)),
        raw.rate_limit.enabled.is_some(),
        probe,
        api,
        client_address,
        proxies,
    ))
}

fn parse_telemetry(raw: &RawSettings, environment: Environment) -> Result<TelemetrySettings, SettingsError> {
    let name = ServiceName::parse(&raw.telemetry.service_name).map_err(|cause| SettingsError::Service { cause })?;
    let filter = LogFilter::parse(&raw.telemetry.filter).map_err(|cause| SettingsError::Filter { cause })?;
    let format = match raw.telemetry.format.as_deref() {
        None => LogFormat::default_for(environment),
        Some(value) => LogFormat::parse(value).map_err(|cause| SettingsError::Format { cause })?,
    };
    Ok(TelemetrySettings::new(name, filter, format, raw.telemetry.format.is_some()))
}

fn parse_catalog(raw: &RawSettings) -> Result<CatalogSettings, SettingsError> {
    let version = DefinitionVersion::parse(&raw.catalog.version).map_err(|cause| SettingsError::Version { cause })?;
    CatalogSettings::parse(PathBuf::from(&raw.catalog.dir), PathBuf::from(&raw.catalog.data_dir), version)
        .map_err(|cause| SettingsError::Catalog { cause })
}

/// The concurrency bounds and the two deadlines that are not per-request.
///
/// Every one of these is a `parse` on a newtype rather than a raw number reaching the runtime,
/// which is what makes an unusable value a refusal at startup instead of a surprise under load.
/// `engine_worker_threads` is the one that may be absent: `EngineWorkers::parse` resolves `None`
/// to what the machine can run and records that nobody chose it, so the startup log can say which.
fn parse_runtime(raw: &RawSettings) -> Result<RuntimeSettings, SettingsError> {
    let concurrency =
        QueryConcurrency::parse(raw.runtime.max_concurrent_queries).map_err(|cause| SettingsError::Bound { cause })?;
    let admission =
        AdmissionTimeout::parse(raw.runtime.admission_timeout_seconds).map_err(|cause| SettingsError::Bound { cause })?;
    let workers = EngineWorkers::parse(raw.runtime.engine_worker_threads).map_err(|cause| SettingsError::Bound { cause })?;
    let grace = ShutdownGrace::parse(raw.runtime.shutdown_grace_seconds).map_err(|cause| SettingsError::Bound { cause })?;
    Ok(RuntimeSettings::new(concurrency, admission, workers, grace))
}

/// The two keys that shape the agent-facing prompt.
///
/// **An empty `instructions_file` is an error here, and that is the opposite of what
/// [`parse_security`] does with an empty token.** The two empties mean different things. An empty
/// access token treated as configured would answer every request `401` for a reason nothing
/// explains, so absent is the safe reading. An empty *path* resolves to the process working
/// directory - a different directory on every host and never the one the operator meant - and
/// absence is already expressible by removing the key, so here the safe reading is a refusal naming
/// the key. It is the argument `catalog.dir` already makes.
///
/// `catalog_prose` is branched on rather than required, so a deployment that removed the key from
/// its own copy of the defaults gets the default rather than a deserialization failure.
fn parse_prompt(raw: &RawSettings) -> Result<PromptSettings, SettingsError> {
    let instructions = match raw.prompt.instructions_file.as_deref() {
        None => None,
        Some(value) => Some(InstructionsFile::parse(value).map_err(|cause| SettingsError::Prompt { cause })?),
    };
    let prose = match raw.prompt.catalog_prose.as_deref() {
        None => CatalogProse::default(),
        Some(value) => CatalogProse::parse(value).map_err(|cause| SettingsError::CatalogProse { cause })?,
    };
    Ok(PromptSettings::new(instructions, prose))
}

#[cfg(test)]
mod tests;
