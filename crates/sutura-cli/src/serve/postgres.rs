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

/// The service a `postgres`-only deployment answers through, under the static broker: the adapter
/// has nowhere for a subject's credential to arrive.
///
/// Where a peer `server.nodes` declares reads one of its sources, the adapters are erased into
/// `kind::AnyWarehouse` and named for splitting as a mix's are (`nodes::splitting`), so a split
/// lookup leg of that source is granted to its readers. Otherwise each adapter is served itself and
/// reads every leg whole.
///
/// **Limit:** erased, a source answers the enum's associated constants rather than its own, as a
/// mix's does (`kind`'s header). `AdbcPostgres` declares `IMPERSONATION` as the enum does, and
/// `EXECUTES_LEGS` and `ACCEPTS_RAW_STATEMENTS`, which the enum forwards per instance; it leaves the
/// rest at the port's default, so today both registries answer alike. A constant it later declares
/// reads as the default here, and only review checks that.
#[cfg(feature = "postgres")]
pub(super) fn served(
    catalogs: &crate::catalog::OpenedCatalogs,
    engines: sutura_app::Warehouses<super::PostgresSource>,
    combiner: sutura_exec_datafusion::DataFusionCombiner,
    settings: &sutura_config::Settings,
) -> Result<super::Serving, String> {
    let tls = settings.server().tls();
    if !super::nodes::read_by_a_peer(&engines, tls) {
        return super::compose::shared_identity_service(catalogs, engines, combiner, None, settings);
    }
    let erased = engines.into_mapped(|postgres| super::kind::AnyWarehouse::Postgres(Box::new(postgres)));
    let engines = super::nodes::splitting(erased, tls, settings.runtime());
    super::compose::shared_identity_service(catalogs, engines, combiner, None, settings)
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
