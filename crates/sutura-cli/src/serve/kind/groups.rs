//! Each kind's group of a mixed deployment, opened through that kind's own single-kind opener and
//! erased into [`AnyWarehouse`] - split out of `kind` for the line cap. Each feature-off twin calls
//! the opener only for the refusal naming the feature that would link it.

use sutura_domain::model::SourceName;

use super::AnyWarehouse;

/// The `BigQuery` group, opened through [`crate::serve::bigquery::open_bigquery`] and erased.
#[cfg(feature = "bigquery")]
pub(super) fn bigquery_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let crate::serve::OpenedSources::BigQuery(engines) =
        crate::serve::bigquery::open_bigquery(sources, registry, request_timeout, outbound)?
    else {
        return Err(String::from(
            "`open_bigquery` returned an arm this dispatcher does not expect",
        ));
    };
    Ok(engines.into_mapped(|engine| AnyWarehouse::BigQuery(Box::new(engine))))
}

/// The feature-off twin: `open_bigquery`'s own refusal always fires here, naming the missing
/// feature - propagated rather than duplicated, so there is one place that names it.
#[cfg(not(feature = "bigquery"))]
pub(super) fn bigquery_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    crate::serve::bigquery::open_bigquery(sources, registry, request_timeout, outbound)?;
    // Unreachable: that call's own feature-off twin only ever returns `Err`, propagated above by
    // `?`. Written as a fallback rather than an `unreachable!` the workspace denies.
    Err(String::from(
        "`open_bigquery` returned an open registry on a build with no BigQuery adapter linked",
    ))
}

/// The `Postgres` group, opened through [`crate::serve::postgres::open_postgres`] and erased.
#[cfg(feature = "postgres")]
pub(super) fn postgres_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let crate::serve::OpenedSources::Postgres(engines) = crate::serve::postgres::open_postgres(sources, registry)? else {
        return Err(String::from(
            "`open_postgres` returned an arm this dispatcher does not expect",
        ));
    };
    Ok(engines.into_mapped(|engine| AnyWarehouse::Postgres(Box::new(engine))))
}

/// [`bigquery_group`]'s feature-off twin, for the same reason.
#[cfg(not(feature = "postgres"))]
pub(super) fn postgres_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    crate::serve::postgres::open_postgres(sources, registry)?;
    // Unreachable, for `bigquery_group`'s feature-off twin's exact reason.
    Err(String::from(
        "`open_postgres` returned an open registry on a build with no Postgres adapter linked",
    ))
}

/// The `ClickHouse` group, opened through [`crate::serve::clickhouse::open_clickhouse`] and erased.
#[cfg(feature = "clickhouse")]
pub(super) fn clickhouse_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let crate::serve::OpenedSources::ClickHouse(engines) = crate::serve::clickhouse::open_clickhouse(sources, registry, runtime)?
    else {
        return Err(String::from(
            "`open_clickhouse` returned an arm this dispatcher does not expect",
        ));
    };
    Ok(engines.into_mapped(|engine| AnyWarehouse::ClickHouse(Box::new(engine))))
}

/// [`bigquery_group`]'s feature-off twin, for the same reason.
#[cfg(not(feature = "clickhouse"))]
pub(super) fn clickhouse_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    crate::serve::clickhouse::open_clickhouse(sources, registry, runtime)?;
    // Unreachable, for `bigquery_group`'s feature-off twin's exact reason.
    Err(String::from(
        "`open_clickhouse` returned an open registry on a build with no ClickHouse adapter linked",
    ))
}

/// The Oracle group, opened through [`crate::serve::oracle::open_oracle`] and erased.
#[cfg(feature = "oracle")]
pub(super) fn oracle_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let crate::serve::OpenedSources::Oracle(engines) = crate::serve::oracle::open_oracle(sources, registry, runtime)? else {
        return Err(String::from("`open_oracle` returned an arm this dispatcher does not expect"));
    };
    Ok(engines.into_mapped(|engine| AnyWarehouse::Oracle(Box::new(engine))))
}

/// [`bigquery_group`]'s feature-off twin, for the same reason.
#[cfg(not(feature = "oracle"))]
pub(super) fn oracle_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    crate::serve::oracle::open_oracle(sources, registry, runtime)?;
    // Unreachable, for `bigquery_group`'s feature-off twin's exact reason.
    Err(String::from(
        "`open_oracle` returned an open registry on a build with no Oracle adapter linked",
    ))
}

/// The `DuckDB` group, opened through [`crate::serve::duckdb::open_duckdb`] and erased.
#[cfg(feature = "duckdb")]
pub(super) fn duckdb_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let crate::serve::OpenedSources::Duckdb(engines) = crate::serve::duckdb::open_duckdb(sources, registry, runtime)? else {
        return Err(String::from("`open_duckdb` returned an arm this dispatcher does not expect"));
    };
    Ok(engines.into_mapped(|engine| AnyWarehouse::Duckdb(Box::new(engine))))
}

/// [`bigquery_group`]'s feature-off twin, for the same reason.
#[cfg(not(feature = "duckdb"))]
pub(super) fn duckdb_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    crate::serve::duckdb::open_duckdb(sources, registry, runtime)?;
    // Unreachable, for `bigquery_group`'s feature-off twin's exact reason.
    Err(String::from(
        "`open_duckdb` returned an open registry on a build with no DuckDB adapter linked",
    ))
}

/// The Cube group, opened through [`crate::serve::cube::open_cube`] and erased.
#[cfg(feature = "cube")]
pub(super) fn cube_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let crate::serve::OpenedSources::Cube(engines) = crate::serve::cube::open_cube(sources, registry, outbound)? else {
        return Err(String::from("`open_cube` returned an arm this dispatcher does not expect"));
    };
    Ok(engines.into_mapped(|engine| AnyWarehouse::Cube(Box::new(engine))))
}

/// [`bigquery_group`]'s feature-off twin, for the same reason.
#[cfg(not(feature = "cube"))]
pub(super) fn cube_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    crate::serve::cube::open_cube(sources, registry, outbound)?;
    // Unreachable, for `bigquery_group`'s feature-off twin's exact reason.
    Err(String::from(
        "`open_cube` returned an open registry on a build with no Cube adapter linked",
    ))
}
