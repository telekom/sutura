//! What the credential reader decides, over documents rather than files.
//!
//! Nothing here reaches a network and nothing here holds a real key. What is asserted is the shape
//! decisions: which kind a document becomes, which refusals name what, and that no value from the file
//! reaches a message.
//!
//! **One thing is deliberately not tested, and the reason is a hard one rather than an omission:**
//! `CredentialFile::well_known` reads three environment variables, `std::env::set_var` is `unsafe` in
//! this edition, and `unsafe_code` is `forbid` in this workspace - so there is no way to give it an
//! environment from here. What is testable is that the two constructors are distinguishable and that
//! the explicit one is exact.

use super::{
    AccessTokens as _, Credential, CredentialFile, Document, KeyUnusable, Kind, PEM_BEGIN_MARK, PEM_END_MARK, QuotaProject,
    UnusableCredential, deadline, readable_key, unwrap_pem,
};
use crate::wire::{BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};
use sutura_domain::identity::{Expiry, Secret};

/// The pinned client, which is the only kind this module accepts.
fn pinned() -> WireAgent {
    WireAgent::pinned(JobBounds::of(
        QueryDeadline::parse(30).expect("30 seconds is a deadline"),
        BytesBilledCeiling::parse(1024 * 1024).expect("a mebibyte is a ceiling"),
    ))
}

/// Where a refusal says the file was. A path that does not exist, never read.
fn at() -> std::path::PathBuf {
    std::path::PathBuf::from("/nonexistent/credentials.json")
}

/// A document with nothing in it but its kind, so each test adds only the fields it is about.
fn document(kind: &str) -> Document {
    Document {
        kind: String::from(kind),
        universe_domain: None,
        client_id: None,
        client_secret: None,
        refresh_token: None,
        client_email: None,
        private_key: None,
        private_key_id: None,
        project_id: None,
    }
}

/// A complete `authorized_user`. None of these values is a credential; they are the shape.
fn authorized_user() -> Document {
    Document {
        client_id: Some(String::from("an-installed-app.apps.example")),
        client_secret: Some(String::from("not-a-secret")),
        refresh_token: Some(String::from("not-a-token")),
        ..document("authorized_user")
    }
}

/// The body every PEM fixture here carries.
///
/// **Four `A`s, and the reason is not brevity.** The first version spelled a short base64 word, and the
/// leak guard reported it as a generic API key - which would have blocked `pre-push` for everybody over
/// a string chosen to be obviously not a key. A run of one character cannot look like an encoded
/// secret to any detector, and nothing in these tests reads the body's value: the parser packs it and
/// `ring` never sees it, because no test here signs.
const FAKE_BODY: &str = "AAAA";

/// A wrapped body, built from the parser's own delimiters.
///
/// **Assembled rather than written out**, for the reason the constants themselves are: a literal PEM
/// header in this file tripped the leak guard, which would have blocked `pre-push` for everybody over
/// a delimiter that is not a key. Building it from the same constants the parser reads also means a
/// fixture cannot drift from what it is testing.
fn wrapped(label: &str, body: &str) -> String {
    format!("{PEM_BEGIN_MARK}{label}-----\n{body}\n{PEM_END_MARK}{label}-----\n")
}

/// A complete `service_account` DOCUMENT, whose PEM block is well-formed text and not a key.
///
/// **Reading this now FAILS, and that is the point of it.** Since review, `read_document` decodes the
/// base64 and hands the DER to `ring`, so a syntactically valid but meaningless body is refused at read
/// - which is the whole finding.
///
/// This fixture is therefore the negative one: it is what the key tests below feed in, and what
/// [`robot`] exists to work around.
fn service_account() -> Document {
    Document {
        client_email: Some(String::from("a-robot@example.invalid")),
        private_key_id: Some(String::from("0123456789abcdef")),
        private_key: Some(wrapped("PRIVATE KEY", FAKE_BODY)),
        project_id: Some(String::from("a-payer")),
        ..document("service_account")
    }
}

/// A service-account credential assembled directly, bypassing the key parse.
///
/// **Why this exists, stated because a test-only constructor is a thing to be suspicious of.** Reading
/// a service-account document now parses the `PKCS#8` key, and a valid 2048-bit RSA key is the one
/// fixture this file cannot have: `ring` deliberately cannot generate one, the workspace's other crypto
/// backend needs `cmake` and is not linked, and a real key committed to a public repository is key
/// material in a public repository whatever it authenticates - the same reason the PEM DELIMITERS above
/// are assembled from constants.
///
/// So the split is: the KEY's own stages are pinned by the refusals below, the positive path over a
/// real key belongs to `tests/acceptance.rs`, which signs with one, and this fixture carries everything
/// ABOVE the key - which kind a document becomes, which project it names, what it asks of a request,
/// and that nothing prints it. It reaches into private fields rather than adding a constructor, so the
/// shipped crate still has exactly one way in.
fn robot() -> Credential {
    Credential {
        agent: pinned(),
        kind: Kind::ServiceAccount {
            client_email: String::from("a-robot@example.invalid"),
            private_key_id: String::from("0123456789abcdef"),
            private_key: Secret::new(FAKE_BODY),
            project_id: String::from("a-payer"),
        },
    }
}

#[test]
fn each_of_the_two_kinds_this_build_reads_becomes_its_own_shape() {
    // **The closed shape is the point.** A document with a refresh token AND a private key cannot
    // become a credential holding both, because the two variants carry disjoint fields - so which
    // exchange will run is decided once, here, by the file's own word.
    let user = Credential::read_document(authorized_user(), &at(), pinned()).expect("a complete user credential reads");
    assert_eq!(user.kind(), "authorized_user");
    assert_eq!(user.project(), None, "an application-default login names no project");

    let robot = robot();
    assert_eq!(robot.kind(), "service_account");
    assert_eq!(
        robot.project().map(String::as_str),
        Some("a-payer"),
        "a service-account key names its own project, which is why CI configures none"
    );
}

#[test]
fn the_quota_project_header_is_required_for_one_kind_and_would_break_the_other() {
    // **Not a preference.** Without the header an END-USER credential is refused with a message about
    // user credentials not being supported; WITH it a service account needs
    // `serviceusage.services.use` on the project, which one holding only dataset grants does not have.
    // So the answer is a property of the kind, and both directions are a real failure.
    let user = Credential::read_document(authorized_user(), &at(), pinned()).expect("a user credential reads");
    assert_eq!(user.quota_project(), QuotaProject::Required);

    assert_eq!(robot().quota_project(), QuotaProject::FromTheCredential);
}

#[test]
fn a_credential_shape_this_build_does_not_read_is_refused_by_name() {
    // **Named rather than "unsupported"**, because each shape has a different answer: the metadata
    // server needs no file, and a federated credential is the per-subject step.
    for named in [
        "external_account",
        "impersonated_service_account",
        "external_account_authorized_user",
        "gdch_service_account",
    ] {
        let refused = Credential::read_document(document(named), &at(), pinned());
        match refused {
            Err(UnusableCredential::UnknownKind { named: ref found, .. }) => assert_eq!(found, named),
            other => panic!("{named} was accepted: {other:?}"),
        }
    }
}

#[test]
fn a_credential_type_that_would_forge_a_log_line_is_bounded_before_it_reaches_one() {
    // The module's own rule applied to the one field that reaches a message. `type` is the least
    // sensitive value in the file and it is still a value: a 16 KiB file can put 16 KiB of newlines
    // there. Asserted at THIS call site rather than left to the shared helper's own test, because what
    // is claimed is that this refusal goes through it.
    let hostile = document("service\naccount \u{1b}[31m\"x\"");
    let refused = Credential::read_document(hostile, &at(), pinned()).expect_err("it refused");
    match refused {
        UnusableCredential::UnknownKind { ref named, .. } => assert_eq!(*named, "serviceaccount31mx"),
        ref other => panic!("a hostile type was mapped to {other:?}"),
    }
    let shown = refused.to_string();
    assert!(!shown.contains('\n'), "the refusal carried a newline: {shown:?}");

    let refused = Credential::read_document(document(&"z".repeat(16 * 1024)), &at(), pinned()).expect_err("it refused");
    match refused {
        UnusableCredential::UnknownKind { ref named, .. } => assert_eq!(named.len(), 64),
        ref other => panic!("an oversized type was mapped to {other:?}"),
    }
}

#[test]
fn every_field_a_kind_needs_is_refused_by_name_when_it_is_absent_or_blank() {
    // A field NAME rather than "the credential is incomplete", because the fix for each is a different
    // line of the file. Both absence and whitespace, because a key written as `""` by a template is the
    // ordinary way this goes wrong.
    let user_cases: [(&str, Document); 4] = [
        (
            "client_id",
            Document {
                client_id: None,
                ..authorized_user()
            },
        ),
        (
            "client_secret",
            Document {
                client_secret: Some(String::from("  ")),
                ..authorized_user()
            },
        ),
        (
            "refresh_token",
            Document {
                refresh_token: None,
                ..authorized_user()
            },
        ),
        (
            "client_id",
            Document {
                client_id: Some(String::from("\t")),
                ..authorized_user()
            },
        ),
    ];
    let robot_cases: [(&str, Document); 4] = [
        (
            "client_email",
            Document {
                client_email: None,
                ..service_account()
            },
        ),
        (
            "private_key_id",
            Document {
                private_key_id: None,
                ..service_account()
            },
        ),
        (
            "private_key",
            Document {
                private_key: Some(String::new()),
                ..service_account()
            },
        ),
        (
            "project_id",
            Document {
                project_id: None,
                ..service_account()
            },
        ),
    ];
    for (field, doc) in user_cases.into_iter().chain(robot_cases) {
        let refused = Credential::read_document(doc, &at(), pinned());
        match refused {
            Err(UnusableCredential::Incomplete { field: found, .. }) => assert_eq!(found, field),
            other => panic!("an absent {field} was accepted: {other:?}"),
        }
    }
}

#[test]
fn a_private_key_is_refused_at_every_stage_a_signature_needs_and_quotes_nothing() {
    // **This test was `..._not_a_pkcs8_pem_block_...` and it proved less than its name.** The reader
    // stripped the delimiters and checked the body was not empty, which is a TEXT check: a block whose
    // body was `!!!` was accepted at boot and `ring` first saw it on the first question, so a
    // deployment with a corrupt key started, looked healthy, and failed the first thing anybody asked
    // it. Review caught that the comment beside it claimed the startup refusal it did not deliver.
    //
    // So the three stages are pinned by NAME, because the fix for each is a different thing: a missing
    // delimiter is a truncated file, a non-base64 body is a corrupted one, and a body that decodes and
    // is not a key is a key of the wrong kind.
    let cases: [(String, KeyUnusable); 7] = [
        (String::from("not a pem block at all"), KeyUnusable::NotAPemBlock),
        // A `PKCS#1` block, which is a different encoding and is refused rather than mis-parsed.
        (wrapped("RSA PRIVATE KEY", FAKE_BODY), KeyUnusable::NotAPemBlock),
        // Delimiters with nothing between them.
        (wrapped("PRIVATE KEY", ""), KeyUnusable::NotAPemBlock),
        // An opening delimiter and no closing one.
        (
            format!("{PEM_BEGIN_MARK}PRIVATE KEY-----\n{FAKE_BODY}"),
            KeyUnusable::NotAPemBlock,
        ),
        // **The case the previous version accepted.** Well-formed delimiters, a non-empty body, and
        // not base64.
        (wrapped("PRIVATE KEY", "!!!"), KeyUnusable::NotBase64),
        // Base64 that decodes to three bytes, which is not a `PKCS#8` document.
        (wrapped("PRIVATE KEY", FAKE_BODY), KeyUnusable::NotAKey),
        // Base64 that decodes to a plausible length and is still not a key - so the refusal is `ring`'s
        // structural check rather than a length heuristic of ours.
        (wrapped("PRIVATE KEY", &"QUJDRA".repeat(64)), KeyUnusable::NotAKey),
    ];
    for (hostile, expected) in cases {
        let doc = Document {
            private_key: Some(hostile.clone()),
            ..service_account()
        };
        let refused = Credential::read_document(doc, &at(), pinned()).expect_err("it refused");
        match refused {
            UnusableCredential::UnreadableKey { because, .. } => {
                assert_eq!(because, expected, "{hostile:?} was refused at the wrong stage");
            }
            ref other => panic!("{hostile:?} was mapped to {other:?}"),
        }
        // **The whole value is key material**, so there is no half of it a message may carry - which is
        // why the refusal names the path and a stage, and never the value.
        let shown = refused.to_string();
        assert!(!shown.contains(FAKE_BODY), "the refusal quoted key material: {shown}");
        assert!(!shown.contains("!!!"), "the refusal quoted key material: {shown}");
    }

    // And the stages are reachable directly, so the ordering is pinned rather than inferred from which
    // refusal a document happened to produce.
    assert_eq!(readable_key("nothing").expect_err("no delimiters"), KeyUnusable::NotAPemBlock);
    assert_eq!(
        readable_key(&wrapped("PRIVATE KEY", "!!!")).expect_err("not base64"),
        KeyUnusable::NotBase64
    );
    assert_eq!(
        readable_key(&wrapped("PRIVATE KEY", FAKE_BODY)).expect_err("not a key"),
        KeyUnusable::NotAKey
    );
}

#[test]
fn a_key_never_shows_itself_through_a_derived_debug() {
    // `Secret` is the mechanism and this is the assertion that it survives being wrapped twice - the
    // key sits inside a private enum inside a public struct, and a derived `Debug` on either would
    // print it.
    let robot = robot();
    let shown = format!("{robot:?}");
    assert!(!shown.contains(FAKE_BODY), "a derived Debug printed the key: {shown}");
    assert!(shown.contains("REDACTED"), "the key was not redacted at all: {shown}");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "reading the unwrapped body IS the assertion that unwrapping strips delimiters and nothing more"
)]
fn the_pem_unwrapping_is_a_text_format_and_nothing_more() {
    // What this function is allowed to do, pinned: strip two delimiters and pack the body. It does not
    // decode, it does not parse ASN.1, and it does not validate - `ring` does that, one layer down.
    let unwrapped =
        unwrap_pem("-----BEGIN PRIVATE KEY-----\nAAAB\nBBBC\n-----END PRIVATE KEY-----\n").expect("a well-formed block unwraps");
    assert_eq!(unwrapped.expose_secret(), "AAABBBBC");
    assert!(unwrap_pem("AAAB").is_none(), "a bare body is not a PEM block");
}

#[test]
fn a_credential_minted_for_another_service_universe_is_refused() {
    // The endpoints this crate reaches are compile-time constants in the default universe, so a
    // credential for another one would be presented to a service it was not issued for.
    let doc = Document {
        universe_domain: Some(String::from("example.test")),
        ..authorized_user()
    };
    let refused = Credential::read_document(doc, &at(), pinned());
    assert!(
        matches!(refused, Err(UnusableCredential::AnotherUniverse { .. })),
        "{refused:?}"
    );

    // And the default universe written out is accepted, so the check is not "any value is wrong".
    let doc = Document {
        universe_domain: Some(String::from("googleapis.com")),
        ..authorized_user()
    };
    Credential::read_document(doc, &at(), pinned()).expect("the default universe is accepted");
}

#[test]
fn a_file_that_is_not_there_is_refused_naming_the_path_and_keeping_the_cause() {
    let refused = Credential::read(&CredentialFile::at(at()), pinned()).expect_err("it refused");
    assert!(matches!(refused, UnusableCredential::Unreadable { .. }), "{refused:?}");
    assert!(
        core::error::Error::source(&refused).is_some(),
        "the io cause did not survive #[source]"
    );
}

#[test]
fn a_refusal_names_the_file_and_never_its_contents() {
    // Every value in this file is either a secret or a project identifier, so the path is the only
    // thing a message may carry - the same rule `ProjectId`'s own refusal follows one module over.
    let doc = Document {
        client_secret: Some(String::from("do-not-print-me")),
        refresh_token: None,
        ..authorized_user()
    };
    let refused = Credential::read_document(doc, &at(), pinned()).expect_err("it refused");
    let shown = refused.to_string();
    assert!(shown.contains("credentials.json"), "{shown}");
    assert!(!shown.contains("do-not-print-me"), "the refusal quoted a secret: {shown}");
}

#[test]
fn an_absent_lifetime_is_no_deadline_rather_than_an_invented_one() {
    // **The honest reading**, and it costs nothing because nothing is cached: there is no window in
    // which the difference between "no deadline stated" and "eternal" could be acted on. What refuses
    // an expired token is the data system.
    assert_eq!(deadline(None, 1_000), Expiry::NothingExpires);
    assert_eq!(deadline(Some(3_600), 1_000), Expiry::At { unix_seconds: 4_600 });
    // Saturating rather than wrapping, so a provider stating an absurd lifetime cannot produce a
    // deadline in the past.
    assert_eq!(deadline(Some(u64::MAX), 1_000), Expiry::At { unix_seconds: u64::MAX });
}

#[test]
fn the_file_location_is_a_claim_and_not_a_path_argument() {
    // `at` says THIS file and `well_known` says wherever this machine keeps it, and a function taking
    // a `PathBuf` could not have told a reader which it was handed. `well_known` itself is untestable
    // here - see the module header.
    let named = CredentialFile::at("/tmp/somewhere/creds.json");
    assert_eq!(named.path(), std::path::Path::new("/tmp/somewhere/creds.json"));
}
