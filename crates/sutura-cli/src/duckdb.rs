//! The ONE `DuckDB` composition, shared by both of this crate's composition roots - `crate::oracle`'s
//! shape, for its reason: `build` below is the only place a declared `duckdb` entry becomes an open
//! `sutura_domain::warehouse::Warehouse`, so the posture cross-check and the read-only open cannot
//! differ between `sutura serve` and `sutura query`/`mcp`.
//!
//! **The file is opened read-only, always** - `sutura_exec_duckdb::READ_ONLY`, then
//! `THEN_LOCKED`, which is also what a raw statement on this source runs under. No settings key opens it writable.
//!
//! The WHOLE module is behind `#[cfg(feature = "duckdb")]` at its declaration in `main.rs`.

use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_duckdb::DuckDbWarehouse;

use crate::commands::render;

/// Builds one `DuckDB` adapter over the declared file, after checking this build can deliver the
/// source's posture.
///
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; the
/// `impersonation-at-source` posture, which this adapter has nowhere to put; no driver to open; or a
/// file that does not open read-only - one that is not there included, since nothing is created.
pub(crate) fn build(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<DuckDbWarehouse, String> {
    let sutura_config::SourcePlacement::Duckdb { ref database_file } = *configured.placement() else {
        return Err(format!(
            "`sources.{source}` reached the DuckDB open step with a placement no DuckDB adapter \
             reads, which the dispatcher should have sent elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    identity
        .posture()
        .deliverable_by(DuckDbWarehouse::IMPERSONATION, source)
        .map_err(|cause| render(&cause))?;
    DuckDbWarehouse::open(
        source.clone(),
        identity.posture().clone(),
        database_file,
        working_set.result_budget(),
    )
    .map_err(|cause| format!("`sources.{source}` did not open: {}", render(&cause)))
}
