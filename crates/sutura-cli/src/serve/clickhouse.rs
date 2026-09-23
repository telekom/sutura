//! The `ClickHouse` half of this composition root: one declared source becomes an open
//! `Warehouse`.
//!
//! **Its own file beside `bigquery`'s and `postgres`' halves**, for the reason those two state: the
//! dispatcher stays in the composition root and each kind's open-and-refuse pair lives in a file of
//! its own, under the unexemptable 1000-line cap. Both `open_clickhouse` definitions are here - the
//! one that opens the adapter and the refusal for a build that linked none - because the compiler
//! picks between them at the one call site.
//!
//! **What is NOT here is the per-source BUILD**, and that is the difference from its two siblings:
//! it lives once in `crate::clickhouse`, shared with `crate::sources`' own root, so the posture
//! cross-check, the credential read and the channel resolution cannot differ between the two
//! composition roots. `crates/sutura-cli/src/sources/postgres.rs` names that sharing as what issue
//! 121 asks for and does not have for `postgres`; this kind arrives with it.

/// Opens one `ClickHouse` adapter per declared source, over its HTTP interface, under the declared
/// credential.
///
/// **Nothing is attached and nothing is registered** - the tables live in the database, the same
/// difference from `open_files` that `open_postgres` documents. What this function does instead is
/// everything that can fail before a listener is bound, through [`crate::clickhouse::build`]: the
/// posture cross-check, the password file and the TLS material.
#[cfg(feature = "clickhouse")]
pub(crate) fn open_clickhouse(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::ClickHouseSource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = crate::clickhouse::build(source, configured)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns. Written as a fallback for
    // the reason `open_files` gives - the workspace denies `unwrap` and `expect`.
    engines
        .map(super::OpenedSources::ClickHouse)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the `ClickHouse` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, for the reason
/// `open_bigquery` gives at the same shape: the dispatcher has exactly one call and the compiler
/// decides which of these it reaches, keeping both signatures identical under `dead_code = "deny"`.
#[cfg(not(feature = "clickhouse"))]
pub(crate) fn open_clickhouse(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: clickhouse`, and this binary was built without the `clickhouse` \
         feature - so it links no ClickHouse adapter. Build `sutura-cli` with \
         `--features clickhouse`, or declare a `files` source"
    ))
}
