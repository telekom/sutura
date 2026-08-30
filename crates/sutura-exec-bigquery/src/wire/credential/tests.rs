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
    AccessTokens as _, Credential, CredentialFile, Document, PEM_BEGIN_MARK, PEM_END_MARK, QuotaProject, UnusableCredential,
    deadline, unwrap_pem,
};
use crate::wire::{BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};
use sutura_domain::identity::Expiry;

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

/// A complete `service_account`, with a PEM block that is well-formed TEXT and not a key.
///
/// **The distinction matters for what these tests can claim:** the reader unwraps the PEM at
/// construction and `ring` parses the DER only when an assertion is signed, so a document with a
/// syntactically valid but meaningless body READS successfully here. Signing it would fail, and no
/// test here signs - that is what the acceptance leg does, with a real key.
fn service_account() -> Document {
    Document {
        client_email: Some(String::from("a-robot@example.invalid")),
        private_key_id: Some(String::from("0123456789abcdef")),
        private_key: Some(wrapped("PRIVATE KEY", FAKE_BODY)),
        project_id: Some(String::from("a-payer")),
        ..document("service_account")
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

    let robot = Credential::read_document(service_account(), &at(), pinned()).expect("a complete service account reads");
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

    let robot = Credential::read_document(service_account(), &at(), pinned()).expect("a service account reads");
    assert_eq!(robot.quota_project(), QuotaProject::FromTheCredential);
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
fn a_private_key_that_is_not_a_pkcs8_pem_block_is_refused_and_quotes_nothing() {
    // **The whole value is key material**, so there is no half of it a message may carry - which is why
    // this refusal names only the path. `PKCS#8` specifically: that is what the endpoint issues and
    // what `ring` reads directly, which is the reason no ASN.1 conversion exists anywhere here.
    for hostile in [
        String::from("not a pem block at all"),
        // A `PKCS#1` block, which is a different encoding and is refused rather than mis-parsed.
        wrapped("RSA PRIVATE KEY", FAKE_BODY),
        // Delimiters with nothing between them.
        wrapped("PRIVATE KEY", ""),
        // An opening delimiter and no closing one.
        format!("{PEM_BEGIN_MARK}PRIVATE KEY-----\n{FAKE_BODY}"),
    ] {
        let doc = Document {
            private_key: Some(hostile.clone()),
            ..service_account()
        };
        let refused = Credential::read_document(doc, &at(), pinned()).expect_err("it refused");
        assert!(
            matches!(refused, UnusableCredential::UnreadableKey { .. }),
            "{hostile:?} was accepted: {refused:?}"
        );
        let shown = refused.to_string();
        assert!(!shown.contains(FAKE_BODY), "the refusal quoted key material: {shown}");
    }
}

#[test]
fn a_key_never_shows_itself_through_a_derived_debug() {
    // `Secret` is the mechanism and this is the assertion that it survives being wrapped twice - the
    // key sits inside a private enum inside a public struct, and a derived `Debug` on either would
    // print it.
    let robot = Credential::read_document(service_account(), &at(), pinned()).expect("a service account reads");
    let shown = format!("{robot:?}");
    assert!(!shown.contains(FAKE_BODY), "a derived Debug printed the key: {shown}");
    assert!(shown.contains("REDACTED"), "the key was not redacted at all: {shown}");
}

#[test]
fn the_pem_unwrapping_is_a_text_format_and_nothing_more() {
    // What this function is allowed to do, pinned: strip two delimiters and pack the body. It does not
    // decode, it does not parse ASN.1, and it does not validate - `ring` does that, one layer down.
    let unwrapped =
        unwrap_pem("-----BEGIN PRIVATE KEY-----\nAAAB\nBBBC\n-----END PRIVATE KEY-----\n").expect("a well-formed block unwraps");
    assert_eq!(unwrapped.expose(), "AAABBBBC");
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
