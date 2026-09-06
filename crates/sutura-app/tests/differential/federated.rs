//! **The whole two-source path, executed:** the real splitter, two real `DuckDB` executions and the
//! combiner, compared against the same corpus answered by one `DataFusion`.
//!
//! `tests/differential.rs` compares the engine against a data source for a MONO question, and
//! `sutura_app::federated`'s own tests drive `answer_federated` over fakes that return literal rows.
//! The leg goldens render hand-built plans. **Nothing joined the splitter, two executions and the
//! combiner**, so every arithmetic claim about a two-source answer rested on rows a test wrote down.
//! `telekom/sutura#325`'s F5 is that gap, and closed #72 deferred it without an owner.
//!
//! # The instrument, and why it is one line wide
//!
//! One corpus, derived from `examples/single-player` into cargo's own target temp directory, and
//! **two catalogs over it that differ in exactly one line** - whether the `customers` model sits on
//! the data system the metric's own model does. On the one-source catalog every question is a
//! whole-answer plan; on the two-source catalog every question that reaches a customer attribute is
//! split. The DATA both sides read is the same directory, so a disagreement cannot be a fixture.
//! [`the_two_catalogs_differ_in_one_document`] holds the width of the difference, because an
//! instrument whose two sides drifted apart would report a corpus edit as a federation defect.
//!
//! **The shared corpus is derived rather than edited**, and that is a scope decision. Placing the
//! dimension model on a second source is not something `examples/single-player` can say - it is one
//! deployment's topology, and the example is a single-source quickstart - and the cases below need a
//! null join key and a metric that cannot federate, neither of which belongs in a document a reader
//! is told to run. Every derivation is one entry in [`CATALOG_CASES`] or [`DATA_CASES`] with its
//! reason beside it, and a `Rewrite` whose text is no longer in the shared document PANICS rather
//! than deriving nothing - so a corpus edit upstream fails this file loudly instead of quietly
//! emptying it.
//!
//! # What is compared, and what the comparison is
//!
//! `sutura_domain::warehouse::agreement` - the one typed policy `tests/differential.rs` and the
//! `BigQuery` acceptance leg also call. Content first (a multiset, per-variant, tolerance on
//! `Value::Real` alone), then order, because a plan that emits `ORDER BY` claims an order and
//! `telekom/sutura#325`'s F6 was the combiner ranking a null group FIRST where the mono path puts it
//! LAST. Two answers to one question, in one order, cell for cell, typed.
//!
//! # What this does NOT establish
//!
//! **No published artifact can run either side of it.** Both legs execute on `DuckDB`, which is a
//! development dependency and the only adapter here declaring `Warehouse::EXECUTES_LEGS`; a shipped
//! binary refuses every two-source question as `FederationNotExecutable` before minting anything.
//! So what is measured is the implemented federation path, not a deployment's answer.
//!
//! And both sides run under one operating-system identity: the corpus's posture is
//! `SharedServiceUser`, so this says nothing about two sources serving two subjects different rows.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sutura_app::Validated;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_domain::warehouse::{RowSet, Value};
use sutura_semantic::{Compiled, compile};

use crate::adapters::{CatalogUnderTest, a_caller, posture, questions, read_question, shared_credential, source, stem, version};

/// The data system the derived two-source catalog puts the dimension model on.
///
/// A second alias and nothing else: the adapter behind it is the same `DuckDB` the fact leg runs
/// on, opened as its own in-memory database with only that source's tables attached. Two
/// registries under one name is what `tests/differential.rs` does to compare adapters; two names
/// in one registry is what federation needs, and `Warehouses` is keyed by the source an adapter
/// declares, so the two databases cannot be confused for one.
const LOOKUP_SOURCE: &str = "geo";

/// An amount of working set no question in this corpus comes near, so only a defect refuses.
///
/// A gibibyte, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` - a literal here for
/// the reason `adapters::DataSystemUnderTest for DataFusionWarehouse` gives, so this suite does
/// not acquire a dependency on the settings tree to obtain one number.
const BUDGET: u64 = 1 << 30;

/// The shared corpus this differential derives from, read off the REGISTERED catalog adapter.
///
/// Not a path written a second time: `CatalogUnderTest::open` is what points the golden suite at
/// `examples/single-player/catalog`, and `LocalCatalog::root` hands that same directory back - so a
/// corpus move cannot leave this file deriving from somewhere else, and `adapters` keeps its own
/// paths private.
fn shared_catalog_root() -> PathBuf {
    <sutura_catalog_local::LocalCatalog as CatalogUnderTest>::open()
        .root()
        .to_path_buf()
}

/// The CSVs beside that catalog, which both sides of the differential read.
fn shared_data_root() -> PathBuf {
    shared_catalog_root()
        .parent()
        .expect("the catalog root sits inside the example corpus")
        .join("data")
}

// ------------------------------------------------------------------ deriving the two catalogs ---

/// What a derived document does to the shared one it came from.
enum Edit {
    /// The one occurrence of `find` becomes `with`.
    ///
    /// Absent from the shared document, the derivation panics: a case that silently stopped
    /// being derived is a green run over a corpus that no longer carries it.
    Rewrite { find: &'static str, with: &'static str },
    /// A whole document the shared corpus does not have.
    Added(&'static str),
    /// Rows appended to a shared file.
    Appended(&'static str),
}

/// **The one difference between the two bundles**: which data system holds the dimension model.
///
/// Applied to the two-source catalog and to nothing else. Every case below is applied to BOTH,
/// so this line is the whole of what the differential varies.
const ON_A_SECOND_DATA_SYSTEM: (&str, Edit) = (
    "models/customers.md",
    Edit::Rewrite {
        find: "source: local",
        with: "source: geo",
    },
);

/// The cases the shared corpus does not carry, derived into BOTH catalogs.
///
/// Each exists because the composed path has a branch nothing else reaches. The prose in each
/// added document says so where a reader of the derived catalog would find it.
const CATALOG_CASES: &[(&str, Edit)] = &[
    // F2: a legal public dimension whose NAME is the remote join target's column name. The
    // splitter used to alias the link column by its own text and put it in the same result
    // namespace as the public labels, so this question produced two fact columns called
    // `customer_key` and the combiner refused the answer. `status` is the backing column, so
    // the name and the column deliberately disagree - which is the shape of the finding.
    (
        "metrics/subscription_months_billed.md",
        Edit::Rewrite {
            find: "anchor:\n  range:",
            with: "  - name: customer_key\n    column: status\n    values: [active, terminated]\n    description: >\n      A legal dimension whose NAME is the remote join target's column, backed by a different\n      column. It is here so the splitter's internal link label has something to collide with.\nanchor:\n  range:",
        },
    ),
    // A zero denominator in ONE subgroup, under `fails`: the answer this metric's own document
    // argues for is a failure rather than a figure, and grouping it by a REMOTE attribute is
    // what makes the guard fire above two legs instead of inside one statement.
    (
        "metrics/revenue_per_churned_subscription.md",
        Edit::Rewrite {
            find: "time_column: month\ngrains: [month]\n---",
            with: "time_column: month\ngrains: [month]\ndimensions:\n  - name: region\n    column: region\n    via: subscription_customer\n    values: [central, east, north, south, west]\n    description: Where the customer is.\n---",
        },
    ),
    // The same zero denominator under `yields_null`, which is the case that produces an ANSWER
    // to compare rather than two failures: one subgroup's denominator sums to zero across the
    // legs and must be null, while its neighbours in the same answer must still be numbers.
    (
        "metrics/revenue_per_churn_or_null.md",
        Edit::Added(
            "---\nkind: metric\nname: revenue_per_churn_or_null\nmodel: subscriptions\nmeasure:\n  ratio:\n    numerator: { aggregate: sum, column: mrr_cents }\n    denominator: { count_if: churned_in_month }\n    zero_denominator: yields_null\ntime_column: month\ngrains: [month]\ndimensions:\n  - name: region\n    column: region\n    via: subscription_customer\n    values: [central, east, north, south, west]\n    description: Where the customer is.\n---\nWhat the month carried for each subscription it lost, or nothing where it lost none.\n\n`revenue_per_churned_subscription` under the other zero-denominator word. Both belong to this\nderived catalog rather than to the shared corpus: what they are for is a subgroup whose\ndenominator is zero while its neighbours are not, which only a grouped ratio can have, and only\na remote grouping key makes the guard run above two legs.\n",
        ),
    ),
    // A distinct value that genuinely SPANS join keys: several customers subscribe to one
    // product, so the number of distinct products in a region is strictly less than the sum of
    // the distinct products per customer. That is what the combiner cannot re-count and what
    // `MeasureDoesNotFederate` exists to refuse. `subscription_key` would not have shown it -
    // a subscription belongs to one customer, so summing per-link distinct counts happens to be
    // right over this corpus, and a refusal protecting nothing reads as coverage.
    // **A federated AVERAGE, which is the classic wrong number.** `Descent::of(Avg)` decomposes it
    // into a sum and a count pushed into the leg and divided ABOVE it, so a combiner that averaged
    // the legs' averages would be wrong by exactly the unevenness of the groups - and this corpus's
    // regions hold different numbers of subscriptions, so it would be wrong here. The shared metric
    // declares no dimension at all, which is why nothing reached the decomposition.
    (
        "metrics/mean_subscription_mrr.md",
        Edit::Rewrite {
            find: "time_column: month\ngrains: [month]\n---",
            with: "time_column: month\ngrains: [month]\ndimensions:\n  - name: region\n    column: region\n    via: subscription_customer\n    values: [central, east, north, south, west]\n    description: Where the customer is.\n---",
        },
    ),
    // `Reduction::Greatest` and `Reduction::Least`, the two arms of the combine's reduction table
    // that no metric in the shared corpus reaches. A leg takes the extreme of its own rows and the
    // combine takes the extreme of those, which is only equal to the whole group's extreme because
    // both ends of that are the same function - the property worth a question rather than a comment.
    (
        "metrics/largest_subscription_mrr.md",
        Edit::Added(
            "---\nkind: metric\nname: largest_subscription_mrr\nmodel: subscriptions\nmeasure:\n  simple: { aggregate: max, column: mrr_cents }\ntime_column: month\ngrains: [month]\ndimensions:\n  - name: region\n    column: region\n    via: subscription_customer\n    values: [central, east, north, south, west]\n    description: Where the customer is.\n---\nThe largest recurring amount any one subscription carried in the period.\n\nHere for the combine's reduction table: a maximum pushed into a leg is re-taken above it, and no\nmetric in the shared corpus declares one.\n",
        ),
    ),
    (
        "metrics/smallest_subscription_mrr.md",
        Edit::Added(
            "---\nkind: metric\nname: smallest_subscription_mrr\nmodel: subscriptions\nmeasure:\n  simple: { aggregate: min, column: mrr_cents }\ntime_column: month\ngrains: [month]\ndimensions:\n  - name: region\n    column: region\n    via: subscription_customer\n    values: [central, east, north, south, west]\n    description: Where the customer is.\n---\nThe smallest recurring amount any one subscription carried in the period.\n\nThe other end of `largest_subscription_mrr`, for the other arm of the same table.\n",
        ),
    ),
    (
        "metrics/products_in_use.md",
        Edit::Added(
            "---\nkind: metric\nname: products_in_use\nmodel: subscriptions\nmeasure:\n  simple: { aggregate: count_distinct, column: product_key }\ntime_column: month\ngrains: [month]\ndimensions:\n  - name: region\n    column: region\n    via: subscription_customer\n    values: [central, east, north, south, west]\n    description: Where the customer is.\n---\nHow many distinct products the period had subscriptions to.\n\nHere because the distinct value spans the join key: one product is subscribed to by several\ncustomers, so no re-aggregation above two legs can recover the count. A two-source question\nover it is refused rather than answered, and the refusal is the assertion.\n",
        ),
    ),
];

/// The rows the shared corpus does not carry, derived into the ONE data directory both sides read.
///
/// July 2026: outside every anchor range and outside every question in the shared corpus, both
/// of which stop at `2026-07-01` exclusive. So these rows change no certified number and no
/// existing snapshot - they are only visible to the questions below that ask for them.
const DATA_CASES: &[(&str, Edit)] = &[(
    "fct_subscription_monthly.csv",
    // F1: the first row's join key is ABSENT, which is the case the corpus has none of. A null
    // link matches nothing, so under LEFT semantics the row is unmatched by construction and
    // must keep its measure under null remote keys; the combiner used to drop it before
    // `include_unmatched` was consulted, losing the measure entirely. The second row's key is
    // present and matches nothing (the corpus's own orphan customer), so the two arrive at one
    // answer group and a defect in either is a wrong number rather than a missing row.
    Edit::Appended(
        "2026-07-01,1901,,3,active,1000,false,monthly\n\
         2026-07-01,1902,41,3,active,2000,false,monthly\n\
         2026-07-01,1903,2,3,active,3000,true,monthly\n\
         2026-07-01,1904,1,4,active,4000,false,annual\n",
    ),
)];

/// Questions the shared corpus does not ask, each reaching a case above.
///
/// Held as text rather than as files because they are read exactly once, by
/// [`every_question`], and a question is a document the reader of this file wants beside the
/// case it exercises.
const DERIVED_QUESTIONS: &[(&str, &str)] = &[
    // F1 and the orphan key in one answer, under LEFT: no remote filter, so an unmatched fact
    // row survives with a null region. Both the absent key and the orphan land in that group.
    (
        "two-source-a-null-key-and-an-orphan-key",
        "metric: recurring_revenue\ngrain: month\nrange:\n  start: 2026-07-01\n  end: 2026-08-01\ndimensions: [region]\n",
    ),
    // F2: a public dimension named after the remote join target, grouped beside the remote one.
    (
        "two-source-a-dimension-named-like-the-link",
        "metric: subscription_months_billed\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\ndimensions: [customer_key, region]\n",
    ),
    // A zero denominator in one subgroup, answered: July's north region churned, its south did
    // not, and the null-region group did not either.
    (
        "two-source-a-zero-denominator-in-one-subgroup",
        "metric: revenue_per_churn_or_null\ngrain: month\nrange:\n  start: 2026-07-01\n  end: 2026-08-01\ndimensions: [region]\n",
    ),
    // The same subgroup under `fails`, where both sides must fail rather than answer.
    (
        "two-source-a-zero-denominator-that-fails",
        "metric: revenue_per_churned_subscription\ngrain: month\nrange:\n  start: 2026-07-01\n  end: 2026-08-01\ndimensions: [region]\n",
    ),
    // The average, decomposed into a sum and a count in each leg and divided above them.
    (
        "two-source-an-average-decomposed-above-the-legs",
        "metric: mean_subscription_mrr\ngrain: month\nrange:\n  start: 2026-01-01\n  end: 2026-07-01\ndimensions: [region]\n",
    ),
    // The two extremes, re-taken above the legs.
    (
        "two-source-a-maximum-re-taken-above-the-legs",
        "metric: largest_subscription_mrr\ngrain: month\nrange:\n  start: 2026-01-01\n  end: 2026-07-01\ndimensions: [region]\n",
    ),
    (
        "two-source-a-minimum-re-taken-above-the-legs",
        "metric: smallest_subscription_mrr\ngrain: month\nrange:\n  start: 2026-01-01\n  end: 2026-07-01\ndimensions: [region]\n",
    ),
    // A distinct value spanning join keys: refused, not answered.
    (
        "two-source-a-distinct-value-spanning-join-keys",
        "metric: products_in_use\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\ndimensions: [region]\n",
    ),
];

/// The derived corpus: one data directory, two catalogs over it.
struct Derived {
    data: PathBuf,
    one_source: PathBuf,
    two_source: PathBuf,
}

/// Derived once per process, into cargo's own temp directory for this target.
///
/// Per PROCESS rather than per target, because the test runner gives each test its own: two
/// processes deriving into one directory would race on files whose bytes are identical, which is
/// a flake with no defect behind it.
fn derived() -> &'static Derived {
    static ONCE: OnceLock<Derived> = OnceLock::new();
    ONCE.get_or_init(|| {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("federated-differential-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        let derived = Derived {
            data: root.join("data"),
            one_source: root.join("catalog-one-source"),
            two_source: root.join("catalog-two-source"),
        };
        copy_tree(&shared_data_root(), &derived.data);
        copy_tree(&shared_catalog_root(), &derived.one_source);
        copy_tree(&shared_catalog_root(), &derived.two_source);
        for &(at, ref edit) in DATA_CASES {
            apply(&derived.data.join(at), edit);
        }
        for &(at, ref edit) in CATALOG_CASES {
            apply(&derived.one_source.join(at), edit);
            apply(&derived.two_source.join(at), edit);
        }
        let (at, ref only_here) = ON_A_SECOND_DATA_SYSTEM;
        apply(&derived.two_source.join(at), only_here);
        derived
    })
}

/// Copies a directory of documents, recursively.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("could not create {}: {e}", to.display()));
    for entry in std::fs::read_dir(from).unwrap_or_else(|e| panic!("could not read {}: {e}", from.display())) {
        let entry = entry.expect("a directory entry is readable");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("an entry has a type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap_or_else(|e| panic!("could not copy {}: {e}", entry.path().display()));
        }
    }
}

/// Applies one edit, refusing to derive nothing.
fn apply(to: &Path, edit: &Edit) {
    match *edit {
        Edit::Rewrite { find, with } => {
            let text = std::fs::read_to_string(to).unwrap_or_else(|e| panic!("could not read {}: {e}", to.display()));
            assert_eq!(
                text.matches(find).count(),
                1,
                "{} no longer holds exactly one {find:?}, so this case would derive nothing",
                to.display()
            );
            write(to, &text.replace(find, with));
        }
        Edit::Added(document) => {
            assert!(!to.exists(), "{} is in the shared corpus already", to.display());
            write(to, document);
        }
        Edit::Appended(rows) => {
            let mut text = std::fs::read_to_string(to).unwrap_or_else(|e| panic!("could not read {}: {e}", to.display()));
            assert!(
                text.ends_with('\n'),
                "{} does not end a row, so appending would join two",
                to.display()
            );
            text.push_str(rows);
            write(to, &text);
        }
    }
}

fn write(to: &Path, text: &str) {
    std::fs::write(to, text).unwrap_or_else(|e| panic!("could not write {}: {e}", to.display()));
}

// ------------------------------------------------------------------------- opening the sides ---

fn lookup_source() -> SourceName {
    SourceName::parse(LOOKUP_SOURCE).expect("the second source alias is a name")
}

/// One bundle, loaded through the real markdown adapter over a derived catalog.
fn bundle(catalog: &Path) -> PinnedDefinitions {
    sutura_catalog_local::LocalCatalog::new(source(), catalog.to_path_buf(), version())
        .load()
        .unwrap_or_else(|e| panic!("the derived catalog at {} does not load: {e}", catalog.display()))
}

/// The one-source side: the ENGINE, over every table the bundle names.
///
/// The bundle and the registry come back together because a `Validated` bundle is only obtainable
/// by re-executing the anchors against the registry that will answer with it - which is what
/// makes "this side reproduces its own certified numbers" a precondition of the comparison rather
/// than a separate test.
fn one_source(pinned: PinnedDefinitions) -> Side<sutura_exec_datafusion::DataFusionWarehouse> {
    let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
    let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
        source(),
        posture(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("an in-process engine starts");
    for (table, csv) in tables_on(&source(), &pinned) {
        engine
            .attach_csv(&table, &csv)
            .unwrap_or_else(|e| panic!("the engine could not attach {}: {e}", csv.display()));
    }
    let warehouses = sutura_app::Warehouses::of(engine);
    let bundle = sutura_app::verify_and_validate(pinned, &warehouses).expect("the anchors hold on one source");
    Side { bundle, warehouses }
}

/// The two-source side: one `DuckDB` per source, each holding only its own tables.
///
/// Two databases rather than one with everything attached, and that is the point of the leg
/// split: neither statement CAN reach the other side's table, so a join across them has to
/// happen above the port or not at all.
fn two_sources(pinned: PinnedDefinitions) -> Side<sutura_exec_duckdb::DuckDbWarehouse> {
    let warehouses = sutura_app::Warehouses::of(duckdb_on(&source(), &pinned))
        .and(duckdb_on(&lookup_source(), &pinned))
        .expect("two sources, one registry");
    let bundle = sutura_app::verify_and_validate(pinned, &warehouses).expect("the anchors hold on two sources");
    Side { bundle, warehouses }
}

/// One side of the differential: a bundle whose anchors it reproduced, and what answers it.
struct Side<W> {
    bundle: Validated<PinnedDefinitions>,
    warehouses: sutura_app::Warehouses<W>,
}

fn duckdb_on(name: &SourceName, pinned: &PinnedDefinitions) -> sutura_exec_duckdb::DuckDbWarehouse {
    let warehouse = sutura_exec_duckdb::DuckDbWarehouse::in_memory(name.clone(), posture()).expect("an in-memory database opens");
    let attached = tables_on(name, pinned);
    assert!(!attached.is_empty(), "no model in the derived bundle sits on {name}");
    for (table, csv) in attached {
        warehouse
            .attach_csv(&table, &csv)
            .unwrap_or_else(|e| panic!("duckdb could not attach {}: {e}", csv.display()));
    }
    warehouse
}

/// Every table on one data system, and the CSV behind it.
fn tables_on(name: &SourceName, pinned: &PinnedDefinitions) -> Vec<(sutura_domain::model::TableName, PathBuf)> {
    pinned
        .definitions()
        .models()
        .values()
        .filter(|model| model.source() == name)
        .map(|model| {
            let table = model.table_name().clone();
            let csv = derived().data.join(format!("{table}.csv"));
            (table, csv)
        })
        .collect()
}

/// Every question this differential reads: the shared corpus's, then the derived ones.
fn every_question() -> Vec<(String, Query)> {
    let mut all: Vec<(String, Query)> = questions().iter().map(|path| (stem(path), read_question(path))).collect();
    for &(name, text) in DERIVED_QUESTIONS {
        let query: Query = serde_norway::from_str(text).unwrap_or_else(|e| panic!("{name} is not a question: {e}"));
        all.push((String::from(name), query));
    }
    all
}

/// One answer, computed through the whole service path.
fn answered<W>(side: &Side<W>, query: &Query, name: &str) -> Result<ToolOutcome, String>
where
    W: sutura_domain::warehouse::Warehouse,
{
    match sutura_app::answer(
        &side.bundle,
        query,
        &a_caller(),
        &shared_credential(),
        &side.warehouses,
        BUDGET,
    ) {
        Ok(answered) => Ok(answered.into_outcome()),
        Err(error) => Err(chain(&error, name)),
    }
}

/// An error and every cause beneath it, as one string.
///
/// `Display` on a `thiserror` enum prints the outermost message and stops, and the outermost one
/// here is "the data system did not answer" or "the combined answer could not be assembled" -
/// true of an outage and of a non-finite cell alike. What tells them apart is one level down.
fn chain(error: &dyn core::error::Error, name: &str) -> String {
    let mut out = format!("{name}: {error}");
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

// ----------------------------------------------------------------------------- the instrument ---

/// The width of the difference between the two bundles, as a property rather than a comment.
///
/// If a case were derived into one catalog and not the other, this file would report a corpus
/// asymmetry as a federation defect - the most expensive kind of false positive a differential
/// can have, because the diagnosis names the wrong subsystem.
#[test]
fn the_two_catalogs_differ_in_one_document() {
    let derived = derived();
    let mut differing: Vec<String> = Vec::new();
    for &(at, _) in CATALOG_CASES {
        compare_document(derived, at, &mut differing);
    }
    for path in every_document(&derived.one_source) {
        let at = path
            .strip_prefix(&derived.one_source)
            .expect("the walk started at this root")
            .to_string_lossy()
            .into_owned();
        compare_document(derived, &at, &mut differing);
    }
    differing.sort();
    differing.dedup();
    assert_eq!(
        differing,
        vec![String::from(ON_A_SECOND_DATA_SYSTEM.0)],
        "the two catalogs must differ in exactly the document that moves the dimension model"
    );
}

fn compare_document(derived: &Derived, at: &str, differing: &mut Vec<String>) {
    let here = std::fs::read_to_string(derived.one_source.join(at)).unwrap_or_default();
    let there = std::fs::read_to_string(derived.two_source.join(at)).unwrap_or_default();
    if here != there {
        differing.push(at.replace('\\', "/"));
    }
}

fn every_document(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap_or_else(|e| panic!("could not read {}: {e}", directory.display())) {
            let entry = entry.expect("a directory entry is readable");
            if entry.file_type().expect("an entry has a type").is_dir() {
                pending.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    found
}

/// **The differential.** Every question the two-source bundle splits, answered both ways.
///
/// The classifier is the two compiles, not a list: a question is a two-source question when the
/// one-source bundle plans a whole answer for it and the two-source bundle does something else.
/// That is what keeps this file from asserting over a hand-maintained set of question names -
/// adding a customer-attribute question to the shared corpus enrols it here.
#[test]
fn a_two_source_answer_is_the_same_answer_as_one_source() {
    let derived = derived();
    let one = one_source(bundle(&derived.one_source));
    let two = two_sources(bundle(&derived.two_source));

    let mut reached: Vec<(String, Reached)> = Vec::new();
    for (name, query) in every_question() {
        let Split::Yes(federated) = split_or_not(&name, &query, one.bundle.get(), two.bundle.get()) else {
            continue;
        };
        let from_one = answered(&one, &query, &name);
        let from_two = answered(&two, &query, &name);
        let outcome = match federated {
            Federated::Split => match (from_one, from_two) {
                (Ok(ToolOutcome::Answer { rows: ref here, .. }), Ok(ToolOutcome::Answer { rows: ref there, .. })) => {
                    agreement_between(&name, here, there);
                    Reached::Agreed
                }
                (Err(ref here), Err(ref there)) => {
                    failed_together(&name, here, there);
                    Reached::FailedTogether
                }
                (here, there) => {
                    panic!("{name}: one side answered and the other did not\n  one source: {here:?}\n  two sources: {there:?}")
                }
            },
            Federated::Refused(reason) => {
                assert!(
                    matches!(from_one, Ok(ToolOutcome::Answer { .. })),
                    "{name}: the one-source deployment must answer what the two-source one refuses, not {from_one:?}"
                );
                assert!(
                    matches!(reason, RefusalReason::MeasureDoesNotFederate { .. }),
                    "{name}: a two-source question this corpus refuses must say why, not {reason:?}"
                );
                Reached::RefusedAsUnfederatable
            }
        };
        reached.push((name, outcome));
    }
    for &(case, wanted) in MUST_BE_REACHED {
        let found = reached.iter().find(|&(name, _)| name == case);
        match found {
            Some(&(_, got)) => assert!(
                got == wanted,
                "{case} reached {got:?} rather than {wanted:?}, so what F5 asks that case to cover \
                 is no longer covered by it"
            ),
            None => panic!("{case} is no longer a two-source question, so this file no longer covers it"),
        }
    }
    // Every arm of the match above, reached by something. An arm no question reaches is an arm that
    // could be replaced by a panic and stay green.
    for wanted in [Reached::Agreed, Reached::FailedTogether, Reached::RefusedAsUnfederatable] {
        assert!(
            reached.iter().any(|&(_, got)| got == wanted),
            "no two-source question reached {wanted:?}, so that arm proved nothing"
        );
    }
}

/// What one two-source question did, as the thing [`MUST_BE_REACHED`] pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reached {
    /// Both topologies answered, and the answers agree in content and in order.
    Agreed,
    /// Both topologies failed, which `zero_denominator: fails` is the one supported way to.
    FailedTogether,
    /// The two-source topology refused a measure it cannot re-aggregate.
    RefusedAsUnfederatable,
}

/// **The cases `telekom/sutura#325`'s F5 asks for, and which arm each has to reach.**
///
/// A hand-written list, deliberately, and it is the only one in this file: the classifier above is
/// mechanical so that a NEW customer-attribute question enrols itself, and this is the other
/// direction - a case that stopped being a two-source question, or that started merely refusing
/// where it used to be compared, is a coverage loss no count would show. Both halves are needed:
/// without the classifier the file asserts over a list, and without the list the file could compare
/// nine of the wrong questions.
const MUST_BE_REACHED: &[(&str, Reached)] = &[
    // F1: a null fact join key beside an unmatched non-null one, retained under LEFT with its
    // measure intact.
    ("two-source-a-null-key-and-an-orphan-key", Reached::Agreed),
    // F2: a legal dimension named after the remote join target.
    ("two-source-a-dimension-named-like-the-link", Reached::Agreed),
    // F6 and the orphan key: the shared corpus's own unmatched customer, whose null group the mono
    // path orders LAST.
    ("recurring-revenue-by-region", Reached::Agreed),
    // A remote filter, which is what makes the join INNER rather than LEFT.
    ("recurring-revenue-annual-in-north", Reached::Agreed),
    // A remote filter and a remote grouping key together.
    ("recurring-revenue-business-only", Reached::Agreed),
    // A same-source join on the fact leg beside the remote one.
    ("recurring-revenue-by-region-and-family", Reached::Agreed),
    // Six buckets and two keys, which is where a key-then-bucket ordering could disagree.
    ("subscription-months-by-region-and-term", Reached::Agreed),
    // The whole reduction table above the legs.
    ("two-source-an-average-decomposed-above-the-legs", Reached::Agreed),
    ("two-source-a-maximum-re-taken-above-the-legs", Reached::Agreed),
    ("two-source-a-minimum-re-taken-above-the-legs", Reached::Agreed),
    // A zero denominator in one subgroup, both ways round.
    ("two-source-a-zero-denominator-in-one-subgroup", Reached::Agreed),
    ("two-source-a-zero-denominator-that-fails", Reached::FailedTogether),
    // A distinct value spanning join keys, which is separate feature work rather than a defect.
    (
        "two-source-a-distinct-value-spanning-join-keys",
        Reached::RefusedAsUnfederatable,
    ),
];

fn agreement_between(name: &str, one_source: &RowSet, two_sources: &RowSet) {
    if let Err(disagreement) = agree_on_content(one_source, two_sources, RealTolerance::DIFFERENTIAL) {
        panic!("{name}: one source and two sources returned different rows - {disagreement}");
    }
    if let Err(disagreement) = agree_on_order(one_source, two_sources, RealTolerance::DIFFERENTIAL) {
        panic!(
            "{name}: one source and two sources returned the same rows in different orders, and the \
             plan's ORDER BY claims one - {disagreement}"
        );
    }
}

/// Both sides failed, and this asserts they failed for the SAME reason.
///
/// `zero_denominator: fails` is the one case in this corpus where a supported question has no
/// figure, and the two sides reach it from opposite directions: the one-source side divides in
/// the engine and the port refuses to carry a non-finite cell, while the two-source side divides
/// above the legs and the combiner refuses. Both must name the metric.
fn failed_together(name: &str, one_source: &str, two_sources: &str) {
    assert!(
        one_source.contains("is not a finite number"),
        "{name}: the one-source side failed for some other reason:\n{one_source}"
    );
    assert!(
        two_sources.contains("could not be assembled") && two_sources.contains("finite"),
        "{name}: the two-source side failed for some other reason:\n{two_sources}"
    );
}

/// **A zero denominator in ONE subgroup, and the neighbours it must not reach.**
///
/// The differential above proves the two sides AGREE on this answer; without this the agreement
/// could be over an answer with no null in it at all, and the case would read as coverage. The
/// divide happens above both legs on this side - the guard cannot be applied inside a leg, where
/// it would be a wrong number rather than a refusal - so what is asserted is that the group whose
/// denominator summed to zero is null while the group beside it is a figure.
#[test]
fn a_subgroup_with_no_denominator_is_null_and_its_neighbours_are_not() {
    let two = two_sources(bundle(&derived().two_source));
    let name = "two-source-a-zero-denominator-in-one-subgroup";
    let query = derived_question(name);
    let outcome = answered(&two, &query, name).unwrap_or_else(|e| panic!("{e}"));
    let ToolOutcome::Answer { ref rows, .. } = outcome else {
        panic!("{name}: a supported two-source question is answered, not {outcome:?}");
    };
    let measure = rows.columns().len().saturating_sub(1);
    let cells: Vec<&Value> = rows.rows().iter().filter_map(|row| row.get(measure)).collect();
    assert!(
        cells.iter().any(|cell| matches!(**cell, Value::Null)),
        "{name}: no subgroup had a zero denominator, so the guard above the legs never ran: {rows:?}"
    );
    assert!(
        cells.iter().any(|cell| !matches!(**cell, Value::Null)),
        "{name}: every subgroup was null, so this says nothing about a zero reaching its neighbours: {rows:?}"
    );
}

/// Which join keys one (month, region, product) triple was seen under.
type SeenUnder = std::collections::BTreeMap<(String, String, String), std::collections::BTreeSet<String>>;

/// **The distinct value really does span the join keys**, which is what the refusal is for.
///
/// Read off the derived corpus rather than asserted, because a `MeasureDoesNotFederate` that
/// protected nothing would read as coverage: over this corpus a distinct SUBSCRIPTION key does
/// not span a customer, so summing per-link distinct counts would happen to be right and the
/// refusal would be untested by the case that motivates it. A distinct PRODUCT key does span,
/// and this is the pair that proves it.
#[test]
fn the_refused_distinct_value_spans_two_join_keys() {
    let mut regions: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for row in rows_of("dim_customer.csv") {
        regions.insert(field(&row, 0), field(&row, 3));
    }
    // (month, region, product) -> the customer keys it was seen under.
    let mut spanning: SeenUnder = std::collections::BTreeMap::new();
    for row in rows_of("fct_subscription_monthly.csv") {
        let customer = field(&row, 2);
        let Some(region) = regions.get(&customer) else {
            continue;
        };
        spanning
            .entry((field(&row, 0), region.clone(), field(&row, 3)))
            .or_default()
            .insert(customer);
    }
    let widest = spanning.values().map(std::collections::BTreeSet::len).max().unwrap_or(0);
    assert!(
        widest > 1,
        "no product in this corpus is subscribed to by two customers in one region and month, so \
         `products_in_use` would federate correctly by accident and its refusal proves nothing"
    );
}

/// One derived question, parsed.
fn derived_question(name: &str) -> Query {
    let (_, text) = DERIVED_QUESTIONS
        .iter()
        .find(|&&(at, _)| at == name)
        .unwrap_or_else(|| panic!("{name} is not a derived question"));
    serde_norway::from_str(text).unwrap_or_else(|e| panic!("{name} is not a question: {e}"))
}

/// The data rows of one derived CSV, header dropped.
fn rows_of(csv: &str) -> Vec<String> {
    let path = derived().data.join(csv);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    text.lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// One comma-separated field. The corpus quotes nothing, which is why this is not a CSV reader.
fn field(row: &str, at: usize) -> String {
    row.split(',').nth(at).map_or_else(String::new, String::from)
}

enum Federated {
    Split,
    Refused(RefusalReason),
}

enum Split {
    Yes(Federated),
    No,
}

/// Whether this question is a two-source question, decided by compiling it against both bundles.
fn split_or_not(name: &str, query: &Query, one: &PinnedDefinitions, two: &PinnedDefinitions) -> Split {
    let here = compile(query, one).unwrap_or_else(|e| panic!("{name} does not compile on one source: {e}"));
    let there = compile(query, two).unwrap_or_else(|e| panic!("{name} does not compile on two sources: {e}"));
    match (here, there) {
        (Compiled::Planned { .. }, Compiled::Federated { .. }) => Split::Yes(Federated::Split),
        (Compiled::Planned { .. }, Compiled::Refused { reason }) => Split::Yes(Federated::Refused(reason)),
        (Compiled::Refused { reason: ref one_reason }, Compiled::Refused { reason: ref two_reason }) => {
            assert_eq!(
                format!("{one_reason:?}"),
                format!("{two_reason:?}"),
                "{name}: a compile-side refusal must not depend on where a model sits"
            );
            Split::No
        }
        (Compiled::Planned { .. }, Compiled::Planned { .. }) => Split::No,
        (here, there) => panic!("{name}: the one-source bundle did not plan a whole answer\n  one: {here:?}\n  two: {there:?}"),
    }
}

/// **Which registered data systems can run a leg, expanded over the registry itself.**
///
/// This file's two-source side names `DuckDB` twice, and that is not a preference: it is the only
/// registered adapter declaring [`Warehouse::EXECUTES_LEGS`], and a registry entry that cannot
/// run a leg cannot be either half of a federated answer. Written as a cell rather than as a
/// sentence so that registering a second leg-executing adapter REDDENS here - the diff that
/// enrols it in this differential then arrives beside the registration, rather than being
/// noticed the next time somebody reads this comment.
///
/// [`Warehouse::EXECUTES_LEGS`]: sutura_domain::warehouse::Warehouse::EXECUTES_LEGS
macro_rules! leg_capability {
    ($name:ident, $adapter:ty) => {
        mod $name {
            use crate::adapters::DataSystemUnderTest;
            use sutura_domain::warehouse::Warehouse;

            #[test]
            fn whether_it_can_run_a_leg_is_what_this_differential_can_use_it_for() {
                let name = <$adapter as DataSystemUnderTest>::NAME;
                assert_eq!(
                    <$adapter as Warehouse>::EXECUTES_LEGS,
                    name == "duckdb",
                    "{name} changed its leg capability; the two-source side of \
                     tests/federated_differential.rs is the list of adapters that have one"
                );
            }
        }
    };
}

crate::adapters::registered!(data_systems: leg_capability);
