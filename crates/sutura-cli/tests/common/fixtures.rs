use std::time::{SystemTime, UNIX_EPOCH};

/// The catalog's declared name.
pub(crate) const CATALOG: &str = "dictionary";

/// The source alias the dictionary rows' models bind to - the `postgres` source's own name.
pub(crate) const SOURCE: &str = "warehouse";

/// The declared environment key the fixture rows carry.
pub(crate) const ENVIRONMENT: &str = "test";

/// Installs the documentation schema on a Postgres tier for the rdbms catalog fixture.
pub(crate) fn install_documentation_schema(config: &tokio_postgres::Config) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let schema = format!("sutura_dictionary_{nonce}_{}", std::process::id());
    let statement = format!(
        "CREATE SCHEMA {schema}; \
         CREATE TABLE {schema}.columns ( \
           environment text NOT NULL, \
           catalog_name text, \
           schema_name text NOT NULL, \
           table_name text NOT NULL, \
           model_name text NOT NULL, \
           table_description text, \
           column_name text NOT NULL, \
           column_ordinal int NOT NULL, \
           column_type text, \
           column_description text, \
           is_primary_key boolean, \
           is_deleted boolean NOT NULL \
         ); \
         INSERT INTO {schema}.columns \
           (environment, schema_name, table_name, model_name, table_description, \
            column_name, column_ordinal, column_type, column_description, is_primary_key, is_deleted) \
         VALUES \
           ('{ENVIRONMENT}', 'public', 'orders', 'orders', 'Customer orders.', \
            'order_id', 1, 'bigint', 'The order key.', true, false), \
           ('{ENVIRONMENT}', 'public', 'orders', 'orders', 'Customer orders.', \
            'amount', 2, 'numeric', 'The order total.', false, false)"
    );
    run_sql(config, &statement);
    schema
}

/// Executes a SQL statement on the tier.
pub(crate) fn run_sql(config: &tokio_postgres::Config, statement: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime builds");
    let (client, connection) = runtime
        .block_on(config.connect(tokio_postgres::NoTls))
        .expect("the tier opens");
    runtime.spawn(async move {
        #[expect(
            clippy::let_underscore_must_use,
            clippy::let_underscore_untyped,
            reason = "the connection driver task's own error has no caller in this setup path"
        )]
        let _ = connection.await;
    });
    runtime
        .block_on(client.batch_execute(statement))
        .expect("the documentation-schema fixture installs");
}
