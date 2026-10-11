use std::time::{SystemTime, UNIX_EPOCH};

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Column, Definitions, Description, Model};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
use sutura_domain::model::{ColumnName, ModelName, QualifiedTable, SourceName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_exec_postgres::adbc::{Channel, FixtureAdmin, PostgresDriver};
use sutura_exec_postgres::fixture::FixtureCredential;

/// The catalog's declared name.
pub(crate) const CATALOG: &str = "dictionary";

/// The source alias the dictionary rows' models bind to - the `postgres` source's own name.
pub(crate) const SOURCE: &str = "warehouse";

/// The declared environment key the fixture rows carry.
pub(crate) const ENVIRONMENT: &str = "test";

/// The schema holding the `orders` table the rows describe. One for every run, so [`expected`]
/// needs no argument; [`install_documentation_schema`] creates it under an advisory lock, because
/// two concurrent `CREATE ... IF NOT EXISTS` of one name can both try and one then fails.
const ORDERS_SCHEMA: &str = "sutura_fixture_orders";

/// Installs the documentation-schema fixture: a per-run schema with a `columns` table whose
/// rows describe `ORDERS_SCHEMA.orders` (model `orders`, one primary-key column and one `amount`
/// column), bound to `ENVIRONMENT` and not soft-deleted - over the tier's published endpoint at
/// `host` and `port`, as the fixture role. The table itself exists too, because the load reads the
/// schema of every table a bundle names and refuses one the source lacks.
pub(crate) fn install_documentation_schema(host: &str, port: u16) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let schema = format!("sutura_dictionary_{nonce}_{}", std::process::id());
    let statement = format!(
        "SELECT pg_advisory_xact_lock(4242); \
         CREATE SCHEMA IF NOT EXISTS {ORDERS_SCHEMA}; \
         CREATE TABLE IF NOT EXISTS {ORDERS_SCHEMA}.orders (order_id bigint, amount numeric); \
         CREATE SCHEMA {schema}; \
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
           ('{ENVIRONMENT}', '{ORDERS_SCHEMA}', 'orders', 'orders', 'Customer orders.', \
            'order_id', 1, 'bigint', 'The order key.', true, false), \
           ('{ENVIRONMENT}', '{ORDERS_SCHEMA}', 'orders', 'orders', 'Customer orders.', \
            'amount', 2, 'numeric', 'The order total.', false, false)"
    );
    let source = SourceName::parse(CATALOG).expect("the catalog name parses");
    let conninfo = FixtureCredential::from_env()
        .unwrap_or_else(|unconfigured| panic!("{unconfigured}"))
        .conninfo(&source, host, port, Channel::Plaintext)
        .expect("the tier's published endpoint builds a connection string");
    let driver = PostgresDriver::from_host().expect("the tier is up, so a driver is named");
    FixtureAdmin::open(&driver, &conninfo)
        .expect("the tier opens")
        .run(&statement)
        .expect("the documentation-schema fixture installs");
    schema
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
        QualifiedTable::parse(format!("{ORDERS_SCHEMA}.orders")).expect("the fixture table parses"),
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
