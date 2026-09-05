//! Tests for [`super`]: the pinned bundle, its version newtype, and the anchor report.
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape once a module
//! plus its cases would pass the thousand-line limit `cargo xtask max-lines` enforces - the same
//! arrangement `catalog/tests.rs` already uses and for the same reason. **Moved here unchanged:**
//! every case below is the one that was in `pinned.rs`, de-indented by one level, with `super::`
//! still naming the module it named before.

use std::collections::BTreeSet;

use super::{
    AnchorCheck, AnchorReport, Contribution, ContributionManifest, DefinitionVersion, InvalidVersion, MAX_VERSION_LEN,
    NotExecutedReason, NotValidated, PinnedDefinitions,
};
use crate::calendar::{Date, TimeRange};
use crate::catalog::{Anchor, Definitions, Description, Metric, Model};
use crate::definitions::DefinitionDigest;
use crate::knowledge::{Capability, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use crate::measure::{AggregatedColumn, Measure, Term};
use crate::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
use crate::source::{ExecutedAs, SourcePosture};

fn metric_name(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a test metric name is a name")
}

/// A one-model, one-metric bundle, with an anchor when `anchor` is set.
fn bundle(anchor: Option<Anchor>) -> PinnedDefinitions {
    pin(definitions(anchor))
}

/// The definitions [`bundle`] pins, before they are pinned.
fn definitions(anchor: Option<Anchor>) -> Definitions {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date")]),
        Description::default(),
    );
    let metric = Metric::new(
        metric_name("revenue"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        anchor,
        Description::default(),
    )
    .expect("no dimensions to duplicate");
    Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent")
}

/// Pins definitions, hashing them for real.
///
/// No stub hasher any more, and that is the point of the change these tests came with: the
/// hashing is this crate's own, so a test does not have to hand one in and therefore cannot hand
/// in one that lies.
fn pin(definitions: Definitions) -> PinnedDefinitions {
    pin_with(definitions, Knowledge::none())
}

/// The same, with knowledge attached: what the digest test below varies.
fn pin_with(definitions: Definitions, knowledge: Knowledge) -> PinnedDefinitions {
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        knowledge,
        manifest("local"),
    )
    .expect("the test definitions hash")
}

/// A one-entry manifest naming one contributor, the shape a single-source test bundle carries.
fn manifest(source: &str) -> ContributionManifest {
    ContributionManifest::single(
        SourceName::parse(source).expect("a test source is a source"),
        Contribution::of(crate::capabilities::MetadataCapabilities::nothing()),
    )
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

#[test]
fn a_version_label_may_not_carry_a_newline() {
    // The bug this prevents: provenance is written to an append-only audit sink a line at a
    // time, so a newline in a version label appends a second record that nobody wrote and that
    // reads exactly like a real one.
    assert_eq!(
        DefinitionVersion::parse("v1\nv2").unwrap_err(),
        InvalidVersion::ControlCharacter {
            value: String::from("v1\nv2")
        }
    );
    assert_eq!(DefinitionVersion::parse("").unwrap_err(), InvalidVersion::Empty);
    let long = "v".repeat(MAX_VERSION_LEN + 1);
    assert_eq!(
        DefinitionVersion::parse(&long).unwrap_err(),
        InvalidVersion::TooLong {
            value: long.clone(),
            len: long.chars().count(),
            limit: MAX_VERSION_LEN,
        }
    );
}

#[test]
fn a_version_label_may_not_carry_an_invisible_character() {
    // The bug this prevents, and the reason the newline check above was not enough: a version is
    // echoed into an audit record, into the body of an answer and into the compile command's
    // output, and `char::is_control` is FALSE for every one of these code points. A label with a
    // right-to-left override in it printed as though it were an ordinary one, in all three
    // places, and the provenance a reader copied out could not be matched against the bundle it
    // named.
    assert_eq!(
        DefinitionVersion::parse("v2.1\u{202E}"),
        Err(InvalidVersion::InvisibleCharacter { code: 0x202E })
    );
    // Both ends of every range, so a typo in one of the bounds fails here.
    for code in [
        0x00AD_u32, 0x200B, 0x200F, 0x202A, 0x202E, 0x2060, 0x2064, 0x2066, 0x2069, 0xFEFF, 0xFFF9, 0xFFFB,
    ] {
        let offending = char::from_u32(code).expect("a listed code point is a character");
        assert_eq!(
            DefinitionVersion::parse(format!("v1{offending}2")),
            Err(InvalidVersion::InvisibleCharacter { code }),
            "{code:#06x}"
        );
    }
    // And the neighbour of each bound is NOT refused, so this is the set written down rather
    // than a sweep that would reject a label somebody has to be able to write.
    for code in [
        0x00AC_u32, 0x00AE, 0x200A, 0x2010, 0x2029, 0x202F, 0x205F, 0x2065, 0x206A, 0xFEFE, 0xFF00, 0xFFFC,
    ] {
        let benign = char::from_u32(code).expect("a listed code point is a character");
        let label = format!("v1{benign}2");
        assert_eq!(
            DefinitionVersion::parse(&label)
                .expect("a label with an ordinary character is a label")
                .as_str(),
            label,
            "{code:#06x}"
        );
    }
}

#[test]
fn a_bundle_with_no_anchors_validates_with_an_empty_report() {
    // Not a loophole: a bundle that certifies nothing has nothing to check. The loophole would
    // be an anchored bundle passing on an empty report, which the next test rules out.
    let plain = bundle(None);
    AnchorReport::new()
        .verdict(&plain)
        .expect("nothing declared means nothing to check");
    assert!(plain.definitions().metric(&metric_name("revenue")).is_some());
}

#[test]
fn an_anchored_metric_with_no_recorded_check_does_not_validate() {
    // The rule, and it is only half of what makes it mean anything: this says an unchecked
    // anchor fails the verdict. What says the checks in a report actually ran is that the report
    // cannot become a servable bundle here at all - `sutura_app::verify_and_validate` is the
    // only thing that mints one, and it calls a `Warehouse`.
    let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
    assert_eq!(
        AnchorReport::new().verdict(&anchored).unwrap_err(),
        NotValidated::AnchorUnchecked {
            metric: metric_name("revenue")
        }
    );
}

#[test]
fn a_mismatched_anchor_does_not_validate() {
    let mut report = AnchorReport::new();
    report.record(
        metric_name("revenue"),
        AnchorCheck::Mismatch {
            expected: String::from("197122"),
            actual: String::from("197100"),
        },
    );
    let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
    assert_eq!(
        report.verdict(&anchored).unwrap_err(),
        NotValidated::AnchorMismatch {
            metric: metric_name("revenue"),
            expected: String::from("197122"),
            actual: String::from("197100"),
        }
    );
}

#[test]
fn an_anchor_that_could_not_run_is_not_the_same_as_a_wrong_one() {
    // Kept separate because the response differs: one is a definition to fix, the other is a
    // data system to page somebody about. Collapsing them makes an outage look like a wrong
    // number, which is the more expensive of the two mistakes.
    let mut report = AnchorReport::new();
    let reason = NotExecutedReason::Failed {
        message: String::from("connection refused"),
        chain: Vec::new(),
    };
    report.record(metric_name("revenue"), AnchorCheck::NotExecuted { reason: reason.clone() });
    let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
    assert_eq!(
        report.verdict(&anchored).unwrap_err(),
        NotValidated::AnchorNotExecuted {
            metric: metric_name("revenue"),
            reason,
        }
    );
}

#[test]
fn a_failed_check_carries_its_causes_all_the_way_out() {
    // The bug this prevents, and it shipped: the reason was one `String`, so an adapter error's
    // own `source` chain was flattened away before it got here and the operator was told "the
    // anchor query failed" with no table, no column and no driver error in it. Anchor
    // verification is the readiness gate, so that message is the whole of what a failed
    // deployment says.
    //
    // Asserted through `Display` and through `source`, because those are the two ways anything
    // renders this: a chain that is only reachable by matching on the variant is a chain no log
    // line would ever show.
    let mut report = AnchorReport::new();
    report.record(
        metric_name("revenue"),
        AnchorCheck::NotExecuted {
            reason: NotExecutedReason::Failed {
                message: String::from("the statement was rejected"),
                chain: vec![String::from("IO error"), String::from("no such file: orders.parquet")],
            },
        },
    );
    let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
    let error = report.verdict(&anchored).unwrap_err();
    let cause = core::error::Error::source(&error).expect("the reason is the source, not the message");
    let rendered = cause.to_string();
    assert!(rendered.contains("no such file: orders.parquet"), "{rendered}");
    assert_eq!(
        rendered,
        "the anchor query failed: the statement was rejected: IO error: no such file: orders.parquet"
    );
}

#[test]
fn an_unconfigured_source_names_the_one_the_metric_reads() {
    // It used to be a sentence in a report field. It is a governance condition - the plan names a
    // data system this process did not open - and naming it is what tells an operator which entry
    // is missing.
    //
    // **It used to name two data systems**, because the check compared the plan's source against
    // the one warehouse the service held. The service holds a registry now, so a plan SELECTS its
    // warehouse rather than being compared against one, and the second half of the pair had
    // nothing left to fill it: the only failure is that no entry exists under the name.
    let reason = NotExecutedReason::SourceNotConfigured {
        plan: SourceName::parse("elsewhere").expect("a test source is a source"),
    };
    assert_eq!(
        reason.to_string(),
        "the metric reads from elsewhere, and no data system is configured under that name"
    );
}

#[test]
fn a_report_about_a_different_bundle_does_not_validate_this_one() {
    // Without this check, evidence gathered against another bundle satisfies the coverage
    // check for whatever names happen to overlap, and a bundle becomes "validated" by a run
    // that never touched it.
    let mut report = AnchorReport::new();
    report.record(metric_name("churn"), AnchorCheck::Matched);
    assert_eq!(
        report.verdict(&bundle(None)).unwrap_err(),
        NotValidated::UnknownMetricChecked {
            metric: metric_name("churn")
        }
    );
}

#[test]
fn a_matched_anchor_passes_the_verdict_and_the_bundle_is_untouched() {
    let mut report = AnchorReport::new();
    report.record(metric_name("revenue"), AnchorCheck::Matched);
    let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
    // The execution record is a required argument, so a `Provenance` cannot be built without
    // saying which posture ran - see `PinnedDefinitions::provenance`. One leg, because the test
    // bundle reads one source.
    let ran_as = || {
        ExecutedAs::of(
            SourceName::parse("local").expect("a test source is a source"),
            SourcePosture::ImpersonationAtSource,
        )
    };
    let expected_provenance = anchored.provenance(ran_as());
    // A verdict, and nothing else. It does NOT hand back a bundle the service would take: that
    // is `sutura_app::verify_and_validate`, and getting one from it means a `Warehouse` was
    // called. The whole reason this returns `()` is that this crate cannot know whether it was.
    report
        .verdict(&anchored)
        .expect("a matched anchor is what the verdict is about");
    assert_eq!(anchored.provenance(ran_as()), expected_provenance);
    assert_eq!(anchored.version().as_str(), "test-1");
}

#[test]
fn the_digest_a_bundle_carries_is_computed_from_the_definitions_it_holds() {
    // The bug this closes, in two steps, because the first fix did not close it.
    //
    // Originally the constructor took a digest BESIDE a set of definitions, and its own comment
    // conceded the two need not be related - so an answer could carry provenance for content
    // that did not produce it. That was replaced by a constructor taking the hash FUNCTION, and
    // a review showed the hole was still open: `|_| Ok(elsewhere)` is safe public code, and the
    // test that shipped with that change asserted only that the closure was HANDED the
    // definitions - which is not evidence it read them.
    //
    // There is no parameter to lie with now. The assertion is therefore about the values: the
    // same definitions hash the same way twice, and different definitions do not collide. That
    // is a property of the stored bundle rather than of what a caller passed in.
    let pinned = pin(definitions(None));
    let again = pin(definitions(None));
    assert_eq!(
        pinned.digest(),
        again.digest(),
        "the same definitions must pin to the same digest, or provenance is not reproducible"
    );

    let anchored = pin(definitions(Some(Anchor::new(june(), String::from("197122")))));
    assert_ne!(
        pinned.digest(),
        anchored.digest(),
        "declaring an anchor changes what the bundle means, so it must change the digest"
    );

    // And the digest is the one the hashing function computes for exactly these definitions -
    // asserted against an independent call rather than against a constant, so the test cannot
    // pass by pinning whatever the implementation currently happens to emit.
    let independent =
        DefinitionDigest::of(pinned.definitions(), pinned.knowledge(), pinned.manifest()).expect("the definitions hash");
    assert_eq!(*pinned.digest(), independent);
}

#[test]
fn two_compositions_that_assemble_identically_have_different_digests() {
    // The contribution manifest's own reason for existing, red before green: `docs/adr/0011`
    // measured that the digest used to be taken over the assembly alone, so two different
    // compositions that happened to assemble to the same definitions and knowledge were
    // indistinguishable - a deployment that swapped which metadata source contributed the content
    // got the same digest, and the provenance travelling with an answer named the old composition.
    //
    // The two bundles here pin the SAME definitions and the SAME knowledge, and differ only in
    // which source the manifest records as having contributed them. A digest that cannot tell them
    // apart has not learned to cover the composition.
    let definitions = definitions(None);
    let knowledge = Knowledge::none();
    let from_local = PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        knowledge.clone(),
        manifest("local"),
    )
    .expect("a single-source conclusion hash");
    let from_datahub = PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        knowledge,
        manifest("datahub"),
    )
    .expect("a single-source conclusion hash");

    assert_ne!(
        from_local.manifest(),
        from_datahub.manifest(),
        "the two bundles differ in which source is recorded as having contributed - that is the premise"
    );
    assert_ne!(
        from_local.digest(),
        from_datahub.digest(),
        "the same assembly from different sources is a different bundle, so it must move the digest"
    );
}

#[test]
fn the_knowledge_is_under_the_digest_too() {
    // A glossary decides which metric an agent asks about, so two bundles that differ only in
    // their knowledge answer different questions from the same words. A digest that did not move
    // would report them as one snapshot, and the provenance travelling with an answer would name
    // content that did not produce it.
    //
    // The two knowledge values here differ only in what is DECLARED - one declares a glossary and
    // records nothing in it, the other declares nothing at all - which is the weakest difference
    // there is and the one the prompt is allowed to say different things about. A deployment that
    // silently stopped declaring a capability has changed what its agent was told, so if the
    // digest cannot tell these apart it cannot certify what the prompt claims.
    let bare = pin_with(definitions(None), Knowledge::none());
    let declared = pin_with(
        definitions(None),
        Knowledge::assemble(
            &definitions(None),
            KnowledgeInput::new(
                KnowledgeCapabilities::of([Capability::Glossary]),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .expect("an empty glossary is consistent with any definitions"),
    );
    assert_ne!(
        bare.digest(),
        declared.digest(),
        "declaring a knowledge capability changes what the bundle says, so it must change the digest"
    );
    assert_eq!(bare.definitions(), declared.definitions());
    assert!(!bare.knowledge().supports(Capability::Glossary));
    assert!(declared.knowledge().supports(Capability::Glossary));
}

#[test]
fn anchored_metrics_lists_only_the_metrics_that_declare_one() {
    let plain = bundle(None);
    assert_eq!(plain.anchored_metrics().count(), 0);
    let anchored = bundle(Some(Anchor::new(june(), String::from("197122"))));
    let listed: Vec<&MetricName> = anchored.anchored_metrics().map(|(name, _)| name).collect();
    assert_eq!(listed, vec![&metric_name("revenue")]);
}
