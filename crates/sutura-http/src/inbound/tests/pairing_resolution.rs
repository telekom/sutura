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

use super::fixtures::paired_direct;
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
