//! Error mapping for the two optional statement ports of the heterogeneous registry.

macro_rules! statement {
    ($warehouse:expr, $method:ident, $($arg:expr),*) => {
        match $warehouse {
            AnyWarehouse::Files(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(AnyWarehouseError::Files)),
            #[cfg(feature = "bigquery")]
            AnyWarehouse::BigQuery(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(AnyWarehouseError::BigQuery)),
            #[cfg(feature = "postgres")]
            AnyWarehouse::Postgres(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(AnyWarehouseError::Postgres)),
            #[cfg(feature = "clickhouse")]
            AnyWarehouse::ClickHouse(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(AnyWarehouseError::ClickHouse)),
            #[cfg(feature = "oracle")]
            AnyWarehouse::Oracle(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(AnyWarehouseError::Oracle)),
            #[cfg(feature = "duckdb")]
            AnyWarehouse::Duckdb(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(AnyWarehouseError::Duckdb)),
            #[cfg(feature = "cube")]
            AnyWarehouse::Cube(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(AnyWarehouseError::Cube)),
            #[cfg(feature = "node")]
            AnyWarehouse::Node(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(AnyWarehouseError::Node)),
            #[cfg(all(test, feature = "bigquery"))]
            AnyWarehouse::Recorded(warehouse) => warehouse
                .$method($($arg),*)
                .map(|result| result.map_err(|never| match never {})),
        }
    };
}

pub(super) use statement;
