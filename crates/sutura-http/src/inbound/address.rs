//! The caller's address out of a claim the operator declared - `security.inbound.address_claim`.
//!
//! The default (`email` beside `email_verified: true`) is read off the verified claims in
//! [`super::token`]; this module is the declared path only.

use sutura_config::ClaimName;
use sutura_domain::identity::VerifiedAddress;

/// The caller's address out of the declared claim, read from the payload `decode` just verified.
///
/// A second read of the same signed bytes rather than a field on the token's claims, so the default path
/// deserializes exactly what it did before a claim could be declared. `email_verified` is not read.
/// The claim must occur once and hold a string that parses; a repeated name - escaped spellings
/// included - is no address, because reading either value would be choosing which one to believe.
pub(super) fn declared_address(token: &str, claim: &ClaimName) -> Option<VerifiedAddress> {
    use base64::Engine as _;
    let mut segments = token.split('.');
    let (Some(_), Some(payload), Some(_), None) = (segments.next(), segments.next(), segments.next(), segments.next()) else {
        return None;
    };
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    let written =
        serde::de::DeserializeSeed::deserialize(OneClaim(claim.as_str()), &mut serde_json::Deserializer::from_slice(&bytes))
            .ok()
            .flatten()?;
    VerifiedAddress::parse(&written).ok()
}

/// Reads one named member of a JSON object: `Some` only for a string that occurs once.
struct OneClaim<'name>(&'name str);

impl<'de> serde::de::DeserializeSeed<'de> for OneClaim<'_> {
    type Value = Option<String>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de> serde::de::Visitor<'de> for OneClaim<'_> {
    type Value = Option<String>;

    fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("a JSON object of claims")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut found = None;
        let mut seen = 0_usize;
        while let Some(name) = map.next_key::<String>()? {
            if name == self.0 {
                seen = seen.saturating_add(1);
                found = map.next_value::<serde_json::Value>()?.as_str().map(String::from);
            } else {
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
        Ok(found.filter(|_| seen == 1))
    }
}

#[cfg(test)]
mod declared_address_tests {
    use base64::Engine as _;
    use sutura_config::AddressClaim;

    use super::{VerifiedAddress, declared_address};

    /// A token whose payload is `json` as written, so a cell can repeat a key. The signature is not
    /// read here: `verify` checked it before this runs.
    fn token(json: &str) -> String {
        format!("e30.{}.c2ln", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
    }

    fn read(claim: &str, json: &str) -> Option<String> {
        let AddressClaim::Declared(name) = AddressClaim::parse(Some(claim)).expect("a claim name") else {
            panic!("{claim} is not the default");
        };
        declared_address(&token(json), &name)
            .as_ref()
            .map(|a| VerifiedAddress::as_str(a).to_owned())
    }

    #[test]
    fn an_entra_shaped_token_gives_its_upn_where_the_claim_is_declared() {
        let entra =
            r#"{"sub":"AAAAAAAAAAAAAAAAAAAAAIkzqFVrSaSaFHy782bbtaQ","upn":"First.Last@Example.com","email":"other@example.org"}"#;
        assert_eq!(read("upn", entra).as_deref(), Some("First.Last@Example.com"));
        let gateway = r#"{"sub":"analyst@example.com"}"#;
        assert_eq!(read("sub", gateway).as_deref(), Some("analyst@example.com"));
    }

    #[test]
    fn a_repeated_absent_or_non_string_claim_gives_no_address() {
        for json in [
            r#"{"upn":"a@example.com","upn":"b@example.com"}"#,
            r#"{"upn":"a@example.com","upn":"a@example.com"}"#,
            r#"{"upn":"a@example.com","\u0075pn":"b@example.com"}"#,
            r#"{"upn":["a@example.com"]}"#,
            r#"{"upn":7}"#,
            r#"{"upn":null}"#,
            r#"{"email":"a@example.com","email_verified":true}"#,
        ] {
            assert_eq!(read("upn", json), None, "{json}");
        }
    }

    #[test]
    fn a_display_name_a_list_or_padding_is_not_an_address() {
        for value in [
            "First Last <a@example.com>",
            "a@example.com, b@example.com",
            "a@example.com,b@example.com",
            " a@example.com",
            "a@example.com\t",
            "a@exam\u{200b}ple.com",
        ] {
            let json = serde_json::json!({ "upn": value }).to_string();
            assert_eq!(read("upn", &json), None, "{value:?}");
        }
    }
}
