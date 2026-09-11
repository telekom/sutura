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
use sutura_domain::model::TableName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};
use sutura_domain::query::Query;
use sutura_domain::warehouse::Warehouse;
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

/// The narrowest metadata source: `Rdbms`, opened over its recorded dictionary corpus.
///
/// Like `datahub`, its corpus is NOT the example markdown - a database dictionary is not a directory
/// of YAML either - so this opens over the crate's own recorded corpus, which is what a
/// `sources.<alias>`-per database deployment reads. The universal cells hold because the dictionary
/// bundle is measured against the adapter's declaration, which is the whole point of the `declaring`
/// path. It provides no measure at all - the narrowest declaration - so it answers no certified
/// question, which the declare cells assert rather than the golden cells (it gets no golden cell).
impl
    CatalogUnderTest
    for sutura_catalog_rdbms::RdbmsCatalog<sutura_catalog_rdbms::fixture::FixtureReader>
{
    const NAME: &'static str = "rdbms";

    fn open() -> Self {
        sutura_catalog_rdbms::fixture::over_fixture_source(source(), version())
    }
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

    /// Opens it with one table attached per model in `pinned`.
    fn open(pinned: &PinnedDefinitions) -> Self;

    /// Whether this adapter can execute HERE at all.
    ///
    /// Defaults to `true`, which is the truth for the in-process and in-memory adapters. A network
    /// adapter - one that needs a provisioned service to be listening - reports whether that tier is
    /// up. When it is `false`, a corpus cell is SKIPPED (the skip notice has already reached stderr
    /// through `sutura_dev::provisioned`); the skip-or-fail direction is
    /// `SUTURA_DEV_REQUIRE_TIER`, read once by the provisioner. A provisioned tier makes the cells
    /// run rather than skip, which is the point of the flag; in the sandbox that tier is the nix one
    /// (`nix/postgres-tier.nix`), elsewhere docker, and both write the same discovery file.
    fn available() -> bool {
        true
    }
}

impl DataSystemUnderTest for sutura_exec_datafusion::DataFusionWarehouse {
    const NAME: &'static str = "datafusion";

    fn open(pinned: &PinnedDefinitions) -> Self {
        // A gibibyte, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` - written as a
        // literal rather than read from that crate, because this suite must not give `sutura-app` a
        // dependency on the settings tree to obtain one number. The corpus is a few hundred rows, so
        // no question in it comes near the bound; what this passes on is the shape a deployment gets,
        // and the bound's own assertions live in the adapter's `pool.rs`.
        let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
        let engine = Self::new(source(), posture(), sutura_exec_datafusion::WorkingSet::of_bytes(ceiling))
            .expect("an in-process engine starts");
        for (table, csv) in fixture_tables(pinned) {
            engine
                .attach_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("the engine could not attach {}: {e}", csv.display()));
        }
        engine
    }
}

impl DataSystemUnderTest for sutura_exec_duckdb::DuckDbWarehouse {
    const NAME: &'static str = "duckdb";

    fn open(pinned: &PinnedDefinitions) -> Self {
        let warehouse = Self::in_memory(source(), posture()).expect("an in-memory database opens");
        for (table, csv) in fixture_tables(pinned) {
            warehouse
                .attach_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("duckdb could not attach {}: {e}", csv.display()));
        }
        warehouse
    }
}

impl DataSystemUnderTest for sutura_exec_postgres::PostgresWarehouse {
    const NAME: &'static str = "postgres";

    fn available() -> bool {
        postgres_tier().is_some()
    }

    fn open(pinned: &PinnedDefinitions) -> Self {
        // `available()` guards every cell, so this is reached only when discovery answered.
        let endpoint = postgres_tier().expect("`available()` guards the Open of every postgres cell");
        // The tier PUBLISHES the credential and nothing here defaults one: a discovered server with
        // no published credential is a provisioner that half-ran, and the refusal names the
        // variable rather than trying `sutura` at whatever host discovery answered with.
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let config = Self::local_config(endpoint.host(), endpoint.port(), &credential);
        // One PRIVATE schema per open, so parallel corpus cells sharing one server cannot clobber one
        // another's tables - the same per-worktree isolation the compose tier gets, applied per cell.
        let schema = format!("cell_{}_{}", std::process::id(), schema_counter());
        let warehouse = Self::connect_in_schema(source(), posture(), &config, &schema)
            .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        for (table, csv) in fixture_tables(pinned) {
            warehouse
                .load_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("postgres could not load {}: {e}", csv.display()));
        }
        warehouse
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
/// `Adapter` implements [`DataSystemUnderTest`]. **Three entries, in two different kinds of thing
/// behind one port**, which is the whole reason the port takes a `QueryPlan` rather than a
/// statement. `DataFusion` is THE ENGINE: the plan becomes a logical plan over Arrow and no SQL is
/// generated, so a dialect bug is unreachable on that path. `DuckDB` and `Postgres` are DATA SOURCES:
/// the plan is rendered into their dialect's SQL and pushed down. All three are `Warehouse`
/// implementations and the corpus does not know which it is talking to. Of the two sources, only
/// `Postgres` needs a provisioned tier to execute, so its cells skip where discovery answers no
/// endpoint - see [`DataSystemUnderTest::available`].
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
        $cell!(postgres, sutura_exec_postgres::PostgresWarehouse);
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
    };
}
pub(crate) use registered;
