//! The derived corpus: the shared example bytes, plus the declared edits that make a topology the
//! quickstart cannot state.
//!
//! The baseline has **one data directory, two catalogs over it, differing in exactly one line** - whether
//! the `customers` model sits on the metric's own data system. That width is
//! [`the_two_catalogs_differ_in_one_document`], and it is the control on the baseline instrument:
//! without it, a corpus asymmetry would be reported by `super` as a federation defect, which is the
//! most expensive kind of false positive because the diagnosis names the wrong subsystem.
//! The refusal-only variants also move products, under distinct stems without mutating the baseline.
//!
//! **Derived rather than committed, and that is a scope decision.** Placing the dimension model on a
//! second source is one deployment's topology, and `examples/single-player` is a single-source
//! quickstart; the cases need a null join key, a dimension named after the remote join target and a
//! measure that cannot federate, none of which belongs in a document a reader is told to run.
//! Editing the shared corpus would also have moved anchors and committed snapshots in three crates.
//!
//! Each baseline derivation is one entry in [`CATALOG_CASES`], [`DATA_CASES`] or [`DERIVED_QUESTIONS`]
//! with its reason beside it, and a [`Edit::Rewrite`] whose text has left the shared document
//! PANICS rather than deriving nothing - a corpus edit upstream fails loudly instead of quietly
//! emptying this file. The appended fact rows are dated `2026-07-01`, outside every anchor range and
//! outside every question the shared corpus asks, so no certified number moves.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sutura_domain::model::SourceName;
use sutura_domain::query::Query;

use crate::adapters::{CatalogUnderTest, questions, read_question, stem};

/// The data system the derived two-source catalog puts the dimension model on.
///
/// A second alias and nothing else: the same `DuckDB`, opened as its own in-memory database with
/// only that source's tables attached. `Warehouses` is keyed by the source an adapter declares, so
/// the two databases cannot be confused for one.
pub(crate) const LOOKUP_SOURCE: &str = "geo";

/// The shared corpus this differential derives from, read off the REGISTERED catalog adapter.
///
/// Not a path written a second time: `CatalogUnderTest::open` points the golden suite at the
/// example catalog and `LocalCatalog::root` hands that directory back, so a corpus move cannot
/// leave this file deriving from somewhere else.
pub(crate) fn shared_catalog_root() -> PathBuf {
    <sutura_catalog_local::LocalCatalog as CatalogUnderTest>::open()
        .root()
        .to_path_buf()
}

/// The CSVs beside that catalog, which both sides of the differential read.
pub(crate) fn shared_data_root() -> PathBuf {
    shared_catalog_root()
        .parent()
        .expect("the catalog root sits inside the example corpus")
        .join("data")
}

// ------------------------------------------------------------------ deriving the two catalogs ---

/// What a derived document does to the shared one it came from.
pub(crate) enum Edit {
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

/// The one-sided edit's shape, and it is a REWRITE of a document both catalogs already have.
///
/// **Its own type rather than an [`Edit`], because the asymmetry an [`Edit`] can express here is
/// the one this file's control test exists to refuse.** That slot was typed `(&str, Edit)`, so an
/// `Edit::Added` in it compiled - and a document only one catalog has is one the other cannot be
/// compared against. Review found it; a type is a better answer than a second check.
pub(crate) struct OneSided {
    at: &'static str,
    find: &'static str,
    with: &'static str,
}

impl OneSided {
    /// The same rewrite, as the thing [`apply`] takes - so a one-sided edit and a shared one are
    /// not applied by two different pieces of code.
    const fn edit(&self) -> Edit {
        Edit::Rewrite {
            find: self.find,
            with: self.with,
        }
    }
}

/// **The one difference between the two bundles**: which data system holds the dimension model.
///
/// Applied to the two-source catalog and to nothing else. Every case below is applied to BOTH,
/// so this line is the whole of what the differential varies.
pub(crate) const ON_A_SECOND_DATA_SYSTEM: OneSided = OneSided {
    at: "models/customers.md",
    find: "source: local",
    with: "source: geo",
};

/// The cases the shared corpus does not carry, derived into BOTH catalogs.
///
/// Each exists because the composed path has a branch nothing else reaches. The prose in each
/// added document says so where a reader of the derived catalog would find it.
pub(crate) const CATALOG_CASES: &[(&str, Edit)] = &[
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
    // A distinct value that genuinely SPANS join keys: several customers subscribe to one product,
    // so the number of distinct products in a region is strictly less than the sum of the distinct
    // products per customer. That is what the combiner cannot re-count and what
    // `MeasureDoesNotFederate` exists to refuse. `subscription_key` would not have shown it - a
    // subscription belongs to one customer, so summing per-link distinct counts happens to be right
    // over this corpus, and a refusal protecting nothing reads as coverage.
    // `the_refused_distinct_value_spans_two_join_keys` measures both halves of that.
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
pub(crate) const DATA_CASES: &[(&str, Edit)] = &[(
    "fct_subscription_monthly.csv",
    // F1: the first row's join key is ABSENT, which is the case the corpus has none of. A null
    // link matches nothing, so under LEFT semantics the row is unmatched by construction and
    // must keep its measure under null remote keys; the combiner used to drop it before
    // `include_unmatched` was consulted, losing the measure entirely. The second row's key is
    // present and matches nothing (the corpus's own orphan customer), so the two arrive at one
    // answer group and a defect in either is a wrong number rather than a missing row.
    //
    // **The last row's `product_key` 99 is a SAME-SOURCE orphan, and it exists because the shared
    // join decision was otherwise observed by nothing.** Every other orphan in this corpus is a
    // REMOTE one - `customer_key` 41, which the derivation puts on the second data system, so the
    // combiner's `include_unmatched` covers it and the fact leg's own `JOIN` never sees an unmatched
    // row. Measured: with the leg path taking `EngineJoin::Inner` for its own same-source hops and
    // the shared definition untouched, the whole suite was 2640 of 2640 passed. `dim_product.csv`
    // stops at 8, so 99 matches nothing on the source the fact leg reads, and
    // `two-source-a-same-source-orphan-beside-a-remote-one` groups by a column of that table -
    // which is what makes LEFT-versus-INNER *inside a leg* a wrong number instead of an
    // unobservable preference.
    Edit::Appended(
        "2026-07-01,1901,,3,active,1000,false,monthly\n\
         2026-07-01,1902,41,3,active,2000,false,monthly\n\
         2026-07-01,1903,2,3,active,3000,true,monthly\n\
         2026-07-01,1904,1,4,active,4000,false,annual\n\
         2026-07-01,1905,1,99,active,5000,false,monthly\n",
    ),
)];

/// Questions the shared corpus does not ask, each reaching a case above.
///
/// Held as text rather than as files because they are read exactly once, by
/// [`every_question`], and a question is a document the reader of this file wants beside the
/// case it exercises.
pub(crate) const DERIVED_QUESTIONS: &[(&str, &str)] = &[
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
    // **A same-source orphan beside a remote one**, which is the only question here whose fact leg
    // has an unmatched row in its OWN `JOIN`. `product_family` comes off the metric's own data
    // system and `region` off the second, so the fact leg carries a same-source hop and the lookup
    // leg the remote one - and subscription 1905's `product_key` matches no product. Under LEFT it
    // survives with a null family and its 5000 stays in the answer; under INNER the leg drops it and
    // the two-source side totals less than the one-source side with nothing raising an error. That
    // is the disagreement `leg::dimension_join` exists to make impossible.
    (
        "two-source-a-same-source-orphan-beside-a-remote-one",
        "metric: recurring_revenue\ngrain: month\nrange:\n  start: 2026-07-01\n  end: 2026-08-01\ndimensions: [region, product_family]\n",
    ),
    // A distinct value spanning join keys: refused, not answered.
    (
        "two-source-a-distinct-value-spanning-join-keys",
        "metric: products_in_use\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\ndimensions: [region]\n",
    ),
];

/// The derived corpus: one data directory, two catalogs over it.
pub(crate) struct Derived {
    pub(crate) data: PathBuf,
    pub(crate) one_source: PathBuf,
    pub(crate) two_source: PathBuf,
}

/// The corpus every question above is answered over: derived once per process, and satisfying every
/// declaration its catalogs make.
pub(crate) fn derived() -> &'static Derived {
    static ONCE: OnceLock<Derived> = OnceLock::new();
    ONCE.get_or_init(|| derive_into("federated-differential"))
}

/// An owned refusal topology: customers are remote, and products take the supplied source line.
/// Distinct caller stems keep these variants separate from each other and the shared baseline.
pub(crate) fn remote_products(stem: &str, source_line: &'static str) -> Derived {
    let derived = derive_into(stem);
    apply(
        &derived.two_source.join("models/products.md"),
        &Edit::Rewrite {
            find: "source: local",
            with: source_line,
        },
    );
    derived
}

/// **The same corpus with the `many_to_one` between the fact and the dimension VIOLATED**, as one
/// appended row.
///
/// A second row for `customer_key = 2`, identical to the one already there. It is a whole second
/// derivation rather than an entry in [`DATA_CASES`] for the obvious reason: every other case is
/// something the differential above answers, and this one is a corpus no deployment may serve.
///
/// **Identical rather than differing, which is what makes it the case the guards missed.**
/// `FederatedFailure::AmbiguousLink` fires when two lookup rows disagree in a column the question
/// projects; when they agree the lookup leg's own `GROUP BY` has already collapsed them, so the
/// two-source side answered the number the declaration promises while the one-source side's `JOIN`
/// added the measure twice. Measured on the tree before the boot check existed: `29138` against
/// `22765` for `recurring-revenue-business-only` in the north, a difference of exactly that
/// customer's June business revenue.
pub(crate) fn violated() -> &'static Derived {
    static ONCE: OnceLock<Derived> = OnceLock::new();
    ONCE.get_or_init(|| {
        let derived = derive_into("federated-differential-violated");
        apply(&derived.data.join(A_DUPLICATED_KEY.0), &Edit::Appended(A_DUPLICATED_KEY.1));
        derived
    })
}

/// The row [`violated`] appends, held here so the test that reads it names the same bytes.
pub(crate) const A_DUPLICATED_KEY: (&str, &str) = ("dim_customer.csv", "2,C0002,business,north\n");

/// **The same corpus with two dimension rows whose join key is ABSENT, and no duplicate anywhere.**
///
/// The corpus that decides the probe's null rule. `COUNT(col)` and `COUNT(DISTINCT col)` both skip
/// nulls, so this table counts 40 over 40 and holds its declaration up - while a probe written with
/// `COUNT(*)` would count 42 over 40 and refuse a deployment for two rows that can join to nothing.
/// A null key matches nothing on either side of any join, so it duplicates no fact row; that
/// sentence is `sutura_domain::warehouse::cardinality`'s, and this is the corpus that measures it
/// rather than leaving it to four dialects' semantics.
pub(crate) fn with_null_keys() -> &'static Derived {
    static ONCE: OnceLock<Derived> = OnceLock::new();
    ONCE.get_or_init(|| {
        let derived = derive_into("federated-differential-null-keys");
        apply(
            &derived.data.join(NULL_DIMENSION_KEYS.0),
            &Edit::Appended(NULL_DIMENSION_KEYS.1),
        );
        derived
    })
}

/// The rows [`with_null_keys`] appends. Two, not one, so counting nulls would produce a difference
/// rather than merely a larger equal pair.
pub(crate) const NULL_DIMENSION_KEYS: (&str, &str) = ("dim_customer.csv", ",C9001,consumer,south\n,C9002,consumer,south\n");

/// One data directory and two catalogs over it, derived under `stem`.
///
/// Per PROCESS because the runner gives each test its own: two processes deriving into one
/// directory would race on files whose bytes are identical, a flake with no defect behind it.
fn derive_into(stem: &str) -> Derived {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{stem}-{}", std::process::id()));
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
    apply(
        &derived.two_source.join(ON_A_SECOND_DATA_SYSTEM.at),
        &ON_A_SECOND_DATA_SYSTEM.edit(),
    );
    derived
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

pub(crate) fn lookup_source() -> SourceName {
    SourceName::parse(LOOKUP_SOURCE).expect("the second source alias is a name")
}

/// The shared corpus's questions, then the derived ones.
pub(crate) fn every_question() -> Vec<(String, Query)> {
    let mut all: Vec<(String, Query)> = questions().iter().map(|path| (stem(path), read_question(path))).collect();
    for &(name, text) in DERIVED_QUESTIONS {
        let query: Query = serde_norway::from_str(text).unwrap_or_else(|e| panic!("{name} is not a question: {e}"));
        all.push((String::from(name), query));
    }
    all
}

/// The width of the difference between the two bundles, as a property rather than a comment.
///
/// If a case were derived into one catalog and not the other, this file would report a corpus
/// asymmetry as a federation defect - the most expensive kind of false positive a differential
/// can have, because the diagnosis names the wrong subsystem.
#[test]
fn the_two_catalogs_differ_in_one_document() {
    let derived = derived();
    let mut differing: Vec<String> = Vec::new();
    // **Both roots, and that direction is the whole assertion.** Walking `one_source` alone left a
    // document only `two_source` has in neither set - not in the walk, and not in `CATALOG_CASES`
    // unless it happened to name that path - so a one-sided ADDITION passed while a one-sided
    // rewrite reddened. Review measured it: a metric added to the two-source catalog only left this
    // binary at `15 tests run: 15 passed`. `OneSided` now makes that particular addition
    // unrepresentable; this walk is what catches one arriving any other way.
    for (root, path) in [&derived.one_source, &derived.two_source]
        .into_iter()
        .flat_map(|root| every_document(root).into_iter().map(move |path| (root, path)))
    {
        let at = path
            .strip_prefix(root)
            .expect("the walk started at this root")
            .to_string_lossy()
            .into_owned();
        compare_document(derived, &at, &mut differing);
    }
    differing.sort();
    differing.dedup();
    assert_eq!(
        differing,
        vec![String::from(ON_A_SECOND_DATA_SYSTEM.at)],
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
    assert!(
        widest_span(&spanning) > 1,
        "no product in this corpus is subscribed to by two customers in one region and month, so \
         `products_in_use` would federate correctly by accident and its refusal proves nothing"
    );
    // **The other half of the pair, which is what makes the choice of metric non-arbitrary.** A
    // distinct SUBSCRIPTION key spans no customer over this corpus, so summing per-link distinct
    // counts would be exactly right for it and the refusal would be untested by the case that
    // motivates it. That was true and unasserted until review measured both spans (4 against 1).
    // A corpus edit that fanned one subscription across two customers makes `subscription_key` an
    // equally good case, and this is the line that says so instead of the file going on claiming
    // otherwise.
    let mut per_subscription: SeenUnder = std::collections::BTreeMap::new();
    for row in rows_of("fct_subscription_monthly.csv") {
        let customer = field(&row, 2);
        let Some(region) = regions.get(&customer) else {
            continue;
        };
        per_subscription
            .entry((field(&row, 0), region.clone(), field(&row, 1)))
            .or_default()
            .insert(customer);
    }
    assert_eq!(
        widest_span(&per_subscription),
        1,
        "a subscription in this corpus now spans two customers, so `subscription_key` would also be \
         a distinct value spanning the join keys - say so, or the pair above is not a pair"
    );
}

/// The most join keys any one triple was seen under.
fn widest_span(seen: &SeenUnder) -> usize {
    seen.values().map(std::collections::BTreeSet::len).max().unwrap_or(0)
}

/// One derived question, parsed.
pub(crate) fn derived_question(name: &str) -> Query {
    let (_, text) = DERIVED_QUESTIONS
        .iter()
        .find(|&&(at, _)| at == name)
        .unwrap_or_else(|| panic!("{name} is not a derived question"));
    serde_norway::from_str(text).unwrap_or_else(|e| panic!("{name} is not a question: {e}"))
}

/// The data rows of one derived CSV, header dropped.
pub(crate) fn rows_of(csv: &str) -> Vec<String> {
    let path = derived().data.join(csv);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    text.lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// One comma-separated field. The corpus quotes nothing, which is why this is not a CSV reader.
pub(crate) fn field(row: &str, at: usize) -> String {
    row.split(',').nth(at).map_or_else(String::new, String::from)
}
