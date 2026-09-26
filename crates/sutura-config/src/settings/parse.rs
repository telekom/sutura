//! The typed parse of each settings section.
//!
//! Split out of `settings.rs` when that file reached the line cap, along the seam the refusals
//! already drew: the parse here turns a raw tree into typed values, and `settings.rs` keeps the
//! public types (`Settings`, `Sources`, `SettingsError`) and the combination checks (`refusals`).
//! Nothing here is `pub` - these are helpers `Settings::parse` calls, moved to a sibling module so
//! the file that describes the posture is not also the file that parses a thousand lines of it.

use sutura_domain::plan::RowCeiling;

use super::SettingsError;
use crate::environment::Environment;
use crate::governance::SpendBudget;
use crate::limits::{Quota, RateLimitSettings};
use crate::prompt::{CatalogProse, InstructionsFile, InstructionsMaxBytes, PromptSettings};
use crate::proxy::{ClientAddressSource, TrustedProxies};
use crate::raw::RawSettings;
use crate::runtime::{AdmissionTimeout, EngineWorkers, QueryConcurrency, RuntimeSettings, ShutdownGrace, WorkingSetCeiling};
use crate::security::{AccessToken, DeploymentIdentity, SecuritySettings, TlsTermination};
use crate::server::{BindAddress, BodyLimit, RequestTimeout, ServerSettings};
use crate::sources::{RawSourceEntry, SourceRegistry};
use crate::telemetry::{LogFilter, LogFormat, ServiceName, TelemetrySettings};

pub(super) fn parse_server(raw: &RawSettings) -> Result<ServerSettings, SettingsError> {
    let bind = BindAddress::parse(&raw.server.host, raw.server.port).map_err(|cause| SettingsError::Bind { cause })?;
    let timeout = RequestTimeout::parse(raw.server.request_timeout_seconds).map_err(|cause| SettingsError::Bound { cause })?;
    let body = BodyLimit::parse(raw.server.max_body_bytes).map_err(|cause| SettingsError::Bound { cause })?;
    let tls = crate::server::TlsMaterial::parse(raw.server.tls_certificate.as_deref(), raw.server.tls_key.as_deref())
        .map_err(|cause| SettingsError::TlsMaterial { cause })?;
    Ok(ServerSettings::new(
        bind,
        timeout,
        body,
        tls,
        raw.server.agent_surface.enabled,
    ))
}

pub(super) fn parse_security(raw: &RawSettings) -> Result<SecuritySettings, SettingsError> {
    let token = match raw.security.access_token.as_deref() {
        // An empty string is the shape an unset variable takes in a shell, and treating it as a
        // configured token would give every request a 401 for a reason nothing explains.
        None | Some("") => None,
        Some(value) => Some(AccessToken::parse(value).map_err(|cause| SettingsError::AccessToken { cause })?),
    };
    let metrics_token = match raw.security.metrics_token.as_deref() {
        None | Some("") => None,
        Some(value) => Some(AccessToken::parse(value).map_err(|cause| SettingsError::MetricsToken { cause })?),
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
    let inbound = match &raw.security.inbound {
        None => None,
        Some(written) => Some(crate::settings::inbound::parse_inbound(written)?),
    };
    let credential_cache = crate::identity_cache::CredentialCacheSettings::parse(
        raw.security.credential_cache.enabled,
        raw.security.credential_cache.capacity,
        raw.security.credential_cache.window_seconds,
    )
    .map_err(|cause| SettingsError::CredentialCache { cause })?;
    // Deployment-wide, and parsed in `crate::settings::outbound` - absence is not a refusal, a
    // PRESENT empty block is. `outbound_identity` is the optional client pair beside the anchors
    // (`github.com/telekom/sutura#911`).
    let (outbound, outbound_identity) = crate::settings::outbound::parse_outbound(raw.security.outbound.as_ref())?;
    let audience_mapping = crate::audience::AudienceMapping::parse(raw.security.audience_mapping.clone())
        .map_err(|cause| SettingsError::AudienceMapping { cause })?;
    Ok(SecuritySettings::new(
        token,
        termination,
        inbound,
        identity,
        metrics_token,
        credential_cache,
        outbound,
        outbound_identity,
        audience_mapping,
    ))
}

/// The data systems this deployment declares.
///
/// The map's keys are the aliases, so this only has to put them beside their entries in a stable order
/// and let `SourceRegistry::parse` do the parsing. `BTreeMap` iteration is sorted, which is what makes
/// "an earlier entry" in the duplicate-alias refusal a deterministic phrase rather than one that
/// depends on how the file was written.
pub(super) fn parse_sources(raw: &RawSettings, mode: Option<&DeploymentIdentity>) -> Result<SourceRegistry, SettingsError> {
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
            host: source.host.as_deref(),
            unix_socket: source.unix_socket.as_deref(),
            port: source.port,
            database: source.database.as_deref(),
            service_name: source.service_name.as_deref(),
            user: source.user.as_deref(),
            password_file: source.password_file.as_deref(),
            transport_mode: source.transport_mode.as_deref(),
            transport_anchors: source.transport_anchors.as_deref(),
            client_certificate: source.client_certificate.as_deref(),
            client_key: source.client_key.as_deref(),
        })
        .collect();
    SourceRegistry::parse(&entries, mode).map_err(|cause| SettingsError::Sources { cause })
}

pub(super) fn parse_rate_limit(raw: &RawSettings, environment: Environment) -> Result<RateLimitSettings, SettingsError> {
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

pub(super) fn parse_telemetry(raw: &RawSettings, environment: Environment) -> Result<TelemetrySettings, SettingsError> {
    let name = ServiceName::parse(&raw.telemetry.service_name).map_err(|cause| SettingsError::Service { cause })?;
    let filter = LogFilter::parse(&raw.telemetry.filter).map_err(|cause| SettingsError::Filter { cause })?;
    let format = match raw.telemetry.format.as_deref() {
        None => LogFormat::default_for(environment),
        Some(value) => LogFormat::parse(value).map_err(|cause| SettingsError::Format { cause })?,
    };
    Ok(TelemetrySettings::new(name, filter, format, raw.telemetry.format.is_some()))
}

/// The concurrency bounds and the two deadlines that are not per-request.
///
/// Every one of these is a `parse` on a newtype rather than a raw number reaching the runtime,
/// which is what makes an unusable value a refusal at startup instead of a surprise under load.
/// `engine_worker_threads` is the one that may be absent: `EngineWorkers::parse` resolves `None`
/// to what the machine can run and records that nobody chose it, so the startup log can say which.
pub(super) fn parse_runtime(raw: &RawSettings) -> Result<RuntimeSettings, SettingsError> {
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

/// The per-replica spend ceiling, if this deployment declared one.
///
/// `None` when `governance.per_replica_spend_ceiling` is absent - `docs/adr/0030`'s decision that
/// no key means no ceiling, which is every deployment's behaviour before this key existed.
pub(super) fn parse_spend_budget(raw: &RawSettings) -> Result<Option<SpendBudget>, SettingsError> {
    raw.governance
        .per_replica_spend_ceiling
        .as_ref()
        .map(|ceiling| SpendBudget::parse(ceiling.bytes, ceiling.window_seconds).map_err(|cause| SettingsError::Bound { cause }))
        .transpose()
}

/// The `top` row ceiling this deployment certifies over - `RowCeiling::DEFAULT` when
/// `governance.top_row_ceiling` is absent, which is every deployment's behaviour before this key
/// existed (`github.com/telekom/sutura#777`).
pub(super) fn parse_row_ceiling(raw: &RawSettings) -> Result<RowCeiling, SettingsError> {
    raw.governance.top_row_ceiling.map_or_else(
        || Ok(RowCeiling::DEFAULT),
        |rows| RowCeiling::parse(rows).map_err(|cause| SettingsError::RowCeiling { cause }),
    )
}

/// The keys that shape the agent-facing prompt and bound its operator section.
///
/// **An empty `instructions_file` is an error here, and that is the opposite of what
/// [`parse_security`] does with an empty token.** The two empties mean different things. An empty
/// access token treated as configured would answer every request `401` for a reason nothing
/// explains, so absent is the safe reading. An empty *path* resolves to the process working
/// directory - a different directory on every host and never the one the operator meant - and
/// absence is already expressible by removing the key, so here the safe reading is a refusal naming
/// the key. It is the argument `catalogs[].dir` already makes.
///
/// `catalog_prose` is branched on rather than required, so a deployment that removed the key from
/// its own copy of the defaults gets the default rather than a deserialization failure.
/// Infallible: a boolean has no invalid form. Named as its own function anyway, matching the other
/// groups, so `Settings::parse` reads as one list of "read this section" calls rather than one
/// inline and the rest not.
pub(super) const fn parse_tools(raw: &RawSettings) -> crate::tools::ToolsSettings {
    crate::tools::ToolsSettings::new(raw.tools.run_sql.enabled)
}

pub(super) fn parse_prompt(raw: &RawSettings) -> Result<PromptSettings, SettingsError> {
    let instructions = match raw.prompt.instructions_file.as_deref() {
        None => None,
        Some(value) => Some(InstructionsFile::parse(value).map_err(|cause| SettingsError::Prompt { cause })?),
    };
    let prose = match raw.prompt.catalog_prose.as_deref() {
        None => CatalogProse::default(),
        Some(value) => CatalogProse::parse(value).map_err(|cause| SettingsError::CatalogProse { cause })?,
    };
    let max_bytes = raw.prompt.instructions_max_bytes.map_or_else(
        || Ok(InstructionsMaxBytes::DEFAULT),
        |bytes| InstructionsMaxBytes::parse(bytes).map_err(|cause| SettingsError::Prompt { cause }),
    )?;
    Ok(PromptSettings::with_max_bytes(instructions, max_bytes, prose).listing_physical_schema(raw.prompt.list_physical_schema))
}
