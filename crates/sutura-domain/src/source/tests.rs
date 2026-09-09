//! What the identity declarations refuse, and what the execution record cannot say.

use super::{
    AcknowledgementReason, AnchorIdentity, ConflictingSourceIdentity, ExecutedAs, ImpersonationCapability, InvalidOperatorText,
    SharedIdentityDeclared, SourceIdentity, SourcePosture, UniformlyExecuted, VerificationIdentity,
};
use crate::model::SourceName;

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source name is a name")
}

fn reason(text: &str) -> AcknowledgementReason {
    AcknowledgementReason::parse(text).expect("a test reason is a reason")
}

fn shared(text: &str) -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(reason(text)),
    }
}

#[test]
fn an_acknowledgement_with_no_words_in_it_is_not_an_acknowledgement() {
    // The variant the witness rests on. A key an operator wrote and left blank reads in a diff and
    // in a startup log exactly like a deliberate acknowledgement, and is not one.
    for blank in ["", "   ", "\t\n "] {
        assert_eq!(
            AcknowledgementReason::parse(blank).expect_err("nothing is not a reason"),
            InvalidOperatorText::Empty {
                name: AcknowledgementReason::KEY
            }
        );
    }
    // And the positive side, without which every assertion here is satisfied by refusing everything.
    assert_eq!(
        reason("  a directory of CSVs this deployment owns  ").as_str(),
        "a directory of CSVs this deployment owns",
        "the reason is trimmed once, in the constructor, so no reader has to"
    );
}

#[test]
fn a_reason_that_would_forge_a_log_line_or_hide_its_own_text_is_refused() {
    // The two character classes, and they are two checks because the first provably cannot see the
    // second: `char::is_control` is false for every code point `crate::text` names.
    let newline = AcknowledgementReason::parse("shared\nlevel=error msg=\"nothing to see\"")
        .expect_err("a newline appends a log line nobody wrote");
    assert!(matches!(newline, InvalidOperatorText::ControlCharacter { .. }));

    let override_code = 0x202E_u32;
    let invisible = AcknowledgementReason::parse("shared \u{202E}resu-ecivres-derahs")
        .expect_err("a direction override makes the text a reviewer reads differ from the text stored");
    assert_eq!(
        invisible,
        InvalidOperatorText::InvisibleCharacter {
            name: AcknowledgementReason::KEY,
            code: override_code
        }
    );
    // The code and not the value, because text whose only defect draws nothing prints as though it
    // were correct.
    let rendered = invisible.to_string();
    assert!(rendered.contains("0x202e"), "{rendered}");
}

#[test]
fn a_reason_and_an_identity_are_bounded_and_each_names_its_own_key() {
    // Two bounds through one parser, and the refusal names the key rather than the category - a
    // refusal that does not say what to change is a support request.
    let long_reason = "r".repeat(401);
    let error = AcknowledgementReason::parse(&long_reason).expect_err("401 characters is a document");
    assert_eq!(
        error,
        InvalidOperatorText::TooLong {
            name: AcknowledgementReason::KEY,
            value: long_reason,
            len: 401,
            limit: 400,
        }
    );

    let long_identity = "i".repeat(257);
    let error = VerificationIdentity::parse(&long_identity).expect_err("257 characters is not a role name");
    assert!(matches!(
        error,
        InvalidOperatorText::TooLong {
            name: "sources.<alias>.verification_identity",
            limit: 256,
            ..
        }
    ));
    assert_eq!(
        VerificationIdentity::parse("sutura_anchor_reader")
            .expect("a role name is a name")
            .as_str(),
        "sutura_anchor_reader"
    );
}

#[test]
fn the_posture_word_is_defined_once_and_every_listed_name_is_a_variant() {
    // The word an answer carries, the word a startup log prints and the word a configuration file
    // writes are one string. `NAMES` is what a refusal lists, so a variant absent from it would be a
    // posture a message could not offer.
    assert_eq!(shared("a reason").as_str(), "shared-service-user");
    assert_eq!(SourcePosture::ImpersonationAtSource.as_str(), "impersonation-at-source");
    let mut named: Vec<&str> = vec![shared("a reason").as_str(), SourcePosture::ImpersonationAtSource.as_str()];
    named.sort_unstable();
    let mut listed: Vec<&str> = SourcePosture::NAMES.to_vec();
    listed.sort_unstable();
    assert_eq!(
        named, listed,
        "every listed spelling is a variant and every variant is listed"
    );
    // The sentence the startup log prints, so neither can drift into claiming this deployment
    // impersonates when it does not.
    assert!(
        shared("a reason")
            .what_decides_what_a_caller_sees()
            .contains("every caller sees the same rows")
    );
    assert!(
        SourcePosture::ImpersonationAtSource
            .what_decides_what_a_caller_sees()
            .contains("the source itself")
    );
}

#[test]
fn a_source_configured_to_impersonate_on_an_adapter_that_cannot_is_refused() {
    // The boot cross-check, at the level of the comparison itself. The composition root's own test
    // asserts it against the adapter this build actually links; this one asserts the rule, including
    // the three combinations that are fine - so it cannot pass by refusing everything.
    let local = source("local");
    let refused = SourcePosture::ImpersonationAtSource
        .deliverable_by(ImpersonationCapability::NoPlaceForASubject, &local)
        .expect_err("an adapter with no place for a subject credential cannot impersonate");
    assert_eq!(refused.at(), &local);
    let rendered = refused.to_string();
    assert!(rendered.contains("local"), "{rendered}");
    // `no fallback` is a substring of the fuller spelling, so the two arms were one fact stated
    // twice; keep the shorter, robust one.
    assert!(rendered.contains("no fallback"), "{rendered}");

    // The three combinations that are fine, so this test is not satisfied by refusing everything.
    // `expect` rather than `assert!(.., is_ok())`, which `assertions_on_result_states` bans for a
    // reason that applies: the message an `Err` would carry is the useful half.
    SourcePosture::ImpersonationAtSource
        .deliverable_by(ImpersonationCapability::PerSubjectCredential, &local)
        .expect("an adapter that can carry a subject credential can impersonate");
    for capability in [
        ImpersonationCapability::PerSubjectCredential,
        ImpersonationCapability::NoPlaceForASubject,
    ] {
        shared("one identity, on purpose")
            .deliverable_by(capability, &local)
            .expect("a shared source asks nothing of the adapter's credential path");
    }
}

#[test]
fn a_verification_identity_on_a_shared_source_is_refused_rather_than_ignored() {
    // A declaration that does nothing reads as a control that is in place. On a shared source the
    // verification identity IS the shared identity, so a name written here is either a
    // misunderstanding or a key on the wrong entry.
    let local = source("local");
    let error = SourceIdentity::declared(
        &local,
        shared("one identity, on purpose"),
        Some(VerificationIdentity::parse("anchor_reader").expect("a name")),
    )
    .expect_err("nothing would read that key");
    assert_eq!(
        error,
        ConflictingSourceIdentity::VerificationIdentityOnASharedSource {
            at: local,
            key: VerificationIdentity::KEY,
        }
    );
}

#[test]
fn which_identity_an_anchor_runs_as_is_a_match_and_the_undeclared_case_says_so() {
    let local = source("local");
    // Shared: nothing is configured, and the anchor is a complete claim - every caller reads that
    // source as that one identity.
    let shared_source =
        SourceIdentity::declared(&local, shared("one identity, on purpose"), None).expect("a shared source declares");
    assert!(matches!(
        shared_source.anchors_run_as(),
        AnchorIdentity::TheSharedIdentity { .. }
    ));

    // Impersonating, with a declared boot identity.
    let identity = VerificationIdentity::parse("anchor_reader").expect("a name");
    let declared = SourceIdentity::declared(&local, SourcePosture::ImpersonationAtSource, Some(identity.clone()))
        .expect("an impersonating source may declare one");
    assert_eq!(
        declared.anchors_run_as(),
        AnchorIdentity::Declared { identity: &identity },
        "the declared identity is what the boot path would run as"
    );

    // And the one that must not become a mode nobody chose. `NoneDeclared` rather than `None`, so a
    // reader gets the case from a variant instead of deciding what an absence permits.
    let undeclared = SourceIdentity::declared(&local, SourcePosture::ImpersonationAtSource, None).expect("declaring none parses");
    assert_eq!(undeclared.anchors_run_as(), AnchorIdentity::NoneDeclared);
    assert_eq!(undeclared.posture(), &SourcePosture::ImpersonationAtSource);
}

#[test]
fn an_execution_record_cannot_claim_that_nothing_ran_and_cannot_record_a_source_twice() {
    // Non-empty by construction: there is no empty form and no `remove`, so an answer cannot carry a
    // record that says no leg executed.
    let local = source("local");
    let warm = source("warehouse");
    let record = ExecutedAs::of(local.clone(), shared("a directory of CSVs"));
    assert_eq!(record.legs().count(), 1);
    assert_eq!(record.posture(&local).map(SourcePosture::as_str), Some("shared-service-user"));
    assert_eq!(
        record.posture(&warm),
        None,
        "a source with no leg has no posture in this answer"
    );

    let two = record
        .and(warm, SourcePosture::ImpersonationAtSource)
        .expect("a second source is a second leg");
    assert_eq!(two.legs().count(), 2);
    assert_eq!(
        two.legs().map(|(name, _)| name.as_str()).collect::<Vec<&str>>(),
        vec!["local", "warehouse"],
        "legs come back in source order, so a record is a function of the answer and not of insertion"
    );

    // Refused rather than overwritten: a wiring defect that ran one source twice under two postures
    // must not resolve to whichever value was written last.
    let error = two
        .and(local, SourcePosture::ImpersonationAtSource)
        .expect_err("one source is one leg");
    assert!(error.to_string().contains("local"), "{error}");
}

#[test]
fn two_shared_sources_with_different_acknowledgements_are_still_one_posture() {
    // **The cell that fails if the predicate is `posture_a != posture_b`.** `SourcePosture` derives
    // `PartialEq` and carries the operator's own acknowledgement, which is resolved PER SOURCE - so
    // two ordinary shared legs whose operators wrote different sentences are two unequal values and
    // one posture. Comparing values would refuse the only federating shape that ships today: every
    // adapter a release links declares it has nowhere for a subject to arrive, so both legs of a
    // shipped two-source answer are `shared-service-user`.
    let one = shared("a directory of CSVs this deployment owns");
    let other = shared("a reference dataset every team reads");
    assert_ne!(one, other, "the values differ, which is exactly the trap");

    let uniform = ExecutedAs::of(source("facts"), one)
        .and(source("geo"), other)
        .expect("two sources are two legs")
        .uniform()
        .expect("two shared legs decide identity the same way");
    assert_eq!(uniform.legs().count(), 2);
    assert_eq!(
        uniform.posture(&source("geo")).map(SourcePosture::as_str),
        Some("shared-service-user")
    );
}

#[test]
fn an_answer_whose_legs_would_run_under_two_postures_is_unconstructible() {
    // The type half. `PinnedDefinitions::provenance` takes a `UniformlyExecuted`, `Provenance::new`
    // is private and `ToolOutcome::Answer` carries a `Provenance` - so this `Err` is the only door
    // a two-leg answer has, and a mixed record has none. The `compile_fail` doctest with its
    // compiling twin lives on `provenance` itself.
    let differently = ExecutedAs::of(source("facts"), shared("a directory of CSVs"))
        .and(source("warehouse"), SourcePosture::ImpersonationAtSource)
        .expect("two sources are two legs")
        .uniform()
        .expect_err("one answer does not combine two identities");
    assert_eq!(
        differently.postures().iter().copied().collect::<Vec<&str>>(),
        vec!["impersonation-at-source", "shared-service-user"],
        "both labels, in name order, off the closed set"
    );

    // **And it carries no acknowledgement text.** `SourcePosture` and `AcknowledgementReason` both
    // derive `Serialize`, so a posture VALUE here would publish an operator's prose to every caller,
    // log and agent context that reads the refusal this becomes. `Debug` is the rendering that
    // reaches a log by accident, so it is the one to assert on, and `Display` is what an operator
    // reads.
    for rendered in [format!("{differently:?}"), differently.to_string()] {
        assert!(
            !rendered.contains("a directory of CSVs"),
            "the acknowledgement leaked: {rendered}"
        );
        assert!(rendered.contains("shared-service-user"), "{rendered}");
        assert!(rendered.contains("impersonation-at-source"), "{rendered}");
    }
}

#[test]
fn one_leg_is_uniform_by_construction_and_needs_no_verdict() {
    // The mono answer path's door, and the reason it returns no `Result`: a single-leg record has one
    // posture, so an `Err` arm on the path every question takes would be a refusal nothing can
    // provoke. Asserted through both doors, so the two cannot disagree about a one-leg record.
    let local = source("local");
    let direct = UniformlyExecuted::of(local.clone(), SourcePosture::ImpersonationAtSource);
    let via_verdict = ExecutedAs::of(local.clone(), SourcePosture::ImpersonationAtSource)
        .uniform()
        .expect("one leg cannot disagree with itself");
    assert_eq!(direct, via_verdict);
    assert_eq!(
        direct.posture(&local).map(SourcePosture::as_str),
        Some("impersonation-at-source")
    );
    assert_eq!(direct.legs().count(), 1);
}
