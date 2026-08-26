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
use crate::raw::RawSettings;
use crate::security::{AccessToken, InvalidAccessToken, SecuritySettings};
use crate::server::{BindAddress, BodyLimit, InvalidBindAddress, InvalidBound, RequestTimeout, ServerSettings};
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
    #[error("a rate limit tier is not a quota")]
    Quota {
        #[source]
        cause: InvalidQuota,
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
    /// The bind address is reachable from other hosts and nobody said that was intended.
    ///
    /// The one refusal that applies in *every* environment, including a laptop. Binding the
    /// wildcard is the single change that turns a local tool into a network service, and on a
    /// surface with no per-caller identity that is the whole of the exposure.
    #[error(
        "server.host is {bind}, which is reachable from other hosts. This service has no \
         per-caller identity, so its bind address is its perimeter: set \
         security.expose_beyond_loopback: true to say you meant it, or bind 127.0.0.1"
    )]
    BindsBeyondLoopback { bind: String },
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
            rate_limit: parse_rate_limit(raw)?,
            telemetry: parse_telemetry(raw, environment)?,
            api: ApiSettings::new(
                raw.api.docs.unwrap_or_else(|| ApiSettings::docs_default_for(environment)),
                raw.api.docs.is_some(),
            ),
            catalog: parse_catalog(raw)?,
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

        if off_host && !self.security.expose_beyond_loopback() {
            refusals.push(NotFitToServe::BindsBeyondLoopback { bind: bind.to_string() });
        }
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
    pub const fn rate_limit(&self) -> RateLimitSettings {
        self.rate_limit
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
    Ok(ServerSettings::new(bind, timeout, body))
}

fn parse_security(raw: &RawSettings) -> Result<SecuritySettings, SettingsError> {
    let token = match raw.security.access_token.as_deref() {
        // An empty string is the shape an unset variable takes in a shell, and treating it as a
        // configured token would give every request a 401 for a reason nothing explains.
        None | Some("") => None,
        Some(value) => Some(AccessToken::parse(value).map_err(|cause| SettingsError::AccessToken { cause })?),
    };
    Ok(SecuritySettings::new(token, raw.security.expose_beyond_loopback))
}

fn parse_rate_limit(raw: &RawSettings) -> Result<RateLimitSettings, SettingsError> {
    let probe = Quota::parse(
        "rate_limit.probe",
        raw.rate_limit.probe_per_second,
        raw.rate_limit.probe_burst,
    )
    .map_err(|cause| SettingsError::Quota { cause })?;
    let api = Quota::parse("rate_limit.api", raw.rate_limit.api_per_second, raw.rate_limit.api_burst)
        .map_err(|cause| SettingsError::Quota { cause })?;
    Ok(RateLimitSettings::new(raw.rate_limit.enabled, probe, api))
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

#[cfg(test)]
mod tests;
