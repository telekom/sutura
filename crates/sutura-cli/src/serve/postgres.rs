//! The `Postgres` half of this composition root: one declared source becomes an open `Warehouse`.
//!
//! **Split out of `main.rs` when this adapter's composition pushed that file past the unexemptable
//! 1000-line cap**, beside `bigquery`'s half and for the same reason. Both `open_postgres`
//! definitions are here - the one that opens the connection and the refusal for a build that linked
//! no adapter - because the compiler picks between them, and both callers compile against whichever
//! exists: `open_engine`'s single-kind arm and the mixed path's `kind::postgres_group`.
//!
//! **Opening is where a misdeclared channel fails, and it fails before the listener binds**: the
//! posture cross-check, the password file, the declared channel and its TLS material are all read
//! in `crate::postgres::build`, so a deployment cannot come up believing a channel is secured when
//! it is not.

/// Opens one `PostgreSQL` adapter per declared source, secured as the source declares.
///
/// Attaches nothing (the tables live in the database), which is the whole difference from the files
/// arm. The composition is `crate::postgres::build`, shared with `crate::sources`' root, so a fix to
/// it lands once and both roots get it.
#[cfg(feature = "postgres")]
pub(crate) fn open_postgres(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::PostgresSource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = crate::postgres::build(source, configured)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    engines
        .map(super::OpenedSources::Postgres)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the `Postgres` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, for the reason
/// `open_bigquery` gives at the same shape: every caller - `open_engine`'s single-kind arm and
/// `kind::postgres_group` - makes one call, and the compiler decides which of these it reaches,
/// keeping both signatures identical under `dead_code = "deny"`.
#[cfg(not(feature = "postgres"))]
pub(crate) fn open_postgres(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: postgres`, and this binary was built without the `postgres` \
         feature - so it links no Postgres adapter. Build `sutura-cli` with `--features postgres`, \
         or declare a `files` source"
    ))
}
