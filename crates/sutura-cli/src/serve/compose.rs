//! The erased service each arm of `run` serves, built over the combiner `run` chose.

use std::sync::Arc;

use sutura_config::{Settings, StaticCredentialBroker};
use sutura_exec_datafusion::DataFusionCombiner;
use sutura_runtime::{banner, telemetry};

use super::Serving;

pub(super) fn install_telemetry(settings: &Settings) -> Result<(), String> {
    telemetry::install(settings.telemetry()).map_err(super::flatten)?;
    sutura_runtime::install_panic_hook();
    banner::announce(settings);
    Ok(())
}

/// Loads the catalogs a second time through their ports, composes them, verifies every anchor, and
/// erases the adapter.
/// Generic in the adapter AND the broker, and the second generic is what lets the two arms below
/// differ: a `files` deployment has no impersonating source, so its broker is the static one; a
/// `bigquery` deployment gets `DeclaredPrincipalBroker`, presenting each subject's own verified
/// assertion rather than exchanging anything itself - the exchanging broker is deleted. Returning
/// `Arc<dyn Surface>` is what lets the shared lines after each arm stop caring which of those it
/// was - the transport takes a trait object, so the monomorphisation ends here rather than through
/// the router.
///
/// **The service itself is built by [`crate::catalog::start_composed`] - the ONE
/// `LocalService::start_composed` match both composition roots dispatch**, and issue #970 moved
/// it out of `serve` to the crate root so `sutura mcp` opens `catalogs:` the same way. What is
/// left here, and what `crate::mcp` deliberately does NOT do, is the erasure `crate::serve`'s
/// transport needs (`Arc<dyn Surface>`) and the agent surface's does not (that one is generic
/// over a sized `S: Surface`).
pub(super) fn started<W, B>(
    catalogs: &crate::catalog::OpenedCatalogs,
    engines: sutura_app::Warehouses<W>,
    broker: B,
    combiner: DataFusionCombiner,
    gate: Option<sutura_app::surface::AdoptionGate<W>>,
    settings: &Settings,
) -> Result<Serving, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
    B: sutura_domain::identity::CredentialBroker + Send + Sync + 'static,
    B::Error: Send + Sync,
{
    crate::catalog::start_composed(catalogs, engines, broker, combiner, settings).map(|service| match gate {
        Some(gate) => Arc::new(service.with_adoption_gate(gate)) as Serving,
        None => Arc::new(service) as Serving,
    })
}

/// The service a mix of kinds answers through.
///
/// **One broker per ANSWER, so the choice is per BUILD and not per kind.** A mix may read a shared
/// source of one kind and an impersonating one of another, and `build_broker` scans the whole
/// registry rather than one kind's entries - so "does this mix need the principal broker" is exactly
/// "does this build link an adapter that can deliver one". With no impersonating source declared it
/// holds the same shared map the static broker would, and refuses the same sources. With no such
/// adapter linked, no adapter in this build declares `PerSubjectCredential` and every impersonating
/// entry is already refused at its own posture cross-check, so the static broker is the whole truth.
pub(super) fn mixed<W>(
    catalogs: &crate::catalog::OpenedCatalogs,
    engines: sutura_app::Warehouses<W>,
    combiner: DataFusionCombiner,
    gate: Option<sutura_app::surface::AdoptionGate<W>>,
    settings: &Settings,
    #[cfg_attr(
        not(any(
            feature = "bigquery",
            feature = "clickhouse",
            feature = "oracle",
            feature = "cube",
            feature = "foundry",
            feature = "node"
        )),
        expect(unused_variables, reason = "only a principal broker dials an identity provider")
    )]
    outbound: Option<&sutura_tls::Declared>,
) -> Result<Serving, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    #[cfg(any(
        feature = "bigquery",
        feature = "clickhouse",
        feature = "oracle",
        feature = "cube",
        feature = "foundry",
        feature = "node"
    ))]
    let broker = super::broker::build_broker(settings.sources(), outbound)?;
    #[cfg(not(any(
        feature = "bigquery",
        feature = "clickhouse",
        feature = "oracle",
        feature = "cube",
        feature = "foundry",
        feature = "node"
    )))]
    let broker = StaticCredentialBroker::from_registry(settings.sources());
    started(catalogs, engines, broker, combiner, gate, settings)
}

/// The service for every shape whose adapter cannot carry a per-subject credential at all.
///
/// **One function rather than the same four lines in five arms**, and the argument is one sentence
/// for all of them: `DataFusionWarehouse`, `AdbcPostgres` and `DuckDbWarehouse` each declare
/// `ImpersonationCapability::NoPlaceForASubject`, each composition
/// root refuses an `impersonation-at-source` entry at the posture cross-check before opening one,
/// and so the only identity a question is answered under is the one this process holds.
/// `sutura_config::StaticCredentialBroker` is exactly that: it reads the `sources:` tree this root
/// already parsed, every source declared `shared-service-user` is served as itself, nothing is ever
/// exchanged - and a source the broker holds nothing for is refused as `credential_unavailable`
/// rather than answered as this process.
///
/// The `Mixed` arm reaches it too, for a mix that opened no `BigQuery` source.
///
/// # Errors
///
/// Whatever [`started`] refuses while loading the catalogs a second time and re-running the anchors.
pub(super) fn shared_identity_service<W>(
    catalogs: &crate::catalog::OpenedCatalogs,
    engines: sutura_app::Warehouses<W>,
    combiner: DataFusionCombiner,
    gate: Option<sutura_app::surface::AdoptionGate<W>>,
    settings: &Settings,
) -> Result<Serving, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    started(
        catalogs,
        engines,
        StaticCredentialBroker::from_registry(settings.sources()),
        combiner,
        gate,
        settings,
    )
}
