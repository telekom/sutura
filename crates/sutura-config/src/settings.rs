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

use sutura_domain::model::{InvalidIdentifier, SourceName};
use sutura_domain::pinned::{DefinitionVersion, InvalidVersion};

use crate::api::ApiSettings;
use crate::catalog::{Catalogs, CatalogKind, CatalogSettings, InvalidCatalogSettings, UnknownCatalogKind};
use crate::environment::{Environment, UnknownEnvironment};
use crate::inbound::{InboundIdentity, InvalidAlgorithms, InvalidInboundValue};
use crate::limits::{InvalidQuota, Quota, RateLimitSettings};
use crate::prompt::{CatalogProse, InstructionsFile, InvalidPromptSettings, PromptSettings, UnknownCatalogProse};
use crate::proxy::{ClientAddressSource, InvalidTrustedProxy, TrustedProxies, UnknownClientAddressSource};
use crate::raw::RawSettings;
use crate::runtime::{AdmissionTimeout, EngineWorkers, QueryConcurrency, RuntimeSettings, ShutdownGrace, WorkingSetCeiling};
use crate::security::{
    AccessToken, DeploymentIdentity, InvalidAccessToken, InvalidDeploymentIdentity, SecuritySettings, TlsTermination,
    UnknownTlsTermination,
};
use crate::server::{
    BindAddress, BodyLimit, InvalidBindAddress, InvalidBound, InvalidTlsMaterial, RequestTimeout, ServerSettings,
};
use crate::sources::{InvalidSourceRegistry, RawSourceEntry, SourceRegistry};
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

pub use posture::NotFitToServe;

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

    /// Layers the files in a configuration directory: `base.yaml`, then `<environment>.yaml`.
    ///
    /// The counterpart to [`Self::with_overlay`] for a real directory. What makes it worth having
    /// beside [`Self::from_process_environment`] is what it does NOT do - it leaves the variable
    /// layer alone - so a caller that started from [`Self::defaults`] keeps the empty variable map,
    /// and a `SUTURA__*` variable in a developer's shell cannot change what the file layers resolve
    /// to.
    #[must_use]
    pub fn with_directory(mut self, directory: PathBuf) -> Self {
        self.directory = Some(directory);
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
    /// A `security.inbound` block exists and does not say which mode.
    ///
    /// **The refusal `docs/adr/0014` asks for by name.** Both defaults are wrong in opposite
    /// directions - `direct` makes a gateway deployment reject every caller, `behind-gateway` makes a
    /// directly exposed deployment accept a forged proof - so a deployment that says nothing does not
    /// start. Note what is *not* refused: no block at all, which is a single-player deployment and is
    /// unaffected by any of this.
    #[error(
        "`security.inbound` is set and `security.inbound.mode` is not. It has no default because \
         both would be wrong: `direct` makes a deployment behind a gateway reject every caller, and \
         `behind-gateway` makes a directly exposed one accept a proof anybody can forge. Write one \
         of: {}. Remove the whole block for a deployment with no per-caller identity",
        InboundIdentity::MODES.join(", ")
    )]
    InboundModeUndeclared,
    #[error("`security.inbound.mode` is `{found}` - one of: {}", InboundIdentity::MODES.join(", "))]
    InboundModeUnknown { found: String },
    /// A key this mode needs. The mode is in the message on purpose - see `required`.
    #[error("`security.inbound.mode` is `{mode}`, which requires `{key}`")]
    InboundKeyMissing { key: &'static str, mode: String },
    #[error("a value in `security.inbound` is not usable")]
    InboundValue {
        #[source]
        cause: InvalidInboundValue,
    },
    #[error("`security.inbound.algorithms` does not pin a usable set")]
    InboundAlgorithms {
        #[source]
        cause: InvalidAlgorithms,
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
    #[error("`catalog.kind` does not name a catalog this configuration can express")]
    CatalogKind {
        #[source]
        cause: UnknownCatalogKind,
    },
    #[error("`catalogs` holds a name that is not a catalog name")]
    CatalogName {
        #[source]
        cause: InvalidIdentifier,
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
    #[error("`security.identity` does not say which kind of deployment this is")]
    Identity {
        #[source]
        cause: InvalidDeploymentIdentity,
    },
    #[error("the `sources` tree is not usable")]
    Sources {
        #[source]
        cause: InvalidSourceRegistry,
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

/// Which configuration files were read, in the order they were applied.
///
/// **The answer to a question the resolved values cannot be asked.** Every file layer is optional, so
/// a mistyped configuration directory and a deployment with no files produce the same settings - and
/// the startup report described those settings in detail while naming no source, which is a report
/// that cannot distinguish "the operator's file is in effect" from "the operator's file was never
/// found". An operator reading a value they did not write has nothing to look at.
///
/// A type rather than a bare `Vec<PathBuf>` for one reason: [`Display`](std::fmt::Display) is the
/// single owner of the wording, including the empty case, so the startup log and the `prompt` command
/// cannot describe the same deployment differently.
///
/// **Paths only, and never a value.** A path is not a credential; a value can be one, and
/// `security.access_token` is set by exactly this mechanism. Nothing read out of a file reaches this
/// type - there is nowhere in it for a value to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLayers {
    files: Vec<PathBuf>,
}

impl ConfigLayers {
    /// The files that were found, in application order: `base.yaml`, then `<environment>.yaml`.
    #[inline]
    #[must_use]
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Did no file layer contribute anything?
    ///
    /// True for a deployment configured entirely by the embedded defaults and the environment, and
    /// equally true for one whose configuration directory is wrong. The two are indistinguishable
    /// here on purpose - that is the fact, and [`Display`](std::fmt::Display) says it plainly rather
    /// than leaving a blank field.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl core::fmt::Display for ConfigLayers {
    /// The layers in effect, or a sentence saying there are none.
    ///
    /// Written out rather than left to a `Debug` of an empty vector, because `[]` in a log line is
    /// read as "the field is not implemented yet" and not as "this process is running on defaults
    /// nobody wrote down".
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.files.is_empty() {
            return f.write_str("embedded defaults only");
        }
        for (index, path) in self.files.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", path.display())?;
        }
        Ok(())
    }
}

/// The whole resolved configuration.
///
/// `Clone` because it is held in the request state, and every field is either `Copy` or a small
/// owned value. `Debug` is safe to log in full: the only credential-shaped field is held in
/// [`sutura_domain::identity::Secret`], whose `Debug` redacts, and a test in [`crate::security`]
/// asserts that at struct depth.
#[derive(Debug, Clone)]
pub struct Settings {
    layers: ConfigLayers,
    environment: Environment,
    server: ServerSettings,
    security: SecuritySettings,
    rate_limit: RateLimitSettings,
    telemetry: TelemetrySettings,
    api: ApiSettings,
    catalogs: Catalogs,
    runtime: RuntimeSettings,
    prompt: PromptSettings,
    sources: SourceRegistry,
}

impl Settings {
    /// Reads every source, parses the result, and refuses a deployment it will not serve.
    ///
    /// The order is the contract: defaults, then `<dir>/base.yaml`, then
    /// `<dir>/<environment>.yaml`, then the variables. Later beats earlier, so a variable is the
    /// last word - which is exactly why [`Self::refusals`] runs on the parsed result rather than
    /// on any one layer.
    pub fn load(sources: &Sources) -> Result<Self, SettingsError> {
        let (raw, layers) = read(sources)?;
        let settings = Self::parse(&raw, sources.environment, layers)?;
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
    fn parse(raw: &RawSettings, environment: Environment, layers: ConfigLayers) -> Result<Self, SettingsError> {
        // Security before sources, and the order is a dependency rather than a habit: a shared source
        // in single-user mode borrows the mode's own declaration as its acknowledgement, so the mode
        // has to be parsed before the entry that may read it.
        let security = parse_security(raw)?;
        let sources = parse_sources(raw, security.identity())?;
        Ok(Self {
            layers,
            environment,
            server: parse_server(raw)?,
            security,
            rate_limit: parse_rate_limit(raw, environment)?,
            telemetry: parse_telemetry(raw, environment)?,
            api: ApiSettings::new(
                raw.api.docs.unwrap_or_else(|| ApiSettings::docs_default_for(environment)),
                raw.api.docs.is_some(),
            ),
            catalogs: parse_catalogs(raw)?,
            runtime: parse_runtime(raw)?,
            prompt: parse_prompt(raw)?,
            sources,
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
        refusals.extend(self.identity_refusals());
        refusals.extend(self.credential_refusals(off_host));
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

    /// Everything wrong with who this deployment says its queries run as.
    ///
    /// **This is the half of the boot check configuration can see, and only that half.** It reads the
    /// declared mode and the per-source acknowledgements - a parsed tree, nothing else - and returns
    /// typed refusals naming the source to change. Whether the *linked adapter* can carry a per-subject
    /// credential at all is a property of the build, so it is checked in the composition root beside
    /// `open_engine` and not here; and neither half belongs in `verify_and_validate`, which re-runs
    /// anchors and would be re-running a configuration check whose inputs a catalog reload cannot
    /// change.
    fn identity_refusals(&self) -> Vec<NotFitToServe> {
        let mut refusals = Vec::new();
        if self.sources.is_empty() {
            return refusals;
        }
        let Some(mode) = self.security.identity() else {
            // No mode, so there is no rule to apply per source: the missing declaration is the whole
            // finding, and listing every shared source underneath it would be noise on top of the one
            // thing to fix.
            refusals.push(NotFitToServe::DeploymentIdentityUndeclared {
                count: self.sources.count(),
            });
            return refusals;
        };
        if !mode.needs_per_source_acknowledgement() {
            return refusals;
        }
        // In multi-user mode a shared source's witness has to come from its own entry, so a source that
        // parsed with no identity at all is exactly the unacknowledged case - `SourceRegistry::parse`
        // had no other witness to reach for.
        for (alias, source) in self.sources.each() {
            if source.identity().is_none() {
                refusals.push(NotFitToServe::SharedSourceNotAcknowledged {
                    alias: String::from(alias.as_str()),
                });
            }
        }
        refusals
    }

    /// Everything wrong with what a request has to present, and with where it presents it.
    ///
    /// **Two rules that used to be one, and separating them is the change.** The deployment token was
    /// required whenever the service was reachable off-host or was in production, because the
    /// alternative was an unauthenticated way to read whatever the process can read. That argument is
    /// about there being *no* credential - and a deployment that verifies every caller's own token has
    /// one, per caller, audience-bound and expiring. So the requirement now reads "some credential",
    /// and the message still names which of the two reasons asked for it.
    ///
    /// The second rule is the collision: see [`NotFitToServe::DeploymentTokenSharesTheHeader`]. The
    /// two rules are here together rather than in two functions because they are one question asked
    /// twice - what does a request present, and can it present it - and a deployment that got the
    /// first wrong usually got the second wrong in the same edit.
    fn credential_refusals(&self, off_host: bool) -> Vec<NotFitToServe> {
        let mut refusals = Vec::new();
        let inbound = self.security.inbound();
        if self.security.access_token().is_none() && inbound.is_none() {
            // Two different reasons, and the message says which: an operator whose production
            // deployment refuses should not have to work out whether it was the bind or the
            // environment that asked for the token.
            if self.environment.is_production() {
                refusals.push(NotFitToServe::AccessTokenRequired {
                    because: "this is a production deployment with no inbound identity configured",
                });
            } else if off_host {
                refusals.push(NotFitToServe::AccessTokenRequired {
                    because: "this service is bound where other hosts can reach it and no inbound identity is configured",
                });
            }
        }
        // Asked of the requirement rather than of the variant, so a mode added later that also lands
        // in `Authorization` cannot slip past this.
        if self.security.access_token().is_some() && inbound.is_some_and(InboundIdentity::reads_the_authorization_header) {
            refusals.push(NotFitToServe::DeploymentTokenSharesTheHeader);
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

    /// Which configuration files this deployment is actually running on.
    ///
    /// Separate from every other accessor here in what it answers: the rest report a resolved value,
    /// and this reports where the values could have come from. A deployment whose configuration
    /// directory is wrong resolves exactly like one that has no directory, so this is the only thing
    /// in a `Settings` that can tell the two apart.
    #[inline]
    pub const fn layers(&self) -> &ConfigLayers {
        &self.layers
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

    /// The metadata sources this deployment reads, in declaration order.
    #[inline]
    pub const fn catalogs(&self) -> &Catalogs {
        &self.catalogs
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

    /// The data systems this deployment declared, keyed by the alias a model's `source:` names.
    ///
    /// Read by the composition root, which opens one adapter per entry, hands each the posture its
    /// entry declared, and refuses a deployment whose catalog names a source with no entry here.
    #[inline]
    pub const fn sources(&self) -> &SourceRegistry {
        &self.sources
    }
}

/// The raw tree, and which files contributed to it.
///
/// Named rather than written inline, because the pair is the thing: a tree without its provenance is
/// what [`read`] used to return, and the whole point of the change is that the two travel together.
type Layered = (RawSettings, ConfigLayers);

/// Builds the layered configuration and deserializes it into the raw tree.
///
/// **Returns which files were actually there, alongside the tree they produced.** Both file layers
/// are `.required(false)`, so a mistyped `--config` directory, a volume that failed to mount, and a
/// deployment that genuinely has no files are the same silent success - and the process then starts
/// on embedded defaults with nothing in the log to distinguish the three. What is returned here is
/// what [`ConfigLayers`] carries into the startup report.
///
/// **Files, and only files.** The overlay layer is supplied as text by a test and has no path, and the
/// variable layer has no path either - so neither can appear in the list, and a deployment configured
/// entirely by `SUTURA__*` variables reports no layers. That is the honest answer to "which files",
/// not a claim that nothing overrode the defaults.
fn read(sources: &Sources) -> Result<Layered, SettingsError> {
    let mut builder = config::Config::builder().add_source(config::File::from_str(DEFAULTS, config::FileFormat::Yaml));
    let mut layers = Vec::new();

    if let Some(directory) = sources.directory.as_deref() {
        for stem in [BASE_STEM, sources.environment.as_str()] {
            // `layer_path` is the only place a layer's filename is constructed, so what is reported
            // as found and what is added as a source cannot name different files.
            //
            // **The limit, stated where the claim is:** this records what was on disk at this
            // instant. A file that appears or disappears between here and the build, or one that
            // exists and cannot be read, is not covered - the first is a race nothing here closes,
            // and the second becomes `SettingsError::Source` a few lines below.
            let path = layer_path(directory, stem);
            if path.is_file() {
                layers.push(path);
            }
            builder = builder.add_source(optional_file(directory, stem));
        }
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
        .map(|raw| (raw, ConfigLayers { files: layers }))
        .map_err(|cause| SettingsError::Source { cause: Box::new(cause) })
}

/// The stem of the layer every environment reads first.
const BASE_STEM: &str = "base";

/// Where a layer's file would be. The one place a layer filename is spelled.
fn layer_path(directory: &Path, stem: &str) -> PathBuf {
    directory.join(format!("{stem}.yaml"))
}

/// A `<stem>.yaml` in `directory`, if it is there.
///
/// Optional rather than required, which is the opposite of what the reference project does, and
/// deliberately: the defaults here are embedded and complete, so a missing file means "nothing to
/// override" rather than "the deployment is half-configured". A required file would make the
/// binary unable to start without a filesystem it does not otherwise need.
///
/// The cost of that choice is that absence is indistinguishable from a wrong path, which is why
/// [`ConfigLayers`] exists: the posture stays fail-open and the log stops being silent about it.
fn optional_file(directory: &Path, stem: &str) -> config::File<config::FileSourceFile, config::FileFormat> {
    config::File::from(layer_path(directory, stem)).required(false)
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
    // **Absent is absent, and is not a third mode.** An empty string is the shape an unset variable
    // takes in a shell, so it reads the same way - and both are then a `NotFitToServe` if any source is
    // configured, which is where the refusal belongs: the check needs to see the `sources` tree, and a
    // parse error here could not name how many sources were left unaccounted for.
    let identity = match raw.security.identity.as_deref() {
        None | Some("") => None,
        Some(value) => Some(
            DeploymentIdentity::parse(value, raw.security.single_user_because.as_deref())
                .map_err(|cause| SettingsError::Identity { cause })?,
        ),
    };
    let inbound = match raw.security.inbound {
        None => None,
        Some(ref written) => Some(crate::settings::inbound::parse_inbound(written)?),
    };
    Ok(SecuritySettings::new(token, termination, inbound, identity))
}

/// The data systems this deployment declares.
///
/// The map's keys are the aliases, so this only has to put them beside their entries in a stable order
/// and let `SourceRegistry::parse` do the parsing. `BTreeMap` iteration is sorted, which is what makes
/// "an earlier entry" in the duplicate-alias refusal a deterministic phrase rather than one that
/// depends on how the file was written.
fn parse_sources(raw: &RawSettings, mode: Option<&DeploymentIdentity>) -> Result<SourceRegistry, SettingsError> {
    let entries: Vec<RawSourceEntry<'_>> = raw
        .sources
        .iter()
        .map(|(written, source)| RawSourceEntry {
            written,
            kind: &source.kind,
            data_dir: source.data_dir.as_deref(),
            billing_project: source.billing_project.as_deref(),
            dataset: source.dataset.as_deref(),
            credential_file: source.credential_file.as_deref(),
            max_bytes_billed: source.max_bytes_billed,
            posture: &source.posture,
            acknowledged_because: source.acknowledged_because.as_deref(),
            verification_identity: source.verification_identity.as_deref(),
            workload_identity: source.workload_identity.clone(),
        })
        .collect();
    SourceRegistry::parse(&entries, mode).map_err(|cause| SettingsError::Sources { cause })
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

fn parse_catalogs(raw: &RawSettings) -> Result<Catalogs, SettingsError> {
    let mut entries = Vec::with_capacity(raw.catalogs.len());
    for raw_catalog in &raw.catalogs {
        // `name` and `kind` are required here for the same reason a source's alias is: the
        // contribution manifest keys on the name and the composition root dispatches the kind, so
        // an entry that omits either is a declaration that cannot be opened. `kind` is parsed as a
        // closed set; an absent one was already defaulted by the raw shape.
        let name = SourceName::parse(&raw_catalog.name).map_err(|cause| SettingsError::CatalogName { cause })?;
        let kind = CatalogKind::parse(&raw_catalog.kind).map_err(|cause| SettingsError::CatalogKind { cause })?;
        let version = DefinitionVersion::parse(&raw_catalog.version).map_err(|cause| SettingsError::Version { cause })?;
        let settings = CatalogSettings::parse(
            name,
            kind,
            PathBuf::from(&raw_catalog.dir),
            PathBuf::from(&raw_catalog.data_dir),
            version,
        )
        .map_err(|cause| SettingsError::Catalog { cause })?;
        entries.push(settings);
    }
    Catalogs::parse(entries).map_err(|cause| SettingsError::Catalog { cause })
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
    // The one place the machine is asked about its memory. `parse` takes the answer rather than
    // probing for it, so the interesting case - a ceiling above what the process can reach - is
    // testable without a machine that has it. `available_memory_bytes` answers `None` where a
    // platform will not say, and that is recorded on the value rather than assumed away.
    let working_set = WorkingSetCeiling::parse(raw.runtime.working_set_max_bytes, crate::runtime::available_memory_bytes())
        .map_err(|cause| SettingsError::Bound { cause })?;
    let grace = ShutdownGrace::parse(raw.runtime.shutdown_grace_seconds).map_err(|cause| SettingsError::Bound { cause })?;
    Ok(RuntimeSettings::new(concurrency, admission, workers, working_set, grace))
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

/// Reading the inbound-identity declaration. Carved out because this file hit the line limit.
mod inbound;

/// The refusal vocabulary. Carved out for the same reason, along the seam this module's own
/// documentation names: the parse is here, the combination checks are there.
mod posture;

#[cfg(test)]
mod tests;
