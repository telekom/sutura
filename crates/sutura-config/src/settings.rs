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
use std::path::PathBuf;

use sutura_domain::model::InvalidIdentifier;
use sutura_domain::pinned::InvalidVersion;

use crate::api::ApiSettings;
use crate::catalog::{Catalogs, InvalidCatalogSettings, UnknownCatalogKind};
use crate::environment::{Environment, UnknownEnvironment};
use crate::governance::SpendBudget;
use crate::inbound::{InboundIdentity, InvalidAlgorithms, InvalidInboundValue};
use crate::limits::{InvalidQuota, RateLimitSettings};
use crate::prompt::{InvalidPromptSettings, PromptSettings, UnknownCatalogProse};
use crate::proxy::{InvalidTrustedProxy, UnknownClientAddressSource};
use crate::raw::RawSettings;
use crate::runtime::RuntimeSettings;
use crate::security::{
    DeploymentIdentity, InvalidAccessToken, InvalidDeploymentIdentity, InvalidOutbound, SecuritySettings, UnknownTlsTermination,
};
use crate::server::{InvalidBindAddress, InvalidBound, InvalidTlsMaterial, ServerSettings};
use crate::sources::{InvalidSourceRegistry, SourceRegistry};
use crate::telemetry::{InvalidLogFilter, InvalidServiceName, TelemetrySettings, UnknownLogFormat};

/// The variable that chooses the deployment environment.
///
/// One name, exported so a startup message and the documentation cannot disagree about it.
pub const ENVIRONMENT_VARIABLE: &str = "SUTURA_ENVIRONMENT";

/// The variable that points at the configuration directory a load layers files from.
///
/// **One name, because both `sutura query`/`sutura mcp` and `sutura serve` read it and neither may own it** - exported for the same reason
/// [`ENVIRONMENT_VARIABLE`] is, so a startup message, a command's `--help` and the documentation
/// cannot disagree about a name none of them owns.
pub const CONFIG_DIR_VARIABLE: &str = "SUTURA_CONFIG_DIR";

/// The prefix every configuration variable carries, and the separator between key segments.
///
/// `SUTURA__SERVER__PORT` sets `server.port`. Two underscores for both, so a key segment that
/// itself contains an underscore - `access_token`, `max_body_bytes` - needs no escaping.
pub const VARIABLE_PREFIX: &str = "SUTURA";
/// The separator between nested key segments in a configuration variable name.
pub const VARIABLE_SEPARATOR: &str = "__";

use layers::read;
pub use layers::{ConfigLayers, SettingsLoadError};
pub use posture::{NotFitToServe, TokenRequiredBy};

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
    /// The counterpart to [`Self::with_overlay`] for a real directory: it leaves the variable
    /// layer alone, so a caller that started from [`Self::defaults`] keeps the empty variable map
    /// and a `SUTURA__*` in a developer's shell cannot change what the files resolve to.
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

/// Reads the configuration directory from the process, if one was named.
///
/// **An empty value is the same as an absent one, deliberately.** A container platform that
/// templates `SUTURA_CONFIG_DIR` from an unset field sets it to the empty string, and
/// `PathBuf::from("")` layers `base.yaml` relative to whatever the working directory happens to be -
/// which is a file nobody wrote resolving somewhere nobody chose. The absence is the safe reading:
/// the embedded defaults are complete.
///
/// Not fallible, and that is not a shortcut: unlike [`environment_from_process`] there is no
/// permissive branch to fall into. A non-Unicode path is still a path this process can open, so it
/// is carried through as an `OsString` rather than refused.
#[must_use]
pub fn config_dir_from_process() -> Option<PathBuf> {
    std::env::var_os(CONFIG_DIR_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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
    #[error("`security.metrics_token` is not usable as a token")]
    MetricsToken {
        #[source]
        cause: InvalidAccessToken,
    },
    #[error("`security.tls_termination` does not name where TLS is terminated")]
    TlsTermination {
        #[source]
        cause: UnknownTlsTermination,
    },
    #[error("`security.credential_cache` is not usable")]
    CredentialCache {
        #[source]
        cause: crate::identity_cache::InvalidCredentialCacheSettings,
    },
    /// A `security.outbound` block exists and is not usable.
    #[error("`security.outbound` is not usable")]
    Outbound {
        #[source]
        cause: InvalidOutbound,
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
    #[error("`catalogs.{catalog}.version` is not a definition version")]
    Version {
        catalog: String,
        #[source]
        cause: InvalidVersion,
    },
    #[error("`catalogs.{catalog}.kind` does not name a catalog this configuration can express")]
    CatalogKind {
        catalog: String,
        #[source]
        cause: UnknownCatalogKind,
    },
    #[error("`catalogs.{written}` is not a catalog name")]
    CatalogName {
        written: String,
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
    tools: crate::tools::ToolsSettings,
    sources: SourceRegistry,
    spend_budget: Option<SpendBudget>,
}

impl Settings {
    /// Reads every source, parses the result, and refuses a deployment it will not serve.
    ///
    /// The order is the contract: defaults, then `<dir>/base.yaml`, then
    /// `<dir>/<environment>.yaml`, then the variables. Later beats earlier, so a variable is the
    /// last word - which is exactly why [`Self::refusals`] runs on the parsed result rather than
    /// on any one layer.
    pub fn load(sources: &Sources) -> Result<Self, SettingsLoadError> {
        let (raw, layers) = read(sources)?;
        // At most two paths: retain file context if parsing fails without cloning settings.
        let settings =
            Self::parse(&raw, sources.environment, layers.clone()).map_err(|reason| SettingsLoadError::new(layers, reason))?;
        let refusals = settings.refusals();
        if refusals.is_empty() {
            Ok(settings)
        } else {
            Err(SettingsLoadError::new(
                settings.layers,
                SettingsError::NotFitToServe { refusals },
            ))
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
        let security = parse::parse_security(raw)?;
        let sources = parse::parse_sources(raw, security.identity())?;
        Ok(Self {
            layers,
            environment,
            server: parse::parse_server(raw)?,
            security,
            rate_limit: parse::parse_rate_limit(raw, environment)?,
            telemetry: parse::parse_telemetry(raw, environment)?,
            api: ApiSettings::new(
                raw.api.docs.unwrap_or_else(|| ApiSettings::docs_default_for(environment)),
                raw.api.docs.is_some(),
            ),
            catalogs: catalogs::parse_catalogs(raw)?,
            runtime: parse::parse_runtime(raw)?,
            prompt: parse::parse_prompt(raw)?,
            tools: parse::parse_tools(raw),
            spend_budget: parse::parse_spend_budget(raw)?,
            sources,
        })
    }

    /// The tool surface's own settings - which capability beside the certified one this deployment
    /// turned on.
    #[inline]
    #[must_use]
    pub const fn tools(&self) -> &crate::tools::ToolsSettings {
        &self.tools
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
            // The key the refusal names, and the layer that supplied it. `layers` retained the
            // provenance `read` used to drop, so the operator is told WHICH variable or file set
            // the off-host bind rather than being handed the list of candidates #386 left them.
            let origin = self
                .layers
                .origin_of("server.host")
                .map_or_else(|| String::from("embedded defaults"), |o| o.describe("server.host"));
            refusals.push(NotFitToServe::TlsTerminationUndeclared { bind, origin });
        }
        refusals.extend(self.tls_refusals());
        refusals.extend(self.keying_refusals());
        refusals.extend(self.identity_refusals());
        refusals.extend(self.run_sql_refusals());
        refusals.extend(self.credential_refusals(off_host));
        // Keyed exactly like `metrics_refusals` below it: an unbounded caller is an unbounded
        // aggregate over the same history whether the deployment is labelled `production` or is
        // simply reachable from other hosts. `EphemeralPortInProduction` stays production-only -
        // port 0 off-host is the normal read-the-port-back shape a test uses.
        if (off_host || self.environment.is_production()) && !self.rate_limit.enabled() {
            refusals.push(NotFitToServe::RateLimitingDisabled {
                because: if self.environment.is_production() {
                    TokenRequiredBy::Production
                } else {
                    TokenRequiredBy::OffHost
                },
            });
        }
        if self.environment.is_production() && bind.port() == 0 {
            refusals.push(NotFitToServe::EphemeralPortInProduction);
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
                refusals.push(NotFitToServe::SharedSourceNotAcknowledged { alias: alias.clone() });
            }
        }
        refusals
    }

    /// Whether the raw SQL tool is enabled over a deployment it may not run over -
    /// `docs/adr/0013`'s boot refusal, reusing the mode `identity_refusals` already reads.
    ///
    /// **What this crate can check, and no more.** Whether the LINKED adapter can actually accept a
    /// raw statement, or carries a per-subject credential, is a property of the composed binary -
    /// `sutura_domain::warehouse::Warehouse::ACCEPTS_RAW_STATEMENTS` and `::IMPERSONATION` - which
    /// this crate never links. So this refuses `multi-user` unconditionally rather than only where an
    /// adapter's declared shape makes it unsafe: today no build links an adapter that is both raw-
    /// capable and per-subject-credential-capable, so the two questions have the same answer. The day
    /// one exists, this refusal needs a composition-root counterpart the way the shared-source
    /// acknowledgement check already has one.
    fn run_sql_refusals(&self) -> Vec<NotFitToServe> {
        if self.tools.run_sql_enabled() && self.security.identity() == Some(&DeploymentIdentity::SubjectPerRequest) {
            return vec![NotFitToServe::RunSqlEnabledInMultiUserMode];
        }
        Vec::new()
    }

    /// Everything wrong with what a request has to present, and with where it presents it.
    ///
    /// **Two rules that used to be one, and separating them is the change.** The deployment token was
    /// required whenever the service was reachable off-host or was in production, because the
    /// alternative was an unauthenticated way to read whatever the process can read. That argument is
    /// about there being *no* credential - and a deployment that verifies every caller's own token has
    /// one, per caller, audience-bound and expiring. So the requirement now reads "some credential",
    /// and the message still names which of the two reasons asked for it.
    fn credential_refusals(&self, off_host: bool) -> Vec<NotFitToServe> {
        let mut refusals = Vec::new();
        let inbound = self.security.inbound();
        if self.security.access_token().is_none() && inbound.is_none() {
            // Two different reasons, and the message says which: an operator whose production
            // deployment refuses should not have to work out whether it was the bind or the
            // environment that asked for the token.
            if self.environment.is_production() {
                refusals.push(NotFitToServe::AccessTokenRequired {
                    because: TokenRequiredBy::Production,
                });
            } else if off_host {
                refusals.push(NotFitToServe::AccessTokenRequired {
                    because: TokenRequiredBy::OffHost,
                });
            }
        }
        // Asked of the requirement rather than of the variant, so a mode added later that also lands
        // in `Authorization` cannot slip past this.
        if self.security.access_token().is_some() && inbound.is_some_and(InboundIdentity::reads_the_authorization_header) {
            refusals.push(NotFitToServe::DeploymentTokenSharesTheHeader);
        }
        // The metrics token is a SECOND credential for a DIFFERENT surface, and the separation
        // `docs/adr/0015` Decision 1 exists for. Three rules, each a silent collapse otherwise.
        refusals.extend(self.metrics_refusals(off_host));
        refusals
    }

    /// Everything wrong with the metrics credential's separation, or none of it.
    ///
    /// `docs/adr/0015` Decision 1: the metrics endpoint is gated by its own token, never the
    /// deployment's, so a scrape cannot interrogate the business. Three startup refusals each
    /// prevent a silent collapse of that separation.
    fn metrics_refusals(&self, off_host: bool) -> Vec<NotFitToServe> {
        let mut refusals = Vec::new();
        let Some(metrics) = self.security.metrics_token() else {
            // The metrics endpoint is always mounted on the one listener, so a deployment that is
            // reachable off-host or is production has an unauthenticated way to read its counters.
            // The same argument `AccessTokenRequired` makes for the API applies to whatever the
            // process can read, which for `/metrics` is the deployment's own counters.
            if off_host || self.environment.is_production() {
                refusals.push(NotFitToServe::MetricsTokenRequired {
                    because: if self.environment.is_production() {
                        TokenRequiredBy::Production
                    } else {
                        TokenRequiredBy::OffHost
                    },
                });
            }
            return refusals;
        };
        // Collision: one token gating both surfaces is the exact privilege escalation the
        // separation exists to prevent, and nothing at runtime would show it.
        if self.security.access_token().is_some_and(|api| api.equals(metrics)) {
            refusals.push(NotFitToServe::MetricsTokenSharesTheApiToken);
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

    /// The per-replica, in-process spend ceiling, if this deployment configured one.
    ///
    /// `None` is a real answer and not an unset field: `docs/adr/0030` decides that absence means
    /// this replica counts nothing and refuses nothing on this account, which is the behaviour
    /// every deployment had before this key existed.
    #[inline]
    pub const fn spend_budget(&self) -> Option<SpendBudget> {
        self.spend_budget
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

/// The typed parse of each settings section. Carved out because this file hit the line limit, along
/// the seam this module's own documentation names: the parse is here, the combination checks are
/// there.
mod parse;

/// Reading the inbound-identity declaration. Carved out because this file hit the line limit.
mod inbound;

mod catalogs;
/// Reading the outbound trust declaration. Carved out for the same reason.
mod outbound;

mod layers;

/// The refusal vocabulary. Carved out for the same reason, along the seam this module's own
/// documentation names: the parse is here, the combination checks are there.
mod posture;

/// Which `SUTURA__*` variables a PROCESS has, for a refusal that has to name them. Carved out
/// because this file hit the line limit again, on the seam the two above use: nothing there parses
/// or refuses anything, it reads an environment and filters names.
mod overlay;

pub use overlay::configuration_variables_from_process;

#[cfg(test)]
mod tests;
