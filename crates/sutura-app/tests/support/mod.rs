//! Shared machinery for the golden suite: where the fixtures are, and the two fakes.
//!
//! In `tests/support/mod.rs` rather than `tests/support.rs` so cargo does not build it as a test
//! target of its own.
//!
//! **Two implementations of each port, which is what makes the suite mean anything.** A corpus run
//! against one implementation tests that implementation. Run against two it tests the port: the
//! hand-written catalog is an independent statement of what the markdown says, and the recording
//! warehouse is what lets every refusal and every generated statement be checked with no database
//! at all.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sutura_catalog_local::{LocalCatalog, digest_of};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Anchor, Definitions, Dimension, Metric, Model, Relationship};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, Measure, MetricName, ModelName, RelationshipName, SourceName,
    TableName,
};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};
use sutura_domain::query::Query;
use sutura_domain::warehouse::{GeneratedQuery, RowSet, Value, Warehouse};

/// The version the goldens are pinned under.
///
/// Fixed, not derived from the working tree. A version that moved between runs would put a new value
/// in every snapshot that carries provenance, and then no snapshot would mean anything.
pub(crate) const VERSION: &str = "golden-fixture-1";

/// The one data system the fixture catalog reads from.
pub(crate) const SOURCE: &str = "local";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub(crate) fn catalog_root() -> PathBuf {
    fixtures().join("catalog")
}

pub(crate) fn data_root() -> PathBuf {
    fixtures().join("data")
}

pub(crate) fn source() -> SourceName {
    SourceName::parse(SOURCE).expect("the fixture source name is a name")
}

fn version() -> DefinitionVersion {
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

/// The catalog as the markdown adapter reads it.
pub(crate) fn load_local() -> PinnedDefinitions {
    LocalCatalog::new(catalog_root(), version())
        .load()
        .unwrap_or_else(|e| panic!("the fixture catalog does not load: {e}"))
}

// ------------------------------------------------------------------ the hand-written catalog ---

/// The same catalog, stated in Rust.
///
/// It exists to be compared against the markdown one. Two adapters reading the same content must
/// produce the same [`Definitions`], and with one real adapter that claim is untestable: this is the
/// second implementation. It is written from the fixture documents by hand on purpose, because a
/// version generated from them would agree with them by construction and prove nothing.
///
/// Descriptions are left empty here. They are prose that only the markdown carries, so the
/// comparison is made over [`without_descriptions`] rather than pretending this file repeats them.
pub(crate) struct HandWrittenCatalog;

/// Why the hand-written catalog could not be built. It cannot fail; the type exists because the
/// port requires one.
#[derive(Debug, thiserror::Error)]
#[error("the hand-written catalog cannot fail")]
pub(crate) struct Never;

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a fixture column is a column")
}

fn values(raw: &[&str]) -> BTreeSet<String> {
    raw.iter().map(|v| String::from(*v)).collect()
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a fixture date is a date"),
        Date::parse("2026-07-01").expect("a fixture date is a date"),
    )
    .expect("June is a range")
}

fn dimension(name: &str, col: &str, via: Option<&str>, allowed: Option<&[&str]>) -> (DimensionName, Dimension) {
    let name = DimensionName::parse(name).expect("a fixture dimension is a dimension");
    let dimension = Dimension::new(
        name.clone(),
        column(col),
        via.map(|v| RelationshipName::parse(v).expect("a fixture relationship is a relationship")),
        allowed.map(values),
        String::new(),
    );
    (name, dimension)
}

impl SemanticCatalog for HandWrittenCatalog {
    type Error = Never;

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \n                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \n                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let orders = Model::new(
            ModelName::parse("orders").expect("a name"),
            source(),
            TableName::parse("orders").expect("a name"),
            BTreeSet::from([
                column("order_id"),
                column("order_date"),
                column("customer_id"),
                column("channel"),
                column("amount_cents"),
            ]),
            String::new(),
        );
        let customers = Model::new(
            ModelName::parse("customers").expect("a name"),
            source(),
            TableName::parse("customers").expect("a name"),
            BTreeSet::from([column("id"), column("region_code"), column("segment")]),
            String::new(),
        );
        let joins = vec![Relationship::new(
            RelationshipName::parse("orders_customer").expect("a name"),
            ModelName::parse("orders").expect("a name"),
            column("customer_id"),
            ModelName::parse("customers").expect("a name"),
            column("id"),
            JoinType::ManyToOne,
        )];

        let revenue = Metric::new(
            MetricName::parse("revenue").expect("a name"),
            ModelName::parse("orders").expect("a name"),
            Measure::new(Aggregate::Sum, column("amount_cents")),
            column("order_date"),
            BTreeSet::from([Grain::Day, Grain::Month]),
            BTreeMap::from([
                dimension("channel", "channel", None, Some(&["web", "store"])),
                dimension(
                    "region",
                    "region_code",
                    Some("orders_customer"),
                    Some(&["north", "south", "west"]),
                ),
                dimension("segment", "segment", Some("orders_customer"), None),
            ]),
            Some(Anchor::new(june(), String::from("470023"))),
            String::new(),
        );
        let orders_placed = Metric::new(
            MetricName::parse("orders_placed").expect("a name"),
            ModelName::parse("orders").expect("a name"),
            Measure::new(Aggregate::Count, column("order_id")),
            column("order_date"),
            BTreeSet::from([Grain::Day, Grain::Month]),
            BTreeMap::from([dimension(
                "region",
                "region_code",
                Some("orders_customer"),
                Some(&["north", "south", "west"]),
            )]),
            Some(Anchor::new(june(), String::from("8"))),
            String::new(),
        );
        let average_order = Metric::new(
            MetricName::parse("average_order").expect("a name"),
            ModelName::parse("orders").expect("a name"),
            Measure::new(Aggregate::Avg, column("amount_cents")),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            BTreeMap::new(),
            None,
            String::new(),
        );

        let definitions = Definitions::assemble(vec![orders, customers], joins, vec![revenue, orders_placed, average_order])
            .expect("the hand-written catalog holds together");
        let digest = digest_of(&definitions).expect("the definitions hash");
        Ok(PinnedDefinitions::new(version(), digest, definitions))
    }
}

/// The same definitions with every description blanked.
///
/// The comparison the differential oracle actually makes. Prose lives in the markdown and nowhere
/// else, so comparing it would be comparing one implementation against a copy of itself. Everything
/// that decides what executes is compared.
pub(crate) fn without_descriptions(definitions: &Definitions) -> Definitions {
    let models = definitions
        .models()
        .values()
        .map(|model| {
            Model::new(
                model.name().clone(),
                model.source().clone(),
                model.table().clone(),
                model.columns().clone(),
                String::new(),
            )
        })
        .collect();
    let metrics = definitions
        .metrics()
        .values()
        .map(|metric| {
            let dimensions = metric
                .dimensions()
                .values()
                .map(|d| {
                    (
                        d.name().clone(),
                        Dimension::new(
                            d.name().clone(),
                            d.column().clone(),
                            d.via().cloned(),
                            d.allowed_values().cloned(),
                            String::new(),
                        ),
                    )
                })
                .collect();
            Metric::new(
                metric.name().clone(),
                metric.model().clone(),
                metric.measure().clone(),
                metric.time_column().clone(),
                metric.grains().clone(),
                dimensions,
                metric.anchor().cloned(),
                String::new(),
            )
        })
        .collect();
    let joins = definitions.relationships().values().cloned().collect();
    Definitions::assemble(models, joins, metrics).expect("stripping prose cannot break consistency")
}

// ------------------------------------------------------------------------ the fake warehouse ---

/// A warehouse that runs nothing and remembers what it was asked.
///
/// What lets the plan and SQL goldens, and every refusal, be checked with no database. It is a fake
/// rather than a mock of a wire protocol: the port is a Rust trait, so the honest stand-in is a type
/// that implements it. A test asserting on the text of an HTTP request would prove something about
/// the test.
pub(crate) struct RecordingWarehouse {
    source: SourceName,
    seen: RefCell<Vec<String>>,
}

impl RecordingWarehouse {
    pub(crate) fn new() -> Self {
        Self {
            source: source(),
            seen: RefCell::new(Vec::new()),
        }
    }

    /// A warehouse claiming to be some other data system, for the refusal that checks the plan's
    /// source against the adapter it is about to run on.
    pub(crate) fn pretending_to_be(name: &str) -> Self {
        Self {
            source: SourceName::parse(name).expect("a test source is a source"),
            seen: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn statements(&self) -> Vec<String> {
        self.seen.borrow().clone()
    }
}

impl Warehouse for RecordingWarehouse {
    type Error = Never;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn dry_run(&self, _query: &GeneratedQuery) -> Result<(), Self::Error> {
        Ok(())
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "the fixed one-cell result is a literal, so a failure to build it is a broken \n                  test rather than an input to handle"
    )]
    fn execute(&self, query: &GeneratedQuery) -> Result<RowSet, Self::Error> {
        self.seen.borrow_mut().push(String::from(query.sql()));
        // One row of nothing, shaped so `RowSet::new` accepts it. A fake that returned plausible
        // numbers would invite a test to assert on them, and those numbers would be this file's
        // opinion rather than a data system's.
        Ok(RowSet::new(vec![String::from("recorded")], vec![vec![Value::Null]]).expect("one column and one cell is rectangular"))
    }
}

/// June 2026, the range the fixture anchors use.
///
/// Exposed because the two-source refusal test builds a question by hand rather than from a file.
pub(crate) fn june_range() -> TimeRange {
    june()
}

/// The fixture catalog with `customers` moved to a second data system.
///
/// Built in code rather than as a fixture directory, because a fixture catalog spanning two data
/// systems would make every other test in the suite span two. It exists to provoke one refusal:
/// a plan whose join would reach a second data system is refused before anything runs, because a
/// second data system is a second identity to satisfy.
pub(crate) fn two_source_catalog() -> TwoSourceCatalog {
    TwoSourceCatalog
}

/// See [`two_source_catalog`].
pub(crate) struct TwoSourceCatalog;

impl SemanticCatalog for TwoSourceCatalog {
    type Error = Never;

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \n                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \n                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let orders = Model::new(
            ModelName::parse("orders").expect("a name"),
            source(),
            TableName::parse("orders").expect("a name"),
            BTreeSet::from([
                column("order_id"),
                column("order_date"),
                column("customer_id"),
                column("amount_cents"),
            ]),
            String::new(),
        );
        let customers = Model::new(
            ModelName::parse("customers").expect("a name"),
            SourceName::parse("elsewhere").expect("a name"),
            TableName::parse("customers").expect("a name"),
            BTreeSet::from([column("id"), column("region_code")]),
            String::new(),
        );
        let joins = vec![Relationship::new(
            RelationshipName::parse("orders_customer").expect("a name"),
            ModelName::parse("orders").expect("a name"),
            column("customer_id"),
            ModelName::parse("customers").expect("a name"),
            column("id"),
            JoinType::ManyToOne,
        )];
        let revenue = Metric::new(
            MetricName::parse("revenue").expect("a name"),
            ModelName::parse("orders").expect("a name"),
            Measure::new(Aggregate::Sum, column("amount_cents")),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            BTreeMap::from([dimension("region", "region_code", Some("orders_customer"), None)]),
            None,
            String::new(),
        );
        let definitions = Definitions::assemble(vec![orders, customers], joins, vec![revenue])
            .expect("a two-source catalog is still internally consistent");
        let digest = digest_of(&definitions).expect("the definitions hash");
        Ok(PinnedDefinitions::new(version(), digest, definitions))
    }
}
