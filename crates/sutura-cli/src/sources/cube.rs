//! The Cube half of the command root: one declared deployment becomes an open `Warehouse`.
//!
//! **`oracle.rs`'s shape, as thin and for its reason**: the per-source BUILD lives once in
//! `crate::cube`, shared with `crate::serve`'s own root. What is here is this root's own two things -
//! the registry it wraps the adapter in, and the refusal for a build that linked no adapter.

use sutura_domain::model::SourceName;

use crate::sources::Opened;
#[cfg(feature = "cube")]
use crate::sources::OpenedWith;

/// Opens one Cube source, under the identity the deployment declared.
///
/// **Nothing is attached** - the cubes live in the deployment - so [`OpenedWith::attached`] is `None`.
///
/// # Errors
///
/// Everything [`crate::cube::build`] refuses.
#[cfg(feature = "cube")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<Opened, String> {
    let engine = crate::cube::build(source, configured, outbound)?;
    Ok(Opened::Cube(OpenedWith {
        engines: sutura_app::Warehouses::of(engine),
        attached: None,
        broker: sutura_config::StaticCredentialBroker::from_registry(registry),
    }))
}

/// The refusal for a build that linked no Cube adapter - `oracle.rs`'s twin shape.
#[cfg(not(feature = "cube"))]
pub(super) fn open(
    source: &SourceName,
    _configured: &sutura_config::ConfiguredSource,
    _registry: &sutura_config::SourceRegistry,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<Opened, String> {
    Err(format!(
        "`sources.{source}` is `kind: cube`, and this binary was built without the `cube` feature - \
         so it links no Cube adapter. Build `sutura-cli` with `--features cube`, or declare a `files` \
         source"
    ))
}
