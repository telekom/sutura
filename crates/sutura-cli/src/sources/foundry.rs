//! The Foundry half of the command root: one declared ontology becomes an open `Warehouse`.

use sutura_domain::model::SourceName;

use crate::sources::Opened;

/// Opens one Foundry SQL source under its declared shared deployment token.
#[cfg(feature = "foundry")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
    outbound: Option<&sutura_tls::Declared>,
    working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<Opened, String> {
    let engine = crate::foundry::build(source, configured, outbound, working_set)?;
    Ok(Opened::Foundry(super::OpenedWith {
        engines: sutura_app::Warehouses::of(engine),
        attached: None,
        broker: sutura_config::StaticCredentialBroker::from_registry(registry),
    }))
}

/// The refusal for a build that did not link the Foundry SQL adapter.
#[cfg(not(feature = "foundry"))]
pub(super) fn open(
    source: &SourceName,
    _configured: &sutura_config::ConfiguredSource,
    _registry: &sutura_config::SourceRegistry,
    _outbound: Option<&sutura_tls::Declared>,
    _working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<Opened, String> {
    Err(format!(
        "`sources.{source}` is `kind: foundry`, and this binary was built without the `foundry` feature - so it links no Foundry SQL adapter. Build `sutura-cli` with `--features foundry`, or declare a `files` source"
    ))
}
