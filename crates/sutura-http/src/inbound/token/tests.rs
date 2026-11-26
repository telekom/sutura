use sutura_config::ProofLifetime;

use super::{NotVerified, TokenRejected, TokenValidator};

#[test]
fn an_assertion_past_its_window_is_refused_as_expired_under_single_use() {
    let settings = crate::testing::settings_with(
        "security:\n  inbound:\n    mode: \"behind-gateway\"\n    transit_header: \"x-transit-proof\"\n    \
         transit_issuer: \"https://issuer.example.com\"\n    transit_audience: \"https://sutura.example.com\"\n    \
         key_set_file: \"/unread.json\"\n    algorithms: [\"ES256\"]\n    transit_token_type: \"at+jwt\"\n    \
         transit_single_use: true\n",
    );
    let validator = TokenValidator::new(&crate::testing::declared_inbound(&settings).requirement());
    let refused = validator.admitted_once(Some(&serde_json::json!("a")), 0);
    assert!(
        matches!(
            refused,
            Err(TokenRejected::NotVerified(NotVerified::Crypto { ref cause }))
                if *cause.kind() == jsonwebtoken::errors::ErrorKind::ExpiredSignature
        ),
        "{refused:?}"
    );
}

#[test]
fn a_seen_entry_lives_the_ceiling_plus_the_leeway_on_both_ends() {
    let ceiling = ProofLifetime::parse(120).expect("120 seconds is a lifetime");
    assert_eq!(super::entry_window(ceiling), 180);
}
