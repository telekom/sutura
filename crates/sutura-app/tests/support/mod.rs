//! The fakes and the oracle: the stand-ins that let the whole surface be tested with no data system.
//!
//! In `tests/support/mod.rs` rather than `tests/support.rs` so cargo does not build it as a test
//! target of its own.
//!
//! **Nothing here is a registered adapter, and that line is the point.** `tests/adapters/mod.rs` holds
//! the registry and the two registration traits: an entry there is something somebody could deploy.
//! What is here cannot be deployed and is not meant to be:
//!
//! - [`HandWrittenCatalog`] is the **oracle**. Every registered catalog adapter is compared against
//!   it, and it is compared against nothing. Two adapters reading the same content must produce the
//!   same `Definitions`, and with one real adapter that claim is untestable - so the second statement
//!   of those definitions is written out in Rust, by hand, from the fixture documents. Generated from
//!   them it would agree by construction; sharing their parser it would share its bugs.
//! - [`RecordingWarehouse`] and [`CertifiedNumbers`] are **fakes**. Ports get fakes rather than mocked
//!   HTTP: the port is a Rust trait, so the honest stand-in is a type that implements it, and a test
//!   asserting on the text of an HTTP request would prove something about the test. They are what lets
//!   every refusal be checked with no database at all.
//! - [`TwoSourceCatalog`] provokes one refusal. It is built in code rather than as a fixture
//!   directory, because a fixture catalog spanning two data systems would make every other test in the
//!   suite span two.
//!
//! Only one test target includes this module, because a fake is used where it is needed rather than
//! everywhere: `unused_imports` and `dead_code` are both `deny` in the workspace lint table, so an
//! item one target did not use would fail the build of the other.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use sutura_catalog_local::digest_of;
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Anchor, Definitions, Dimension, Metric, Model, Relationship};
use sutura_domain::measure::{AggregatedColumn, Measure, RequiredFilter, Term, ZeroDenominator};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog};
use sutura_domain::plan::QueryPlan;
use sutura_domain::warehouse::{RowSet, Value, Warehouse};

use crate::adapters::{CatalogUnderTest, load, source, version};

// ------------------------------------------------------------------ the hand-written catalog ---

/// The same catalog, stated in Rust. **The oracle, and deliberately not a registry entry.**
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

type ModelsAndJoins = (Vec<Model>, Vec<Relationship>);

/// The two models and the one relationship between them.
///
/// Split out of `load` because a fixture catalog is a list of literals, and one function holding all
/// of them grows with every shape the vocabulary gains. Three functions that each build one kind of
/// thing stay readable where one does not.
fn tables() -> ModelsAndJoins {
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
            column("refunded"),
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

    (vec![orders, customers], joins)
}

/// Every metric the fixture declares.
///
/// Two lists rather than one, split where the vocabulary was widened: the ones the original
/// "one aggregate over one column" could express, and the ones it could not. Split for the same
/// reason [`tables`] is split out - a fixture catalog is a list of literals, and one function
/// holding all of them grows with every shape the vocabulary gains.
fn metrics() -> Vec<Metric> {
    let mut all = metrics_the_original_vocabulary_could_express();
    all.extend(metrics_the_original_vocabulary_could_not());
    all
}

/// One aggregate over one column, no filter, no ratio.
fn metrics_the_original_vocabulary_could_express() -> Vec<Metric> {
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a name"),
        ModelName::parse("orders").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
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
        Some(Anchor::new(june(), String::from("570022"))),
        String::new(),
    );
    let orders_placed = Metric::new(
        MetricName::parse("orders_placed").expect("a name"),
        ModelName::parse("orders").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Count, column("order_id")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::from([dimension(
            "region",
            "region_code",
            Some("orders_customer"),
            Some(&["north", "south", "west"]),
        )]),
        Some(Anchor::new(june(), String::from("9"))),
        String::new(),
    );
    let average_order = Metric::new(
        MetricName::parse("average_order").expect("a name"),
        ModelName::parse("orders").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Avg, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        BTreeMap::new(),
        None,
        String::new(),
    );

    vec![revenue, orders_placed, average_order]
}

/// A ratio, a required filter, and both `zero_denominator` words.
///
/// Mirroring the fixture documents of the same names. They are the reason this catalog exists: if
/// the markdown reader and this hand-written one disagree about a ratio or a required filter, one of
/// them is wrong.
fn metrics_the_original_vocabulary_could_not() -> Vec<Metric> {
    let average_order_value = Metric::new(
        MetricName::parse("average_order_value").expect("a name"),
        ModelName::parse("orders").expect("a name"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::new(Aggregate::CountDistinct, column("order_id"))),
            zero_denominator: ZeroDenominator::Null,
        },
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::new(),
        None,
        String::new(),
    );
    let web_revenue = Metric::new(
        MetricName::parse("web_revenue").expect("a name"),
        ModelName::parse("orders").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        vec![RequiredFilter::Equals {
            column: column("channel"),
            value: String::from("web"),
        }],
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::new(),
        None,
        String::new(),
    );

    // The other `zero_denominator` word, mirroring the fixture document of the same name. It is the
    // only metric here that chooses `fails`, and the reason it exists is that a variant nothing
    // executes is not covered: the enum had a test for its spelling and nothing for its behaviour.
    let revenue_per_refunded_order = Metric::new(
        MetricName::parse("revenue_per_refunded_order").expect("a name"),
        ModelName::parse("orders").expect("a name"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::CountIf {
                column: column("refunded"),
            },
            zero_denominator: ZeroDenominator::Fail,
        },
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::new(),
        None,
        String::new(),
    );

    vec![average_order_value, web_revenue, revenue_per_refunded_order]
}

impl SemanticCatalog for HandWrittenCatalog {
    type Error = Never;

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \n                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \n                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let (models, joins) = tables();
        let definitions = Definitions::assemble(models, joins, metrics()).expect("the hand-written catalog holds together");
        Ok(PinnedDefinitions::pin(version(), definitions, digest_of).expect("the definitions hash"))
    }
}

/// The same definitions with every description blanked.
///
/// The comparison the differential oracle actually makes. Prose lives in the markdown and nowhere
/// else, so comparing it would be comparing one implementation against a copy of itself. Everything
/// that decides what executes is compared.
fn without_descriptions(definitions: &Definitions) -> Definitions {
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
                metric.required_filters().to_vec(),
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

/// Everything a registered catalog says that decides what executes, with prose stripped.
pub(crate) fn executable_definitions<C>() -> Definitions
where
    C: CatalogUnderTest,
{
    without_descriptions(load::<C>().definitions())
}

/// What every registered catalog has to say, stated independently of all of them.
pub(crate) fn oracle_definitions() -> Definitions {
    let pinned = HandWrittenCatalog.load().expect("the hand-written catalog cannot fail");
    without_descriptions(pinned.definitions())
}

// ------------------------------------------------------------------------ the fake warehouse ---

/// A warehouse that runs nothing and remembers what it was asked.
///
/// What lets every refusal be checked with no database. It is a fake rather than a mock of a wire
/// protocol: the port is a Rust trait, so the honest stand-in is a type that implements it. A test
/// asserting on the text of an HTTP request would prove something about the test.
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

    /// Which metrics this warehouse was asked about, in order.
    ///
    /// It records the plan's metric rather than a rendered statement, because the port takes a plan
    /// now and this adapter never renders one. What the tests need from it is "was it reached at
    /// all", which a metric name answers and a statement would only answer more verbosely.
    pub(crate) fn asked_about(&self) -> Vec<String> {
        self.seen.borrow().clone()
    }
}

impl Warehouse for RecordingWarehouse {
    type Error = Never;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn dry_run(&self, _plan: &QueryPlan) -> Result<(), Self::Error> {
        Ok(())
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "the fixed one-cell result is a literal, so a failure to build it is a broken \n                  test rather than an input to handle"
    )]
    fn execute(&self, plan: &QueryPlan) -> Result<RowSet, Self::Error> {
        self.seen.borrow_mut().push(String::from(plan.metric().as_str()));
        // One row of nothing, shaped so `RowSet::new` accepts it. A fake that returned plausible
        // numbers would invite a test to assert on them, and those numbers would be this file's
        // opinion rather than a data system's.
        Ok(RowSet::new(vec![String::from("recorded")], vec![vec![Value::Null]]).expect("one column and one cell is rectangular"))
    }
}

// -------------------------------------------------------------- the certified-numbers fake ---

/// A data system that answers every anchor query with the number its catalog document certified.
///
/// It exists because `sutura_app::Validated` is minted by `sutura_app::verify_and_validate` and by
/// nothing else, so a test that is not *about* anchors still has to obtain its bundle from a real
/// verification pass. This is what makes that cheap, and it is what keeps the property this file is
/// built around: every refusal checked with no database at all.
///
/// **What it replaced was not a fake, it was a forgery.** The suite used to build an `AnchorReport`
/// by hand - `Matched` recorded for every anchored metric, nothing executed - and hand it to a
/// constructor that returned a bundle the service would serve. So `Validated` proved that this file
/// had asserted something, and the assertion was free. Here a statement is planned, pushed at a
/// warehouse, and the number that comes back is compared with the declared one; the only thing this
/// type gets to decide is what the data system says.
pub(crate) struct CertifiedNumbers {
    source: SourceName,
    numbers: BTreeMap<String, String>,
}

impl CertifiedNumbers {
    /// The declared number of every anchored metric in `pinned`.
    ///
    /// Read off the bundle rather than written out, so a metric gaining an anchor does not make an
    /// unrelated test fail for a reason that has nothing to do with it.
    pub(crate) fn of(pinned: &PinnedDefinitions) -> Self {
        Self {
            source: source(),
            numbers: pinned
                .anchored_metrics()
                .map(|(name, anchor)| (String::from(name.as_str()), String::from(anchor.value())))
                .collect(),
        }
    }

    /// The same data system, with one metric answering something else.
    ///
    /// A definition that has stopped computing its own number, which is the condition an anchor
    /// exists to catch and the one thing a bundle must not be servable after.
    pub(crate) fn misreporting(mut self, metric: &MetricName, value: &str) -> Self {
        drop(self.numbers.insert(String::from(metric.as_str()), String::from(value)));
        self
    }
}

impl Warehouse for CertifiedNumbers {
    type Error = Never;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn dry_run(&self, _plan: &QueryPlan) -> Result<(), Self::Error> {
        Ok(())
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "the one-cell result is built from a literal shape, so a failure to build it is a \n                  broken test rather than an input to handle"
    )]
    fn execute(&self, plan: &QueryPlan) -> Result<RowSet, Self::Error> {
        // Labelled after the plan's metric, because that is the column an anchor check looks for. A
        // metric this fake holds no number for answers nothing, which reads as a mismatch rather
        // than as a pass.
        let label = String::from(plan.metric().as_str());
        let value = self.numbers.get(&label).cloned().unwrap_or_default();
        Ok(RowSet::new(vec![label], vec![vec![Value::Text(value)]]).expect("one column and one cell is rectangular"))
    }
}

/// The fixture bundle, validated the only way there is: by running its anchors.
///
/// For the tests that need a servable bundle and are about something else - a refusal, a source
/// mismatch. `sutura_app::verify_and_validate` is the whole of the path, so this cannot drift into
/// asserting a bundle is fit to serve without the anchors having been executed.
pub(crate) fn validated_bundle(pinned: PinnedDefinitions) -> sutura_app::Validated<PinnedDefinitions> {
    let certified = CertifiedNumbers::of(&pinned);
    sutura_app::verify_and_validate(pinned, &certified).expect("a catalog's own declared numbers reproduce themselves")
}

/// June 2026, the range the fixture anchors use.
///
/// Exposed because the two-source refusal test builds a question by hand rather than from a file.
pub(crate) fn june_range() -> TimeRange {
    june()
}

/// The fixture catalog with `customers` moved to a second data system.
///
/// It exists to provoke one refusal: a plan whose join would reach a second data system is refused
/// before anything runs, because a second data system is a second identity to satisfy.
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
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            BTreeMap::from([dimension("region", "region_code", Some("orders_customer"), None)]),
            None,
            String::new(),
        );
        let definitions = Definitions::assemble(vec![orders, customers], joins, vec![revenue])
            .expect("a two-source catalog is still internally consistent");
        Ok(PinnedDefinitions::pin(version(), definitions, digest_of).expect("the definitions hash"))
    }
}
