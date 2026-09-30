use std::time::{SystemTime, UNIX_EPOCH};

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Column, Definitions, Description, Model};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
use sutura_domain::model::{ColumnName, ModelName, QualifiedTable, SourceName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

/// The catalog's declared name.
pub(crate) const CATALOG: &str = "dictionary";

/// The source alias the dictionary rows' models bind to - the `postgres` source's own name.
pub(crate) const SOURCE: &str = "warehouse";

/// The declared environment key the fixture rows carry.
pub(crate) const ENVIRONMENT: &str = "test";

/// Installs the documentation-schema fixture: a per-run schema with a `columns` table whose
/// rows describe `public.orders` (model `orders`, one primary-key column and one `amount`
/// column), bound to `ENVIRONMENT` and not soft-deleted.
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

fn run_sql(config: &tokio_postgres::Config, statement: &str) {
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

/// The bundle a cell expects, built independently of the binary under test from the same
/// documentation-schema fixture [`install_documentation_schema`] writes - never from a reply, so a
/// reader that silently drops or corrupts what it measures (a description, a column, a type, the
/// primary key) reddens the digest comparison, not only the "the field is present" checks a lone
/// non-empty-digest assertion left standing. `version` is the caller's own, because each harness
/// declares its own.
pub(crate) fn expected(version: &str) -> PinnedDefinitions {
    let columns = [
        ("order_id", "bigint", "The order key."),
        ("amount", "numeric", "The order total."),
    ]
    .into_iter()
    .map(|(name, data_type, description)| {
        Column::from_metadata(
            ColumnName::parse(name).expect("a fixture column name parses"),
            Some(data_type),
            Some(description),
            None,
        )
        .expect("a fixture column description parses")
    });
    let model = Model::new(
        ModelName::parse("orders").expect("the fixture model name parses"),
        SourceName::parse(SOURCE).expect("the fixture source alias parses"),
        QualifiedTable::parse("public.orders").expect("the fixture table parses"),
        columns,
        Description::parse("Customer orders.").expect("the fixture description parses"),
    )
    .with_primary_key([ColumnName::parse("order_id").expect("the fixture primary-key column name parses")])
    .expect("the fixture primary key names one of the model's own columns");
    let declared = MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure]).and_may_provide([
            DefinitionKind::Descriptions,
            DefinitionKind::Relationships,
            DefinitionKind::ColumnTypes,
            DefinitionKind::ColumnDescriptions,
        ]),
        KnowledgeCapabilities::none(),
    );
    PinnedDefinitions::pin(
        DefinitionVersion::parse(version).expect("the declared version parses"),
        Definitions::assemble(vec![model], vec![], vec![]).expect("the fixture definitions assemble"),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse(CATALOG).expect("the catalog name parses"),
            Contribution::of(declared),
        ),
    )
    .expect("the expected bundle hashes")
}
