//! The `security` group's declaration and posture tests.
//!
use crate::identity_cache::CredentialCacheSettings;
use crate::inbound::{IssuerUrl, KeySetFile, PinnedAlgorithms, ResourceIdentifier, SigningAlgorithm};

use super::{
    AccessToken, DeploymentIdentity, InboundIdentity, InvalidAccessToken, InvalidDeploymentIdentity, InvalidOutbound,
    OutboundAnchors, SecuritySettings, TlsTermination, parse_outbound,
};

/// Thirty-two characters, which is the floor.
const GOOD: &str = "0123456789abcdef0123456789abcdef";

/// A deployment that is its own resource server. The narrowest declaration that exists.
fn direct() -> InboundIdentity {
    InboundIdentity::Direct {
        resource: ResourceIdentifier::parse("https://sutura.example.com").expect("a test resource is a resource"),
        authorization_server: IssuerUrl::parse("https://issuer.example.com").expect("a test issuer is an issuer"),
        key_set: KeySetFile::parse("/etc/sutura/jwks.json").expect("a test path is a path"),
        algorithms: PinnedAlgorithms::of(SigningAlgorithm::Rs256),
        token_type: crate::inbound::RequiredTokenType::access_token(),
    }
}

#[test]
fn a_short_token_is_refused_and_the_value_is_not_in_the_message() {
    let error = AccessToken::parse("hunter2").expect_err("seven characters is not a token");
    assert_eq!(
        error,
        InvalidAccessToken::TooShort {
            found: 7,
            minimum: AccessToken::MIN_LENGTH
        }
    );
    // The half that matters: an error about a credential must not carry the credential.
    let rendered = error.to_string();
    assert!(!rendered.contains("hunter2"), "{rendered}");
}

#[test]
fn the_minimum_length_is_accepted_and_one_character_less_is_not() {
    assert!(
        AccessToken::parse(GOOD)
            .expect("the floor is a token")
            .matches_in_constant_time(GOOD)
    );
    let short: String = GOOD.chars().take(AccessToken::MIN_LENGTH.saturating_sub(1)).collect();
    assert_eq!(
        AccessToken::parse(short).expect_err("one character short is not a token"),
        InvalidAccessToken::TooShort {
            found: AccessToken::MIN_LENGTH.saturating_sub(1),
            minimum: AccessToken::MIN_LENGTH
        }
    );
}

#[test]
fn a_token_with_surrounding_whitespace_is_refused_rather_than_trimmed() {
    // Trimming would be friendlier and worse: the operator and the service would then hold
    // different strings, and every request would fail with nothing in the log to explain it.
    //
    // Written as `expect_err` rather than `assert_eq!` on the whole `Result`, and that is not
    // a style choice: `AccessToken` has no `PartialEq`, so comparing two
    // `Result<AccessToken, _>` values does not compile. The awkwardness is the invariant.
    assert_eq!(
        AccessToken::parse(format!("{GOOD} ")).expect_err("a trailing space is not a token"),
        InvalidAccessToken::Untrimmed
    );
    assert_eq!(
        AccessToken::parse(format!("\n{GOOD}")).expect_err("a leading newline is not a token"),
        InvalidAccessToken::Untrimmed
    );
}

#[test]
fn a_token_is_not_printed_by_debug_at_any_depth() {
    // The token is stored only as a digest, and `Debug` prints a placeholder anyway. The
    // startup log prints the whole settings tree with `Debug`, so this is the assertion that
    // keeps that safe.
    let settings = SecuritySettings::new(
        Some(AccessToken::parse(GOOD).expect("a valid token")),
        TlsTermination::None,
        None,
        Some(DeploymentIdentity::SubjectPerRequest),
        None,
        CredentialCacheSettings::default(),
        None,
    );
    let rendered = format!("{settings:?}");
    assert!(!rendered.contains(GOOD), "{rendered}");
    assert!(rendered.contains("REDACTED"), "{rendered}");
}

#[test]
fn the_right_token_matches_and_the_wrong_one_does_not() {
    let token = AccessToken::parse(GOOD).expect("a valid token");
    assert!(token.matches_in_constant_time(GOOD));
    assert!(!token.matches_in_constant_time("0123456789abcdef0123456789abcdeF"));
    assert!(!token.matches_in_constant_time(""));
    // A prefix of the real token must not match. Hashing both sides is what makes the
    // length difference irrelevant to the comparison rather than something it branches on.
    assert!(!token.matches_in_constant_time("0123456789abcdef"));
    // Nor a superstring of it.
    assert!(!token.matches_in_constant_time(&format!("{GOOD}x")));
}

#[test]
fn an_overlong_token_is_refused_rather_than_accepted() {
    // The ceiling is the far side of the floor: a bearer token an operator generates has no
    // reason to reach it, and an unbounded configured string is an availability surface.
    let at_limit = "a".repeat(AccessToken::MAX_LENGTH);
    assert!(
        AccessToken::parse(&at_limit)
            .expect("at the ceiling is a token")
            .matches_in_constant_time(&at_limit),
        "the ceiling itself is a token"
    );
    let over = "a".repeat(AccessToken::MAX_LENGTH + 1);
    assert_eq!(
        AccessToken::parse(over).expect_err("one over the ceiling is not a token"),
        InvalidAccessToken::TooLong {
            found: AccessToken::MAX_LENGTH + 1,
            limit: AccessToken::MAX_LENGTH
        }
    );
    // The value is never in the message, for the same reason `TooShort` carries a length.
    let rendered = AccessToken::parse("a".repeat(AccessToken::MAX_LENGTH + 1))
        .expect_err("one over the ceiling is refused")
        .to_string();
    assert!(!rendered.contains(&"a".repeat(AccessToken::MAX_LENGTH + 1)), "{rendered}");
}

#[test]
fn parse_retains_a_digest_and_not_the_configured_token() {
    use sha2::Digest as _;
    // The mechanism C8's hash-at-parse rests on: the configured token is reduced to its SHA-256
    // once, at parse, and the raw value is not retained. A revert that holds the raw value (or
    // re-hashes it per request) is caught here - the one stored thing is a digest of the token,
    // not the token itself.
    let token = AccessToken::parse(GOOD).expect("a valid token");
    let expected: [u8; 32] = sha2::Sha256::digest(GOOD.as_bytes()).into();
    assert_eq!(token.digest, expected);
    assert_ne!(&token.digest[..], GOOD.as_bytes(), "the digest is not the configured value");
}

#[test]
fn a_shared_token_does_not_claim_to_know_who_the_caller_is_and_an_inbound_declaration_does() {
    // The distinction the startup log prints, and the reason `describes_identity` stopped being a
    // constant. A token - in EVERY termination posture, which is why one is set here - proves the
    // caller holds a secret an operator distributed, and says nothing about which caller.
    let with_token = SecuritySettings::new(
        Some(AccessToken::parse(GOOD).expect("a valid token")),
        TlsTermination::Ingress,
        None,
        Some(DeploymentIdentity::SubjectPerRequest),
        None,
        CredentialCacheSettings::default(),
        None,
    );
    let without = SecuritySettings::default();
    assert!(!with_token.describes_identity(), "a shared token is not an identity");
    assert!(!without.describes_identity());
    assert_eq!(with_token.token_state(), "configured");
    assert_eq!(without.token_state(), "absent");
    assert_eq!(with_token.inbound_mode(), "none");

    // And the other half, which is what makes the assertions above load-bearing rather than a
    // tautology about a constant: a deployment that validates a caller's token DOES establish an
    // identity, and the same function says so.
    let verifying = SecuritySettings::new(
        None,
        TlsTermination::Ingress,
        Some(direct()),
        None,
        None,
        CredentialCacheSettings::default(),
        None,
    );
    assert!(verifying.describes_identity());
    assert_eq!(verifying.inbound_mode(), "direct");
    assert_eq!(verifying.token_state(), "absent");
    assert!(verifying.inbound().is_some());

    // And the third fact, which is the one the merge of leg 1 and the source registry made
    // available to assert: the two declarations are independent. A DECLARED multi-user mode
    // says what the deployment intends and decides where a shared source's acknowledgement
    // has to be written; it does not make a caller identity arrive, and this function does
    // not read it.
    assert_eq!(with_token.identity(), Some(&DeploymentIdentity::SubjectPerRequest));
    assert!(
        !with_token.describes_identity(),
        "a declared mode is not an established caller"
    );
    assert_eq!(verifying.identity(), None, "nor does establishing a caller declare a mode");
    assert_eq!(without.identity(), None, "the mode has no default");
}

#[test]
fn the_deployment_mode_is_declared_with_a_reason_or_not_at_all() {
    // Single-user needs the reason: it is the sentence a reviewer reads and the one a shared source
    // borrows as its acknowledgement, so a mode without it is a word rather than a declaration.
    assert_eq!(
        DeploymentIdentity::parse("single-user", None).expect_err("single-user needs a reason"),
        InvalidDeploymentIdentity::SingleUserWithoutAReason
    );
    assert_eq!(
        DeploymentIdentity::parse("single-user", Some("   ")).expect_err("whitespace is not a reason"),
        InvalidDeploymentIdentity::SingleUserWithoutAReason
    );
    let single = DeploymentIdentity::parse("single-user", Some("one operator, their own files"))
        .expect("a declared single-user mode parses");
    assert_eq!(single.as_str(), "single-user");
    assert!(
        single.shared_witness().is_some(),
        "single-user mode is where a shared source's witness comes from"
    );
    assert!(!single.needs_per_source_acknowledgement());
}

#[test]
fn the_multi_user_mode_refuses_the_reason_the_other_one_requires() {
    // Split from the test above by `cognitive_complexity`, and the split is along the seam the two
    // modes already have: one requires the reason and the other refuses it.
    //
    // Multi-user refuses it for the reason a certificate nothing reads is refused: a value nothing
    // reads is a control that appears to be in place.
    assert_eq!(
        DeploymentIdentity::parse("multi-user", Some("because")).expect_err("nothing would read it"),
        InvalidDeploymentIdentity::ReasonWithoutSingleUser
    );
    let multi = DeploymentIdentity::parse("multi-user", None).expect("multi-user needs nothing else");
    assert_eq!(multi, DeploymentIdentity::SubjectPerRequest);
    assert_eq!(multi.shared_witness(), None);
    assert!(multi.needs_per_source_acknowledgement());

    // And a third word is not a third mode.
    let unknown = DeploymentIdentity::parse("impersonating", None).expect_err("there are two modes");
    assert!(matches!(unknown, InvalidDeploymentIdentity::Unknown { .. }));
    let rendered = unknown.to_string();
    assert!(rendered.contains("security.identity"), "{rendered}");

    // Every listed spelling parses, so `NAMES` cannot offer a mode the parser refuses. The reason is
    // supplied for exactly the mode that requires one, which is what makes this a round trip rather
    // than a loop that only exercises one arm.
    for name in DeploymentIdentity::NAMES {
        let reason = (*name == "single-user").then_some("a stated reason");
        assert_eq!(
            DeploymentIdentity::parse(name, reason)
                .expect("a listed name parses")
                .as_str(),
            *name
        );
    }
}

#[test]
fn a_token_the_authorization_header_could_not_carry_is_refused_at_startup() {
    // THE bug this variant exists for. Thirty-two characters, so the length floor is satisfied,
    // and not one of them is representable in a header value - `HeaderValue::to_str` accepts
    // visible ASCII only. Without this the process starts, every request is a 401, and nothing
    // in the log connects the two.
    //
    // Written with an escape rather than the character itself because `clippy::non_ascii_literal`
    // is on: an invisible byte in a source literal is exactly what that lint is for.
    let unrepresentable = "\u{e9}".repeat(AccessToken::MIN_LENGTH);
    assert_eq!(unrepresentable.chars().count(), AccessToken::MIN_LENGTH);
    assert_eq!(
        AccessToken::parse(&unrepresentable).expect_err("a token no header can carry is not a token"),
        InvalidAccessToken::NotRepresentableOnTheWire { position: 0 }
    );
}

#[test]
fn a_control_character_inside_a_token_is_refused_and_the_position_is_named() {
    // The interior case, which the length and trim checks both pass: a newline in the middle of
    // a pasted token is a copy-paste artefact that no request could present either.
    let interior = String::from("0123456789abcdef\u{1}23456789abcdef0");
    assert_eq!(interior.chars().count(), AccessToken::MIN_LENGTH);
    assert_eq!(
        AccessToken::parse(&interior).expect_err("an interior control character is not a token"),
        InvalidAccessToken::NotRepresentableOnTheWire { position: 16 }
    );
    // And the value is not in the message, which is the rule every variant here obeys.
    let rendered = AccessToken::parse(&interior)
        .expect_err("an interior control character is not a token")
        .to_string();
    assert!(!rendered.contains(&interior), "{rendered}");
}

#[test]
fn the_b64token_alphabet_is_accepted_and_padding_is_only_a_suffix() {
    // The positive side, without which every assertion above is satisfied by refusing
    // everything. Base64 with either alphabet, and a hex token, are what an operator generates.
    for good in [
        "0123456789abcdef0123456789abcdef",
        "abcdefghijklmnopqrstuvwxyz-._~+/",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
    ] {
        assert!(AccessToken::parse(good).is_ok(), "{good} should be a token");
    }
    // Padding in the middle is not `b64token`, and a token with a `=` in it is one some
    // manifest or shell will split.
    let interior_padding = "AAAAAAAAAAAAAAAA=BBBBBBBBBBBBBBB";
    assert_eq!(
        AccessToken::parse(interior_padding).expect_err("interior padding is not a token"),
        InvalidAccessToken::NotRepresentableOnTheWire { position: 17 }
    );
    // A space is representable in a header and is refused anyway - see `wire_grammar`.
    let spaced = "0123456789abcdef 123456789abcdef";
    assert!(matches!(
        AccessToken::parse(spaced),
        Err(InvalidAccessToken::NotRepresentableOnTheWire { position: 16 })
    ));
}

#[test]
fn a_termination_declaration_round_trips_and_says_what_crosses_in_cleartext() {
    for name in TlsTermination::NAMES {
        let parsed = TlsTermination::parse(name).expect("a listed name parses");
        assert_eq!(parsed.as_str(), *name);
        // Every declaration says something about the hop, and only one of them says there is
        // none. That sentence is what the startup log prints, so it is asserted here rather
        // than trusted.
        assert!(!parsed.cleartext_hop().is_empty(), "each cleartext hop is named");
    }
    assert_eq!(TlsTermination::default(), TlsTermination::None);
    assert!(!TlsTermination::None.is_declared());
    assert!(TlsTermination::Ingress.is_declared());
    assert!(!TlsTermination::Ingress.terminates_here());
    assert!(TlsTermination::InProcess.terminates_here());
    assert!(TlsTermination::InProcess.cleartext_hop().contains("no cleartext hop"));
    assert!(TlsTermination::Ingress.cleartext_hop().contains("pod network"));
    TlsTermination::parse("tls").unwrap_err();
}

#[test]
fn outbound_system_and_a_bundle_path_both_parse() {
    assert_eq!(parse_outbound(Some("system")).unwrap(), OutboundAnchors::System);
    assert_eq!(
        parse_outbound(Some("/etc/sutura/outbound-ca.pem")).unwrap(),
        OutboundAnchors::Bundle(std::path::PathBuf::from("/etc/sutura/outbound-ca.pem"))
    );
}

#[test]
fn a_present_outbound_block_with_no_anchors_is_refused() {
    // The argument `security.inbound` with no `mode` already makes: a present block that names
    // nothing declares nothing, so absence inside a written block is a refusal and not the same
    // `None` an ABSENT block reads as (`SecuritySettings::outbound` returning `None`).
    assert_eq!(parse_outbound(None), Err(InvalidOutbound::NoAnchors));
    assert_eq!(parse_outbound(Some("")), Err(InvalidOutbound::NoAnchors));
    assert_eq!(parse_outbound(Some("   ")), Err(InvalidOutbound::NoAnchors));
}

#[test]
fn a_relative_outbound_bundle_path_is_refused() {
    assert_eq!(
        parse_outbound(Some("outbound-ca.pem")),
        Err(InvalidOutbound::RelativePath {
            path: std::path::PathBuf::from("outbound-ca.pem")
        })
    );
}

#[test]
fn security_settings_outbound_accessor_round_trips() {
    let none = SecuritySettings::default();
    assert!(none.outbound().is_none());
    let declared = SecuritySettings::new(
        None,
        TlsTermination::None,
        None,
        None,
        None,
        CredentialCacheSettings::default(),
        Some(OutboundAnchors::System),
    );
    assert_eq!(declared.outbound(), Some(&OutboundAnchors::System));
}
