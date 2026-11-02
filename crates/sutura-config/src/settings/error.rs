use sutura_domain::model::InvalidIdentifier;
use sutura_domain::pinned::InvalidVersion;
use sutura_domain::plan::{InvalidFederatedRowCeiling, InvalidPlanCeiling, InvalidRowCeiling};

use super::{ENVIRONMENT_VARIABLE, NotFitToServe};
use crate::catalog::{InvalidCatalogSettings, UnknownCatalogKind};
use crate::environment::UnknownEnvironment;
use crate::inbound::{InboundIdentity, InvalidAlgorithms, InvalidInboundValue};
use crate::limits::InvalidQuota;
use crate::prompt::{InvalidPromptSettings, UnknownCatalogProse};
use crate::proxy::{Cidr, InvalidTrustedProxy, UnknownClientAddressSource};
use crate::security::{InvalidAccessToken, InvalidDeploymentIdentity, InvalidOutbound, UnknownTlsTermination};
use crate::server::{InvalidBindAddress, InvalidBound, InvalidTlsMaterial, UnknownServerRole};
use crate::sources::InvalidSourceRegistry;
use crate::telemetry::{InvalidLogFilter, InvalidServiceName, UnknownLogFormat};
use crate::tools::InvalidRunSqlSources;

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
    /// A `security.outbound` block exists and is not usable.
    #[error("`security.outbound` is not usable")]
    Outbound {
        #[source]
        cause: InvalidOutbound,
    },
    /// `security.audience_mapping` names an audience that is not a usable identifier - `docs/adr/0028`.
    #[error("`security.audience_mapping` is not usable")]
    AudienceMapping {
        #[source]
        cause: crate::audience::InvalidAudienceMapping,
    },
    #[error("`server.allowed_hosts` holds an entry that is not a host name or an IP address")]
    AllowedHost {
        #[source]
        cause: crate::server::InvalidAllowedHost,
    },
    #[error("`server.role` does not name what this node is for")]
    Role {
        #[source]
        cause: UnknownServerRole,
    },
    #[error("`server.tls_certificate`, `server.tls_key` and `server.tls_node_authority` are not usable together")]
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
    /// A key the configured mode does not read. Refused rather than ignored, because an ignored
    /// audience or grant is a deployment that believes it says something it does not.
    #[error("`{key}` is read only when `security.inbound.mode` is `{mode}`")]
    InboundKeyNotForMode { key: &'static str, mode: &'static str },
    #[error("`security.inbound` names both `key_set_file` and `key_set_url` - one key set, one source")]
    InboundKeySetTwice,
    /// A configured grant of raw SQL. `run_sql` is enabled and acknowledged by name, never handed to
    /// every scopeless assertion a gateway signs.
    #[error(
        "`security.inbound.granted_scopes` names `sutura:sql.run`. Raw SQL is granted by a token's own \
         `scope`, never by configuration - remove it from the list"
    )]
    RawSqlGranted,
    #[error("a value in `security.inbound` is not usable")]
    InboundValue {
        #[source]
        cause: InvalidInboundValue,
    },
    /// `direct` with `token_type: any` and no by-name acceptance of it.
    #[error(
        "`security.inbound.token_type` is `any`, so any token the issuer signs for this audience is \
         accepted as an access token, an OIDC ID token included. Remove it for RFC 9068's `at+jwt`, \
         write the class the issuer mints, or set `security.inbound.accept_any_token_type: true`"
    )]
    InboundAnyTokenTypeNotAccepted,
    /// `security.inbound.address_claim` names a claim other than `email`, with no by-name acceptance.
    #[error(
        "`security.inbound.address_claim` is `{claim}`, so a `domain_delegation` source runs each caller \
         as the Google account that claim names, with no `email_verified` beside it. Set \
         `security.inbound.accept_address_claim: true` only if the issuer verifies the claim and users \
         cannot change it; otherwise remove the key"
    )]
    InboundAddressClaimNotAccepted { claim: crate::inbound::ClaimName },
    /// A transit lifetime above [`crate::inbound::ProofLifetime::SHORT_LIVED_SECONDS`] with no by-name acceptance of it.
    #[error(
        "`security.inbound.transit_max_lifetime_seconds` is {found}, above {limit}, and an assertion \
         replays for as long as it lives. Lower it, or set \
         `security.inbound.accept_long_transit_lifetime: true`"
    )]
    TransitLifetimeNotAccepted { found: u64, limit: u64 },
    /// A trusted-proxy block covering a whole address family, with no by-name acceptance of it.
    #[error(
        "`rate_limit.trusted_proxies` holds `{block}`, which trusts every address, so every caller's \
         `X-Forwarded-For` is believed. Name the proxy hops, or set \
         `rate_limit.accept_every_address_as_proxy: true`"
    )]
    EveryAddressAsProxyNotAccepted { block: Cidr },
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
    #[error("`governance.top_row_ceiling` is not usable")]
    RowCeiling {
        #[source]
        cause: InvalidRowCeiling,
    },
    #[error("`governance.federated_row_ceiling` is not usable")]
    FederatedRowCeiling {
        #[source]
        cause: InvalidFederatedRowCeiling,
    },
    #[error("`governance.source_ceiling` is not usable")]
    SourceCeiling {
        #[source]
        cause: InvalidPlanCeiling,
    },
    #[error("`governance.lookup_leg_ceiling` is not usable")]
    LookupLegCeiling {
        #[source]
        cause: InvalidPlanCeiling,
    },
    #[error("`governance.leg_ceiling` is not usable")]
    LegCeiling {
        #[source]
        cause: InvalidPlanCeiling,
    },
    #[error("`tools.run_sql.sources` is not usable")]
    RunSqlSources {
        #[source]
        cause: InvalidRunSqlSources,
    },
    /// The values are all well formed and the deployment they describe is one this service will not serve.
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
    /// The merged configuration nests more deeply than the origin walk descends.
    #[error("the configuration nests more deeply than {limit} levels (the walk stopped at depth {found})")]
    MergeDepth { found: u32, limit: u32 },
}
