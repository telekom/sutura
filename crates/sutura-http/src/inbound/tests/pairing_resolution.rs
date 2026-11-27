//! A `direct` declaration carrying [`sutura_config::GatewayPairing`] is only ever usable through
//! [`InboundGate::from_verifiers_with_outbound`], which alone sees every declared verifier and can
//! resolve the named target - `github.com/telekom/sutura#2104`.
//!
//! **Red against base**: before this fix, [`InboundGate::from_declaration`] built a gate anyway for
//! a paired declaration (the `requires_gateway` requirement just never attached, because this
//! single-declaration builder has no sibling list to resolve it against), and
//! `from_verifiers_with_outbound`'s own lookup silently dropped a `requires_gateway` naming a
//! verifier id absent from the declared list, continuing past it rather than refusing.

use sutura_config::{InboundVerifier, McpPath};
use sutura_dev::issuer::MockIssuer;
use sutura_domain::identity::VerifierId;

use super::fixtures::{direct_over, gateway_declaring, paired_direct};
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
#[test]
fn from_verifiers_with_outbound_refuses_a_paired_target_with_no_slot_of_its_own() {
    let gateway_id = VerifierId::parse("gateway").expect("a fixture verifier id");
    let target_id = VerifierId::parse("paired-target").expect("a fixture verifier id");
    let outer_id = VerifierId::parse("outer").expect("a fixture verifier id");

    let gateway = InboundVerifier::new(
        gateway_id,
        McpPath::parse("/gateway").expect("a fixture path"),
        gateway_declaring(
            sutura_config::RequiredTokenType::access_token(),
            sutura_config::ProofLifetime::default_lifetime(),
        ),
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

    let declarations = vec![gateway, target, outer];
    match InboundGate::from_verifiers_with_outbound(&declarations, None) {
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
