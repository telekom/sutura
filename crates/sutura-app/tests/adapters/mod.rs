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
//!    open it over the fixtures;
//! 2. one line in [`registered`].
//!
//! In `tests/adapters/mod.rs` rather than `tests/adapters.rs` so cargo does not build it as a test
//! target of its own. It is shared by every test target here, which is why nothing in it is
//! target-specific: `unused_imports` and `dead_code` are both `deny` in the workspace lint table, so
//! an item only one target used would fail the build of the other. The fakes live in
//! `tests/support/mod.rs`, which one target includes.
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

use sutura_domain::model::TableName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};
use sutura_domain::query::Query;
use sutura_domain::warehouse::Warehouse;

/// The version the goldens are pinned under.
///
/// Fixed, not derived from the working tree. A version that moved between runs would put a new value
/// in every snapshot that carries provenance, and then no snapshot would mean anything.
const VERSION: &str = "golden-fixture-1";

/// The one data system the fixture catalog reads from.
const SOURCE: &str = "local";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn catalog_root() -> PathBuf {
    fixtures().join("catalog")
}

fn data_root() -> PathBuf {
    fixtures().join("data")
}

pub(crate) fn source() -> sutura_domain::model::SourceName {
    sutura_domain::model::SourceName::parse(SOURCE).expect("the fixture source name is a name")
}

pub(crate) fn version() -> DefinitionVersion {
    DefinitionVersion::parse(VERSION).expect("the fixture version is a version")
}

/// Every question in the corpus, in sorted order.
///
/// Sorted so the corpus is a function of the directory rather than of the filesystem: a suite whose
/// order changes between runs produces snapshot churn that has nothing to do with the change under
/// review.
pub(crate) fn questions() -> Vec<PathBuf> {
    let dir = fixtures().join("questions");
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

    /// Builds it over the fixture catalog.
    fn open() -> Self;
}

impl CatalogUnderTest for sutura_catalog_local::LocalCatalog {
    const NAME: &'static str = "markdown";

    fn open() -> Self {
        Self::new(catalog_root(), version())
    }
}

/// A `Warehouse` adapter this suite executes the fixture corpus against.
///
/// [`open`] takes the bundle because attaching a table per model is what makes a data system able to
/// answer anything, and only the bundle knows which tables there are. It is on the trait rather than
/// in each test for the reason the trait exists at all: two copies of "open a data system over the
/// fixture CSVs" is two things to keep in step, and they did not stay in step.
///
/// [`open`]: DataSystemUnderTest::open
pub(crate) trait DataSystemUnderTest: Warehouse + Sized {
    /// The name this adapter's tests and snapshots carry.
    const NAME: &'static str;

    /// Opens it with one table attached per model in `pinned`.
    fn open(pinned: &PinnedDefinitions) -> Self;
}

impl DataSystemUnderTest for sutura_exec_datafusion::DataFusionWarehouse {
    const NAME: &'static str = "datafusion";

    fn open(pinned: &PinnedDefinitions) -> Self {
        let engine = Self::new(source()).expect("an in-process engine starts");
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
        let warehouse = Self::in_memory(source()).expect("an in-memory database opens");
        for (table, csv) in fixture_tables(pinned) {
            warehouse
                .attach_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("duckdb could not attach {}: {e}", csv.display()));
        }
        warehouse
    }
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
            let table = model.table().clone();
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
        .unwrap_or_else(|e| panic!("the fixture catalog does not load through the {} adapter: {e}", C::NAME))
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
/// check over `sutura_semantic::dialect::ALL` collects.
///
/// # `catalogs: $cell` - `$cell!(name, Adapter)`
///
/// `Adapter` implements [`CatalogUnderTest`]. **One entry today, and that is the honest number: there
/// is one catalog adapter.** What this changes is the cost of the second one.
///
/// # `data_systems: $cell` - `$cell!(name, Adapter)`
///
/// `Adapter` implements [`DataSystemUnderTest`]. **Two entries, and they are two different kinds of
/// thing behind one port**, which is the whole reason the port takes a `QueryPlan` rather than a
/// statement. `DataFusion` is THE ENGINE: the plan becomes a logical plan over Arrow and no SQL is
/// generated, so a dialect bug is unreachable on that path. `DuckDB` is a DATA SOURCE: the plan is
/// rendered into `DuckDB` SQL and pushed down. Both are `Warehouse` implementations and the corpus
/// does not know which it is talking to.
///
/// # `dialects: $cell` - `$cell!(name, Dialect, ParseTarget)`
///
/// The parse target rides along because the dialect layer names the same three targets under its own
/// spelling, and a golden that renders for one and parse-checks against another would pass while
/// proving nothing. Pairing them here is what keeps that from being two lists.
///
/// A dialect is not a data system: rendering `ClickHouse` SQL says nothing about a `ClickHouse`
/// existing anywhere. `sutura_semantic::dialect::ALL` is the compiler's own list, and
/// `every_dialect_the_compiler_renders_for_is_registered` compares this arm against it - so a variant
/// added there without a line here fails rather than rendering with no golden.
macro_rules! registered {
    (catalogs: $cell:ident) => {
        // `sutura-catalog-local`: a directory of markdown documents with YAML frontmatter.
        $cell!(markdown, sutura_catalog_local::LocalCatalog);
    };

    (data_systems: $cell:ident) => {
        // THE ENGINE, and the reference the others are compared against: a data source exists to run
        // a subplan of what the engine could otherwise compute itself, so the engine's answer is the
        // one a pushdown has to reproduce.
        $cell!(datafusion, sutura_exec_datafusion::DataFusionWarehouse);
        // A DATA SOURCE, and a development dependency rather than a shipped one.
        $cell!(duckdb, sutura_exec_duckdb::DuckDbWarehouse);
    };

    (dialects: $cell:ident) => {
        $cell!(
            duckdb,
            sutura_semantic::Dialect::DuckDb,
            polyglot_sql::DialectType::DuckDB
        );
        $cell!(
            postgres,
            sutura_semantic::Dialect::Postgres,
            polyglot_sql::DialectType::PostgreSQL
        );
        $cell!(
            clickhouse,
            sutura_semantic::Dialect::ClickHouse,
            polyglot_sql::DialectType::ClickHouse
        );
    };
}
pub(crate) use registered;
