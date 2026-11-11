use super::*;
use crate::ErrorKind;

#[test]
fn approved_group_checks_peer_range_and_subgroup_before_key_exchange() {
    let modulus = GROUP14.to_be_bytes();
    let range_error = Some("invalid Diffie-Hellman public value");
    // -2 is in range but outside the order-q subgroup: p = 3 mod 4 and 2 is a residue.
    for (peer, expected) in [
        (U2048::from_u8(4), None),
        (U2048::ZERO, range_error),
        (U2048::ONE, range_error),
        (GROUP14.wrapping_sub(&U2048::ONE), range_error),
        (GROUP14, range_error),
        (GROUP14.wrapping_sub(&U2048::from_u8(2)), Some("invalid Diffie-Hellman subgroup")),
    ] {
        let bytes = peer.to_be_bytes();
        let negotiated = Negotiated {
            encryption: 17,
            integrity: 5,
            group_bits: 2048,
            generator: &[2],
            modulus: &modulus,
            peer: &bytes,
        };
        let result = exchange(&negotiated);
        if let Some(expected) = expected {
            let error = result.err().unwrap();
            assert!(matches!(error.kind(), ErrorKind::NativeEncryptionRefused(reason) if *reason == expected));
        } else {
            assert!(result.is_ok());
        }
    }
}
