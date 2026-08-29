//! The credential port's own suite.
//!
//! In its own file for the reason `crate::source`'s is: the module it tests is close enough to the
//! 1000-line gate that a suite inline would have decided how much documentation the types get.

use std::collections::BTreeMap;

use super::{CredentialsDoNotCoverThePlan, Expiry, LegCredentials, Presented, PrincipalName, SourceSet};
use crate::identity::{InvalidPrincipalId, Secret, Subject, SubjectId};
use crate::model::SourceName;
use crate::source::{AcknowledgementReason, SharedIdentityDeclared};

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
    assert_eq!(credentials.legs().count(), 2);
    assert_eq!(
        credentials.legs().map(|(name, _)| name.as_str()).collect::<Vec<&str>>(),
        vec!["local", "warehouse"],
        "legs are keyed by source, in source order"
    );
    // And the two legs ran under different postures, which is what makes the one `asked_by` field a
    // claim about who ASKED rather than about what each leg executed as.
    assert_eq!(
        credentials.presented_for(&source("local")).map(Presented::as_str),
        Some("the deployment's own identity for this source")
    );
    assert_eq!(
        credentials.presented_for(&source("warehouse")).map(Presented::as_str),
        Some("the asker's own credential")
    );
}

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
fn a_lookup_cannot_be_none_for_a_source_the_set_was_minted_for() {
    // What the constructor above buys: `presented_for` is an `Option` and its `None` is unreachable
    // for a source in the set. Asserted so that a future change loosening `minted` shows up here
    // rather than as an unexplained fallback at a call site.
    let mut presented = BTreeMap::new();
    drop(presented.insert(source("local"), shared()));
    let credentials = LegCredentials::minted(
        Subject::TheDeploymentItself,
        Expiry::NothingExpires,
        &SourceSet::of(source("local")),
        presented,
    )
    .expect("the set covers the plan");
    assert!(credentials.presented_for(&source("local")).is_some());
    assert!(
        credentials.presented_for(&source("nowhere")).is_none(),
        "a source nobody minted for has nothing here"
    );
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
    // Ordered, so whoever mints can take the earliest across what it minted. `NothingExpires` sorts
    // first because it is declared first, which is why the comparison below is the one asserted: an
    // ordering over these two is not a comparison of instants and nothing may read it as one.
    assert_ne!(Expiry::NothingExpires, Expiry::At { unix_seconds: 0 });
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
        Err(InvalidPrincipalId::ControlCharacter {
            value: String::from("analyst\nsubject=admin"),
        })
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
