//! A `direct` declaration carrying [`sutura_config::GatewayPairing`] is only ever usable through
//! [`InboundGate::from_verifiers_with_outbound`], which alone sees every declared verifier and can
//! resolve the named target - `github.com/telekom/sutura#2104`.
//!
//! [`InboundGate::from_declaration`] refuses a paired declaration outright
//! (`from_declaration_refuses_a_paired_direct_identity`), having no sibling list to resolve the
//! pairing against. `from_verifiers_with_outbound` resolves every `requires_gateway` over the FULL
//! declared list - standalone or itself a nested target - and refuses rather than silently dropping
//! a pairing it cannot place: an absent target, a declaration with no slot of its own once excluded
//! from `standalone`, or two declarations anywhere in the list sharing an issuer. That last check
//! runs over every declaration, not only the ones built standalone: `find_mut` resolves a
//! `requires_gateway` pairing by issuer string alone, so a nested target excluded from `standalone`
//! still has to be checked against every other declaration's issuer, or its own pairing could be
//! misattached to an unrelated declaration that happens to share it -
//! `from_verifiers_with_outbound_refuses_a_nested_target_sharing_its_issuer_with_an_unrelated_declaration`
//! pins exactly that shape refused.

use sutura_config::{InboundVerifier, McpPath};
use sutura_dev::issuer::MockIssuer;
use sutura_domain::identity::VerifierId;

use super::TRANSIT_HEADER;
use super::fixtures::{direct_over, paired_direct};
use crate::inbound::{InboundGate, InboundNotUsable};
use crate::testing::{ISSUER, RESOURCE};

/// `from_declaration`/`from_declaration_trusting` see only this one declaration, with no sibling
/// list to find the paired `behind-gateway` target in - refused rather than built with a
/// `requires_gateway` that would silently never attach.
#[test]
fn from_declaration_refuses_a_paired_direct_identity() {
    let identity = paired_direct("/unread/in/this/test.json", "x-user-token");
    let refused = InboundGate::from_declaration(&identity).expect_err("a paired declaration built alone is refused");
    assert!(
        matches!(refused, InboundNotUsable::PairedAlone),
        "expected PairedAlone, got {refused:?}"
    );
}

/// `from_verifiers_with_outbound` sees every declared verifier and can resolve a pairing - but a
/// `requires_gateway` naming an id absent from that list is refused rather than silently dropped,
/// which is what the `continue` this cell pins used to do.
#[test]
fn from_verifiers_with_outbound_refuses_a_pairing_naming_an_absent_target() {
    let issuer = MockIssuer::generating(ISSUER, RESOURCE, "staff-key").expect("a fixture issuer generates a key");
    let path = std::env::temp_dir().join(format!("sutura-pairing-resolution-staff-{}.json", std::process::id()));
    issuer.publish(&path).expect("the staff key set is writable");

    let id = VerifierId::parse("staff").expect("a fixture verifier id");
    let identity = paired_direct(&path.to_string_lossy(), "x-user-token");
    let declarations = vec![InboundVerifier::new(
        id.clone(),
        McpPath::parse("/staff").expect("a fixture path"),
        identity,
    )];

    let result = InboundGate::from_verifiers_with_outbound(&declarations, None);
    drop(std::fs::remove_file(&path));

    match result {
        Err(InboundNotUsable::RequiresGatewayTargetMissing { verifier, target }) => {
            assert_eq!(verifier, id);
            assert_eq!(target.as_str(), "gateway", "`paired_direct` always pairs with id `gateway`");
        }
        other => panic!("expected RequiresGatewayTargetMissing, got {other:?}"),
    }
}

/// `also_accept` keys a built gate by issuer and [`InboundGate`]'s own lookup finds a slot by
/// issuer too - a declaration assembled directly in Rust is not bound by
/// `sutura_config::NotFitToServe::DuplicateVerifierIssuer`, the admission check that already rules
/// a shared issuer out for a declaration parsed from settings, so the builder refuses it itself
/// rather than letting the later gate silently replace the earlier one's.
#[test]
fn from_verifiers_with_outbound_refuses_two_declarations_sharing_an_issuer() {
    let earlier_id = VerifierId::parse("earlier").expect("a fixture verifier id");
    let later_id = VerifierId::parse("later").expect("a fixture verifier id");
    let declarations = vec![
        InboundVerifier::new(
            earlier_id.clone(),
            McpPath::parse("/earlier").expect("a fixture path"),
            direct_over("/unread/in/this/test-earlier.json"),
        ),
        InboundVerifier::new(
            later_id.clone(),
            McpPath::parse("/later").expect("a fixture path"),
            direct_over("/unread/in/this/test-later.json"),
        ),
    ];

    match InboundGate::from_verifiers_with_outbound(&declarations, None) {
        Err(InboundNotUsable::DuplicateVerifierIssuer { earlier, later, issuer }) => {
            assert_eq!(earlier, earlier_id);
            assert_eq!(later, later_id);
            assert_eq!(issuer, ISSUER);
        }
        other => panic!("expected DuplicateVerifierIssuer, got {other:?}"),
    }
}

/// A `requires_gateway` target that is itself a paired `direct` verifier - rather than the
/// `behind-gateway` entry every other pairing targets - is excluded from `standalone` the moment
/// some other declaration names it, so it is never built into the gate or `also_accept` under its
/// own issuer. Resolving its OWN `requires_gateway` then finds no slot of its own to attach to.
///
/// `gateway` is given an issuer of its own, distinct from `target`'s (`ISSUER`, via
/// `paired_direct`): the duplicate-issuer check runs over every declaration before any of this is
/// built, and `gateway`/`target` sharing an issuer would be refused as `DuplicateVerifierIssuer`
/// before ever reaching the `RequiresGatewaySlotMissing` this cell pins. That shared-issuer shape
/// is its own cell -
/// `from_verifiers_with_outbound_refuses_a_nested_target_sharing_its_issuer_with_an_unrelated_declaration`.
#[test]
fn from_verifiers_with_outbound_refuses_a_paired_target_with_no_slot_of_its_own() {
    let gateway_id = VerifierId::parse("gateway").expect("a fixture verifier id");
    let target_id = VerifierId::parse("paired-target").expect("a fixture verifier id");
    let outer_id = VerifierId::parse("outer").expect("a fixture verifier id");

    // The builder reads every standalone declaration's key set before resolving any pairing. `gateway`
    // and `outer` are both built (see below), so both need a real, readable key set - `target`'s own
    // key set is never read in this scenario: it is excluded from `standalone`, and its own pairing
    // resolves against `gateway` by issuer and fails before `outer`'s pairing is ever looked at.
    let gateway_issuer =
        MockIssuer::generating("https://gateway.example.com", RESOURCE, "gateway-key").expect("a fixture issuer generates a key");
    let gateway_path = std::env::temp_dir().join(format!("sutura-pairing-resolution-gateway-{}.json", std::process::id()));
    gateway_issuer
        .publish(&gateway_path)
        .expect("the gateway key set is writable");

    let outer_issuer = MockIssuer::generating("https://outer.example.com", "https://outer.example.com/resource", "outer-key")
        .expect("a fixture issuer generates a key");
    let outer_path = std::env::temp_dir().join(format!("sutura-pairing-resolution-outer-{}.json", std::process::id()));
    outer_issuer.publish(&outer_path).expect("the outer key set is writable");

    let gateway = InboundVerifier::new(
        gateway_id,
        McpPath::parse("/gateway").expect("a fixture path"),
        sutura_config::InboundIdentity::BehindGateway {
            transit: sutura_config::TransitProof::new(
                sutura_config::ProofHeader::parse(TRANSIT_HEADER).expect("a fixture header"),
                sutura_config::IssuerUrl::parse_transit_issuer("https://gateway.example.com").expect("a fixture issuer"),
                sutura_config::TransitBinding::Audience(
                    sutura_config::ResourceIdentifier::parse_transit_audience(RESOURCE).expect("a fixture audience"),
                ),
                sutura_config::KeySetLocation::File(
                    sutura_config::KeySetFile::parse(gateway_path.to_string_lossy()).expect("a fixture path"),
                ),
                sutura_config::PinnedAlgorithms::of(sutura_config::SigningAlgorithm::Es256),
                sutura_config::RequiredTokenType::access_token(),
                sutura_config::ProofLifetime::default_lifetime(),
            ),
        },
    );
    // Paired with `gateway` above - exactly the shape `paired_direct` builds - but ALSO named as
    // `outer`'s own `requires_gateway` target below, which is what excludes it from `standalone`.
    let target = InboundVerifier::new(
        target_id.clone(),
        McpPath::parse("/paired-target").expect("a fixture path"),
        paired_direct("/unread/in/this/test-target.json", "x-target-token"),
    );
    let outer = InboundVerifier::new(
        outer_id,
        McpPath::parse("/outer").expect("a fixture path"),
        sutura_config::InboundIdentity::Direct {
            resource: sutura_config::ResourceIdentifier::parse("https://outer.example.com/resource").expect("a fixture resource"),
            authorization_server: sutura_config::IssuerUrl::parse("https://outer.example.com").expect("a fixture issuer"),
            key_set: sutura_config::KeySetLocation::File(
                sutura_config::KeySetFile::parse(outer_path.to_string_lossy()).expect("a fixture path"),
            ),
            algorithms: sutura_config::PinnedAlgorithms::of(sutura_config::SigningAlgorithm::Es256),
            token_type: sutura_config::RequiredTokenType::access_token(),
            address_claim: sutura_config::AddressClaim::VerifiedEmail,
            pairing: Some(sutura_config::GatewayPairing::new(
                sutura_config::ProofHeader::parse_named(sutura_config::ProofHeader::TOKEN_HEADER_KEY, "x-outer-token")
                    .expect("a fixture header"),
                target_id.clone(),
            )),
        },
    );

    let declarations = vec![gateway, target, outer];
    let result = InboundGate::from_verifiers_with_outbound(&declarations, None);
    drop(std::fs::remove_file(&gateway_path));
    drop(std::fs::remove_file(&outer_path));

    match result {
        Err(InboundNotUsable::RequiresGatewaySlotMissing { verifier, issuer }) => {
            assert_eq!(verifier, target_id);
            assert_eq!(
                issuer, ISSUER,
                "`paired_direct` always reads the caller's own issuer from `ISSUER`"
            );
        }
        other => panic!("expected RequiresGatewaySlotMissing, got {other:?}"),
    }
}

/// A pairing chain nested two deep - `outer` paired with `target`, `target` itself paired with
/// `gateway` - and an unrelated, unpaired declaration (`unrelated`) that happens to share
/// `target`'s issuer. `target` is excluded from `standalone` (named by `outer`'s own
/// `requires_gateway`), so the OLD duplicate-issuer check - scoped to `standalone` alone - never
/// compared it against `unrelated`: `target`'s own pairing then resolved `find_mut` by issuer
/// string and landed on `unrelated`'s slot, attaching `target`'s `required_gateway` to a
/// declaration that never asked to be paired with anything. The check now runs over every
/// declaration, standalone or not, so the shared issuer is refused before any of that - whichever
/// of `target`/`unrelated` the scan reaches second. `github.com/telekom/sutura#2104`.
#[test]
fn from_verifiers_with_outbound_refuses_a_nested_target_sharing_its_issuer_with_an_unrelated_declaration() {
    let gateway_id = VerifierId::parse("gateway").expect("a fixture verifier id");
    let target_id = VerifierId::parse("paired-target").expect("a fixture verifier id");
    let outer_id = VerifierId::parse("outer").expect("a fixture verifier id");
    let unrelated_id = VerifierId::parse("unrelated").expect("a fixture verifier id");

    // The duplicate-issuer check runs before any key set is read, so every path below is one no
    // fixture ever publishes - exactly the style `from_verifiers_with_outbound_refuses_two_declarations_sharing_an_issuer`
    // already uses for the same reason.
    let gateway = InboundVerifier::new(
        gateway_id,
        McpPath::parse("/gateway").expect("a fixture path"),
        sutura_config::InboundIdentity::BehindGateway {
            transit: sutura_config::TransitProof::new(
                sutura_config::ProofHeader::parse(TRANSIT_HEADER).expect("a fixture header"),
                sutura_config::IssuerUrl::parse_transit_issuer("https://gateway.example.com").expect("a fixture issuer"),
                sutura_config::TransitBinding::Audience(
                    sutura_config::ResourceIdentifier::parse_transit_audience(RESOURCE).expect("a fixture audience"),
                ),
                sutura_config::KeySetLocation::File(
                    sutura_config::KeySetFile::parse("/unread/in/this/test-gateway.json").expect("a fixture path"),
                ),
                sutura_config::PinnedAlgorithms::of(sutura_config::SigningAlgorithm::Es256),
                sutura_config::RequiredTokenType::access_token(),
                sutura_config::ProofLifetime::default_lifetime(),
            ),
        },
    );
    // Paired with `gateway`, and ALSO `outer`'s own `requires_gateway` target - excluded from
    // `standalone` for that reason, same as the cell above. Its issuer is `ISSUER`, same as
    // `unrelated`'s below: that is the collision this cell pins.
    let target = InboundVerifier::new(
        target_id.clone(),
        McpPath::parse("/paired-target").expect("a fixture path"),
        paired_direct("/unread/in/this/test-target.json", "x-target-token"),
    );
    let outer = InboundVerifier::new(
        outer_id,
        McpPath::parse("/outer").expect("a fixture path"),
        sutura_config::InboundIdentity::Direct {
            resource: sutura_config::ResourceIdentifier::parse("https://outer.example.com/resource").expect("a fixture resource"),
            authorization_server: sutura_config::IssuerUrl::parse("https://outer.example.com").expect("a fixture issuer"),
            key_set: sutura_config::KeySetLocation::File(
                sutura_config::KeySetFile::parse("/unread/in/this/test-outer.json").expect("a fixture path"),
            ),
            algorithms: sutura_config::PinnedAlgorithms::of(sutura_config::SigningAlgorithm::Es256),
            token_type: sutura_config::RequiredTokenType::access_token(),
            address_claim: sutura_config::AddressClaim::VerifiedEmail,
            pairing: Some(sutura_config::GatewayPairing::new(
                sutura_config::ProofHeader::parse_named(sutura_config::ProofHeader::TOKEN_HEADER_KEY, "x-outer-token")
                    .expect("a fixture header"),
                target_id.clone(),
            )),
        },
    );
    // Standalone and unpaired - never named by anybody's `requires_gateway` - but reusing
    // `target`'s own issuer, `ISSUER`, via the same `direct_over` fixture every other issuer-sharing
    // cell in this file uses.
    let unrelated = InboundVerifier::new(
        unrelated_id.clone(),
        McpPath::parse("/unrelated").expect("a fixture path"),
        direct_over("/unread/in/this/test-unrelated.json"),
    );

    let declarations = vec![gateway, target, outer, unrelated];
    let result = InboundGate::from_verifiers_with_outbound(&declarations, None);

    match result {
        Err(InboundNotUsable::DuplicateVerifierIssuer { earlier, later, issuer }) => {
            assert_eq!(earlier, target_id);
            assert_eq!(later, unrelated_id);
            assert_eq!(issuer, ISSUER);
        }
        other => panic!("expected DuplicateVerifierIssuer, got {other:?}"),
    }
}
