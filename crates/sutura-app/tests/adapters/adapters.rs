//! The adapters under test: how one is registered, and which ones are.
//!
//! **This module is the plurality claim, written as data.** `SemanticCatalog` and `Warehouse` exist so
//! that a metadata provider and a data system are choices rather than facts about the code. A suite
//! that reached for one adapter by name in every test proved the opposite: it proved that those two
//! work, and it made a third one a test edit. So [`registered`] is the only place an adapter is named,
//! and every corpus in this crate's tests is expanded once per entry.
//!
//! Registering an adapter is two things and neither of them is a test:
//!
//! 1. an `impl` of [`CatalogUnderTest`] or [`DataSystemUnderTest`] - what it is called, and how to
//!    open it over the example corpus;
//! 2. one line in [`registered`].
//!
//! Declared by `golden.rs` and `differential.rs` with `#[path = "adapters/adapters.rs"] mod adapters;`
//! and living in a self-named `tests/adapters/adapters.rs` rather than `tests/adapters.rs` at the
//! root, so cargo does not build it as a test target of its own. It is shared by every test target
//! here, which is why nothing in it is target-specific: `unused_imports` and `dead_code` are both
//! `deny` in the workspace lint table, so an item only one target used would fail the build of the
//! other. The fakes live in `tests/support/support.rs`, which one target includes.
//!
//! # The axes, and why each artefact sits on the one it does
//!
//! | Axis | What it decides | What is expanded over it |
//! | --- | --- | --- |
//! | catalog | the `Definitions` and the digest | the pinned bundle, every plan, every compile-side refusal |
//! | dialect | the rendered statement | the SQL and its parameters, the parse check, the quoting and no-injection assertions |
//! | data system | the rows | the anchor check, the executed corpus, `dry_run` acceptance, agreement with the engine |
//!
//! A plan is a function of the definitions and not of the dialect, and rows are a function of
//! neither. Putting an artefact on the wrong axis is what produced three byte-identical copies of
//! every plan snapshot, one per dialect, before this module existed.
//!
//! # What is deliberately NOT registered
//!
//! The fakes. `support::HandWrittenCatalog` is the oracle every registered catalog is compared
//! against, and `support::RecordingWarehouse` and `support::CertifiedNumbers` execute nothing. A
//! registry entry is something somebody could deploy; a list of Rust literals and a warehouse that
//! returns one null cell are not. Registering either would put a cell in the execution matrix that
//! cannot execute, and a cell that cannot fail reads as coverage.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use sutura_dev::discovery::Endpoint;
use sutura_dev::provisioned::{self, Provisioned};
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};
use sutura_domain::query::Query;
use sutura_domain::warehouse::Warehouse;
use sutura_exec_bigquery::BigQueryWarehouse;
use sutura_exec_bigquery::adbc::{AdbcBigQuery, BytesBilledCeiling, DriverLocation, Impersonation};
use sutura_exec_bigquery::transport::{DatasetId, ProjectId};
use sutura_exec_postgres::fixture::FixtureCredential;

/// The version the goldens are pinned under.
///
/// Fixed, not derived from the working tree. A version that moved between runs would put a new value
/// in every snapshot that carries provenance, and then no snapshot would mean anything.
///
/// **Deliberately not the string `crates/sutura-cli/tests/example.rs` stamps over the same
/// directory.** The two suites read the same bytes and pin different things, so one shared constant
/// would couple two snapshot sets that have no reason to move together. Keeping them apart costs
/// nothing: the digest is taken over the parsed definitions and does not include the version, so both
/// suites still pin the same digest for the same catalog, which is the number that would matter if
/// they ever disagreed about it.
const VERSION: &str = "golden-fixture-1";

/// The one data system every model in the example catalog declares.
const SOURCE: &str = "local";

/// The corpus: the catalog documents, the CSVs and the questions this suite reads.
///
/// **One directory, two purposes, and no second copy of either.** `examples/single-player` is what a
/// reader is told to run, and it is also the corpus this suite expands over every registered adapter -
/// the same bytes in both roles. A quickstart that stopped working therefore fails this build rather
/// than failing the next person who tried it, and a catalog kept only for the tests cannot drift from
/// the one the README shows, because there is no second one to drift.
///
/// It reaches OUTSIDE this crate, which is the one thing worth flagging to whoever reads the paths
/// below. The source filter in `flake.nix` names `crates/*/tests` and `examples/` separately and a nix
/// build sees only what it keeps, so this suite now depends on BOTH clauses: dropping either one makes
/// it compile and find nothing, which is green over an empty corpus and only in CI.
fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
}

fn catalog_root() -> PathBuf {
    example_root().join("catalog")
}

pub(crate) fn data_root() -> PathBuf {
    example_root().join("data")
}

pub(crate) fn source() -> sutura_domain::model::SourceName {
    sutura_domain::model::SourceName::parse(SOURCE).expect("the example source name is a name")
}

/// The posture every registered data system in this matrix is opened with.
///
/// **Part of the registration, and it belongs here rather than on each adapter** - the mode is
/// configuration and this file is the matrix's configuration. Every registered adapter happens to
/// declare the same *capability* (none can carry a per-subject credential), but that is a fact about
/// them and not the reason this value is shared: a deployment of either one is legitimately
/// `shared-service-user`, because one process reads a file or holds a connection under one
/// operating-system identity.
///
/// When an adapter that CAN impersonate is registered, this is the line that grows a second value -
/// which is what makes adding it a registration rather than a test edit.
pub(crate) fn posture() -> sutura_domain::source::SourcePosture {
    sutura_domain::source::SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse(
                "the example corpus is a directory of CSVs read in this process under one identity",
            )
            .expect("the fixture reason is a reason"),
        ),
    }
}

/// The materialisation budget every row-speaking data system in this matrix is opened with - a
/// gibibyte, like the engine's own ceiling, for the reason that datafusion impl gives. The corpus
/// is a few hundred rows, so no question comes near the bound; the bound's own assertions live in
/// each adapter's own suite.
pub(crate) fn result_budget() -> sutura_domain::warehouse::ResultBudget {
    sutura_domain::warehouse::ResultBudget::of_bytes(
        core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive"),
    )
}

/// What one leg of an answer in this matrix executes as.
///
/// Read off [`posture`] rather than written again, so the credential a leg presents and the posture
/// the adapter was opened with cannot drift apart - which is exactly the disagreement each adapter's
/// exhaustive match on what it received exists to catch.
pub(crate) fn presented() -> sutura_domain::identity::Presented {
    match posture() {
        sutura_domain::source::SourcePosture::SharedServiceUser { declared } => {
            sutura_domain::identity::Presented::SharedServiceUser { declared }
        }
        sutura_domain::source::SourcePosture::ImpersonationAtSource => {
            panic!("every adapter in this matrix is opened shared, one function above")
        }
    }
}

/// The port's deadline every adapter in this matrix executes under - a generous budget, since
/// nothing in this suite is about time.
pub(crate) fn deadline() -> sutura_domain::warehouse::deadline::Deadline {
    sutura_domain::warehouse::deadline::Deadline::opened_at(
        std::time::Instant::now(),
        sutura_domain::warehouse::deadline::Budget::parse(std::time::Duration::from_secs(30))
            .expect("thirty seconds is a budget"),
    )
}

/// Never returned: this broker mints from a constant.
#[derive(Debug, thiserror::Error)]
#[error("the matrix's credential broker cannot fail")]
pub(crate) struct BrokerCannotFail;

/// The credential broker every question in this matrix is answered with.
///
/// **Part of the registration, for the reason [`posture`] is:** which credential a leg presents is
/// configuration, and this file is the matrix's configuration. It grants what [`presented`] says,
/// which is what every registered adapter can execute with - and when an adapter that CAN
/// impersonate is registered, this and `posture` are the two lines that grow a second value.
pub(crate) struct GrantsWhatTheMatrixDeclares;

impl sutura_domain::identity::CredentialBroker for GrantsWhatTheMatrixDeclares {
    type Error = BrokerCannotFail;

    #[expect(
        clippy::unwrap_in_result,
        reason = "the map is built from the same source set it is checked against, so a failure there \
                  is a broken fixture rather than an input to handle"
    )]
    fn mint(
        &self,
        context: &sutura_domain::identity::RequestContext,
        sources: &sutura_domain::identity::SourceSet,
    ) -> Result<sutura_domain::identity::Minted, Self::Error> {
        let mut presented_by_source = std::collections::BTreeMap::new();
        for name in sources.iter() {
            drop(presented_by_source.insert(name.clone(), presented()));
        }
        let credentials = sutura_domain::identity::LegCredentials::minted(
            context.chain().subject().clone(),
            sutura_domain::identity::Expiry::NothingExpires,
            sources,
            presented_by_source,
        )
        .expect("the map is built from the same source set, so it covers it");
        Ok(sutura_domain::identity::Minted::Granted { credentials })
    }
}

/// The broker every call to `sutura_app::answer` in this suite passes.
pub(crate) const fn shared_credential() -> GrantsWhatTheMatrixDeclares {
    GrantsWhatTheMatrixDeclares
}

/// The request context every call to `sutura_app::answer` in this suite passes.
///
/// `Subject::TheDeploymentItself`, which is the honest value: this suite has no transport, so nothing
/// established a caller. What the credential port changed for this corpus is that the CREDENTIAL is
/// explicit - not who asked.
pub(crate) fn a_caller() -> sutura_domain::identity::RequestContext {
    sutura_domain::identity::RequestContext::of(sutura_domain::identity::PrincipalChain::of(
        sutura_domain::identity::Subject::TheDeploymentItself,
    ))
}

pub(crate) fn version() -> DefinitionVersion {
    DefinitionVersion::parse(VERSION).expect("the pinned version is a version")
}

/// Every question in the corpus, in sorted order.
///
/// Sorted so the corpus is a function of the directory rather than of the filesystem: a suite whose
/// order changes between runs produces snapshot churn that has nothing to do with the change under
/// review.
pub(crate) fn questions() -> Vec<PathBuf> {
    let dir = example_root().join("questions");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the questions directory is there")
        .map(|entry| entry.expect("a directory entry is readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no questions under {}", dir.display());
    found
}

pub(crate) fn read_question(path: &Path) -> Query {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    serde_norway::from_str(&text).unwrap_or_else(|e| panic!("{} is not a question: {e}", path.display()))
}

/// The stem of a question file, which is what a snapshot is named after.
pub(crate) fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| String::from("unnamed"), |s| s.to_string_lossy().into_owned())
}

// ------------------------------------------------------------------------ registering an adapter ---

/// A `SemanticCatalog` adapter this suite runs its corpus through.
///
/// **A trait rather than a pair of loose functions, on purpose.** The supertrait bound is the port: an
/// entry cannot claim to register something that does not implement `SemanticCatalog`. The name is an
/// associated constant rather than a string a test passes in, so the name a snapshot carries and the
/// adapter it came from cannot disagree.
pub(crate) trait CatalogUnderTest: SemanticCatalog + Sized {
    /// The name this adapter's tests and snapshots carry.
    const NAME: &'static str;

    /// Builds it over the example catalog.
    fn open() -> Self;
}

impl CatalogUnderTest for sutura_catalog_local::LocalCatalog {
    const NAME: &'static str = "markdown";

    fn open() -> Self {
        Self::new(source(), catalog_root(), version())
    }
}

/// The first DECLARING catalog: `DataHub`, opened over its recorded fixture corpus.
///
/// Its corpus is NOT the example markdown - a metadata service over HTTP has no directory of YAML to
/// share - so this opens over the crate's own recorded aspects, which is what a `sources.<alias>`-per
/// platform deployment reads. The three universal cells hold because the fixture bundle is measured
/// against the adapter's declaration, which is the whole point of the `declaring` path.
impl CatalogUnderTest for sutura_catalog_datahub::DataHubCatalog<sutura_catalog_datahub::fixture::FixtureReader> {
    const NAME: &'static str = "datahub";

    fn open() -> Self {
        sutura_catalog_datahub::fixture::over_fixture_source(source(), version())
    }
}

/// The richest measured metadata source: `OpenMetadata`, opened over its recorded fixture corpus.
///
/// Its corpus is NOT the example markdown - a metadata service over HTTP has no directory of YAML to
/// share - so this opens over the crate's own recorded documents, which is what a `sources.<alias>`-
/// per-service deployment reads. It supplies the physical model, descriptions and declared
/// non-duplicating joins and declares the metric/grain/cardinality as may-provide kinds; a metric
/// whose measure is an expression string is reported-not-defined. The universal cells hold because
/// the fixture bundle is measured against the adapter's declaration.
impl CatalogUnderTest for sutura_catalog_openmetadata::OpenMetadataCatalog<sutura_catalog_openmetadata::fixture::FixtureReader> {
    const NAME: &'static str = "openmetadata";

    fn open() -> Self {
        sutura_catalog_openmetadata::fixture::over_fixture_source(source(), version())
    }
}

/// The narrowest metadata source: `Rdbms`, opened over its recorded dictionary corpus.
///
/// Like `datahub`, its corpus is NOT the example markdown - a database dictionary is not a directory
/// of YAML either - so this opens over the crate's own recorded corpus, which is what a
/// `sources.<alias>`-per database deployment reads. The universal cells hold because the dictionary
/// bundle is measured against the adapter's declaration, which is the whole point of the `declaring`
/// path. It provides no measure at all - the narrowest declaration - so it answers no certified
/// question, which the declare cells assert rather than the golden cells (it gets no golden cell).
impl CatalogUnderTest for sutura_catalog_rdbms::RdbmsCatalog<sutura_catalog_rdbms::fixture::FixtureReader> {
    const NAME: &'static str = "rdbms";

    fn open() -> Self {
        sutura_catalog_rdbms::fixture::over_fixture_source(source(), version())
    }
}

/// The second on-disk catalog: `Okf`, opened over a directory of Table Schema descriptors.
///
/// Like `markdown` its corpus is on disk, but it is NOT the example markdown - OKF Table Schema is a
/// different vocabulary from a directory of `kind:`-tagged documents, so it opens over its own
/// descriptors under `examples/okf`. The universal cells hold because the descriptor bundle is
/// measured against the adapter's declaration, which is the whole point of the `declaring` path.
impl CatalogUnderTest for sutura_catalog_okf::OkfCatalog {
    const NAME: &'static str = "okf";

    fn open() -> Self {
        Self::new(source(), okf_root(), version())
    }
}

/// The third-on-disk catalog: `DataContract`, opened over a directory of ODCS v3 contracts.
///
/// Like `markdown` and `okf` its corpus is on disk, but it is NOT the example markdown - ODCS is a
/// different vocabulary from a directory of `kind:`-tagged documents, so it opens over its own
/// contracts under `examples/datacontract`. The universal cells hold because the contract bundle is
/// measured against the adapter's declaration, which is the whole point of the `declaring` path.
impl CatalogUnderTest for sutura_catalog_datacontract::DataContractCatalog {
    const NAME: &'static str = "datacontract";

    fn open() -> Self {
        Self::new(source(), datacontract_root(), version())
    }
}

/// The directory of ODCS v3 contract documents this suite opens the `datacontract` catalog over.
fn datacontract_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/datacontract")
}

/// The directory of OKF Table Schema descriptors this suite opens the `okf` catalog over.
fn okf_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/okf")
}

/// A `Warehouse` adapter this suite executes the example corpus against.
///
/// [`open`] takes the bundle because attaching a table per model is what makes a data system able to
/// answer anything, and only the bundle knows which tables there are. It is on the trait rather than
/// in each test for the reason the trait exists at all: two copies of "open a data system over the
/// example CSVs" is two things to keep in step, and they did not stay in step.
///
/// [`open`]: DataSystemUnderTest::open
pub(crate) trait DataSystemUnderTest: Warehouse + Sized {
    /// The name this adapter's tests and snapshots carry.
    const NAME: &'static str;

    /// Opens it as the data system `name`, with each `(table, csv)` attached.
    ///
    /// The one open every entry writes, so a cell that needs a SECOND source - the two-fact ratio's
    /// lookup on its own data system - opens the same adapter the corpus cells do.
    fn open_on(name: SourceName, tables: Vec<(TableName, PathBuf)>) -> Self;

    /// Opens it with one table attached per model in `pinned`.
    fn open(pinned: &PinnedDefinitions) -> Self {
        Self::open_on(source(), fixture_tables(pinned))
    }

    /// Whether this adapter can execute HERE at all.
    ///
    /// Defaults to `true`, the truth for the in-process adapters; a network adapter reports whether
    /// its venue is up. When it is `false` a cell skips - through [`runs_here`] alone, which refuses
    /// a skip `exemptions::EXEMPTIONS` does not name. For a tier, `SUTURA_DEV_REQUIRE_TIER` (read once by the
    /// provisioner) turns that skip into a failure wherever a gate provisioned one.
    fn available() -> bool {
        true
    }
}

#[path = "exemptions.rs"]
pub(crate) mod exemptions;
pub(crate) use exemptions::runs_here;

impl DataSystemUnderTest for sutura_exec_datafusion::DataFusionWarehouse {
    const NAME: &'static str = "datafusion";

    fn open_on(name: SourceName, tables: Vec<(TableName, PathBuf)>) -> Self {
        // A gibibyte, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` - written as a
        // literal rather than read from that crate, because this suite must not give `sutura-app` a
        // dependency on the settings tree to obtain one number. The corpus is a few hundred rows, so
        // no question in it comes near the bound; what this passes on is the shape a deployment gets,
        // and the bound's own assertions live in the adapter's `pool.rs`.
        let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
        let engine = Self::new(name, posture(), sutura_exec_datafusion::WorkingSet::of_bytes(ceiling))
            .expect("an in-process engine starts");
        for (table, csv) in tables {
            engine
                .attach_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("the engine could not attach {}: {e}", csv.display()));
        }
        engine
    }
}

impl DataSystemUnderTest for sutura_exec_duckdb::DuckDbWarehouse {
    const NAME: &'static str = "duckdb";

    fn open_on(name: SourceName, tables: Vec<(TableName, PathBuf)>) -> Self {
        let warehouse = Self::in_memory(name, posture(), result_budget()).expect("an in-memory database opens");
        for (table, csv) in tables {
            warehouse
                .attach_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("duckdb could not attach {}: {e}", csv.display()));
        }
        warehouse
    }
}

impl DataSystemUnderTest for sutura_exec_postgres::adbc::AdbcPostgres {
    const NAME: &'static str = "postgres";

    fn available() -> bool {
        postgres_tier().is_some()
    }

    fn open_on(name: SourceName, tables: Vec<(TableName, PathBuf)>) -> Self {
        // `available()` guards every cell, so this is reached only when discovery answered.
        let endpoint = postgres_tier().expect("`available()` guards the Open of every postgres cell");
        // The tier PUBLISHES the credential and nothing here defaults one: a discovered server with
        // no published credential is a provisioner that half-ran, and the refusal names the
        // variable rather than trying `sutura` at whatever host discovery answered with.
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        // One PRIVATE schema per open, so parallel corpus cells sharing one server cannot clobber one
        // another's tables - the same per-worktree isolation the compose tier gets, applied per cell.
        let schema = format!("cell_{}_{}", std::process::id(), schema_counter());
        let conninfo = credential
            .conninfo_in(&name, endpoint.host(), endpoint.port(), &schema)
            .unwrap_or_else(|e| panic!("no connection string for the tier at {endpoint}: {e}"));
        let warehouse = Self::new(
            name,
            posture(),
            sutura_exec_postgres::adbc::PostgresDriver::from_host().expect("the tier is up, so a driver is named"),
            conninfo,
        )
        .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        warehouse
            .create_schema(&schema)
            .unwrap_or_else(|e| panic!("postgres could not create {schema}: {e}"));
        for (table, csv) in tables {
            warehouse
                .load_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("postgres could not load {}: {e}", csv.display()));
        }
        warehouse
    }
}

/// A DATA SOURCE over HTTP, executed against the worktree's `nix/clickhouse-tier.nix` server -
/// `github.com/telekom/sutura#920`. Postgres's registration, ported: discovery decides `available()`,
/// the tier publishes the credential and nothing here defaults one, and each open gets a private
/// database so parallel cells sharing one server cannot clobber one another's tables.
///
/// **What it executes as:** the tier's one declared user, over plaintext loopback - the
/// `shared-service-user` posture every entry here is opened with, never a caller's own identity.
impl DataSystemUnderTest for sutura_exec_clickhouse::ClickHouseWarehouse<sutura_exec_clickhouse::transport::Http> {
    const NAME: &'static str = "clickhouse";

    fn available() -> bool {
        clickhouse_tier().is_some()
    }

    fn open_on(name: SourceName, tables: Vec<(TableName, PathBuf)>) -> Self {
        let endpoint = clickhouse_tier().expect("`available()` guards the Open of every clickhouse cell");
        let auth = sutura_exec_clickhouse::fixture::credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let database = format!("cell_{}_{}", std::process::id(), schema_counter());
        let warehouse = Self::connect_in_database(
            name,
            posture(),
            sutura_exec_clickhouse::transport::Endpoint::plaintext(endpoint.host(), endpoint.port()),
            auth,
            &database,
            result_budget(),
        )
        .unwrap_or_else(|e| panic!("clickhouse did not open at {endpoint}: {e}"));
        for (table, csv) in tables {
            warehouse
                .load_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("clickhouse could not load {}: {e}", csv.display()));
        }
        warehouse
    }
}

/// The two-fact ratio's calendar, a table the example corpus lacks. Here rather than in
/// `federated::corpus`, which adds it for every other data system, because the `bigquery`
/// provisioning cell sits in the golden target and loads it too - one copy of the rows for both.
pub(crate) const DIM_CALENDAR: &str =
    "month\n2026-01-01\n2026-02-01\n2026-03-01\n2026-04-01\n2026-05-01\n2026-06-01\n2026-07-01\n2026-08-01\n";

/// The two-fact ratio's second fact, for [`DIM_CALENDAR`]'s reason.
pub(crate) const FCT_TICKET_MONTHLY: &str = "month,customer_key,tickets\n2026-04-01,5,50\n2026-05-01,2,4\n2026-05-01,5,3\n\
     2026-05-01,42,5\n2026-06-01,1,2\n2026-06-01,2,3\n2026-06-01,11,1\n2026-06-01,20,6\n2026-07-01,2,100\n";

/// One table the `bigquery` dataset holds, and what the provisioning cell loads into it: `None` for an
/// example-corpus table, which loads its committed CSV, and the rows of a two-fact table the corpus
/// lacks.
pub(crate) type DatasetTable = (&'static str, Option<&'static str>);

/// Every table the `bigquery` dataset holds. The derived corpus's APPENDED rows are not here: the
/// dataset holds one `fct_subscription_monthly`, the committed one every other cell reads.
pub(crate) const BIGQUERY_TABLES: [DatasetTable; 7] = [
    ("dim_customer", None),
    ("dim_product", None),
    ("dim_region", None),
    ("fct_subscription_monthly", None),
    ("fct_usage_daily", None),
    ("dim_calendar", Some(DIM_CALENDAR)),
    ("fct_ticket_monthly", Some(FCT_TICKET_MONTHLY)),
];

/// `BigQuery`: cloud-only, so its venue is the dataset the `bigquery-conformance` CI job loads
/// [`BIGQUERY_TABLES`] into before this axis runs. Every cell READS those tables and none writes, so
/// an open over any other table is refused rather than loaded: it would replace a table other cells
/// are reading, and one dataset has no per-open schema to give it. **Its limit:** a table is matched
/// by NAME, so a derived corpus's edit to an example table is not what runs here - the two-fact
/// differential's appended `fct_subscription_monthly` rows fall outside its question's range, which
/// is what lets that cell compare. It executes as the job's one CI identity under the matrix's
/// `shared-service-user` posture, so it says nothing about impersonation.
impl DataSystemUnderTest for BigQueryWarehouse<AdbcBigQuery> {
    const NAME: &'static str = "bigquery";

    fn available() -> bool {
        bigquery_venue().is_some()
    }

    fn open_on(name: SourceName, tables: Vec<(TableName, PathBuf)>) -> Self {
        for (table, _) in &tables {
            assert!(
                BIGQUERY_TABLES.iter().any(|&(held, _)| held == table.as_str()),
                "{table} is not a table the bigquery dataset holds, and loading it would replace a table \
                 the other cells are reading"
            );
        }
        bigquery_warehouse(name)
    }
}

/// The provisioned dataset as a warehouse on source `name` - one address for the provisioning cell
/// that loads it and every cell that reads it.
pub(crate) fn bigquery_warehouse(name: SourceName) -> BigQueryWarehouse<AdbcBigQuery> {
    let venue = bigquery_venue().expect("`available()` guards every bigquery open");
    BigQueryWarehouse::over_adbc(
        name,
        posture(),
        venue.project.clone(),
        venue.dataset.clone(),
        venue.driver.clone(),
        Impersonation::Disabled,
        BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a ceiling"),
    )
}

/// Where a provisioned `BigQuery` dataset is, read once from the job's environment.
struct BigQueryVenue {
    project: ProjectId,
    dataset: DatasetId,
    driver: DriverLocation,
}

/// The `BigQuery` venue, or `None` on a host that names none.
///
/// **`SUTURA_BQ_DATASET` is the switch, and past it nothing is optional**: a half-configured venue
/// panics naming the variable, because one that skipped would be a green run over nothing. No value
/// is printed - a public log should not learn a resource name from a refusal.
fn bigquery_venue() -> Option<&'static BigQueryVenue> {
    static CACHED: OnceLock<Option<BigQueryVenue>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let dataset = set("SUTURA_BQ_DATASET")?;
            let required = |name: &str| {
                set(name).unwrap_or_else(|| panic!("SUTURA_BQ_DATASET names a bigquery venue and {name} is unset or empty"))
            };
            drop(required("GOOGLE_APPLICATION_CREDENTIALS"));
            Some(BigQueryVenue {
                project: ProjectId::parse(required("SUTURA_BQ_PROJECT"))
                    .unwrap_or_else(|_| panic!("SUTURA_BQ_PROJECT is not a project id")),
                dataset: DatasetId::parse(dataset).unwrap_or_else(|_| panic!("SUTURA_BQ_DATASET is not a dataset id")),
                driver: DriverLocation::parse(&required("SUTURA_BIGQUERY_ADBC_DRIVER"))
                    .unwrap_or_else(|_| panic!("SUTURA_BIGQUERY_ADBC_DRIVER is not an absolute path")),
            })
        })
        .as_ref()
}

/// An environment value, or `None` where it is unset or blank.
fn set(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

/// The fourth DATA SOURCE, over the wire like Postgres - `github.com/telekom/sutura#127` PR 2.
///
/// **`available()` answers `false` unconditionally.** Oracle Database HAS a local venue -
/// `compose.services.yaml`'s `oracle` profile, over `just dev-up-oracle` - but two things stop this
/// cell from reaching it yet:
///
/// - **No nix-native venue exists**: Oracle Database is not packaged in `nixpkgs`, so no
///   `just validate` leg can provision one. And `available()` is deliberately NOT a discovery read:
///   `sutura_dev::provisioned::here` applies `SUTURA_DEV_REQUIRE_TIER` per PROCESS, set the moment
///   the unrelated Postgres tier comes up, so asking it about `"oracle"` would make every tiered
///   run fatal over a service nothing asked for.
/// - **No CSV-fixture importer exists yet.** `AdbcPostgres`/`DuckDbWarehouse` both grow a
///   `load_csv`/`attach_csv` that infers a schema from the example corpus's CSVs and loads it;
///   `OracleWarehouse` has no such method, because writing one blind - with no live Oracle to
///   validate the generated DDL and bulk-insert shape against - is exactly the "looks like coverage,
///   proves nothing" failure `AGENTS.md` warns against.
///
/// So this registration compiles `OracleWarehouse` against the matrix's own `DataSystemUnderTest`
/// port and reserves its place; every cell this axis is expanded over skips it under its named
/// entry in `exemptions::EXEMPTIONS`, and `crates/sutura-app/tests/golden/dialects.rs`'s `oracle` render
/// cells are still the only Oracle coverage this suite produces.
impl DataSystemUnderTest for sutura_exec_oracle::OracleWarehouse {
    const NAME: &'static str = "oracle";

    fn available() -> bool {
        false
    }

    fn open_on(_name: SourceName, _tables: Vec<(TableName, PathBuf)>) -> Self {
        panic!("`available()` guards the Open of every oracle cell")
    }
}

/// A per-process counter, so each `open` in one test process gets a distinct schema name.
fn schema_counter() -> usize {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// This worktree's provisioned Postgres endpoint, if the tier is up.
///
/// Resolved once and cached: `sutura_dev::provisioned::here` prints its skip-or-fail notice on the
/// skip path, and a corpus run reaches it from several cells - one notice is enough. The skip-or-fail
/// direction is read by `here` from `SUTURA_DEV_REQUIRE_TIER`, so the nix sandbox (which provisions
/// the tier itself) and a docker tier both make the cells RUN rather than skip.
///
/// The directory the walk starts from is this crate's manifest dir, which is inside the worktree and
/// so resolves to the worktree root - the same discovery file `just dev-up` writes per worktree.
fn postgres_tier() -> Option<&'static Endpoint> {
    static CACHED: OnceLock<Option<Endpoint>> = OnceLock::new();
    CACHED
        .get_or_init(
            || match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "postgres") {
                Provisioned::At(endpoint) => Some(endpoint),
                Provisioned::Skipped(_) => None,
            },
        )
        .as_ref()
}

/// This worktree's provisioned `ClickHouse` endpoint, if the tier is up - [`postgres_tier`]'s
/// reasoning, for the `clickhouse` entry `nix/clickhouse-tier.nix` publishes.
fn clickhouse_tier() -> Option<&'static Endpoint> {
    static CACHED: OnceLock<Option<Endpoint>> = OnceLock::new();
    CACHED
        .get_or_init(
            || match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse") {
                Provisioned::At(endpoint) => Some(endpoint),
                Provisioned::Skipped(_) => None,
            },
        )
        .as_ref()
}

/// The catalog the axes that are not *about* a catalog read through.
///
/// Named once here rather than at each call site, so "which catalog produced this golden" has one
/// answer and moving it is one edit. Which one it is does not matter to the dialect and data-system
/// axes, and the catalog axis is what makes that true: every registered catalog is compared against
/// the same independently written oracle, so they all state the same definitions.
pub(crate) type ReferenceCatalog = sutura_catalog_local::LocalCatalog;

/// One table name and the CSV behind it, per model in the bundle.
///
/// Named after `table` rather than after the model, because that is the file on disk. Committed CSVs
/// rather than a database file: a binary in a repository is a fixture nobody reviews, and a table
/// read from the CSV every time cannot drift from it.
fn fixture_tables(pinned: &PinnedDefinitions) -> Vec<(TableName, PathBuf)> {
    pinned
        .definitions()
        .models()
        .values()
        .map(|model| {
            let table = model.table_name().clone();
            let csv = data_root().join(format!("{table}.csv"));
            (table, csv)
        })
        .collect()
}

/// The bundle a registered catalog reads.
pub(crate) fn load<C>() -> PinnedDefinitions
where
    C: CatalogUnderTest,
{
    C::open()
        .load()
        .unwrap_or_else(|e| panic!("the example catalog does not load through the {} adapter: {e}", C::NAME))
}

/// A registered data system, opened over the bundle it is about to be asked about.
pub(crate) fn open<W>(pinned: &PinnedDefinitions) -> W
where
    W: DataSystemUnderTest,
{
    W::open(pinned)
}

// ------------------------------------------------------------------------------------ the registry ---

/// What this suite is a matrix over. **The only place an adapter or a dialect is named.**
///
/// One macro with one arm per axis, and one arm is invoked as `registered!(axis: cell)`. `cell` is a
/// **cell macro**: the registry expands it once per entry, and the cell decides what a cell of that
/// axis *does* - a `#[test]` per entry, or an expression per entry - written where the assertions are,
/// next to what it asserts. The registry decides only what the entries *are*. That split is why
/// adding an adapter does not touch a test: the list grows and every cell already written expands one
/// more time.
///
/// One macro rather than three, because a test target that used only one of three would leave the
/// other two unused, and `unused_imports` is `deny` here. An arm nobody invokes costs nothing.
///
/// An identifier passed into a `macro_rules!` macro carries the caller's resolution, so a cell defined
/// beside the tests resolves there. `macro_rules!` hygiene covers local variables and labels, which is
/// why a cell expands to an item or an expression and never writes to a local this module cannot see -
/// unless the cell is itself defined in the scope holding that local, which is how the exhaustiveness
/// check over `sutura_sql::dialect::ALL` collects.
///
/// # `catalogs: $cell` - `$cell!(name, kind, Adapter)`
///
/// `kind` is `golden` or `declaring`, and it selects which cells the adapter is expanded over:
/// a golden registration gets every cell, a declaring one gets the universal cells only (see
/// `golden/catalogs.rs` for the split and why it is in the type system). **One golden entry today,
/// and that is the honest number: there is one deployable catalog adapter, and it is the reference.**
/// The first narrow adapter is registered here with `declaring` and its declaration in front of a
/// reviewer - `docs/adr/0016` decides that it is measured against that declaration rather than the
/// oracle, which is why it does not get the golden cells at all.
///
/// # `data_systems: $cell` - `$cell!(name, Adapter)`
///
/// `Adapter` implements [`DataSystemUnderTest`]. **Two different kinds of thing behind one port**,
/// which is the whole reason the port takes a `QueryPlan` rather than a statement. `DataFusion` is
/// THE ENGINE: the plan becomes a logical plan over Arrow and no SQL is generated, so a dialect bug
/// is unreachable on that path. Every other entry is a DATA SOURCE: the plan is rendered into its
/// dialect's SQL and pushed down. All are `Warehouse` implementations and the corpus does not know
/// which it is talking to. `Postgres` and `ClickHouse` need a provisioned tier, `BigQuery` a
/// provisioned dataset, and `Oracle` has no venue at all, so each skips where
/// [`DataSystemUnderTest::available`] answers `false` - and only under its named entry in
/// `exemptions::EXEMPTIONS`, which [`runs_here`] is the one reader of.
///
/// # `dialects: $cell` - `$cell!(name, Dialect, ParseTarget)`
///
/// The parse target rides along because the dialect layer names the same targets under its own
/// spelling, and a golden that renders for one and parse-checks against another would pass while
/// proving nothing. Pairing them here is what keeps that from being two lists.
///
/// A dialect is not a data system: rendering `ClickHouse` SQL says nothing about a `ClickHouse`
/// existing anywhere. `sutura_sql::dialect::ALL` is the renderer's own list, and
/// `every_dialect_the_renderer_supports_is_registered` compares this arm against it - so a variant
/// added there without a line here fails rather than rendering with no golden.
macro_rules! registered {
    (catalogs: $cell:ident) => {
        // `sutura-catalog-local`, the metadata reference: a directory of markdown documents with
        // YAML frontmatter. Golden - it defines the model here, so it is held to the whole of it.
        $cell!(markdown, golden, sutura_catalog_local::LocalCatalog);
        // `sutura-catalog-datahub`, the first narrow adapter: a DECLARING source that supplies the
        // physical model, the descriptions, the join columns and - for a metric carrying the
        // deployment-defined structured property - a certified measure. It gets the universal cells
        // and no golden-only cell, and is measured against its own declaration - `docs/adr/0016`.
        // Its corpus is the crate's recorded aspects rather than the example markdown, which is what
        // a metadata service over HTTP reads.
        $cell!(
            datahub,
            declaring,
            sutura_catalog_datahub::DataHubCatalog<sutura_catalog_datahub::fixture::FixtureReader>
        );
        // `sutura-catalog-rdbms`, the narrowest metadata source: a DECLARING adapter over an RDBMS
        // dictionary that supplies the physical model, the descriptions and the join columns a
        // foreign key records and NOTHING else - no measure, no grain, no definitional filter, no
        // value allowlist and no anchor. It gets the universal cells and no golden-only cell and is
        // measured against its own declaration - `docs/adr/0011` specifies it, `docs/adr/0016`
        // decides the declaring path. Its corpus is the crate's recorded dictionary rather than the
        // example markdown.
        $cell!(
            rdbms,
            declaring,
            sutura_catalog_rdbms::RdbmsCatalog<sutura_catalog_rdbms::fixture::FixtureReader>
        );
        // `sutura-catalog-okf`, the second on-disk vocabulary and the cheapest connector: a DECLARING
        // adapter over a directory of OKF Frictionless Table Schema descriptors. It supplies the
        // physical model and the descriptions and declares the join, the measure, the grain, the
        // required filter, the allowlist, the anchor and the knowledge out - `docs/what-okf-can-carry.md`
        // is the field-by-field finding, `#153` the issue. It gets the universal cells and no golden-
        // only cell and is measured against its own declaration - `docs/adr/0011`, `docs/adr/0016`.
        // Its corpus is the descriptors under `examples/okf`, which a directory of Table Schema files
        // is what a deployment without a metadata service actually reads.
        $cell!(okf, declaring, sutura_catalog_okf::OkfCatalog);
        // `sutura-catalog-datacontract`, the third on-disk vocabulary and the one for interface
        // catalogues exported as data-contract YAML: a DECLARING adapter over a directory of ODCS
        // v3 contracts. It provides Structure and may-provide Descriptions, ColumnTypes,
        // ColumnDescriptions and (v3.1+) Relationships under the rdbms target-uniqueness rule, and
        // declares the metric, the grain, the filter, the allowlist, the anchor and the cardinality
        // out - `docs/what-a-data-contract-can-carry.md` is the field-by-field finding, `#973` the
        // issue. It gets the universal cells and no golden-only cell and is measured against its
        // own declaration - `docs/adr/0011`, `docs/adr/0016`. Its corpus is the contracts under
        // `examples/datacontract`.
        $cell!(
            datacontract,
            declaring,
            sutura_catalog_datacontract::DataContractCatalog
        );
        // `sutura-catalog-openmetadata`, the richest of the three measured metadata sources and still
        // a DECLARING connector: it needs a service, and it keeps two half-a-definition slots. It
        // supplies the physical model, the descriptions and the declared non-duplicating joins, and
        // declares the metric (typed `metricType` + granularity, where a column binding resolves), the
        // grain and the cardinality as declared-and-empty may-provide kinds, reporting-not-defining
        // the free-text expression strings and the SQL filters - `docs/what-openmetadata-can-carry.md`
        // is the finding, `#152` the issue. It gets the universal cells and no golden-only cell and is
        // measured against its own declaration.
        $cell!(
            openmetadata,
            declaring,
            sutura_catalog_openmetadata::OpenMetadataCatalog<sutura_catalog_openmetadata::fixture::FixtureReader>
        );
        // `sutura import wren`'s output read by `LocalCatalog` - the path a wren user runs. DECLARING:
        // its corpus is `sutura-cli`'s synthetic wren fixture, not the golden catalog. Universal cells only.
        $cell!(wren, declaring, crate::catalogs::WrenImport);
    };

    (data_systems: $cell:ident) => {
        // THE ENGINE, and the reference the others are compared against: a data source exists to run
        // a subplan of what the engine could otherwise compute itself, so the engine's answer is the
        // one a pushdown has to reproduce.
        $cell!(datafusion, sutura_exec_datafusion::DataFusionWarehouse);
        // A DATA SOURCE, and a development dependency rather than a shipped one.
        $cell!(duckdb, sutura_exec_duckdb::DuckDbWarehouse);
        // A DATA SOURCE, reached over the wire, and likewise a development dependency: it proves the
        // Postgres statement we render is ACCEPTED by a real Postgres, which parse-checking cannot.
        // Its cells run against this worktree's provisioned tier and skip where none is up.
        $cell!(postgres, sutura_exec_postgres::adbc::AdbcPostgres);
        // A DATA SOURCE over HTTP, likewise a development dependency, and likewise run against this
        // worktree's provisioned tier (`nix/clickhouse-tier.nix`) and skipped where none is up.
        $cell!(
            clickhouse,
            sutura_exec_clickhouse::ClickHouseWarehouse<sutura_exec_clickhouse::transport::Http>
        );
        // CLOUD-ONLY: it executes where `SUTURA_BQ_DATASET` names the dataset the
        // `bigquery-conformance` job provisioned, and skips under a named exemption elsewhere.
        $cell!(
            bigquery,
            sutura_exec_bigquery::BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery>
        );
        // `available()` also answers `false` unconditionally, for its own two reasons: see the impl.
        $cell!(oracle, sutura_exec_oracle::OracleWarehouse);
    };

    (dialects: $cell:ident) => {
        $cell!(duckdb, sutura_sql::Dialect::DuckDb, polyglot_sql::DialectType::DuckDB);
        $cell!(
            postgres,
            sutura_sql::Dialect::Postgres,
            polyglot_sql::DialectType::PostgreSQL
        );
        $cell!(
            clickhouse,
            sutura_sql::Dialect::ClickHouse,
            polyglot_sql::DialectType::ClickHouse
        );
        $cell!(
            bigquery,
            sutura_sql::Dialect::BigQuery,
            polyglot_sql::DialectType::BigQuery
        );
        // Registered as a data system with `available() == false` (see that impl), so the parse
        // check this cell runs is the whole of what backs the render: Oracle's grammar accepts it,
        // which says nothing about a real Oracle agreeing with the number.
        $cell!(oracle, sutura_sql::Dialect::Oracle, polyglot_sql::DialectType::Oracle);
    };
}
pub(crate) use registered;
