//! `DeclaredPrincipalBroker`'s own suite. A child module of `principal`, so every private item that
//! file declares is reachable through `super::`.

use std::collections::BTreeMap;

use sutura_domain::identity::{
    CredentialBroker as _, Minted, Presented, PrincipalChain, PrincipalName, RequestContext, SourceSet, Subject, SubjectKey,
};
use sutura_domain::model::SourceName;
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared};

use super::{DeclaredPrincipalBroker, DeclaredPrincipals, NoDeclaredPrincipals};

/// The instant every agreement below is measured against: 2096-10-02.
///
/// **Far out on purpose**, for the reason the deleted exchanging broker's suite recorded: one
/// measured against the wall clock is scheduled to go red on a date nobody is watching. Before
/// [`A_FIXTURE_EXPIRY`], so a fixture caller's assertion is still usable at it - the cell that cares
/// about the other direction names its own instants.
const A_FIXED_NOW: u64 = 4_000_000_000;

/// The instant a fixture caller's assertion stops being one: 2100-01-01.
///
/// After [`A_FIXED_NOW`], because most cells here are about what was presented and to whom rather
/// than about when it lapses. `the_minted_expiry_is_the_asking_assertions_own` is the cell on the
/// lifetime itself and states its instants inline.
const A_FIXTURE_EXPIRY: u64 = 4_102_444_800;

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source name is a name")
}

fn declared_shared() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(
        AcknowledgementReason::parse("a read-only reporting replica every caller is entitled to see")
            .expect("a test reason is a reason"),
    )
}

/// A caller whose verified `sub` is `raw`, carrying the assertion leg 1 verified for it.
///
/// **With an assertion, because a federating broker requires one**: what it presents IS the
/// caller's own document, so a context without one is a caller that cannot be served. `no_assertion`
/// below is the cell on that direction.
fn caller(raw: &str) -> RequestContext {
    RequestContext::with_assertion(
        PrincipalChain::of(Subject::verified(raw).expect("a test subject is a subject")),
        sutura_domain::identity::Secret::new(format!("assertion.for.{raw}")),
        A_FIXTURE_EXPIRY,
    )
}

/// The same caller with nothing for a pool to verify.
fn no_assertion(raw: &str) -> RequestContext {
    RequestContext::of(PrincipalChain::of(
        Subject::verified(raw).expect("a test subject is a subject"),
    ))
}

/// A request with no caller identity at all - the deployment's own front door and nobody behind it.
fn no_caller() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
}

fn subject(raw: &str) -> SubjectKey {
    SubjectKey::parse(raw).expect("a test subject key is a key")
}

fn principal(raw: &str) -> PrincipalName {
    PrincipalName::parse(raw).expect("a test principal is a principal")
}

/// The two-subject declaration every cell below is measured against.
fn two_analysts() -> DeclaredPrincipals {
    DeclaredPrincipals::parse(BTreeMap::from([
        (subject("analyst-a@example.com"), principal("bq-a@sutura.example.com")),
        (subject("analyst-b@example.com"), principal("bq-b@sutura.example.com")),
    ]))
    .expect("a two-subject declaration names somebody")
}

fn warehouse() -> DeclaredPrincipalBroker {
    DeclaredPrincipalBroker::empty().impersonating(source("warehouse"), two_analysts())
}

/// The credentials a mint granted, bound to the request that asked for them.
///
/// **Bound rather than read straight off the mint, because `LegCredentials` deliberately has no
/// per-leg accessor**: `Minted::agreeing_with` is the only door to a value an adapter executes
/// with, so a suite that read a leg out directly would be asserting on something no caller can
/// reach. Its own doc records that the two accessors moved for exactly that reason.
fn granted(minted: Minted, raw: &str, sources: &SourceSet) -> sutura_domain::identity::BoundToTheRequest {
    let asked_by = Subject::verified(raw).expect("a test subject is a subject");
    let agreed = minted
        .agreeing_with(&asked_by, sources, A_FIXED_NOW)
        .expect("the grant agrees with the request");
    let sutura_domain::identity::Agreed::Granted { credentials } = agreed else {
        panic!("expected a granted agreement")
    };
    credentials
}

/// The ACCOUNT a mint declared for one source's leg, or `None` where it declared none.
///
/// **The value read through `agreeing_with` like every other observable here**, because a cell that
/// reached into the map would be asserting on something no adapter can reach.
fn target_at(minted: Minted, raw: &str, sources: &SourceSet, at: &SourceName) -> Option<String> {
    match granted(minted, raw, sources)
        .presented_for(at)
        .expect("the source was asked for")
    {
        Presented::SubjectToken { impersonate, .. } => impersonate.as_ref().map(PrincipalName::to_string),
        other => panic!("expected the asker's own credential for {at}, got {other:?}"),
    }
}

/// The principal a mint presented for one source, or a panic naming how it did not.
fn presented_at(minted: Minted, raw: &str, sources: &SourceSet, at: &SourceName) -> String {
    #[expect(
        clippy::disallowed_methods,
        reason = "a cell asserting WHOSE assertion reached the leg needs its text; production \
                  never reads it as a string"
    )]
    match granted(minted, raw, sources)
        .presented_for(at)
        .expect("the source was asked for")
    {
        Presented::SubjectToken { material, .. } => String::from(material.expose_secret()),
        other => panic!("expected the asker's own assertion for {at}, got {other:?}"),
    }
}

#[test]
fn two_distinct_subjects_are_served_their_own_assertions_and_never_each_others() {
    // **The leg-2 bar, at the one seam this repository can assert without a cloud account.** Two
    // verified callers, one source, one broker - and two different accounts out. Everything past
    // here is the driver's: `adbc::identity` turns the name into the driver's impersonation target,
    // and only a hosted run can show the data system agreeing. What this cell holds is that the
    // resolution is per subject and not per deployment.
    let broker = warehouse();
    let at = source("warehouse");
    let asked = SourceSet::of(at.clone());
    let first = broker
        .mint(&caller("analyst-a@example.com"), &asked)
        .expect("a declared subject is mintable");
    let second = broker
        .mint(&caller("analyst-b@example.com"), &asked)
        .expect("a declared subject is mintable");
    let first = presented_at(first, "analyst-a@example.com", &asked, &at);
    let second = presented_at(second, "analyst-b@example.com", &asked, &at);
    assert_eq!(first, "assertion.for.analyst-a@example.com");
    assert_eq!(second, "assertion.for.analyst-b@example.com");
    assert_ne!(
        first, second,
        "both subjects were served one document, so the pool would resolve them to one principal"
    );
}

#[test]
fn a_verified_caller_this_source_does_not_name_is_refused_and_never_widened() {
    // `docs/adr/0032`'s rule, here rather than only in the exchanging broker: a source that declares
    // WHO may execute there has named an authorization set, and a caller outside it is refused
    // rather than served as the deployment. The refusal names the source and never the subject.
    let refused = warehouse()
        .mint(&caller("someone-else@example.com"), &SourceSet::of(source("warehouse")))
        .expect("an undeclared caller is a refusal, not an error");
    assert!(
        matches!(refused, Minted::Refused { ref source } if source.as_str() == "warehouse"),
        "{refused:?}"
    );
    assert!(
        !format!("{refused:?}").contains("someone-else@example.com"),
        "a refusal that reaches a log may not carry the caller: {refused:?}"
    );
}

#[test]
fn a_verified_caller_with_no_assertion_cannot_be_served_by_an_impersonating_source() {
    // **The other half of *no fallback*.** A federating broker presents the caller's OWN document,
    // so a request that carries none has nothing for Google's token service to verify - and the
    // only alternative to refusing is answering as the deployment. Leg 1 is what fills this field
    // in; a deployment serving an impersonating source without an inbound gate reaches here.
    let refused = warehouse()
        .mint(&no_assertion("analyst-a@example.com"), &SourceSet::of(source("warehouse")))
        .expect("a caller with no assertion is a refusal, not an error");
    assert!(matches!(refused, Minted::Refused { .. }), "{refused:?}");
}

#[test]
fn a_request_with_no_verified_caller_cannot_be_served_by_an_impersonating_source() {
    // There is no key to look up, and the fallback that does not exist is the point: a deployment's
    // own front door with nobody behind it must not read an impersonating source as itself.
    let refused = warehouse()
        .mint(&no_caller(), &SourceSet::of(source("warehouse")))
        .expect("an unidentified caller is a refusal, not an error");
    assert!(matches!(refused, Minted::Refused { .. }), "{refused:?}");
}

#[test]
fn a_source_this_broker_holds_neither_half_for_is_refused() {
    // The forgotten-attachment case. A composition root that opened a source and never declared it
    // here gets a refusal per question, never every row as the deployment.
    let refused = warehouse()
        .mint(&caller("analyst-a@example.com"), &SourceSet::of(source("replica")))
        .expect("an undeclared source is a refusal, not an error");
    assert!(
        matches!(refused, Minted::Refused { ref source } if source.as_str() == "replica"),
        "{refused:?}"
    );
}

#[test]
fn a_plan_reading_one_shared_source_and_one_impersonating_source_gets_one_of_each() {
    // One broker per ANSWER rather than per source, which is why both maps are held: a federated
    // plan touching a shared replica and an impersonating warehouse is served once.
    let broker = warehouse().shared(source("replica"), declared_shared());
    let asked = SourceSet::of(source("warehouse")).and(source("replica"));
    let minted = broker
        .mint(&caller("analyst-a@example.com"), &asked)
        .expect("both halves are declared");
    let bound = granted(minted, "analyst-a@example.com", &asked);
    assert!(
        matches!(bound.presented_for(&source("warehouse")), Ok(Presented::SubjectToken { .. })),
        "an impersonating source is presented the asking subject's own assertion"
    );
    assert!(
        matches!(
            bound.presented_for(&source("replica")),
            Ok(Presented::SharedServiceUser { .. })
        ),
        "a shared source is presented the operator's witness, never a principal"
    );
    assert_eq!(broker.count(), 2);
}

#[test]
fn nothing_is_minted_for_a_plan_whose_second_source_cannot_be_served() {
    // `StaticCredentialBroker`'s two-pass property, asserted here too: the refusal is decided
    // before anything is built, so "refused before anything was minted" is a fact about what
    // happened rather than about what escaped. The observable is that the answer is the REFUSAL and
    // names the unservable source rather than the one that would have been minted first.
    let refused = warehouse()
        .mint(
            &caller("analyst-a@example.com"),
            &SourceSet::of(source("warehouse")).and(source("replica")),
        )
        .expect("an undeclared source is a refusal, not an error");
    assert!(
        matches!(refused, Minted::Refused { ref source } if source.as_str() == "replica"),
        "{refused:?}"
    );
}

#[test]
fn a_declaration_naming_nobody_is_refused_at_parse_rather_than_per_request() {
    // **The defect class this whole change removes, as a cell.** An impersonating source whose map
    // is empty can serve no caller at all. If that were a value, the deployment would boot clean
    // and refuse every question - so it is not a value: `DeclaredPrincipals::parse` refuses it, and
    // the composition root turns that into a startup failure.
    let refused = DeclaredPrincipals::parse(BTreeMap::new()).expect_err("an empty declaration names nobody");
    assert_eq!(refused, NoDeclaredPrincipals::Empty);
    assert_eq!(two_analysts().count(), 2);
}

#[test]
fn an_empty_broker_can_serve_nothing() {
    // The default is *nothing declared*, and nothing declared refuses. A broker that widened an
    // unknown source into the deployment's own identity is the fallback the port exists to remove.
    let broker = DeclaredPrincipalBroker::empty();
    assert_eq!(broker.count(), 0);
    let refused = broker
        .mint(&caller("analyst-a@example.com"), &SourceSet::of(source("warehouse")))
        .expect("an empty broker refuses rather than erring");
    assert!(matches!(refused, Minted::Refused { .. }), "{refused:?}");
}

/// The same caller, with the instant its assertion stops being one stated by the cell.
fn caller_expiring_at(raw: &str, unix_seconds: u64) -> RequestContext {
    RequestContext::with_assertion(
        PrincipalChain::of(Subject::verified(raw).expect("a test subject is a subject")),
        sutura_domain::identity::Secret::new(format!("assertion.for.{raw}")),
        unix_seconds,
    )
}

#[test]
fn the_minted_expiry_is_the_asking_assertions_own() {
    // **A round of this broker minted `Expiry::NothingExpires`**, which was honest while it
    // presented a principal's NAME - a name does not age - and became a check that always answers
    // yes the moment it started presenting the caller's own assertion. A federated leg is valid for
    // exactly as long as that assertion is, and `still_usable_at` is the reader.
    //
    // Measured through `agreeing_with`, which is the door the application goes through, rather than
    // by reading a field: the expiry only matters if a boundary compares it.
    let lapses_at = 1_700_000_000;
    let asked = SourceSet::of(source("warehouse"));
    let minted = warehouse()
        .mint(&caller_expiring_at("analyst-a@example.com", lapses_at), &asked)
        .expect("a declared caller is served");
    let asked_by = Subject::verified("analyst-a@example.com").expect("a test subject is a subject");
    let refused = minted
        .agreeing_with(&asked_by, &asked, lapses_at + 1)
        .expect_err("an assertion that has lapsed is not one a leg may present");
    assert!(
        matches!(
            refused,
            sutura_domain::identity::CredentialsDoNotFitTheRequest::Expired {
                deadline_unix_seconds,
                ..
            } if deadline_unix_seconds == lapses_at
        ),
        "the refusal names the assertion's own deadline: {refused:?}"
    );
    // And one second earlier it is usable, so the cell is about the deadline rather than about
    // everything being refused.
    let minted = warehouse()
        .mint(&caller_expiring_at("analyst-a@example.com", lapses_at), &asked)
        .expect("a declared caller is served");
    drop(
        minted
            .agreeing_with(&asked_by, &asked, lapses_at - 1)
            .expect("an assertion still in date is one a leg may present"),
    );
}

#[test]
fn a_shared_only_plan_does_not_inherit_the_callers_own_deadline() {
    // The other direction of the same fold, and the reason it is `Expiry::earliest` over one entry
    // per source rather than one expiry for the whole mint. A shared leg runs on a credential this
    // broker did not mint and which does not age with the caller; giving it the caller's deadline
    // would refuse answers for a lifetime that does not apply to them.
    let lapses_at = 1_700_000_000;
    let asked = SourceSet::of(source("replica"));
    let broker = DeclaredPrincipalBroker::empty().shared(source("replica"), declared_shared());
    let minted = broker
        .mint(&caller_expiring_at("analyst-a@example.com", lapses_at), &asked)
        .expect("a declared shared source is served");
    let asked_by = Subject::verified("analyst-a@example.com").expect("a test subject is a subject");
    drop(
        minted
            .agreeing_with(&asked_by, &asked, lapses_at + 100_000)
            .expect("a shared leg does not lapse when the caller's own assertion does"),
    );
}

#[test]
fn each_subject_is_minted_the_account_declared_beside_it() {
    // **THE F3 cell: the map's VALUES now decide something.** A round of this broker read the KEY
    // to answer *may this caller be served* and dropped the account beside it, so changing a
    // declared target principal had no effect on anything - a security-critical setting accepted
    // and then ignored (`telekom/sutura#929`'s own review). What this holds is that each subject is
    // minted the account declared FOR THAT SUBJECT: not `None`, not the first entry for everyone,
    // and not the key it was looked up by.
    let broker = warehouse();
    let at = source("warehouse");
    let asked = SourceSet::of(at.clone());
    let for_a = broker
        .mint(&caller("analyst-a@example.com"), &asked)
        .expect("a declared subject is mintable");
    let for_b = broker
        .mint(&caller("analyst-b@example.com"), &asked)
        .expect("a declared subject is mintable");
    let a = target_at(for_a, "analyst-a@example.com", &asked, &at);
    let b = target_at(for_b, "analyst-b@example.com", &asked, &at);
    assert_eq!(a.as_deref(), Some("bq-a@sutura.example.com"));
    assert_eq!(b.as_deref(), Some("bq-b@sutura.example.com"));
    assert_ne!(
        a, b,
        "both subjects were declared one account, so one map entry decided both legs"
    );
    // And it is the VALUE rather than the key: the keys are the two `analyst-*` subjects, so a
    // broker minting what it looked up by would have put one of those here.
    assert!(
        !format!("{a:?}{b:?}").contains("analyst-"),
        "the subject key was minted as the account to execute as: {a:?} / {b:?}"
    );
}

#[test]
fn a_declared_target_that_is_not_a_service_account_email_is_refused_at_parse() {
    // **The send-side narrowing, as a mechanism rather than a paragraph.** The account is
    // interpolated into ONE PATH SEGMENT of the URL that decides which account a question runs as,
    // and `cloud.google.com/go/auth`'s impersonation provider POSTs that URL verbatim with no shape
    // check of its own. `PrincipalName::parse` ACCEPTS every value below - it is the parser every
    // principal identifier in the domain shares, and a role name is not an email - so without this
    // refusal a declaration carrying `/` re-points the segment at an account nobody declared.
    //
    // Refused at PARSE, so `sutura_cli`'s `build_broker` turns it into a startup failure: a
    // deployment that cannot name its declared accounts must not boot clean.
    let traversal = "sa/../../projects/-/serviceAccounts/other@x.iam.gserviceaccount.com";
    for hostile in [
        traversal,
        "no-at-sign.example.com",
        "two@at@signs.example.com",
        "has space@example.com",
        "has\"quote@example.com",
        "query@example.com?alt=json",
    ] {
        let declared = principal(hostile);
        // The domain's own parser is what accepts it, which is why this cell exists at all.
        assert_eq!(
            declared.as_str(),
            hostile,
            "the domain narrowed this, and the cell below proves nothing"
        );
        let refused = DeclaredPrincipals::parse(BTreeMap::from([(subject("analyst-a@example.com"), declared.clone())]))
            .expect_err("a value that is not a service-account address is not a declarable target");
        assert_eq!(refused, NoDeclaredPrincipals::NotAServiceAccount { target: declared });
    }
    // And the accepted direction, so the refusal above is not passing against a parse that refuses
    // everything: `two_analysts` is built by `parse` and every cell here depends on it.
    assert_eq!(two_analysts().count(), 2);
}
