//! What the inbound declaration accepts, and what it will not.
//!
//! Every test here is about a value that is compared against something a caller sent, so each one
//! names the failure it prevents rather than the rule it checks.

use super::{
    InboundIdentity, InvalidAlgorithms, InvalidInboundValue, IssuerUrl, KeyFamily, KeySetFile, PinnedAlgorithms, ProofHeader,
    ProofLifetime, RequiredTokenType, ResourceIdentifier, SigningAlgorithm, TokenLocation, TokenType, TransitProof,
};

fn resource() -> ResourceIdentifier {
    ResourceIdentifier::parse("https://sutura.example.com").expect("a test resource is a resource")
}

fn issuer() -> IssuerUrl {
    IssuerUrl::parse("https://issuer.example.com").expect("a test issuer is an issuer")
}

fn key_set() -> KeySetFile {
    KeySetFile::parse("/etc/sutura/jwks.json").expect("a test path is a path")
}

fn rs256() -> PinnedAlgorithms {
    PinnedAlgorithms::of(SigningAlgorithm::Rs256)
}

fn direct() -> InboundIdentity {
    InboundIdentity::Direct {
        resource: resource(),
        authorization_server: issuer(),
        key_set: key_set(),
        algorithms: rs256(),
        token_type: RequiredTokenType::access_token(),
    }
}

fn behind_gateway() -> InboundIdentity {
    InboundIdentity::BehindGateway {
        transit: TransitProof::new(
            ProofHeader::parse("x-transit-proof").expect("a test header is a header"),
            IssuerUrl::parse_transit_issuer("https://gateway.example.com").expect("a test issuer is an issuer"),
            ResourceIdentifier::parse_transit_audience("https://sutura.example.com").expect("a test audience is one"),
            key_set(),
            PinnedAlgorithms::of(SigningAlgorithm::Es256),
            RequiredTokenType::Any,
            ProofLifetime::default_lifetime(),
        ),
    }
}

#[test]
fn a_symmetric_algorithm_is_refused_by_name_and_the_reason_is_in_the_message() {
    // THE algorithm-confusion refusal. A validator that accepts `HS256` accepts a token the caller
    // signed with the issuer's own public key as an HMAC secret, and the defect is silent when it
    // works - so the refusal has to explain rather than say "unknown".
    let error = SigningAlgorithm::parse("HS256").expect_err("a symmetric algorithm is not one of these");
    assert_eq!(
        error,
        InvalidAlgorithms::Symmetric {
            found: String::from("HS256")
        }
    );
    let rendered = error.to_string();
    assert!(rendered.contains("PUBLIC key"), "{rendered}");
    // And the whole family, not just the one an operator is most likely to type.
    for symmetric in ["HS384", "HS512", "hs256"] {
        assert!(
            matches!(SigningAlgorithm::parse(symmetric), Err(InvalidAlgorithms::Symmetric { .. })),
            "{symmetric} should be refused as symmetric"
        );
    }
}

#[test]
fn none_is_refused_before_the_unknown_name_refusal_can_see_it() {
    // `none` IS in the JWS registry, so an "unknown algorithm" message would be both wrong and
    // unhelpful. The ordering inside `parse` is what makes the diagnostic the accurate one.
    let error = SigningAlgorithm::parse("none").expect_err("`none` is the absence of a signature");
    assert_eq!(error, InvalidAlgorithms::Unsigned);
    assert!(error.to_string().contains("anybody can write"), "{error}");
    assert_eq!(SigningAlgorithm::parse("NONE"), Err(InvalidAlgorithms::Unsigned));
}

#[test]
fn the_accepted_algorithms_round_trip_and_carry_their_key_family() {
    // The positive side, without which every refusal above is satisfied by refusing everything. The
    // family is asserted per algorithm because it is what the mixed-list refusal below is derived
    // from, and a wrong family there would make that refusal fire on a correct configuration.
    for name in SigningAlgorithm::NAMES {
        let parsed = SigningAlgorithm::parse(name).expect("a listed name parses");
        assert_eq!(parsed.as_str(), *name);
    }
    assert_eq!(SigningAlgorithm::Rs256.family(), KeyFamily::Rsa);
    assert_eq!(SigningAlgorithm::Ps512.family(), KeyFamily::Rsa);
    assert_eq!(SigningAlgorithm::Es384.family(), KeyFamily::EllipticCurve);
    assert_eq!(SigningAlgorithm::EdDsa.family(), KeyFamily::EdwardsCurve);
    assert_eq!(SigningAlgorithm::parse("rs256"), Ok(SigningAlgorithm::Rs256));
}

#[test]
fn a_pinned_list_spanning_two_key_families_is_refused_at_startup() {
    // The finding that made this refusal exist: one token is verified by one key, and the validator
    // refuses a permitted-algorithm list whose family disagrees with the key it looked up. So a list
    // naming both an RSA and an elliptic-curve algorithm is a deployment that starts and
    // authenticates nobody - which is the failure shape this crate refuses everywhere else.
    let error =
        PinnedAlgorithms::parse(&[String::from("RS256"), String::from("ES256")]).expect_err("two families verify nothing");
    assert_eq!(
        error,
        InvalidAlgorithms::MixedFamilies {
            first: "RSA",
            first_named: "RS256",
            second: "elliptic curve",
            second_named: "ES256",
        }
    );
    // Two of the SAME family is fine, which is what makes the refusal about families rather than
    // about the length of the list: rotating from RS256 to RS512 needs both permitted at once.
    let both = PinnedAlgorithms::parse(&[String::from("RS256"), String::from("RS512")]).expect("one family, two algorithms");
    assert_eq!(both.count(), 2);
    assert_eq!(both.family(), KeyFamily::Rsa);
    assert_eq!(both.to_string(), "RS256, RS512");
}

#[test]
fn an_empty_pinned_list_is_refused_rather_than_defaulted() {
    // A default here would be this crate deciding what an issuer signs with. Pinning is the control
    // and the deployment is what states it.
    assert_eq!(PinnedAlgorithms::parse(&[]), Err(InvalidAlgorithms::Empty));
    // And the type has no empty state, so nothing downstream checks: one algorithm is the floor.
    assert_eq!(rs256().count(), 1);
    assert_eq!(
        rs256().iter().collect::<Vec<SigningAlgorithm>>(),
        vec![SigningAlgorithm::Rs256]
    );
}

#[test]
fn a_resource_identifier_is_stored_exactly_as_written_and_nothing_is_normalised() {
    // URL parsing validates only the structure. An audience is compared byte for byte against a token
    // claim, so storing a parser's lower-cased host or elided default port would make this deployment
    // accept a token minted for a DIFFERENT spelling than the operator wrote.
    let written = "https://Sutura.Example.com:8443/v1";
    let parsed = ResourceIdentifier::parse(written).expect("a mixed-case identifier is an identifier");
    assert_eq!(parsed.as_str(), written, "the value was normalised");
    // Surrounding whitespace is the one thing removed, because a YAML file adds it and no issuer
    // ever put it in a claim.
    assert_eq!(
        ResourceIdentifier::parse(format!("  {written}  ")).expect("trimmed").as_str(),
        written
    );
}

#[test]
fn a_cleartext_issuer_and_a_query_or_fragment_are_both_refused_and_say_which() {
    // `http://` is an issuer whose signing keys an active network attacker chooses.
    assert_eq!(
        IssuerUrl::parse("http://issuer.example.com").expect_err("cleartext is not an issuer"),
        InvalidInboundValue::NotHttps { key: IssuerUrl::KEY }
    );
    assert_eq!(
        ResourceIdentifier::parse("sutura.example.com").expect_err("a bare host is not an identifier"),
        InvalidInboundValue::NotHttps {
            key: ResourceIdentifier::KEY
        }
    );
    // A query or a fragment is refused because an authorization server may mint the audience
    // WITHOUT it, so the byte-exact comparison would fail against a correct token.
    assert_eq!(
        ResourceIdentifier::parse("https://sutura.example.com/#main").expect_err("a fragment is not part of an audience"),
        InvalidInboundValue::HasDelimiter {
            key: ResourceIdentifier::KEY,
            delimiter: '#',
            position: 27,
        }
    );
    assert!(matches!(
        ResourceIdentifier::parse("https://sutura.example.com/?v=1"),
        Err(InvalidInboundValue::HasDelimiter { delimiter: '?', .. })
    ));
}

#[test]
fn an_invisible_or_homoglyph_character_cannot_reach_an_identifier() {
    // There is no separate invisible-character check, and this is why there does not need to be one:
    // the accepted set is ASCII, so every invisible code point, every direction-changing one and
    // every Cyrillic homoglyph is outside it. Two identifiers a reader cannot tell apart is exactly
    // the confusion an audience check exists to prevent.
    let error = ResourceIdentifier::parse("https://sutura.example.com\u{200B}")
        .expect_err("a zero-width space is not part of an identifier");
    assert_eq!(
        error,
        InvalidInboundValue::NotPermitted {
            key: ResourceIdentifier::KEY,
            position: 26,
            accepted: "letters, digits, and any of - . _ ~ : / [ ] @ ! $ & ' ( ) * + , ; = %",
        }
    );
    // The character is NOT in the message, because one whose only defect is that it draws nothing
    // would print as though the value were correct.
    let rendered = error.to_string();
    assert!(!rendered.contains('\u{200B}'), "the invisible character reached the message");
    // A Cyrillic `е` in place of the Latin one.
    assert!(matches!(
        ResourceIdentifier::parse("https://sutura.\u{435}xample.com"),
        Err(InvalidInboundValue::NotPermitted { .. })
    ));
    // And an empty value is its own refusal: empty is not absent.
    assert_eq!(
        ResourceIdentifier::parse("   ").expect_err("whitespace is not an identifier"),
        InvalidInboundValue::Empty {
            key: ResourceIdentifier::KEY
        }
    );
}

#[test]
fn a_transit_proof_cannot_be_configured_to_arrive_in_the_deployment_tokens_header() {
    // A request cannot present two different credentials in one header, and a proof read out of
    // `Authorization` would be read out of the header the deployment bearer token already owns.
    let error = ProofHeader::parse("Authorization").expect_err("that header is taken");
    assert_eq!(
        error,
        InvalidInboundValue::ReservedHeader {
            key: ProofHeader::KEY,
            found: String::from("authorization"),
        }
    );
    // Case is folded BEFORE the check, which is what makes the refusal reachable from the spelling an
    // operator would actually write - and what makes a configured `X-Transit` match an arriving
    // `x-transit` rather than missing an entry that is present.
    assert_eq!(
        ProofHeader::parse("  X-Transit-Proof  ")
            .expect("a mixed-case header is a header")
            .as_str(),
        "x-transit-proof"
    );
    assert!(matches!(
        ProofHeader::parse("x transit"),
        Err(InvalidInboundValue::NotPermitted { position: 1, .. })
    ));
    assert_eq!(
        ProofHeader::parse("").expect_err("empty is not a header name"),
        InvalidInboundValue::Empty { key: ProofHeader::KEY }
    );
}

#[test]
fn each_mode_derives_one_requirement_and_they_differ_only_in_where_the_token_is() {
    // `docs/adr/0014`: the modes are one fact rather than two code paths. The assertion is that both
    // produce the same SHAPE, so there is one validator - and that the one thing that differs is the
    // header, because an OAuth client has no option but `Authorization` and a component setting its
    // own header would collide with the deployment token there.
    let direct = direct();
    let requirement = direct.requirement();
    assert_eq!(requirement.location(), TokenLocation::AuthorizationBearer);
    assert_eq!(requirement.issuer(), &issuer());
    assert_eq!(requirement.audience(), &resource(), "the audience IS the resource identifier");
    assert_eq!(requirement.algorithms().family(), KeyFamily::Rsa);
    assert_eq!(direct.mode(), "direct");
    assert!(direct.reads_the_authorization_header());

    let gateway = behind_gateway();
    let requirement = gateway.requirement();
    let TokenLocation::Header { name } = requirement.location() else {
        panic!("a transit proof arrives in its own header, not in Authorization");
    };
    assert_eq!(name.as_str(), "x-transit-proof");
    assert_eq!(requirement.algorithms().family(), KeyFamily::EllipticCurve);
    assert_eq!(gateway.mode(), "behind-gateway");
    assert!(
        !gateway.reads_the_authorization_header(),
        "a gateway deployment leaves Authorization to the deployment token"
    );
    assert_eq!(requirement.key_set().path(), key_set().path());
}

#[test]
fn what_each_mode_says_at_startup_is_read_from_the_type_and_never_claims_leg_two() {
    // The line an operator reads on every boot. It is a function rather than a comment so the log,
    // the documentation and this test read one string - which is the mechanism that keeps a
    // deployment from believing it has per-user access because it has authentication.
    assert!(direct().who_authenticated().contains("resource server"));
    assert!(behind_gateway().who_authenticated().contains("fronting component"));
    let limit = InboundIdentity::what_it_does_not_do();
    assert!(limit.contains("leg 2"), "{limit}");
    assert!(limit.contains("does not"), "{limit}");
    // The same sentence for both modes, deliberately: the mode changes who authenticates the caller
    // and changes nothing about leg 2.
    for mode in [direct(), behind_gateway()] {
        assert!(!mode.mode().is_empty(), "each inbound mode names an authenticator");
        assert!(InboundIdentity::MODES.contains(&mode.mode()), "{}", mode.mode());
    }
}

#[test]
fn the_three_spellings_of_one_media_type_compare_equal_and_nothing_else_does() {
    // RFC 7515 section 4.1.9 says `typ` is a media type and that the `application/` prefix may be
    // omitted, so a comparison that treated these as three values would refuse tokens that are
    // correct. Folded at construction, which is why the comparison downstream needs no normalising.
    let required = RequiredTokenType::access_token();
    for spelling in ["at+jwt", "AT+JWT", "application/at+jwt", "Application/AT+JWT", "  at+jwt  "] {
        let presented = TokenType::parse(TokenType::KEY, spelling).expect("a spelling of a media type is one");
        assert!(required.accepts(Some(&presented)), "{spelling} should satisfy at+jwt");
    }
    // And what must NOT satisfy it: the class an OIDC ID token carries, and no `typ` at all.
    let id_token = TokenType::parse(TokenType::KEY, "JWT").expect("`JWT` is a media type");
    assert!(
        !required.accepts(Some(&id_token)),
        "a plain `JWT` is the class an ID token carries and is not an access token"
    );
    assert!(
        !required.accepts(None),
        "a token with no `typ` satisfies nothing but `any` - the check must not be satisfiable by omission"
    );
}

#[test]
fn turning_the_class_check_off_is_a_word_and_never_a_silence() {
    // The half that makes the default safe: `any` has to be written, and a deployment running with it
    // is legible on every boot because the sentence the log prints comes off the type.
    let off = RequiredTokenType::parse(TokenType::KEY, "any").expect("`any` is a word");
    assert_eq!(off, RequiredTokenType::Any);
    assert_eq!(RequiredTokenType::parse(TokenType::KEY, "ANY"), Ok(RequiredTokenType::Any));
    assert!(off.accepts(None), "`any` accepts a token with no typ at all");
    assert_eq!(off.as_str(), "any");

    let gateway = InboundIdentity::BehindGateway {
        transit: TransitProof::new(
            ProofHeader::parse("x-transit-proof").expect("a test header is a header"),
            IssuerUrl::parse_transit_issuer("https://gateway.example.com").expect("a test issuer is an issuer"),
            ResourceIdentifier::parse_transit_audience("https://sutura.example.com").expect("a test audience is one"),
            key_set(),
            PinnedAlgorithms::of(SigningAlgorithm::Es256),
            RequiredTokenType::Any,
            ProofLifetime::default_lifetime(),
        ),
    };
    assert!(gateway.accepts_any_token_class());
    assert!(gateway.type_check().contains("ANY class"), "{}", gateway.type_check());
    // And the default: the sentence names the substitution it prevents rather than the rule it applies.
    assert!(!direct().accepts_any_token_class());
    assert!(direct().type_check().contains("ID token"), "{}", direct().type_check());
}

#[test]
fn a_typ_that_could_not_be_a_media_type_does_not_parse() {
    // A `typ` is read off a token header as well as out of configuration, and the same parser reads
    // both - so this bounds the caller-supplied side too.
    assert!(matches!(
        TokenType::parse(TokenType::KEY, "at jwt"),
        Err(InvalidInboundValue::NotPermitted { position: 2, .. })
    ));
    assert!(matches!(
        TokenType::parse(TokenType::KEY, "at\u{202E}jwt"),
        Err(InvalidInboundValue::NotPermitted { .. })
    ));
    assert!(matches!(
        TokenType::parse(TokenType::KEY, "application/"),
        Err(InvalidInboundValue::Empty { .. })
    ));
    assert!(matches!(
        TokenType::parse(TokenType::KEY, "a".repeat(513)),
        Err(InvalidInboundValue::TooLong { .. })
    ));
}

#[test]
fn a_transit_lifetime_ceiling_is_bounded_at_both_ends_and_only_the_gateway_mode_has_one() {
    // The ceiling exists because the record calls a gateway assertion short-lived while its lifetime is
    // the component's to choose. A zero would refuse every assertion, and a year is not a ceiling.
    assert_eq!(ProofLifetime::default_lifetime().seconds(), ProofLifetime::DEFAULT_SECONDS);
    assert_eq!(ProofLifetime::parse(1).expect("one second is a lifetime").seconds(), 1);
    assert_eq!(
        ProofLifetime::parse(ProofLifetime::MAX_SECONDS)
            .expect("the ceiling itself is a lifetime")
            .seconds(),
        ProofLifetime::MAX_SECONDS
    );
    assert_eq!(
        ProofLifetime::parse(0).expect_err("a zero refuses every proof"),
        InvalidInboundValue::LifetimeOutOfRange {
            found: 0,
            limit: ProofLifetime::MAX_SECONDS
        }
    );
    assert!(matches!(
        ProofLifetime::parse(ProofLifetime::MAX_SECONDS + 1),
        Err(InvalidInboundValue::LifetimeOutOfRange { .. })
    ));

    // Only the gateway mode carries one, and that asymmetry is the decision: an access token's
    // lifetime belongs to the authorization server, so a ceiling in the direct mode would refuse
    // tokens an issuer minted correctly.
    assert_eq!(
        behind_gateway().requirement().max_lifetime(),
        Some(ProofLifetime::default_lifetime())
    );
    assert_eq!(
        direct().requirement().max_lifetime(),
        None,
        "an access token's lifetime is its issuer's to choose"
    );
}
