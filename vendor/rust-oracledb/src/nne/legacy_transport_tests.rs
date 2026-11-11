use super::*;
use crate::{Config, ErrorKind};

#[test]
fn legacy_opt_in_still_requires_an_actually_encrypted_transport() {
    for verifier in [0x939, 0xb152, 0x1b25] {
        for native_requested in [false, true] {
            let config = Config::default()
                .set_credentials("user", "password")
                .set_allow_legacy_verifiers(true)
                .set_native_encryption(native_requested);
            let mut client = Client::new(config, String::new());
            let mut message = AuthMessage::new();
            message.verifier_type = Some(verifier);
            let error = message.generate_verifier(&mut client).unwrap_err();
            assert!(matches!(error.kind(), ErrorKind::LegacyVerifierRequiresEncryption(id) if *id == verifier));
            assert!(message.combo_key.is_none());
            assert!(message.pairs.is_empty());
        }
    }
}
