//! The four things a data system can answer about a declared key, and what each does to the bundle.
//!
//! Over fakes rather than a data system, because what is being held here is the BOOT PATH's
//! decision - which of the four outcomes refuses - and a real adapter would only prove that one of
//! them can be produced. `crates/sutura-app/tests/differential/federated.rs` is the other half: the
//! same refusal reached through a real engine and a real embedded database, on both topologies.

use std::collections::BTreeSet;

use sutura_domain::calendar::TimeRange;
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Anchor, AnchorValue, Definitions, Description, Metric, Model, Relationship};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, JoinType, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, NotValidated, PinnedDefinitions};

use crate::Warehouses;
use crate::tests::{certified, june, metric, shared, source};
use crate::tests_support::{CountsBack, FixedWarehouse};
use crate::verify_and_validate;

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn named(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a model")
}

fn relationship() -> RelationshipName {
    RelationshipName::parse("orders_customer").expect("a test relationship is a relationship")
}

/// A metric's own model, a dimension model beside it, and the `many_to_one` between them.
///
/// **No metric declares a dimension over the join, and that is deliberate.** What the boot path asks
/// about is the RELATIONSHIP: the declaration is a claim about the target table that the join path
/// spends whenever a question does reach it, and a fixture that only carried it where a question
/// uses it would say the check is about a question.
fn bundle_joined(anchor_range: TimeRange) -> PinnedDefinitions {
    let orders = Model::new(
        named("orders"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date"), column("customer_key")]),
        Description::default(),
    );
    let customers = Model::new(
        named("customers"),
        source(),
        TableName::parse("dim_customer").expect("a test table is a table"),
        BTreeSet::from([column("customer_key"), column("region")]),
        Description::default(),
    );
    let joined = Relationship::new(
        relationship(),
        named("orders"),
        column("customer_key"),
        named("customers"),
        column("customer_key"),
        JoinType::ManyToOne,
    );
    let revenue = Metric::new(
        metric(),
        named("orders"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        Some(Anchor::new(
            anchor_range,
            AnchorValue::parse("197122").expect("a test anchor value is a value"),
        )),
        Description::default(),
    )
    .expect("no dimensions to duplicate");
    let definitions =
        Definitions::assemble(vec![orders, customers], vec![joined], vec![revenue]).expect("the joined bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

/// One registry whose fake answers `counts` about a declared key and the certified number about the
/// anchor, so a refusal here can only be about the key.
fn counting(counts: CountsBack) -> Warehouses<FixedWarehouse> {
    Warehouses::of(FixedWarehouse::answering_and_counting(
        source(),
        shared(),
        certified(),
        counts,
    ))
}

/// **The defect `telekom/sutura#354` reports, refused at the one place both topologies pass
/// through.**
///
/// A `many_to_one` whose target column is not unique makes one question answer two ways - a rendered
/// `JOIN` adds the measure twice, a federated lookup leg's `GROUP BY` collapses the duplicate away
/// first - and before this neither topology refused. The BUNDLE is what both of them read, so
/// refusing it is what makes them agree again.
#[test]
fn a_declared_key_the_data_contradicts_is_not_a_validated_bundle() {
    let refused = verify_and_validate(
        bundle_joined(june()),
        &counting(CountsBack::Counted { rows: 41, distinct: 40 }),
    )
    .expect_err("a violated declaration is not a bundle this deployment may serve");
    let NotValidated::DeclaredKeyNotUnique(ref violation) = refused else {
        panic!("a violated declaration is refused as one, not as {refused:?}");
    };
    assert_eq!(violation.relationship(), &relationship());
    assert_eq!(violation.model(), &named("customers"));
    assert_eq!(violation.table().to_string(), "dim_customer");
    assert_eq!(violation.column(), &column("customer_key"));
    assert_eq!(violation.counts().rows(), 41);
    assert_eq!(violation.counts().distinct(), 40);
    assert_eq!(violation.counts().duplicated(), 1);
    // What an operator reads names the table and the column and NO key value, which is the decision
    // `sutura_domain::warehouse::cardinality` makes: a boot log is not a place to copy source data
    // into.
    let said = refused.to_string();
    assert!(said.contains("dim_customer"), "{said}");
    assert!(said.contains("customer_key"), "{said}");
    assert!(said.contains("41") && said.contains("40"), "{said}");
}

#[test]
fn a_declared_key_the_data_holds_up_validates() {
    drop(
        verify_and_validate(
            bundle_joined(june()),
            &counting(CountsBack::Counted { rows: 40, distinct: 40 }),
        )
        .expect("forty rows under forty keys is what many_to_one claims"),
    );
}

/// **The one outcome that is deliberately NOT a refusal**, pinned so it does not become one by
/// accident.
///
/// An adapter that took the port's default did not count. That is a fact about what was LINKED
/// rather than about a run, so refusing on it would stop every deployment whose data system has no
/// cheap way to ask. It is also the whole of what stays quiet, and `crate::declared_keys`'s header
/// says how much of that quiet a mechanism closes.
#[test]
fn an_adapter_that_did_not_count_leaves_the_bundle_validated() {
    drop(
        verify_and_validate(bundle_joined(june()), &counting(CountsBack::NotAsked))
            .expect("nobody counted, which is not a violated declaration"),
    );
}

/// **An adapter that COULD not count refuses the bundle, and the refusal carries its complaint.**
///
/// This used to pass, on `preflight`'s argument that *could not verify* must not stop a deployment
/// that would have worked - and review measured what that argument costs where it was applied:
/// `preflight` runs in a root, where the outcome has a `WARN` line to go to, while this runs inside
/// the operation that mints the proof, where the only alternative to refusing is silence. Silent, a
/// data system refusing to let this identity read the dimension table looked exactly like a clean
/// check.
///
/// The cause travels: the adapter's own message AND the driver's underneath it, which is the half a
/// single string throws away and the half that names a grant when the failure is a grant.
#[test]
fn an_adapter_that_could_not_count_refuses_the_bundle_and_says_why() {
    let refused = verify_and_validate(bundle_joined(june()), &counting(CountsBack::Failed))
        .expect_err("a declaration nothing could check is not a declaration this bundle may serve");
    let NotValidated::DeclaredKeyNotCounted(ref uncounted) = refused else {
        panic!("a probe that failed is refused as one, not as {refused:?}");
    };
    assert_eq!(uncounted.relationship(), &relationship());
    assert_eq!(uncounted.model(), &named("customers"));
    assert_eq!(uncounted.source(), &source());

    let said = refused.to_string();
    // The adapter's own sentence, and the driver's beneath it. `AdapterFailure::Statement` wraps
    // `DriverFailure`, so a message that stopped at the outer one would lose the file name - which
    // is exactly the loss `NotExecutedReason::Failed` was reshaped to stop for anchors.
    assert!(said.contains("rejected the statement"), "{said}");
    assert!(
        said.contains("no such file"),
        "the driver's own complaint must survive: {said}"
    );
}

/// The key check runs BEFORE the anchors, which is what makes the diagnostic the actionable one.
///
/// Both are wrong here: the declaration is violated AND the anchor range covers three months at the
/// metric's coarsest grain, so the anchor cannot reduce to one number either. A duplicated dimension
/// key is a table an operator can go and look at; an anchor that will not reduce, on a bundle whose
/// join may be doubling, sends them to the definition instead.
#[test]
fn a_violated_declaration_is_reported_ahead_of_a_failing_anchor() {
    let three_months = TimeRange::new(
        sutura_domain::calendar::Date::parse("2026-06-01").expect("a test date is a date"),
        sutura_domain::calendar::Date::parse("2026-09-01").expect("a test date is a date"),
    )
    .expect("three months is a range");
    let refused = verify_and_validate(
        bundle_joined(three_months),
        &counting(CountsBack::Counted { rows: 41, distinct: 40 }),
    )
    .expect_err("both halves are wrong, so this bundle is not validated");
    assert!(
        matches!(refused, NotValidated::DeclaredKeyNotUnique(_)),
        "the cheaper and more actionable half must be the one reported: {refused:?}"
    );
}
