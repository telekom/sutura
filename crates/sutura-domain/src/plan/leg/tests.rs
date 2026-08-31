//! What the leg shapes are, asserted where possible by a pattern rather than by a value.
//!
//! **Several of these are compile-time assertions written inside a `#[test]`, on purpose.** An
//! exhaustive struct pattern with no `..` fails to compile when a field is added or removed, so a
//! test whose body destructures a variant completely is a claim about the SHAPE that a reviewer can
//! read by name - which is what the plan asked for when it said *the type, not a runtime check*.
//! The unrepresentability claims that cannot be written as a pattern at all are `compile_fail`
//! doctests on the types themselves, each with a compiling twin so a rename cannot make one pass
//! vacuously.

use super::{Executable, LegPlan, LegTerm};
use crate::calendar::{Date, TimeRange};
use crate::federation::{Above, Carried, Federation};
use crate::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
use crate::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use crate::plan::{PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanPredicate, PlanTerm, PredicateOrigin, StatementTables};
use crate::warehouse::ParamValue;

fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

fn other_source() -> SourceName {
    SourceName::parse("crm").expect("a test source is a source")
}

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a test table is a table")
}

fn column(table_name: &str, column_name: &str) -> PlanColumn {
    PlanColumn::new(
        table(table_name),
        ColumnName::parse(column_name).expect("a test column is a column"),
    )
}

fn range() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("a test range is a range")
}

fn bucket() -> PlanBucket {
    PlanBucket::new(
        String::from("period"),
        Grain::Month,
        column("fct_subscription_monthly", "month"),
    )
}

fn key(label: &str, table_name: &str, column_name: &str) -> PlanKey {
    PlanKey::new(String::from(label), column(table_name, column_name))
}

fn fact(terms: Vec<LegTerm>) -> LegPlan {
    LegPlan::Fact {
        source: source(),
        metric: MetricName::parse("recurring_revenue").expect("a test metric is a metric"),
        tables: StatementTables::only(table("fct_subscription_monthly")),
        bucket: bucket(),
        keys: vec![key("customer_key", "fct_subscription_monthly", "customer_key")],
        terms,
        filters: vec![PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::AtOrAfter {
                column: column("fct_subscription_monthly", "month"),
                param: 0,
            },
        )],
        params: vec![ParamValue::Date(Date::parse("2026-06-01").expect("a test date is a date"))],
        range: range(),
    }
}

fn lookup() -> LegPlan {
    LegPlan::Lookup {
        source: other_source(),
        table: table("dim_customer").into(),
        keys: vec![
            key("customer_key", "dim_customer", "customer_key"),
            key("region", "dim_customer", "region"),
        ],
        filters: Vec::new(),
        params: Vec::new(),
    }
}

/// The average of a column, which is the one measure that decomposes.
fn mean_of(column_name: &str) -> Measure {
    Measure::Simple(Term::Aggregate(AggregatedColumn::new(
        Aggregate::Avg,
        ColumnName::parse(column_name).expect("a test column is a column"),
    )))
}

/// The terms a leg carries for one measure, taken from the classification rather than chosen here.
///
/// This is a TEST caller of [`crate::federation`] and not a production one: there is still no
/// splitter, so nothing in a binary builds a leg. What it buys is that the fixtures below cannot
/// disagree with the classification about how many columns travel - a hand-picked pair of terms
/// would prove the shape is expressible and nothing about whether anything produces it.
fn carried_terms(measure: &Measure) -> Vec<LegTerm> {
    let federation = Federation::of(measure);
    federation
        .carried()
        .into_iter()
        .enumerate()
        .map(|(index, carried)| {
            let term = match *carried {
                Carried::Aggregated { pushed, ref column } => PlanTerm::Aggregate {
                    aggregate: pushed.push(),
                    column: column_of(column),
                },
                Carried::CountIf { ref column } => PlanTerm::CountIf {
                    column: column_of(column),
                },
                // A pulled-up column is not a term at all: it travels in `keys`. A fixture that
                // reached here would be building the distinct-key leg wrongly.
                Carried::Keys { .. } => panic!("a pulled-up column is a key, not a term"),
            };
            LegTerm::new(term, format!("term_{index}"))
        })
        .collect()
}

fn column_of(name: &ColumnName) -> PlanColumn {
    PlanColumn::new(table("fct_subscription_monthly"), name.clone())
}

#[test]
fn a_lookup_leg_has_no_measure_and_no_bucket() {
    // The assertion is the PATTERN, not the bodies. Destructured with no `..`, so a `bucket`, a
    // `terms` or a `measure` field added to this variant fails to compile here - which is the shape
    // claim `docs/adr/0007` makes when it says a dimension model has no time column and no measure.
    let LegPlan::Lookup {
        ref source,
        ref table,
        ref keys,
        ref filters,
        ref params,
    } = lookup()
    else {
        panic!("the lookup fixture is a lookup");
    };
    assert_eq!(source.as_str(), "crm");
    assert_eq!(table.to_string(), "dim_customer");
    assert_eq!(keys.len(), 2);
    assert!(filters.is_empty());
    assert!(params.is_empty());
    // And the labels it projects are its keys and nothing else: no bucket label, no measure label.
    assert_eq!(
        lookup().result_labels(),
        vec![String::from("customer_key"), String::from("region")]
    );
}

#[test]
fn a_lookup_leg_cannot_carry_a_time_range() {
    // The same pattern read for the other absence, and the reason the two-variant split is the one
    // 0007 chose over folding both distinct shapes into one: that collapse needs
    // `Option<PlanBucket>` and `Option<TimeRange>` on the shared variant, which makes a dimension
    // leg carrying a time range CONSTRUCTIBLE. Here it is not, and the `compile_fail` doctest on
    // `LegPlan` is the half of this claim a pattern cannot state.
    //
    // **Asserted against the fact leg rather than over an empty set**, because a `Lookup` has no
    // range field to read and a test that only checked its filter list would pass with the whole
    // distinction removed. So the comparison is the one that carries information: the shape that
    // reads a time column HAS a range and bounds it with two predicates, and the shape that reads a
    // dimension table has neither - and cannot be given either, which is the doctest's half.
    let LegPlan::Fact {
        range: bounded,
        ref filters,
        ..
    } = fact(Vec::new())
    else {
        panic!("the fact fixture is a fact");
    };
    assert_eq!(bounded, range());
    assert!(
        filters
            .iter()
            .any(|filter| matches!(*filter.predicate(), PlanPredicate::AtOrAfter { .. })),
        "a fact leg bounds its range as a predicate"
    );

    let LegPlan::Lookup { ref filters, .. } = lookup() else {
        panic!("the lookup fixture is a lookup");
    };
    assert!(
        !filters.iter().any(|filter| matches!(
            *filter.predicate(),
            PlanPredicate::AtOrAfter { .. } | PlanPredicate::Before { .. }
        )),
        "a lookup leg has no time column for a range to bound"
    );
}

#[test]
fn a_leg_cannot_be_constructed_that_divides_a_ratio() {
    // **The mechanical half of this claim is the `compile_fail` doctest on [`LegTerm`]**, which is
    // where a shape claim has to live: `LegTerm::new` takes a `PlanTerm`, a ratio is a `PlanMeasure`,
    // and handing one over is an E0308 rather than something a reviewer has to notice. That block
    // was checked non-vacuous by unmarking it.
    //
    // What is asserted here is the other side, and it is the side a reviewer can read: a ratio
    // measure federates as two undivided terms with the quotient ABOVE them, so the shape a leg
    // cannot hold is not a shape anything wanted it to hold.
    let ratio = Measure::Ratio {
        numerator: Term::Aggregate(AggregatedColumn::new(
            Aggregate::Sum,
            ColumnName::parse("mrr_cents").expect("a test column is a column"),
        )),
        denominator: Term::Aggregate(AggregatedColumn::new(
            Aggregate::CountDistinct,
            ColumnName::parse("customer_key").expect("a test column is a column"),
        )),
        zero_denominator: ZeroDenominator::Null,
    };
    let federation = Federation::of(&ratio);
    assert!(
        matches!(*federation.above(), Above::Quotient { zero_denominator, .. } if zero_denominator == ZeroDenominator::Null),
        "the zero guard the definition asked for applies once, above every leg"
    );
    // The numerator descends as a term and the denominator does not descend at all, so this ratio
    // travels as one term column and one grouping key rather than as a division. The leg that
    // carries it holds neither half's guard, because there is no field for one.
    let carried = federation.carried();
    assert_eq!(carried.len(), 2);
    assert!(
        federation.pulls_up_rows(),
        "an exact distinct count in the denominator is what makes this ratio pull rows up"
    );
    let leg = fact(vec![term_of(carried.first().copied())]);
    let LegPlan::Fact { ref terms, .. } = leg else {
        panic!("the fact fixture is a fact");
    };
    assert_eq!(terms.len(), 1, "the numerator travels alone, undivided: {terms:?}");
}

/// One carried column as a leg term, for the ratio test above.
///
/// Separate from [`carried_terms`] because that one refuses a pulled-up column - it belongs in
/// `keys` - and this test wants only the half that does descend.
fn term_of(carried: Option<&Carried>) -> LegTerm {
    let Some(&Carried::Aggregated { pushed, ref column }) = carried else {
        panic!("the numerator of this ratio descends as one aggregate");
    };
    LegTerm::new(
        PlanTerm::Aggregate {
            aggregate: pushed.push(),
            column: column_of(column),
        },
        String::from("numerator"),
    )
}

#[test]
fn an_aggregate_leg_projects_a_decomposed_average_as_two_terms_undivided() {
    // The mean of a column is the one measure that does not descend as written, and it travels as a
    // sum beside a count. Two terms, side by side, with nothing that divides them: the division is
    // an `Above::Quotient`, one level up, where every leg is already below it.
    let measure = mean_of("mrr_cents");
    let terms = carried_terms(&measure);
    assert_eq!(terms.len(), 2, "an average travels as two columns: {terms:?}");
    let pushed: Vec<Aggregate> = terms
        .iter()
        .map(|term| match *term.term() {
            PlanTerm::Aggregate { aggregate, .. } => aggregate,
            PlanTerm::CountIf { .. } => panic!("an average decomposes into two aggregates"),
        })
        .collect();
    assert_eq!(pushed, vec![Aggregate::Sum, Aggregate::Count]);
    // Distinct labels, so the combine can tell the two columns apart in what came back.
    assert_ne!(terms.first().map(LegTerm::label), terms.get(1).map(LegTerm::label));

    // And the division is above, not in the leg. `LegTerm` has no shape that could hold it - the
    // `compile_fail` doctest on that type is the mechanical half - so this asserts the other side:
    // the classification does put a quotient above these two.
    assert!(
        matches!(*Federation::of(&measure).above(), Above::Quotient { .. }),
        "the division a leg cannot express happens above it"
    );

    let leg = fact(terms);
    assert_eq!(
        leg.result_labels(),
        vec![
            String::from("customer_key"),
            String::from("period"),
            String::from("term_0"),
            String::from("term_1"),
        ]
    );
}

#[test]
fn a_fact_leg_with_no_terms_projects_keys_rather_than_a_count() {
    // The third shape, and it is not a third variant: a `Fact` with an empty `terms` list groups by
    // its key list and projects it, which is a distinct set of keys. That is what an exact
    // `CountDistinct` needs transported, because adding two exact distinct counts over-counts every
    // key the two legs share.
    let leg = fact(Vec::new());
    let LegPlan::Fact { ref terms, ref keys, .. } = leg else {
        panic!("the fact fixture is a fact");
    };
    assert!(terms.is_empty(), "the distinct-key leg aggregates nothing");
    assert!(!keys.is_empty(), "it projects keys instead");
    // Keys and the bucket, and no measure column at all.
    assert_eq!(
        leg.result_labels(),
        vec![String::from("customer_key"), String::from("period")]
    );

    // The measure it serves does not descend, which is why the leg carries keys rather than a
    // number. Read off the classification rather than asserted about this fixture.
    let counted = Measure::Simple(Term::Aggregate(AggregatedColumn::new(
        Aggregate::CountDistinct,
        ColumnName::parse("subscription_key").expect("a test column is a column"),
    )));
    assert!(
        Federation::of(&counted).pulls_up_rows(),
        "an exact distinct count is the case that pulls rows up"
    );
}

#[test]
fn every_leg_names_exactly_one_data_system() {
    // *A plan cannot silently span two sources*, per leg. Neither variant has a second `SourceName`
    // to disagree with the one it carries, and `Executable` reads the same field whichever shape it
    // was handed - which is what lets a composition root check the adapter it is about to call
    // against the plan without knowing which of the two it holds.
    let aggregate = fact(Vec::new());
    let dimension = lookup();
    assert_eq!(Executable::from(&aggregate).source().as_str(), "local");
    assert_eq!(Executable::from(&dimension).source().as_str(), "crm");
    assert_eq!(Executable::from(&aggregate).params().len(), 1);
    assert!(Executable::from(&dimension).params().is_empty());
    assert_eq!(
        Executable::from(&dimension).result_labels(),
        dimension.result_labels(),
        "the port's view of the labels is the leg's own"
    );
}
