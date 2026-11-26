use super::{Claims, VerifiedAddress, address_of};

fn claims(extra: &serde_json::Value) -> Claims {
    let mut claims = serde_json::json!({ "sub": "someone@example.com", "exp": 4_102_444_800_i64 });
    if let (Some(into), Some(from)) = (claims.as_object_mut(), extra.as_object()) {
        into.extend(from.clone());
    }
    serde_json::from_value(claims).expect("a test claim set deserializes")
}

#[test]
fn a_verified_email_is_the_callers_address_as_written() {
    let address = address_of(&claims(
        &serde_json::json!({ "email": "Some.One@Example.com", "email_verified": true }),
    ));
    assert_eq!(address.as_ref().map(VerifiedAddress::as_str), Some("Some.One@Example.com"));
}

#[test]
fn an_unverified_email_yields_no_address() {
    for verified in [serde_json::json!(false), serde_json::json!("true"), serde_json::json!(1)] {
        let unverified = claims(&serde_json::json!({ "email": "someone@example.com", "email_verified": verified }));
        assert!(address_of(&unverified).is_none(), "{verified}");
    }
    assert!(address_of(&claims(&serde_json::json!({ "email": "someone@example.com" }))).is_none());
}

#[test]
fn an_absent_email_yields_no_address() {
    // The `sub` beside it is an address too, so an absent `email` that fell back to it would show.
    for email in [serde_json::json!(null), serde_json::json!(["someone@example.com"])] {
        let verified = claims(&serde_json::json!({ "email": email, "email_verified": true }));
        assert!(address_of(&verified).is_none(), "{email}");
    }
    assert!(address_of(&claims(&serde_json::json!({ "email_verified": true }))).is_none());
}
