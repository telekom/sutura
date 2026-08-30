//! The static broker's own suite. No network, no clock, no data system.

use sutura_domain::identity::{Agreed, Expiry, Minted, Presented, PrincipalChain, RequestContext, SourceSet, Subject, SubjectId};
use sutura_domain::model::SourceName;

use super::StaticCredentialBroker;
use crate::security::DeploymentIdentity;
use crate::sources::{RawSourceEntry, SourceRegistry};
use sutura_domain::identity::CredentialBroker as _;

/// The absolute directory every entry here points at, so a path refusal is never why a test fails.
const DATA: &str = "/srv/sutura/data";

fn alias(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test alias is a name")
}

fn single_user() -> DeploymentIdentity {
    DeploymentIdentity::parse("single-user", Some("one operator, their own files, their own credentials"))
        .expect("a test mode is a mode")
}

fn shared(written: &str) -> RawSourceEntry<'_> {
    RawSourceEntry {
        written,
        kind: "files",
        data_dir: Some(DATA),
        billing_project: None,
        dataset: None,
        posture: "shared-service-user",
        acknowledged_because: Some("one process reading a directory of files as itself"),
        verification_identity: None,
    }
}

fn impersonating(written: &str) -> RawSourceEntry<'_> {
    RawSourceEntry {
        written,
        kind: "files",
        data_dir: Some(DATA),
        billing_project: None,
        dataset: None,
        posture: "impersonation-at-source",
        acknowledged_because: None,
        verification_identity: None,
    }
}

fn registry(entries: &[RawSourceEntry<'_>]) -> SourceRegistry {
    SourceRegistry::parse(entries, Some(&single_user())).expect("a test registry parses")
}

fn asked_by_a_person() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::Verified {
        id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
    }))
}

#[test]
fn the_static_broker_serves_a_shared_source_under_the_operators_own_acknowledgement() {
    // The single-user path, which is a shipping deployment mode rather than a fixture: the credential
    // is configuration, and what the leg presents is the witness the operator wrote - not a secret,
    // and not a placeholder standing in for one.
    let broker = StaticCredentialBroker::from_registry(&registry(&[shared("local")]));
    assert_eq!(broker.count(), 1);

    let context = asked_by_a_person();
    let requested = SourceSet::of(alias("local"));
    let minted = broker
        .mint(&context, &requested)
        .expect("a static broker cannot fail to mint what it holds");
    let Minted::Granted { ref credentials } = minted else {
        panic!("a declared shared source has a credential: {minted:?}");
    };
    // One asker for the whole answer, and it is the subject the transport established rather than
    // anything this broker chose.
    assert_eq!(credentials.asked_by(), context.chain().subject());
    assert_eq!(
        credentials.not_after(),
        Expiry::NothingExpires,
        "a credential an operator wrote in a file has no lifetime, and the type says so as a case"
    );
    // The leg is read through the guard the application runs, because that is the only way to one -
    // and this broker's answer is expected to pass it: minting for the asker, covering the set it was
    // asked about, with nothing that expires. `NOON` is any instant; nothing here expires.
    let Agreed::Granted { credentials } = minted
        .agreeing_with(context.chain().subject(), &requested, 1_777_000_000)
        .expect("what this broker minted agrees with the request it was minted for")
    else {
        panic!("a granted answer is granted");
    };
    let leg = credentials.presented_for(&alias("local")).expect("the leg was minted");
    assert!(
        matches!(*leg, Presented::SharedServiceUser { .. }),
        "the shape has to be the shared one, so an answer records what actually ran: {leg:?}"
    );
    assert!(
        format!("{leg:?}").contains("one process reading a directory of files as itself"),
        "the leg carries the operator's stated reason: {leg:?}"
    );
}

#[test]
fn a_subject_with_no_credential_is_refused_rather_than_downgraded() {
    // THE REFUSAL THIS STEP ADDS, provoked by the real implementor from a real configuration - no
    // network and no fake. A source declared `impersonation-at-source` has no static credential that
    // could execute as the asking subject, and this broker has nothing to present for it.
    //
    // What must NOT happen is the alternative: minting the deployment's own identity for it, which
    // would answer the question with rows the asker may never have been permitted to see.
    let broker = StaticCredentialBroker::from_registry(&registry(&[impersonating("warehouse")]));
    assert_eq!(
        broker.count(),
        0,
        "an impersonating source is absent rather than mapped to a default"
    );

    let minted = broker
        .mint(&asked_by_a_person(), &SourceSet::of(alias("warehouse")))
        .expect("a refusal is a result, not an error");
    let Minted::Refused { source } = minted else {
        panic!("a static broker cannot impersonate anybody: {minted:?}");
    };
    assert_eq!(source, alias("warehouse"));
}

#[test]
fn one_unmintable_leg_refuses_the_whole_answer() {
    // The federated shape, before there is a splitter to build one: minting is ONE call for every
    // source the plan reads, so a plan whose second leg has no credential is refused rather than
    // half-answered. Asserted here because the alternative - grant what you can, leave the rest -
    // is the shape somebody reaches for when the port takes one source at a time.
    let broker = StaticCredentialBroker::from_registry(&registry(&[shared("local"), impersonating("warehouse")]));
    assert_eq!(broker.count(), 1);

    let both = SourceSet::of(alias("local")).and(alias("warehouse"));
    let minted = broker
        .mint(&asked_by_a_person(), &both)
        .expect("a refusal is a result, not an error");
    assert!(
        matches!(minted, Minted::Refused { ref source } if *source == alias("warehouse")),
        "the leg that cannot be minted names itself and the answer is refused: {minted:?}"
    );

    // And the source that CAN be minted for is still mintable on its own, so the test above is not
    // passing against a broker that refuses everything.
    assert!(matches!(
        broker
            .mint(&asked_by_a_person(), &SourceSet::of(alias("local")))
            .expect("a static broker cannot fail to mint what it holds"),
        Minted::Granted { .. }
    ));

    // **The refusal is decided before anything is constructed, and the ORDER is what is assertable.**
    // A review asked for a count of `Presented` values built, as zero. The count is not observable
    // from outside a broker - the map is a local inside `mint` - and under the single pass this
    // replaced it would not have been zero either: sources iterate in name order, so `local`'s
    // credential was built and then dropped when `warehouse` refused. What is checkable is that the
    // refusal does not depend on where the unmintable source sits in the set, which is exactly what a
    // "check everything first" pass buys and a "build as you go" one does not.
    for ordering in [
        SourceSet::of(alias("warehouse")).and(alias("local")),
        SourceSet::of(alias("local")).and(alias("warehouse")),
    ] {
        assert!(
            matches!(
                broker
                    .mint(&asked_by_a_person(), &ordering)
                    .expect("a refusal is a result, not an error"),
                Minted::Refused { ref source } if *source == alias("warehouse")
            ),
            "the unmintable leg refuses the answer wherever it sits in the set: {ordering:?}"
        );
    }
}

#[test]
fn a_deployment_that_declares_no_source_can_mint_nothing() {
    // The honest answer for a settings tree with no `sources:` key: nothing is declared, so nothing
    // is presentable, and every question is refused by source. What it must not be is a broker that
    // grants a credential for a source nobody declared.
    let broker = StaticCredentialBroker::from_registry(&registry(&[]));
    assert_eq!(broker.count(), 0);
    assert!(matches!(
        broker
            .mint(&asked_by_a_person(), &SourceSet::of(alias("local")))
            .expect("a refusal is a result, not an error"),
        Minted::Refused { .. }
    ));
}
