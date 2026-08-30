//! The oracle: the same definitions every registered catalog reads, stated a second time in Rust.
//!
//! Split out of `support/mod.rs` rather than kept beside the fakes, and a gate is why: a catalog is
//! a list of literals, so this file grows with every model, metric and dimension the corpus
//! declares, and `cargo xtask max-lines` fails at a thousand lines. `devco/max-lines-ignore`
//! refuses any pattern under `crates/` on purpose - the answer there is "split the file instead" -
//! so the split happens before the limit rather than as a reaction to it.
//!
//! A submodule of `support` and not a test target of its own: `tests/*.rs` at the top level is a
//! target each, and `tests/support/oracle.rs` is a module of `tests/support/mod.rs`.
//!
//! **Nothing here is a registered adapter, and that line is the point.** [`HandWrittenCatalog`] is
//! compared against every registered catalog adapter and is itself compared against nothing. Two
//! adapters reading the same content must produce the same `Definitions`, and with one real adapter
//! that claim is untestable - so the second statement of those definitions is written out here, by
//! hand, from the catalog documents. Generated from them it would agree by construction; sharing
//! their parser it would share its bugs.
//!
//! # Transcribed from the PROSE, and reconciled with the frontmatter afterwards
//!
//! **The procedure matters as much as the content, because the failure mode of this file is a GREEN
//! test that proves nothing.** Copying each document's YAML into Rust produces an oracle that agrees
//! by transcription: it shares whatever misreading the copying carried, and
//! `agrees_with_the_oracle` then reports success over two statements of the same mistake. That is
//! strictly worse than having no oracle, because it reads as coverage.
//!
//! So every metric below was written from what its document SAYS it measures - "a count of ROWS and
//! not of subscriptions", "the whole base and not the survivors", "an event inside the month rather
//! than a state at the end of it" - and each document's frontmatter was read last, as a check on the
//! transcription rather than as its source. The one-line summary above each metric is the prose it
//! came from, kept so the next edit can be made the same way.
//!
//! Where the two ever disagree, **the disagreement is the finding**. A document whose prose and
//! frontmatter describe different definitions is a certified metric nobody can check, and the fix is
//! in the document rather than here.
//!
//! [`TwoSourceCatalog`] provokes one refusal and is here for the same reason: it is built in code
//! rather than as a catalog directory, because a corpus spanning two data systems would make every
//! other test in the suite span two. [`SameNameTablesCatalog`] is the second of those, and the same
//! sentence applies to it: qualifying the shipped corpus would move every existing golden and break
//! the executed axis to demonstrate a refusal.

// The other half of the same statement: what the catalog says ABOUT what it defines. Its own file for
// the reason this one is its own file - a hand-written catalog is a list of literals and
// `cargo xtask max-lines` fails at a thousand of them.
mod knowledge;

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Anchor, Definitions, Description, Dimension, DimensionValue, Metric, Model, Relationship};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, RequiredFilter, Term, ZeroDenominator};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, QualifiedTable, RelationshipName, SourceName,
    TableName,
};
use sutura_domain::pinned::{CatalogKind, PinnedDefinitions, SemanticCatalog};

use super::Never;
use crate::adapters::{CatalogUnderTest, load, source, version};

// ------------------------------------------------------------------ the hand-written catalog ---

/// The same catalog, stated in Rust. **The oracle, and deliberately not a registry entry.**
///
/// Descriptions are left empty here. They are prose that only the markdown carries, so the
/// comparison is made over [`without_descriptions`] rather than pretending this file repeats them.
pub(crate) struct HandWrittenCatalog;

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a corpus column is a column")
}

fn declared_value(raw: &str) -> DimensionValue {
    DimensionValue::parse(raw).expect("a corpus value is a value")
}

fn values(raw: &[&str]) -> BTreeSet<DimensionValue> {
    raw.iter().map(|v| declared_value(v)).collect()
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a corpus date is a date"),
        Date::parse("2026-07-01").expect("a corpus date is a date"),
    )
    .expect("June is a range")
}

fn dimension(name: &str, col: &str, via: Option<&str>, allowed: Option<&[&str]>) -> (DimensionName, Dimension) {
    let name = DimensionName::parse(name).expect("a corpus dimension is a dimension");
    let dimension = Dimension::new(
        name.clone(),
        column(col),
        via.map(|v| RelationshipName::parse(v).expect("a corpus relationship is a relationship")),
        allowed.map(values),
        Description::default(),
    );
    (name, dimension)
}

// -------------------------------------------------------------------------- the five dimensions ---

// **Each written once, and that is a claim about the catalog rather than a shortcut.** Five
// dimensions appear across the metric documents and every document that declares one declares it
// the same way: same column, same relationship or none, same value list. A document that started
// naming a sixth region or reaching `segment` through another relationship would disagree with the
// function below, and `agrees_with_the_oracle` is where that surfaces - so writing them once loses
// no coverage and keeps the metrics readable as definitions rather than as walls of literals.
//
// One function each rather than one table, because the metrics take different SUBSETS and picking a
// subset out of a map is more code than listing the members.

/// The commercial segment of the customer, one many-to-one join away.
fn segment() -> (DimensionName, Dimension) {
    dimension(
        "segment",
        "segment",
        Some("subscription_customer"),
        Some(&["business", "consumer", "wholesale"]),
    )
}

/// Where the customer is, through the same join.
fn region() -> (DimensionName, Dimension) {
    dimension(
        "region",
        "region",
        Some("subscription_customer"),
        Some(&["central", "east", "north", "south", "west"]),
    )
}

/// The kind of product rather than the individual tariff, through the product join.
fn product_family() -> (DimensionName, Dimension) {
    dimension(
        "product_family",
        "product_family",
        Some("subscription_product"),
        Some(&["convergent", "fixed_internet", "mobile", "tv"]),
    )
}

/// The individual tariff. **No value list, so it can be grouped by and not filtered on** - which is
/// what `refused-dimension-not-filterable.yaml` exists to reach.
fn product_name() -> (DimensionName, Dimension) {
    dimension("product_name", "product_name", Some("subscription_product"), None)
}

/// **The one dimension in this catalog that names no relationship.**
///
/// It sits on the snapshot row itself, so a plan grouping by it contributes no join at all - the
/// plainest group-by key the compiler resolves, and the one a catalog of exclusively joined
/// dimensions would never once compile. Declared on two metrics rather than one, so the case
/// survives either of them being rewritten.
fn contract_term() -> (DimensionName, Dimension) {
    dimension("contract_term", "contract_term", None, Some(&["annual", "monthly"]))
}

/// The three that four of these metrics declare together.
fn segment_region_and_family() -> BTreeMap<DimensionName, Dimension> {
    BTreeMap::from([segment(), region(), product_family()])
}

type ModelsAndJoins = (Vec<Model>, Vec<Relationship>);

/// The four models and the two relationships between them.
///
/// Split out of `load` because a catalog is a list of literals, and one function holding all of
/// them grows with every shape the vocabulary gains. Three functions that each build one kind of
/// thing stay readable where one does not.
///
/// **Two relationships and not three, and the absent one is a decision rather than an omission.**
/// `daily_usage` carries `subscription_key` and nothing reaches from there to the monthly snapshot:
/// the snapshot has one row per subscription per MONTH, so a join on that key alone would match
/// every month the subscription existed and multiply each day of usage by that count. A
/// relationship declares one column on each side, so the correct join - which would also constrain
/// the snapshot month to the usage month - cannot be written. It is therefore absent rather than
/// declared wrongly, and every metric on `daily_usage` groups by time and by nothing else.
fn tables() -> ModelsAndJoins {
    let subscriptions = Model::new(
        ModelName::parse("subscriptions").expect("a name"),
        source(),
        TableName::parse("fct_subscription_monthly").expect("a name"),
        BTreeSet::from([
            column("month"),
            column("subscription_key"),
            column("customer_key"),
            column("product_key"),
            column("status"),
            column("mrr_cents"),
            column("churned_in_month"),
            column("contract_term"),
        ]),
        Description::default(),
    );
    let customers = Model::new(
        ModelName::parse("customers").expect("a name"),
        source(),
        TableName::parse("dim_customer").expect("a name"),
        BTreeSet::from([
            column("customer_key"),
            column("customer_id"),
            column("segment"),
            column("region"),
        ]),
        Description::default(),
    );
    let products = Model::new(
        ModelName::parse("products").expect("a name"),
        source(),
        TableName::parse("dim_product").expect("a name"),
        BTreeSet::from([column("product_key"), column("product_name"), column("product_family")]),
        Description::default(),
    );
    let daily_usage = Model::new(
        ModelName::parse("daily_usage").expect("a name"),
        source(),
        TableName::parse("fct_usage_daily").expect("a name"),
        BTreeSet::from([
            column("usage_date"),
            column("subscription_key"),
            column("data_gb"),
            column("voice_min"),
        ]),
        Description::default(),
    );
    // Many subscription-months to one of each. The cardinality is declared rather than inferred
    // because it decides whether a join may change a measure: many-to-one cannot duplicate a
    // snapshot row, so a total is the same number grouped or ungrouped, and the catalog refuses to
    // reach a dimension through a direction that may duplicate.
    let joins = vec![
        Relationship::new(
            RelationshipName::parse("subscription_customer").expect("a name"),
            ModelName::parse("subscriptions").expect("a name"),
            column("customer_key"),
            ModelName::parse("customers").expect("a name"),
            column("customer_key"),
            JoinType::ManyToOne,
        ),
        Relationship::new(
            RelationshipName::parse("subscription_product").expect("a name"),
            ModelName::parse("subscriptions").expect("a name"),
            column("product_key"),
            ModelName::parse("products").expect("a name"),
            column("product_key"),
            JoinType::ManyToOne,
        ),
    ];

    (vec![subscriptions, customers, products, daily_usage], joins)
}

/// Every metric the catalog declares.
///
/// Two lists rather than one, split where the vocabulary was widened: the ones the original
/// "one aggregate over one column" could express, and the ones it could not. Split for the same
/// reason [`tables`] is split out - a catalog is a list of literals, and one function holding all
/// of them grows with every shape the vocabulary gains.
fn metrics() -> Vec<Metric> {
    let mut all = metrics_the_original_vocabulary_could_express();
    all.extend(metrics_the_original_vocabulary_could_not());
    all
}

/// One aggregate over one column, no filter, no ratio.
///
/// Three of the eleven, and the plainest three. `voice_minutes` is here on purpose rather than by
/// accident: most certified metrics look like this, and a catalog whose every entry needed a
/// paragraph of justification would be a catalog nobody trusted.
fn metrics_the_original_vocabulary_could_express() -> Vec<Metric> {
    // "How many subscriptions the month held, whatever state they ended it in." Unfiltered as the
    // definition rather than by oversight: this is the denominator a churn share is taken against,
    // and a base counting only the survivors would leave the terminated subscriptions in the
    // numerator and out of the denominator at the same time.
    let subscription_base = Metric::new(
        MetricName::parse("subscription_base").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::CountDistinct,
            column("subscription_key"),
        ))),
        Vec::new(),
        column("month"),
        BTreeSet::from([Grain::Month]),
        segment_region_and_family(),
        Some(Anchor::new(june(), String::from("62"))),
        Description::default(),
    );
    // "How many subscription-months the period billed." A count of ROWS and not of subscriptions:
    // the same column as `subscription_base` under a different aggregate, and the same number over
    // this catalog only because the snapshot holds one row per subscription per month and this
    // declares only the month grain. Two anchors over the same month, so an edit that collapsed one
    // definition into the other cannot pass as a rename. **The only plain `count` in the
    // repository**, so no other document renders that generator arm.
    let subscription_months_billed = Metric::new(
        MetricName::parse("subscription_months_billed").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Count,
            column("subscription_key"),
        ))),
        Vec::new(),
        column("month"),
        BTreeSet::from([Grain::Month]),
        BTreeMap::from([segment(), region(), product_family(), contract_term()]),
        Some(Anchor::new(june(), String::from("62"))),
        Description::default(),
    );
    // "Outgoing voice minutes." One aggregate over one column, no definitional filter, no join, no
    // ratio. No anchor, for the reason `data_per_subscription` gives: a sum of decimals is a float,
    // so an anchor written as text would pin a formatting decision rather than a number.
    let voice_minutes = Metric::new(
        MetricName::parse("voice_minutes").expect("a name"),
        ModelName::parse("daily_usage").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("voice_min")))),
        Vec::new(),
        column("usage_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::new(),
        None,
        Description::default(),
    );

    vec![subscription_base, subscription_months_billed, voice_minutes]
}

/// A required filter, a mean of a column, a conditional count, and four ratios.
///
/// Mirroring the catalog documents of the same names. They are the reason this file exists: if the
/// markdown reader and this hand-written one disagree about a ratio, a required filter or which half
/// of a ratio a conditional count sits in, one of them is wrong.
///
/// Two halves rather than one body, and the seam was chosen by a gate: `clippy::too_many_lines`
/// fails at a hundred and eight metrics do not fit. The split is between the ones that widened the
/// SHAPE and the ones that widened what a shape may hold - four `simple` measures that each needed
/// something the first vocabulary had no field for, and the four `ratio`s.
fn metrics_the_original_vocabulary_could_not() -> Vec<Metric> {
    let mut all = simple_measures_that_needed_a_wider_vocabulary();
    all.extend(the_ratios());
    all
}

/// A definitional filter, a mean of a column, and a conditional count as a whole measure.
///
/// All four are `simple` in shape. What the first vocabulary could not write is on each of them
/// separately: a predicate that is part of the name, `avg` over a column, and `count_if` as a term.
fn simple_measures_that_needed_a_wider_vocabulary() -> Vec<Metric> {
    // "Recurring revenue recognised in the month, in minor units, from active subscriptions only."
    // The filter is part of the NAME: a statement that left the predicate out would return revenue
    // including terminated subscriptions under a certified name, so a caller can neither see it nor
    // turn it off, and `status` is not a dimension here. `contract_term` is declared without a
    // relationship - see [`contract_term`].
    let recurring_revenue = Metric::new(
        MetricName::parse("recurring_revenue").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents")))),
        vec![RequiredFilter::Equals {
            column: column("status"),
            value: declared_value("active"),
        }],
        column("month"),
        BTreeSet::from([Grain::Month]),
        BTreeMap::from([segment(), region(), product_family(), product_name(), contract_term()]),
        Some(Anchor::new(june(), String::from("202121"))),
        Description::default(),
    );
    // "How many subscriptions were active at the end of the month." `subscription_base` with one
    // predicate added, counting the key distinctly rather than counting rows. **The two anchors are
    // what `required_filters` buys**: 59 against 62 over the same rows in the same month, with one
    // definitional predicate between them, shown as two numbers rather than asserted in a sentence.
    let active_subscriptions = Metric::new(
        MetricName::parse("active_subscriptions").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::CountDistinct,
            column("subscription_key"),
        ))),
        vec![RequiredFilter::Equals {
            column: column("status"),
            value: declared_value("active"),
        }],
        column("month"),
        BTreeSet::from([Grain::Month]),
        segment_region_and_family(),
        Some(Anchor::new(june(), String::from("59"))),
        Description::default(),
    );
    // "What the average active subscription was worth in the month, in minor units." The mean of a
    // COLUMN, which is a different definition from every ratio here: it averages subscription-MONTHS,
    // where `revenue_per_customer` averages customers. **The only `avg` in the repository**, so an
    // aggregate no other document writes is a generator arm nothing else renders. No anchor: a mean
    // is a division, a division is not exact in binary, and an anchor is compared as rendered text.
    let mean_subscription_mrr = Metric::new(
        MetricName::parse("mean_subscription_mrr").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Avg, column("mrr_cents")))),
        vec![RequiredFilter::Equals {
            column: column("status"),
            value: declared_value("active"),
        }],
        column("month"),
        BTreeSet::from([Grain::Month]),
        BTreeMap::new(),
        None,
        Description::default(),
    );
    // "How many subscriptions terminated inside the month." `count_if` and not a count of the
    // column, and the difference is a wrong number that raises no error: `count(churned_in_month)`
    // counts the rows where the column is not null, which is every row, so it would report the whole
    // base as churn. No status filter, because churn is an event INSIDE the month rather than a state
    // at the end of it, and narrowing on the surviving state would remove the rows being counted.
    let subscriptions_churned = Metric::new(
        MetricName::parse("subscriptions_churned").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Simple(Term::CountIf {
            column: column("churned_in_month"),
        }),
        Vec::new(),
        column("month"),
        BTreeSet::from([Grain::Month]),
        segment_region_and_family(),
        Some(Anchor::new(june(), String::from("3"))),
        Description::default(),
    );
    vec![
        recurring_revenue,
        active_subscriptions,
        mean_subscription_mrr,
        subscriptions_churned,
    ]
}

/// The four ratios, and between them both meanings of an empty denominator and both positions a
/// conditional count can sit in.
///
/// `churn_rate` puts one in the NUMERATOR and `revenue_per_churned_subscription` in the
/// DENOMINATOR: nothing about the vocabulary makes the two positions different, and a catalog that
/// wrote only one of them would demonstrate half of that and read as though it had shown all of it.
fn the_ratios() -> Vec<Metric> {
    // "What share of the month's subscriptions terminated inside it." The two halves are the two
    // metrics that used to ship in its place: `subscriptions_churned` over `subscription_base`,
    // unfiltered, because a base counting only the survivors would leave the terminated
    // subscriptions in the numerator and out of the denominator at once. `yields_null` because a
    // month with no subscriptions at all has no churn share - zero would claim they all survived.
    //
    // **The only anchor in the repository that re-executes a conditional count inside a ratio**,
    // which is the shape the vocabulary was changed for, and it is anchorable where the other ratios
    // are not because both halves are exact integers in a double: one division, one rounding, and
    // nothing before it an evaluation order could perturb. Readable rather than opaque because both
    // halves are separately certified - 3 over 62, over this same month.
    let churn_rate = Metric::new(
        MetricName::parse("churn_rate").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Ratio {
            numerator: Term::CountIf {
                column: column("churned_in_month"),
            },
            denominator: Term::Aggregate(AggregatedColumn::new(Aggregate::CountDistinct, column("subscription_key"))),
            zero_denominator: ZeroDenominator::Null,
        },
        Vec::new(),
        column("month"),
        BTreeSet::from([Grain::Month]),
        segment_region_and_family(),
        Some(Anchor::new(june(), String::from("0.04838709677419355"))),
        Description::default(),
    );
    // "Data volume per subscription, in gigabytes." A ratio with no definitional filter, which is the
    // other half of what `required_filters` is for: this one means what it measures over every row in
    // range and there is no predicate a reader has to be warned about. The denominator counts the
    // subscriptions that APPEAR in the range rather than every subscription that existed during it,
    // so it is volume per subscription that used the network. No dimensions at all, because
    // `daily_usage` reaches the snapshot through no relationship. No anchor: a sum of decimal
    // gigabytes over a count is a float.
    let data_per_subscription = Metric::new(
        MetricName::parse("data_per_subscription").expect("a name"),
        ModelName::parse("daily_usage").expect("a name"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("data_gb"))),
            denominator: Term::Aggregate(AggregatedColumn::new(Aggregate::CountDistinct, column("subscription_key"))),
            zero_denominator: ZeroDenominator::Null,
        },
        Vec::new(),
        column("usage_date"),
        BTreeSet::from([Grain::Day, Grain::Week, Grain::Month]),
        BTreeMap::new(),
        None,
        Description::default(),
    );
    // "Recurring revenue per customer, in minor units, over active subscriptions." Per CUSTOMER and
    // not per subscription, which is the whole reason it is a ratio rather than an `avg`: a customer
    // holding three subscriptions is one customer and three rows, so the two answers differ by
    // however much the base fans out. `yields_null` because a month with no customers is a month
    // with no revenue per customer, which is a different statement from a figure of zero.
    let revenue_per_customer = Metric::new(
        MetricName::parse("revenue_per_customer").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::new(Aggregate::CountDistinct, column("customer_key"))),
            zero_denominator: ZeroDenominator::Null,
        },
        vec![RequiredFilter::Equals {
            column: column("status"),
            value: declared_value("active"),
        }],
        column("month"),
        BTreeSet::from([Grain::Month]),
        BTreeMap::from([segment()]),
        None,
        Description::default(),
    );
    // "How much recurring revenue the month carried for each subscription it lost, in minor units."
    // **The mirror of `churn_rate`**: there a conditional count is the NUMERATOR of a ratio, here it
    // is the DENOMINATOR. Nothing about the vocabulary makes the two positions different, and a
    // catalog that only ever wrote one of them would demonstrate half of that claim.
    //
    // No status filter, which is `churn_rate`'s argument about its denominator turned around: the
    // rows the denominator counts are the terminated ones, so narrowing to the surviving state would
    // take them out of the numerator while leaving them in the denominator. The numerator is the
    // whole base, which is what the name means - revenue the month carried, not revenue it kept.
    //
    // **The only metric here that chooses `fails`**, and the reason it exists is that a variant
    // nothing executes is not covered: the enum had a test for its spelling and nothing for its
    // behaviour, which is how `fails` came to answer the string `inf` under a certified name. No
    // anchor, and not for the float reason - both halves are exact integers in a double. An anchor
    // is re-executed before the bundle may answer anything, so anchoring this would mean picking a
    // range where the denominator happens not to be zero and making readiness depend on that staying
    // true; the period it exists to demonstrate is the one where it has no figure.
    let revenue_per_churned_subscription = Metric::new(
        MetricName::parse("revenue_per_churned_subscription").expect("a name"),
        ModelName::parse("subscriptions").expect("a name"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents"))),
            denominator: Term::CountIf {
                column: column("churned_in_month"),
            },
            zero_denominator: ZeroDenominator::Fail,
        },
        Vec::new(),
        column("month"),
        BTreeSet::from([Grain::Month]),
        BTreeMap::new(),
        None,
        Description::default(),
    );

    vec![
        churn_rate,
        data_per_subscription,
        revenue_per_customer,
        revenue_per_churned_subscription,
    ]
}

impl SemanticCatalog for HandWrittenCatalog {
    type Error = Never;

    /// **Declaring, and that is the honest class for the oracle to hold.** It states the whole model
    /// except prose - descriptions live in the markdown and nowhere else - so it supplies part of
    /// the model and is measured against its declaration rather than against itself. The
    /// declaration below is the two directions of that in one value.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// **Everything except prose, and that makes this suite's oracle its own worked declaring
    /// adapter.** Descriptions are deliberately left empty here - they live in the markdown and
    /// nowhere else, which is the whole reason `without_descriptions` exists - so an adapter that
    /// declared them would be claiming something it does not supply. Written as
    /// [`DefinitionCapabilities::of`] over the other eight rather than as `all()` minus one, because
    /// `of` is what an adapter mapping a schema it does not own writes, and a tenth kind must not
    /// widen this line by accident.
    ///
    /// **This is not a claim checked against itself.** The declaration says what this fake is FOR,
    /// which is stated in its own doc comment and was stated there before this line existed;
    /// `a_declaring_adapter_provides_exactly_what_it_declares` in `tests/golden/catalogs.rs` is what
    /// compares it against the bundle. Declare `Descriptions` here and that test goes red naming the
    /// kind.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Relationships,
                DefinitionKind::Cardinality,
                DefinitionKind::Metrics,
                DefinitionKind::RequiredFilters,
                DefinitionKind::Grains,
                DefinitionKind::AllowedValues,
                DefinitionKind::Anchors,
            ]),
            // All four, and this half is not narrowed: the notes are written out in Rust in
            // `oracle/knowledge.rs` with bodies, so every knowledge kind is really carried.
            sutura_domain::knowledge::KnowledgeCapabilities::all(),
        )
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \n                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \n                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let (models, joins) = tables();
        let definitions = Definitions::assemble(models, joins, metrics()).expect("the hand-written catalog holds together");
        // The knowledge is checked against these definitions, by the same function every adapter goes
        // through - so a hand-written note naming a metric this hand-written catalog does not define
        // fails here rather than being compared successfully against a markdown one that also has it
        // wrong.
        let stated = knowledge::stated(&definitions);
        Ok(PinnedDefinitions::pin(version(), definitions, stated).expect("the definitions hash"))
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
                Description::default(),
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
                            Description::default(),
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
                Description::default(),
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

/// What a registered catalog says ABOUT what it defines, with the prose blanked.
pub(crate) fn stated_knowledge<C>() -> Knowledge
where
    C: CatalogUnderTest,
{
    let pinned = load::<C>();
    knowledge::without_bodies(pinned.definitions(), pinned.knowledge())
}

/// The same, stated independently of every adapter.
///
/// The notes are written out in Rust in `oracle/knowledge.rs`, from the documents' own prose, for the
/// reason the metrics are: generated from them this would agree by construction, and sharing their
/// parser it would share its bugs.
pub(crate) fn oracle_knowledge() -> Knowledge {
    let pinned = HandWrittenCatalog.load().expect("the hand-written catalog cannot fail");
    knowledge::without_bodies(pinned.definitions(), pinned.knowledge())
}

/// June 2026, the range every anchor in this catalog is written for.
///
/// Exposed because the two-source refusal test builds a question by hand rather than from a file.
pub(crate) fn june_range() -> TimeRange {
    june()
}

/// The snapshot and its customers, with `customers` moved to a second data system.
///
/// It exists to provoke one refusal: a plan whose join would reach a second data system is refused
/// before anything runs, because a second data system is a second identity to satisfy.
///
/// **Two models and one metric rather than the whole catalog, and the reduction is deliberate.**
/// Nothing here ever executes and nothing compares it against a document, so carrying eleven
/// metrics would be eleven more literals to keep in step with a directory this type is not a
/// statement of. What it does have to carry is the shape the refusal needs: a metric on the LOCAL
/// model whose dimension is reached `via` a relationship whose target sits ELSEWHERE. The
/// definitional filter the real `recurring_revenue` declares is left off for the same reason - it
/// changes no plan that is refused before planning finishes.
pub(crate) fn two_source_catalog() -> TwoSourceCatalog {
    TwoSourceCatalog
}

/// See [`two_source_catalog`].
pub(crate) struct TwoSourceCatalog;

impl SemanticCatalog for TwoSourceCatalog {
    type Error = Never;

    /// **Declaring**, the narrow pole of the fidelity test: two models on two data systems and none
    /// of the kinds a plan would only consume after the refusal this fake exists to provoke.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// **Five declared absences, which is what makes this the narrow end of the fidelity test.** Two
    /// models on two data systems, one metric, one join that licenses one dimension - and no prose,
    /// no definitional filter, no value allowlist and no anchor, because the refusal this fake exists
    /// to provoke happens before any of those would matter. Its own doc comment says so; this line is
    /// the same reduction stated where a caller could read it.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Relationships,
                DefinitionKind::Cardinality,
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
            ]),
            sutura_domain::knowledge::KnowledgeCapabilities::none(),
        )
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \n                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \n                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let subscriptions = Model::new(
            ModelName::parse("subscriptions").expect("a name"),
            source(),
            TableName::parse("fct_subscription_monthly").expect("a name"),
            BTreeSet::from([
                column("month"),
                column("subscription_key"),
                column("customer_key"),
                column("mrr_cents"),
            ]),
            Description::default(),
        );
        let customers = Model::new(
            ModelName::parse("customers").expect("a name"),
            SourceName::parse("elsewhere").expect("a name"),
            TableName::parse("dim_customer").expect("a name"),
            BTreeSet::from([column("customer_key"), column("region")]),
            Description::default(),
        );
        let joins = vec![Relationship::new(
            RelationshipName::parse("subscription_customer").expect("a name"),
            ModelName::parse("subscriptions").expect("a name"),
            column("customer_key"),
            ModelName::parse("customers").expect("a name"),
            column("customer_key"),
            JoinType::ManyToOne,
        )];
        let recurring_revenue = Metric::new(
            MetricName::parse("recurring_revenue").expect("a name"),
            ModelName::parse("subscriptions").expect("a name"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_cents")))),
            Vec::new(),
            column("month"),
            BTreeSet::from([Grain::Month]),
            BTreeMap::from([dimension("region", "region", Some("subscription_customer"), None)]),
            None,
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![subscriptions, customers], joins, vec![recurring_revenue])
            .expect("a two-source catalog is still internally consistent");
        Ok(PinnedDefinitions::pin(version(), definitions, Knowledge::none()).expect("the definitions hash"))
    }
}

// ------------------------------------------------- a catalog whose two tables share a name ---

/// Two models in two datasets whose tables are both called `orders`.
///
/// It exists to provoke one refusal, and that refusal is the one a review reproduced: a column in a
/// plan is qualified by the LAST part of a table path, so two paths ending the same way render under
/// one implicit alias and the `ON` clause compares one table with itself. A real `DuckDB` answers such a
/// statement with `Binder Error: Ambiguous reference to table "orders"`; a target that binds it to one
/// side instead returns a number under a certified metric name.
///
/// **The catalog LOADS, and that is the design decision rather than an oversight.** Unlike a colliding
/// label, a physical table name is not something an author can rename, and same-name tables across
/// datasets are the normal shape of the estate qualified paths exist for - so the metric stays
/// authorable and only a question that actually puts both tables in one statement is declined.
/// `sutura_domain::plan::tables` is where that is argued and where the guard lives.
///
/// **Two models, one relationship and one metric, for the reason [`TwoSourceCatalog`] gives:** nothing
/// here executes and nothing compares it against a document, so what it carries is the shape the
/// refusal needs and nothing else. The metric has TWO dimensions rather than one, and that is the
/// exception: one is reached through the colliding join and one is not, so a test can show the refusal
/// is about the QUESTION rather than about the metric. Both models are on ONE source, which is the
/// half that makes this about aliasing rather than about federation.
pub(crate) fn same_name_tables_catalog() -> SameNameTablesCatalog {
    SameNameTablesCatalog
}

/// See [`same_name_tables_catalog`].
pub(crate) struct SameNameTablesCatalog;

impl SemanticCatalog for SameNameTablesCatalog {
    type Error = Never;

    /// **Declaring**, for [`TwoSourceCatalog`]'s reason: it supplies part of the model and none of
    /// the kinds a plan would consume after the refusal this fake exists to provoke.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// The same five declared absences [`TwoSourceCatalog`] declares, and for the same reason: the
    /// refusal this fake provokes happens before prose, a definitional filter, an allowlist or an
    /// anchor would matter.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Relationships,
                DefinitionKind::Cardinality,
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
            ]),
            sutura_domain::knowledge::KnowledgeCapabilities::none(),
        )
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \
                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \
                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let fact = Model::new(
            ModelName::parse("sales_orders").expect("a name"),
            source(),
            QualifiedTable::parse("analytics_prod.sales.orders").expect("a path"),
            BTreeSet::from([column("order_date"), column("customer_id"), column("amount_cents")]),
            Description::default(),
        );
        // The SAME table name, in another dataset of another project, reached by the same credential.
        // One source, two paths, one implicit alias.
        let lookup = Model::new(
            ModelName::parse("crm_orders").expect("a name"),
            source(),
            QualifiedTable::parse("reference_data.crm.orders").expect("a path"),
            BTreeSet::from([column("customer_id"), column("region")]),
            Description::default(),
        );
        let joins = vec![Relationship::new(
            RelationshipName::parse("order_crm").expect("a name"),
            ModelName::parse("sales_orders").expect("a name"),
            column("customer_id"),
            ModelName::parse("crm_orders").expect("a name"),
            column("customer_id"),
            JoinType::ManyToOne,
        )];
        let revenue = Metric::new(
            MetricName::parse("revenue").expect("a name"),
            ModelName::parse("sales_orders").expect("a name"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            // Two dimensions on purpose: one needs the colliding join and one does not, so the test
            // can show that the refusal is about the QUESTION rather than about the metric.
            BTreeMap::from([
                dimension("region", "region", Some("order_crm"), None),
                dimension("customer", "customer_id", None, None),
            ]),
            None,
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![fact, lookup], joins, vec![revenue])
            .expect("two tables of one name are still internally consistent - the QUESTION is what is refused");
        Ok(PinnedDefinitions::pin(version(), definitions, Knowledge::none()).expect("the definitions hash"))
    }
}

// -------------------------- a catalog whose FEDERATED fact leg reads two tables of one name ---

/// [`SameNameTablesCatalog`]'s collision, on a catalog that also reaches a SECOND data system.
///
/// **It exists because the guard that catches the collision was reached by one of the two plan
/// shapes and not by the other, and the second shape is the one that renders the wrong number.**
/// `sutura_semantic::plan` splits a two-source question into a fact leg and a lookup leg; the fact
/// leg keeps every SAME-SOURCE hop as a `JOIN` of its own, so the whole ambiguity
/// [`SameNameTablesCatalog`] provokes is available inside one leg's statement - and until the change
/// this fake arrived with, the splitter built that leg by struct literal and never asked
/// `sutura_domain::plan::StatementTables::parse` about it.
///
/// **Three models rather than two, and each one is load-bearing.** The fact model and the
/// same-source dimension model are the collision - two paths ending in `orders`, one credential, one
/// statement. The third model sits on `elsewhere`, and it is what makes the question FEDERATE rather
/// than take the whole-answer path the sibling fake already covers: without it the plan stage would
/// never reach the splitter, and the bypass would stay invisible.
pub(crate) fn federated_same_name_tables_catalog() -> FederatedSameNameTablesCatalog {
    FederatedSameNameTablesCatalog
}

/// See [`federated_same_name_tables_catalog`].
pub(crate) struct FederatedSameNameTablesCatalog;

impl SemanticCatalog for FederatedSameNameTablesCatalog {
    type Error = Never;

    /// **Declaring**, for [`TwoSourceCatalog`]'s reason: it supplies part of the model and none of
    /// the kinds a plan would consume after the refusal this fake exists to provoke.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// The same five declared absences [`TwoSourceCatalog`] declares, and for the same reason.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Relationships,
                DefinitionKind::Cardinality,
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
            ]),
            sutura_domain::knowledge::KnowledgeCapabilities::none(),
        )
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "every value here is a literal in this file, so a parse failure is a broken test \
                  rather than an input to handle; `allow-expect-in-tests` covers the bare lint but \
                  not this one, which fires on position rather than on being test code"
    )]
    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let fact = Model::new(
            ModelName::parse("sales_orders").expect("a name"),
            source(),
            QualifiedTable::parse("analytics_prod.sales.orders").expect("a path"),
            BTreeSet::from([column("order_date"), column("customer_id"), column("amount_cents")]),
            Description::default(),
        );
        // The SAME table name in another dataset of another project, on the SAME source - so the
        // splitter keeps it as a join on the fact leg and both paths land in one statement.
        let crm = Model::new(
            ModelName::parse("crm_orders").expect("a name"),
            source(),
            QualifiedTable::parse("reference_data.crm.orders").expect("a path"),
            BTreeSet::from([column("customer_id"), column("segment")]),
            Description::default(),
        );
        // The second data system, and the only reason this question federates at all.
        let geo = Model::new(
            ModelName::parse("geo").expect("a name"),
            SourceName::parse("elsewhere").expect("a name"),
            TableName::parse("dim_region").expect("a name"),
            BTreeSet::from([column("customer_id"), column("region")]),
            Description::default(),
        );
        let joins = vec![
            Relationship::new(
                RelationshipName::parse("order_crm").expect("a name"),
                ModelName::parse("sales_orders").expect("a name"),
                column("customer_id"),
                ModelName::parse("crm_orders").expect("a name"),
                column("customer_id"),
                JoinType::ManyToOne,
            ),
            Relationship::new(
                RelationshipName::parse("order_geo").expect("a name"),
                ModelName::parse("sales_orders").expect("a name"),
                column("customer_id"),
                ModelName::parse("geo").expect("a name"),
                column("customer_id"),
                JoinType::ManyToOne,
            ),
        ];
        let revenue = Metric::new(
            MetricName::parse("revenue").expect("a name"),
            ModelName::parse("sales_orders").expect("a name"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            // `segment` reaches the colliding same-source table, `region` reaches the second data
            // system, and `customer` reaches neither - so one question can federate WITH the
            // collision, and another can federate without it.
            BTreeMap::from([
                dimension("segment", "segment", Some("order_crm"), None),
                dimension("region", "region", Some("order_geo"), None),
                dimension("customer", "customer_id", None, None),
            ]),
            None,
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![fact, crm, geo], joins, vec![revenue])
            .expect("two tables of one name are still internally consistent - the QUESTION is what is refused");
        Ok(PinnedDefinitions::pin(version(), definitions, Knowledge::none()).expect("the definitions hash"))
    }
}
