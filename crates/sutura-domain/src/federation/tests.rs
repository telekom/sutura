//! What each aggregate does when a query reaches two data systems, provoked once each.
//!
//! The classification is a `match`, so the compiler is what stops a seventh aggregate arriving
//! without an answer. These tests are the other half: they say WHICH answer each of the six got,
//! because an arm that returns the wrong class compiles perfectly and produces a wrong number rather
//! than an error.
//!
//! Two of them are arithmetic that looks right and is not - a `Count` re-aggregated with a `Count`,
//! and a division applied inside a leg - and each has a test of its own naming the mistake.
//!
//! The measures here are the shapes of `examples/single-player/catalog/metrics/`, which is both the
//! quickstart and the corpus the branch above this one will run: a sum, a conditional count, a
//! distinct count, an average, and the two ratios that mix them.

use super::{Above, Carried, Descent, Federation, Pulled, Pushed, descend};
use crate::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
use crate::model::{Aggregate, ColumnName};

fn column(name: &str) -> ColumnName {
    ColumnName::parse(name).expect("a plain identifier is one")
}

/// `aggregate(column)` as a term.
fn term(aggregate: Aggregate, name: &str) -> Term {
    Term::Aggregate(AggregatedColumn::new(aggregate, column(name)))
}

/// The pushed and combining aggregates, for the four that descend as written.
///
/// `None` for the two that do not, so a table-driven assertion can say which class an aggregate is
/// in without restating the whole enum at each row.
fn as_written(aggregate: Aggregate) -> Option<(Aggregate, Aggregate)> {
    match Descent::of(aggregate) {
        Descent::AsWritten(pushed) => Some((pushed.push(), pushed.combine())),
        _ => None,
    }
}

/// The aggregate one leg computes, for the leg shapes that compute one.
///
/// `None` for a conditional count, which is not spelled as an aggregate, and for pulled-up keys,
/// which are not aggregated in the leg at all.
fn pushed(carried: &Carried) -> Option<Aggregate> {
    match *carried {
        Carried::Aggregated { pushed, .. } => Some(pushed.push()),
        _ => None,
    }
}

/// How many divisions the tree above the legs performs.
fn divisions(above: &Above) -> usize {
    match *above {
        Above::Total(_) => 0,
        Above::Quotient {
            ref numerator,
            ref denominator,
            ..
        } => 1 + divisions(numerator) + divisions(denominator),
    }
}

/// Every zero-denominator guard in the tree, outermost first.
///
/// There is no equivalent function over [`Carried`] and there cannot be: that type has no field a
/// guard fits in, which is what the `compile_fail` block on it pins.
fn guards(above: &Above) -> Vec<ZeroDenominator> {
    match *above {
        Above::Total(_) => Vec::new(),
        Above::Quotient {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            let mut all = vec![zero_denominator];
            all.extend(guards(numerator));
            all.extend(guards(denominator));
            all
        }
    }
}

#[test]
fn every_aggregate_states_how_it_federates() {
    // The six names are the coverage claim; the exhaustive `match` in `Descent::of` is what makes a
    // seventh fail to compile rather than fall through to a default.
    //
    // Pushable as written, with the function that re-aggregates the leg's column above it.
    assert_eq!(as_written(Aggregate::Sum), Some((Aggregate::Sum, Aggregate::Sum)));
    assert_eq!(as_written(Aggregate::Count), Some((Aggregate::Count, Aggregate::Sum)));
    assert_eq!(as_written(Aggregate::Min), Some((Aggregate::Min, Aggregate::Min)));
    assert_eq!(as_written(Aggregate::Max), Some((Aggregate::Max, Aggregate::Max)));

    // Pushable decomposed. The struct literals are writable here because this module is a child of
    // the one that classifies - `Pushed` has no public constructor anywhere else, which is what
    // stops a `Carried` naming an aggregate that does not descend.
    assert_eq!(
        Descent::of(Aggregate::Avg),
        Descent::Decomposed {
            numerator: Pushed {
                push: Aggregate::Sum,
                combine: Aggregate::Sum,
            },
            denominator: Pushed {
                push: Aggregate::Count,
                combine: Aggregate::Sum,
            },
            zero_denominator: ZeroDenominator::Null,
        }
    );

    // Not pushable, and it names the aggregate that runs above the keys it carried.
    assert_eq!(
        Descent::of(Aggregate::CountDistinct),
        Descent::AsGroupingKey(Pulled {
            above: Aggregate::CountDistinct
        })
    );

    // Four, one and one: the split the two records price the federated path on.
    let every = [
        Aggregate::Sum,
        Aggregate::Count,
        Aggregate::Min,
        Aggregate::Max,
        Aggregate::Avg,
        Aggregate::CountDistinct,
    ];
    let descents = every.map(Descent::of);
    assert_eq!(descents.iter().filter(|d| matches!(**d, Descent::AsWritten(_))).count(), 4);
    assert_eq!(
        descents.iter().filter(|d| matches!(**d, Descent::Decomposed { .. })).count(),
        1
    );
    assert_eq!(
        descents.iter().filter(|d| matches!(**d, Descent::AsGroupingKey(_))).count(),
        1
    );
}

#[test]
fn a_count_is_re_aggregated_with_a_sum_and_not_with_a_count() {
    // The mistake this exists for: `COUNT` per leg and `COUNT` above counts LEGS. Adding the leg
    // counts is the count. It raises no error and the number is small rather than absurd, which is
    // what makes it survive review.
    let federation = Federation::of(&Measure::Simple(term(Aggregate::Count, "subscription_month_key")));
    let carried = federation.carried();
    assert_eq!(carried.len(), 1);
    assert_eq!(pushed(carried[0]), Some(Aggregate::Count));
    assert_eq!(carried[0].combine(), Aggregate::Sum);
}

#[test]
fn an_average_travels_as_a_sum_and_a_count() {
    // `mean_subscription_mrr`. An `AVG` of `AVG`s weights every leg equally regardless of how many
    // rows it saw, so the two halves travel separately and the division waits for the combine.
    let federation = Federation::of(&Measure::Simple(term(Aggregate::Avg, "mrr_cents")));
    let carried = federation.carried();

    assert_eq!(carried.len(), 2, "an average is two columns in the leg, not one");
    assert_eq!(pushed(carried[0]), Some(Aggregate::Sum));
    assert_eq!(pushed(carried[1]), Some(Aggregate::Count));
    // Both halves read the same column, and both re-aggregate with a sum.
    assert_eq!(carried[0].column().as_str(), "mrr_cents");
    assert_eq!(carried[1].column().as_str(), "mrr_cents");
    assert_eq!(carried[0].combine(), Aggregate::Sum);
    assert_eq!(carried[1].combine(), Aggregate::Sum);
    // No leg computes an average, and nothing above one does either: the division is the tree.
    assert!(carried.iter().all(|leg| leg.combine() != Aggregate::Avg));
    assert_eq!(divisions(federation.above()), 1);
    // An `AVG` over no rows is null, so the decomposition's own division has to be.
    assert_eq!(guards(federation.above()), vec![ZeroDenominator::Null]);
    // It is the cheap kind of pull-up: two columns, still one row per group.
    assert!(!federation.pulls_up_rows());
}

#[test]
fn a_distinct_count_is_not_pushable() {
    // `active_subscriptions`. Two join keys can share a subscription, so adding two exact distinct
    // counts over-counts and no re-aggregating function repairs it. The leg carries the keys.
    let federation = Federation::of(&Measure::Simple(term(Aggregate::CountDistinct, "subscription_key")));
    let carried = federation.carried();

    assert_eq!(carried.len(), 1);
    assert!(carried[0].is_pulled_up(), "a distinct count travels as a grouping key");
    assert!(federation.pulls_up_rows());
    assert_eq!(carried[0].column().as_str(), "subscription_key");
    // Counted above, on the keys the leg carried, rather than in the leg.
    assert_eq!(carried[0].combine(), Aggregate::CountDistinct);

    // And it is a plan rather than a decline: the classification is total, so there is no error to
    // assert here and no refusal variant behind one.
    assert!(matches!(*federation.above(), Above::Total(_)));

    // The contrast, so this test cannot pass because everything looks pulled up.
    assert!(!Federation::of(&Measure::Simple(term(Aggregate::Sum, "amount_cents"))).pulls_up_rows());
}

#[test]
fn a_conditional_count_descends_and_is_summed_above() {
    // `subscriptions_churned`. Spelled `COUNTIF` in one dialect and `SUM(CASE WHEN ..)` in another,
    // and a count either way - so it descends as written and the combine adds the leg counts.
    let above = descend(&Term::CountIf {
        column: column("churned"),
    });
    assert_eq!(
        above,
        Above::Total(Carried::CountIf {
            column: column("churned")
        })
    );
    match above {
        Above::Total(ref carried) => {
            assert_eq!(carried.combine(), Aggregate::Sum);
            assert!(!carried.is_pulled_up());
        }
        Above::Quotient { .. } => panic!("a conditional count is one column, not a division"),
    }
}

#[test]
fn a_ratio_is_decomposed_rather_than_divided_per_leg() {
    // `churn_rate`: count_if(churned) / count_distinct(subscription_key), null on a zero
    // denominator. The trap: `ZeroDenominator::Null` renders as `NULLIF(d, 0)`, so applied INSIDE a
    // leg it makes a zero-denominator subgroup null, the `SUM` above skips nulls, and that
    // subgroup's numerator is dropped from the answer instead of nulling it. A wrong number, no
    // error.
    let measure = Measure::Ratio {
        numerator: Term::CountIf {
            column: column("churned"),
        },
        denominator: term(Aggregate::CountDistinct, "subscription_key"),
        zero_denominator: ZeroDenominator::Null,
    };
    let federation = Federation::of(&measure);

    // The division is the outermost node, and there is exactly one of it.
    assert!(matches!(*federation.above(), Above::Quotient { .. }));
    assert_eq!(divisions(federation.above()), 1);
    assert_eq!(guards(federation.above()), vec![ZeroDenominator::Null]);

    // Numerator and denominator travel as separate leg columns, each with its own combine.
    let carried = federation.carried();
    assert_eq!(carried.len(), 2);
    assert!(matches!(*carried[0], Carried::CountIf { .. }));
    assert!(matches!(*carried[1], Carried::Keys { .. }));
    assert_eq!(carried[0].combine(), Aggregate::Sum);
    assert_eq!(carried[1].combine(), Aggregate::CountDistinct);

    // The half that is a shape rather than an assertion: nothing a leg carries can hold a division
    // or a guard, so there is no value to inspect for one. `Carried`'s `compile_fail` doctests are
    // where that is pinned, because the code that would express it does not compile.
}

#[test]
fn a_ratio_of_averages_keeps_every_division_above_the_legs() {
    // Not in the corpus, and representable - which is why the tree is a tree. An `Avg` numerator is
    // itself a division, so the above-step nests to depth two, and the four columns underneath it
    // are still four plain leg aggregates.
    let measure = Measure::Ratio {
        numerator: term(Aggregate::Avg, "mrr_cents"),
        denominator: term(Aggregate::Avg, "seats"),
        zero_denominator: ZeroDenominator::Fail,
    };
    let federation = Federation::of(&measure);

    assert_eq!(divisions(federation.above()), 3);
    // Outermost is the one the definition asked for; the two inside it belong to the averages.
    assert_eq!(
        guards(federation.above()),
        vec![ZeroDenominator::Fail, ZeroDenominator::Null, ZeroDenominator::Null]
    );

    let carried = federation.carried();
    assert_eq!(carried.len(), 4);
    assert!(carried.iter().all(|leg| matches!(**leg, Carried::Aggregated { .. })));
    assert!(carried.iter().all(|leg| leg.combine() == Aggregate::Sum));
    assert!(!federation.pulls_up_rows());
}

#[test]
fn the_zero_guard_a_definition_asked_for_reaches_the_final_denominator() {
    // `revenue_per_churned_subscription`: sum / count_if, and the definition says an empty period is
    // a fault. The guard belongs to the one division above the legs, and it is the FINAL denominator
    // it applies to - a leg's denominator is a partial total and failing on one would refuse a
    // question whose real denominator is not zero at all.
    let measure = Measure::Ratio {
        numerator: term(Aggregate::Sum, "amount_cents"),
        denominator: Term::CountIf {
            column: column("churned"),
        },
        zero_denominator: ZeroDenominator::Fail,
    };
    let federation = Federation::of(&measure);

    assert_eq!(guards(federation.above()), vec![ZeroDenominator::Fail]);
    assert_eq!(divisions(federation.above()), 1);
    assert!(federation.carried().iter().all(|leg| !leg.is_pulled_up()));
}

#[test]
fn no_leg_carries_an_aggregate_that_does_not_descend() {
    // The postcondition behind `Pushed` having no public constructor: for every aggregate in the
    // vocabulary, the columns a leg computes are drawn only from the four that descend as written.
    // An `Avg` or a `CountDistinct` reaching a leg is the whole bug this branch exists to prevent.
    for aggregate in [
        Aggregate::Sum,
        Aggregate::Count,
        Aggregate::Min,
        Aggregate::Max,
        Aggregate::Avg,
        Aggregate::CountDistinct,
    ] {
        let federation = Federation::of(&Measure::Simple(term(aggregate, "amount_cents")));
        for leg in federation.carried() {
            if let Carried::Aggregated { pushed, .. } = *leg {
                assert!(
                    matches!(
                        pushed.push(),
                        Aggregate::Sum | Aggregate::Count | Aggregate::Min | Aggregate::Max
                    ),
                    "{aggregate} put {} into a leg",
                    pushed.push()
                );
            }
        }
    }
}
