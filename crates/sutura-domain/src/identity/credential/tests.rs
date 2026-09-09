//! The credential port's own suite.
//!
//! In its own file for the reason `crate::source`'s is: the module it tests is close enough to the
//! 1000-line gate that a suite inline would have decided how much documentation the types get.

use std::collections::BTreeMap;

use super::{
    Agreed, BoundToTheRequest, CredentialsDoNotCoverThePlan, CredentialsDoNotFitTheRequest, Expiry, LegCredentials, Minted,
    Presented, PresentedDisagreesWithPosture, PrincipalName, SourceSet,
};
use crate::identity::{InvalidPrincipalId, Secret, Subject, SubjectId};
use crate::model::SourceName;
use crate::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source is a source")
}

fn a_person() -> Subject {
    Subject::Verified {
        id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
    }
}

fn acknowledged() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(
        AcknowledgementReason::parse("a directory of CSVs read by one process").expect("a test reason is a reason"),
    )
}

fn shared() -> Presented {
    Presented::SharedServiceUser {
        declared: acknowledged(),
    }
}

#[test]
fn a_shared_leg_carries_no_credential_material() {
    // THE VARIANT THIS ENUM EXISTS FOR, and the assertion is on the shape rather than on a comment:
    // a third variant holding a placeholder secret would be the service-identity fallback arriving
    // back as a value nobody looked at. What it carries is the operator's own words.
    let leg = shared();
    let rendered = format!("{leg:?}");
    assert!(
        rendered.contains("a directory of CSVs read by one process"),
        "the shared leg carries the acknowledgement: {rendered}"
    );
    assert!(
        !rendered.contains("Secret"),
        "the shared leg holds no credential material at all: {rendered}"
    );
    assert_eq!(leg.as_str(), "the deployment's own identity for this source");

    // And the two subject shapes are the ones that DO carry something, so the assertion above is not
    // passing because nothing anywhere holds material.
    let subject_material = Presented::SubjectToken {
        material: Secret::new("an-exchanged-token"),
    };
    let rendered = format!("{subject_material:?}");
    assert!(
        rendered.contains("Secret"),
        "a subject token is credential material: {rendered}"
    );
    assert!(
        !rendered.contains("an-exchanged-token"),
        "and it is redacted wherever it is printed: {rendered}"
    );
    assert_eq!(
        Presented::SubjectPrincipal {
            name: PrincipalName::parse("analyst_role").expect("a test name is a name"),
        }
        .as_str(),
        "a principal to switch to as the asker"
    );
}

#[test]
fn one_asker_holds_for_every_leg_because_there_is_one_field() {
    // The property `docs/adr/0008` part 4 hoists for: two legs, one `asked_by`, and no method that
    // could add a third leg with a subject of its own. The compile-fail doctest on `LegCredentials`
    // is the other half - this is the half that shows the value a federated answer would carry.
    let sources = SourceSet::of(source("local")).and(source("warehouse"));
    let mut presented = BTreeMap::new();
    drop(presented.insert(source("local"), shared()));
    drop(presented.insert(
        source("warehouse"),
        Presented::SubjectToken {
            material: Secret::new("an-exchanged-token"),
        },
    ));

    let credentials =
        LegCredentials::minted(a_person(), Expiry::NothingExpires, &sources, presented).expect("the set covers the plan");
    assert_eq!(credentials.asked_by(), &a_person());
    assert_eq!(credentials.count(), 2);
    // The legs are reached through the guard and not off the grant, which is the whole of what
    // `BoundToTheRequest` is for - `LegCredentials` has no accessor that yields a `Presented`.
    let bound = granted(Minted::Granted { credentials }, &a_person(), &sources);
    assert_eq!(
        bound.legs().map(|(name, _)| name.as_str()).collect::<Vec<&str>>(),
        vec!["local", "warehouse"],
        "legs are keyed by source, in source order"
    );
    // And the two legs ran under different postures, which is what makes the one `asked_by` field a
    // claim about who ASKED rather than about what each leg executed as.
    assert_eq!(
        bound.presented_for(&source("local")).map(Presented::as_str),
        Ok("the deployment's own identity for this source")
    );
    assert_eq!(
        bound.presented_for(&source("warehouse")).map(Presented::as_str),
        Ok("the asker's own credential")
    );
}

/// The grant behind a [`Minted`] that agrees with the request, for a test that is about something
/// else.
///
/// A helper because the guard is now the only way to a leg, so every test that wants one runs it -
/// and a test whose subject is the legs should not be spelling out the check.
fn granted(minted: Minted, asked_by: &Subject, sources: &SourceSet) -> BoundToTheRequest {
    let Agreed::Granted { credentials } = minted
        .agreeing_with(asked_by, sources, NOON)
        .expect("the fixture grant agrees with the fixture request")
    else {
        panic!("a granted answer is granted");
    };
    credentials
}

/// An instant, for the comparisons below. Nothing resolves it from a clock: the domain has none.
const NOON: u64 = 1_777_000_000;

#[test]
fn credentials_that_do_not_cover_the_plan_are_refused_at_construction_in_both_directions() {
    // A wiring defect between a broker and the plan, caught once, where the two values are both in
    // scope. Discovered at execution time instead, a missing leg is a leg that either runs as the
    // process or fails with nothing named - which is the failure this whole port exists to remove.
    let sources = SourceSet::of(source("local")).and(source("warehouse"));
    let mut only_one = BTreeMap::new();
    drop(only_one.insert(source("local"), shared()));
    assert_eq!(
        LegCredentials::minted(a_person(), Expiry::NothingExpires, &sources, only_one).unwrap_err(),
        CredentialsDoNotCoverThePlan::Missing { at: source("warehouse") }
    );

    // The other direction: minted for a source this answer does not read. Refused rather than
    // ignored, because which half is wrong - the broker or the plan - may be either one.
    let mut one_too_many = BTreeMap::new();
    drop(one_too_many.insert(source("local"), shared()));
    drop(one_too_many.insert(source("elsewhere"), shared()));
    assert_eq!(
        LegCredentials::minted(
            a_person(),
            Expiry::NothingExpires,
            &SourceSet::of(source("local")),
            one_too_many
        )
        .unwrap_err(),
        CredentialsDoNotCoverThePlan::Unasked { at: source("elsewhere") }
    );
}

#[test]
fn a_lookup_cannot_fail_for_a_source_the_request_asked_about() {
    // What the constructor and the guard buy together: `presented_for` is a `Result` and its `Err`
    // is unreachable for a source in the set the grant was checked against. Asserted so that a
    // future change loosening either one shows up here rather than as an unexplained fallback at a
    // call site.
    let mut presented = BTreeMap::new();
    drop(presented.insert(source("local"), shared()));
    let sources = SourceSet::of(source("local"));
    let credentials = LegCredentials::minted(Subject::TheDeploymentItself, Expiry::NothingExpires, &sources, presented)
        .expect("the set covers the plan");
    let bound = granted(Minted::Granted { credentials }, &Subject::TheDeploymentItself, &sources);
    assert_eq!(
        bound.presented_for(&source("local")).map(Presented::as_str),
        Ok("the deployment's own identity for this source")
    );
    assert_eq!(
        bound.presented_for(&source("nowhere")).map(Presented::as_str),
        Err(CredentialsDoNotFitTheRequest::Coverage {
            cause: CredentialsDoNotCoverThePlan::Missing { at: source("nowhere") }
        }),
        "a source nobody minted for has nothing here, and it says which"
    );
}

#[test]
fn a_grant_is_checked_against_the_request_in_four_directions_and_one_call() {
    // **THE GUARD, and the reason it is one call rather than three.** Three findings on this port -
    // a grant minted for another subject, a deadline nothing read, and a refusal naming a source
    // nobody asked about - were three shapes of the same defect: a broker's answer acted on without
    // being compared with the request it was made for. Each direction below is one of them, and the
    // fourth is the coverage check the constructor already made against a set the broker itself
    // chose.
    let sources = SourceSet::of(source("local"));
    let mint = |asked_by: Subject, not_after: Expiry| {
        let mut presented = BTreeMap::new();
        drop(presented.insert(source("local"), shared()));
        Minted::Granted {
            credentials: LegCredentials::minted(asked_by, not_after, &sources, presented).expect("the set covers the plan"),
        }
    };

    // 1. Another subject. `LegCredentials::minted` is `pub` and takes any `Subject`, so this is a
    //    value a real broker returns by defect or under compromise - and the audit record would
    //    have named the asker while the leg carried this.
    assert_eq!(
        mint(Subject::TheDeploymentItself, Expiry::NothingExpires)
            .agreeing_with(&a_person(), &sources, NOON)
            .unwrap_err(),
        CredentialsDoNotFitTheRequest::AnotherSubject {
            asked: a_person(),
            granted: Subject::TheDeploymentItself,
        }
    );

    // 2. A deadline that has passed. The epoch, because it is in the past for every clock - and
    //    because the defect this closes is a deadline computed correctly and read by nobody.
    assert_eq!(
        mint(a_person(), Expiry::At { unix_seconds: 0 })
            .agreeing_with(&a_person(), &sources, NOON)
            .unwrap_err(),
        CredentialsDoNotFitTheRequest::Expired {
            deadline_unix_seconds: 0,
            now_unix_seconds: NOON,
        }
    );
    // And the boundary, which is the direction this control rounds: `not_after` is whole seconds, so
    // at equality there is under a second of life left and the grant is refused rather than used.
    assert_eq!(
        mint(a_person(), Expiry::At { unix_seconds: NOON })
            .agreeing_with(&a_person(), &sources, NOON)
            .unwrap_err(),
        CredentialsDoNotFitTheRequest::Expired {
            deadline_unix_seconds: NOON,
            now_unix_seconds: NOON,
        }
    );
    // One second later is a grant, so the assertion above is about the boundary and not about every
    // deadline being refused.
    assert!(matches!(
        mint(a_person(), Expiry::At { unix_seconds: NOON + 1 }).agreeing_with(&a_person(), &sources, NOON),
        Ok(Agreed::Granted { .. })
    ));

    // 3. The set the REQUEST asked about, which is not the set the broker passed to the constructor.
    assert_eq!(
        mint(a_person(), Expiry::NothingExpires)
            .agreeing_with(&a_person(), &SourceSet::of(source("warehouse")), NOON)
            .unwrap_err(),
        CredentialsDoNotFitTheRequest::Coverage {
            cause: CredentialsDoNotCoverThePlan::Missing { at: source("warehouse") }
        }
    );

    // 4. A refusal naming a source nobody asked about. It stays a refusal for a source that WAS
    //    asked about, which is the half that makes this a check rather than a wall.
    assert_eq!(
        Minted::Refused {
            source: source("elsewhere")
        }
        .agreeing_with(&a_person(), &sources, NOON)
        .unwrap_err(),
        CredentialsDoNotFitTheRequest::RefusalNamesAnUnaskedSource { at: source("elsewhere") }
    );
    assert!(matches!(
        Minted::Refused { source: source("local") }
            .agreeing_with(&a_person(), &sources, NOON)
            .expect("a refusal about a source the request named is an answer"),
        Agreed::Refused { ref source } if source == &self::source("local")
    ));

    // And the honest grant passes all four, so none of the above is passing against a guard that
    // refuses everything.
    let Agreed::Granted { credentials } = mint(a_person(), Expiry::At { unix_seconds: NOON + 60 })
        .agreeing_with(&a_person(), &sources, NOON)
        .expect("a grant for the asker, covering the plan, with life left, is an answer")
    else {
        panic!("a granted answer is granted");
    };
    assert_eq!(credentials.asked_by(), &a_person());
    assert_eq!(credentials.not_after(), Expiry::At { unix_seconds: NOON + 60 });
}

#[test]
fn what_a_failure_says_names_the_subject_the_way_a_record_does_and_carries_no_material() {
    // The sentences reach an operator's log and never a caller - a `SurfaceFailure` is logged by the
    // transport - so what they carry is what makes a bad broker mapping findable: what established
    // each subject, and the identifier where there is one. `Subject::established` alone cannot tell
    // two verified people apart, which is why both halves are in the line.
    let rendered = CredentialsDoNotFitTheRequest::AnotherSubject {
        asked: a_person(),
        granted: Subject::TheDeploymentItself,
    }
    .to_string();
    assert!(rendered.contains("a verified subject `s***@example.com`"), "{rendered}");
    assert!(rendered.contains("a deployment subject"), "{rendered}");
    // No variant of this enum holds credential material, so there is nothing here to redact - which
    // is a property of the shape rather than of the sentence.
    assert!(!rendered.contains("Secret"), "{rendered}");
}

#[test]
fn nothing_expiring_is_a_case_a_reader_names_rather_than_a_sentinel_instant() {
    // A static credential an operator wrote does not expire, and the two wrong shapes are an
    // `Option<Expiry>` - where every reader decides what an absence permits - and a sentinel like
    // `u64::MAX`, which reads as a deadline and compares as one.
    assert_eq!(Expiry::NothingExpires.unix_seconds(), None);
    assert_eq!(
        Expiry::At {
            unix_seconds: 1_777_000_000
        }
        .unix_seconds(),
        Some(1_777_000_000)
    );
    assert_ne!(Expiry::NothingExpires, Expiry::At { unix_seconds: 0 });
}

#[test]
fn the_earliest_of_a_static_credential_and_an_expiring_token_is_the_token() {
    // THE DEFECT THIS REPLACED, asserted in the direction that was wrong. `Expiry` derived `Ord`, a
    // derived ordering on an enum is DECLARATION ORDER, and `NothingExpires` is declared first - so
    // `.min()` over exactly this pair answered `NothingExpires`: no deadline at all, for a set holding
    // a token that expires. The one operation this type tells a minter to perform, silently inverted.
    //
    // The previous test pinned that ordering and added a comment telling a reader not to read it as
    // instants. A type should not need the comment, so the ordering is gone and the operation is a
    // function.
    let token = Expiry::At {
        unix_seconds: 1_777_000_000,
    };
    assert_eq!(
        Expiry::earliest([Expiry::NothingExpires, token]),
        token,
        "a set holding one thing that expires has a deadline"
    );
    assert_eq!(
        Expiry::earliest([token, Expiry::NothingExpires]),
        token,
        "and the answer does not depend on the order the broker minted in"
    );

    // Two deadlines: the earlier one, which is the whole point of the fold.
    let sooner = Expiry::At {
        unix_seconds: 1_776_000_000,
    };
    assert_eq!(Expiry::earliest([token, sooner]), sooner);
    assert_eq!(Expiry::earliest([sooner, token]), sooner);
    assert_eq!(sooner.earlier_of(sooner), sooner, "and it is idempotent on one value");

    // Nothing minted, and nothing minted that expires, are the same answer - which is what the
    // static-credential broker that ships returns.
    assert_eq!(Expiry::earliest(core::iter::empty()), Expiry::NothingExpires);
    assert_eq!(
        Expiry::earliest([Expiry::NothingExpires, Expiry::NothingExpires]),
        Expiry::NothingExpires
    );
}

#[test]
fn a_source_set_is_non_empty_and_reading_one_source_twice_is_reading_it_once() {
    let one = SourceSet::of(source("local"));
    assert_eq!(one.count(), 1);
    assert!(one.contains(&source("local")));
    assert!(!one.contains(&source("elsewhere")));

    let same_again = SourceSet::of(source("local")).and(source("local"));
    assert_eq!(
        same_again.count(),
        1,
        "a plan that reads one source twice reads it once - unlike an execution record, where a \
         second leg for one source is a defect"
    );
    assert_eq!(
        SourceSet::of(source("warehouse"))
            .and(source("local"))
            .iter()
            .map(SourceName::as_str)
            .collect::<Vec<&str>>(),
        vec!["local", "warehouse"],
        "in source order, so a broker's N exchanges happen in a deterministic one"
    );
}

#[test]
fn a_principal_name_that_could_forge_a_record_line_does_not_parse() {
    // The same parser every principal identifier in this module goes through, so a role name reaches
    // an audit line under the same rules a subject identifier does. The newline is the one that
    // matters: a record is one line, so a newline here is a second record nobody wrote.
    assert_eq!(
        PrincipalName::parse("analyst\nsubject=admin"),
        Err(InvalidPrincipalId::ControlCharacter { code: 0x0A })
    );
    assert_eq!(
        PrincipalName::parse("analyst\u{202E}role"),
        Err(InvalidPrincipalId::InvisibleCharacter { code: 0x202E })
    );
    assert_eq!(PrincipalName::parse("  "), Err(InvalidPrincipalId::Empty));
    // Every way in is the same parse, so there is no second copy of those rules to drift.
    let parsed = PrincipalName::parse("  analyst_role  ").expect("a test name is a name");
    assert_eq!(
        parsed,
        PrincipalName::try_from(String::from("analyst_role")).expect("the same value converts")
    );
    assert_eq!(parsed.as_str(), "analyst_role");
    assert_eq!(parsed.to_string(), "analyst_role");
}

#[test]
fn a_leg_is_checked_against_the_posture_and_not_only_against_its_own_shape() {
    // The domain half of the check both adapters were missing. `agrees_with` is one exhaustive match
    // over the PAIR, so the three ways a leg can disagree are named rather than falling through.
    let at = source("local");
    let mine = SourcePosture::SharedServiceUser {
        declared: acknowledged(),
    };

    // The agreeing case first, so every refusal below is not passing against a function that refuses
    // everything.
    assert_eq!(shared().agrees_with(&mine, &at), Ok(()));
    assert_eq!(
        Presented::SubjectToken {
            material: Secret::new("an-exchanged-token"),
        }
        .agrees_with(&SourcePosture::ImpersonationAtSource, &at),
        Ok(())
    );

    // The witness case: right variant, wrong acknowledgement. This is the one a shape match cannot
    // see, and the one the review's substance is about - provenance is read off the posture, so a leg
    // accepted here would be recorded under an acknowledgement it did not carry.
    let elsewhere_witness = Presented::SharedServiceUser {
        declared: SharedIdentityDeclared::of(
            AcknowledgementReason::parse("a witness no operator wrote for this source").expect("a test reason is a reason"),
        ),
    };
    assert_eq!(elsewhere_witness.as_str(), shared().as_str(), "the shapes are equal");
    assert_eq!(
        elsewhere_witness.agrees_with(&mine, &at),
        Err(PresentedDisagreesWithPosture::WitnessIsNotThisSources { at: at.clone() })
    );

    // And the two shape disagreements, in both directions.
    assert_eq!(
        shared().agrees_with(&SourcePosture::ImpersonationAtSource, &at),
        Err(PresentedDisagreesWithPosture::ShapeIsNotThePosture {
            at: at.clone(),
            posture: "impersonation-at-source",
            presented: "the deployment's own identity for this source",
        })
    );
    assert_eq!(
        Presented::SubjectPrincipal {
            name: PrincipalName::parse("analyst_role").expect("a test name is a name"),
        }
        .agrees_with(&mine, &at),
        Err(PresentedDisagreesWithPosture::ShapeIsNotThePosture {
            at,
            posture: "shared-service-user",
            presented: "a principal to switch to as the asker",
        })
    );
}
