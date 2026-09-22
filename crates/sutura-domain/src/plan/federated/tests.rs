//! What a [`FederatedPlan`] refuses to BE, which is the whole of what this suite is about now.
//!
//! **The combine's own arithmetic used to be asserted here and is not any more.** `docs/adr/0039`
//! step 3 moved the combine to a `DataFusion` plan in `sutura-exec-datafusion`, so its cells moved
//! with it - a suite asserting on a pure function that no longer exists would be a suite asserting
//! on nothing. What is left is the constructor's refusals, which are properties of the plan TYPE
//! and of the closed aggregate vocabulary rather than of anything that executes: they hold whatever
//! combines.
//!
//! Beside the implementation, and the reason is the causality gate - see `crate::plan::federated`'s
//! own header.

use crate::federation::Federation;
use crate::measure::Measure;
use crate::model::{Aggregate, ColumnName, DimensionName, Grain, InvalidIdentifier, MetricName, TableName};
use crate::plan::leg::LegPlan;
use crate::plan::{
    AnswerKey, FederatedPlan, FederatedPlanError, InternalLabel, PlanBindings, PlanBucket, PlanColumn, ResultLabel,
    StatementTables,
};

mod fixtures;
use fixtures::*;

/// Case 2's own ranking cells, split out for this file's own `max-lines` reason - see the
/// module's own header.
#[cfg(test)]
mod ranking;

#[test]
fn a_plan_with_two_legs_on_one_source_does_not_construct() {
    // The constructor is the newtype convention: a plan that is not a fact leg beside a lookup
    // leg on a different data system is not a value of this type. A lookup passed in the fact
    // slot is the first guard to fire, before any source agreement is even compared.
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        lookup_leg(),
        lookup_leg(),
        true,
        Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents"))),
        vec![AnswerKey::fact(ResultLabel::dimension(&dimension("product_family")))],
    );
    assert!(matches!(plan, Err(FederatedPlanError::NotFact { .. })));
}

#[test]
fn an_internal_label_is_in_a_namespace_no_question_can_name() {
    // **The property first: nothing a catalog author or a caller can write reaches this namespace.**
    // Every rendering starts with a digit, which the identifier parser refuses as a FIRST character,
    // so it is one refusal for every spelling and length, not a list of reserved words to maintain.
    let widest = InternalLabel::Leaf(usize::MAX).label();
    for label in [InternalLabel::Link.label(), InternalLabel::Leaf(0).label(), widest.clone()] {
        assert!(
            matches!(DimensionName::parse(&label), Err(InvalidIdentifier::BadFirstCharacter { .. })),
            "{label} must not be a dimension name"
        );
        assert!(MetricName::parse(&label).is_err(), "{label} must not be a metric name");
        assert!(ColumnName::parse(&label).is_err(), "{label} must not be a column name");
        assert!(TableName::parse(&label).is_err(), "{label} must not be a table name");
    }

    // Then the SPELLING, which this test owns: every other test here takes its labels from the type,
    // so a test that derived this expectation too would assert nothing about what the splitter and
    // the combiner actually agree on.
    assert_eq!(InternalLabel::Link.label(), "0_link");
    assert_eq!(InternalLabel::Leaf(0).label(), "0_leaf_0");
    assert_eq!(InternalLabel::Leaf(7).label(), "0_leaf_7");

    // And the length, against the limit a data system TRUNCATES at rather than refusing - which
    // turns two distinct leaf columns into one. The scheme this replaced was `metric__{n}`, which
    // over a 63-character metric name was 66 characters; nothing here reads a metric's name, so the
    // widest label a `usize` can index is the bound.
    assert!(
        widest.len() <= 63,
        "an internal label must fit the tightest identifier limit, {widest} is {} characters",
        widest.len()
    );
}

#[test]
fn a_plan_whose_legs_do_not_project_the_link_does_not_construct() {
    // The link is not an answer key, so the answer-key loop never saw it and the combiner reported
    // a missing column when a leg had not projected it. A plan that cannot be joined is not a plan.
    let unlinked = LegPlan::Lookup {
        source: source(REMOTE_SOURCE),
        table: table(FACT).into(),
        keys: vec![key("region", FACT)],
        bindings: PlanBindings::none(),
    };
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        fact_leg(Vec::new()),
        unlinked,
        true,
        Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents"))),
        Vec::new(),
    );
    match plan {
        Err(FederatedPlanError::KeyNotOnLeg { side, ref label }) => {
            assert!(matches!(side, crate::plan::LegSide::Lookup));
            assert_eq!(*label, InternalLabel::Link.label());
        }
        ref other => panic!("a lookup leg that projects no link is not a plan, got {other:?}"),
    }
}

#[test]
fn a_fact_leg_that_does_not_project_the_link_does_not_construct_either() {
    // **The other arm of the same check, and the reachable one.** The test above builds an unlinked
    // LOOKUP leg, so `KeyNotOnLeg { side: Fact }` was the untested half - and the half that matters:
    // the fact leg is the one the splitter builds from the question's own keys, so a change there
    // that stopped pushing `InternalLabel::Link` is what this arm catches. The only production
    // caller no longer erases the cause (`telekom/sutura#338`): it leaves as
    // `sutura_semantic::CompileFailure::NotAssembled`, keeping the side and label this reads.
    let unlinked = LegPlan::Fact {
        source: source(FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: vec![key("product_family", FACT)],
        terms: Vec::new(),
        bindings: PlanBindings::none(),
        range: range(),
    };
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        unlinked,
        lookup_leg(),
        true,
        Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents"))),
        Vec::new(),
    );
    match plan {
        Err(FederatedPlanError::KeyNotOnLeg { side, ref label }) => {
            assert!(matches!(side, crate::plan::LegSide::Fact));
            assert_eq!(*label, InternalLabel::Link.label());
        }
        ref other => panic!("a fact leg that projects no link is not a plan, got {other:?}"),
    }
}

#[test]
fn a_bucket_that_does_not_match_the_fact_legs_own_does_not_construct() {
    // D9: `bucket` and the fact leg's own bucket are two independently supplied arguments; the one
    // production splitter clones one value into both, so a producer that stops doing that is what
    // this catches.
    let federation = Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents")));
    let mismatched_bucket = PlanBucket::new(
        ResultLabel::bucket(),
        Grain::Week,
        PlanColumn::new(table(FACT), column("week")),
    );
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        mismatched_bucket,
        fact_leg(terms_for(&federation)),
        lookup_leg(),
        true,
        federation,
        Vec::new(),
    );
    assert!(matches!(plan, Err(FederatedPlanError::BucketMismatch)), "{plan:?}");
}

#[test]
fn fact_terms_that_do_not_match_the_federations_labels_do_not_construct() {
    // D9: `fact`'s own terms are supplied by the producer rather than derived here, and the one
    // production splitter zips `federation.carried()` with `labels(&federation)` to build them.
    // Empty terms are what a producer with no leaves to project would send, which is never this
    // federation's own shape.
    let federation = Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents")));
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        fact_leg(Vec::new()),
        lookup_leg(),
        true,
        federation,
        Vec::new(),
    );
    assert!(matches!(plan, Err(FederatedPlanError::TermsDoNotMatchFederation)), "{plan:?}");
}

#[test]
fn matching_bucket_and_terms_construct_the_negative_control() {
    // Negative control for the two tests above: the same shapes, unmutated, still construct - so
    // the two refusals are about the mismatch, not about the fixtures being unusable.
    let federation = Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents")));
    let plan = FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        fact_leg(terms_for(&federation)),
        lookup_leg(),
        true,
        federation,
        Vec::new(),
    );
    assert!(plan.is_ok(), "{plan:?}");
}

/// **A carried leaf with no re-aggregating function is refused before a plan exists**, which is the
/// one thing `reaggregates` decides and the refusal that keeps a combiner's own
/// unsupported-aggregate arm unreachable through this constructor.
///
/// An exact distinct count is the case: no function adds per-group distinct counts back up, so the
/// honest answer is to refuse the slice rather than to certify a re-count. It is refused HERE, at
/// construction, rather than when a group is reduced - reduced, the same plan refused a group
/// holding a value and answered null for a group of nulls, under the metric's own certified name.
#[test]
fn a_carried_leaf_with_no_reaggregating_function_does_not_construct() {
    let refused = try_plan_for(
        "distinct_customers",
        &Measure::Simple(term(Aggregate::CountDistinct, "customer_key")),
        true,
    )
    .expect_err("a distinct count has no re-aggregating function");
    assert!(
        matches!(
            refused,
            FederatedPlanError::LeafDoesNotReaggregate {
                aggregate: Aggregate::CountDistinct
            }
        ),
        "{refused:?}"
    );
}

/// The negative control for the cell above, and it is not decoration: without it the refusal would
/// pass just as well against a constructor that refused every measure. A sum re-aggregates with a
/// sum, so the same shape constructs.
#[test]
fn a_carried_leaf_that_reaggregates_constructs() {
    assert!(
        try_plan_for("revenue", &Measure::Simple(term(Aggregate::Sum, "mrr_cents")), true).is_ok(),
        "a sum re-aggregates with a sum"
    );
}
