//! Which claim of a verified token names the caller's address - read by a `domain_delegation`
//! source alone, which runs the caller's question as the Google account with that address.

use crate::inbound::InvalidInboundValue;

/// What a declared claim name may hold: a registered name like `upn`, or a namespaced one like
/// `https://example.com/google_email`.
const CLAIM_CHARACTERS: &str = "ASCII letters, digits, `_`, `-`, `.`, `:` and `/`";

/// The longest claim name accepted.
const MAX_CLAIM_LEN: usize = 128;

/// Where the caller's address comes from.
///
/// **The default is the only spelling that reads `email_verified`.** A declared claim is read
/// without it, because the issuers that need one (Entra ID, a gateway naming the user in `sub`)
/// never send it - so a declared claim starts only beside `accept_address_claim: true`, the
/// operator's statement that the issuer verifies the claim and the user cannot change it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressClaim {
    /// `email`, only where `email_verified` is the JSON boolean `true`.
    VerifiedEmail,
    /// The claim the operator declared, read with no verification flag beside it.
    Declared(ClaimName),
}

/// A claim name, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimName(String);

impl AddressClaim {
    pub const KEY: &'static str = "security.inbound.address_claim";
    pub const ACCEPT_KEY: &'static str = "security.inbound.accept_address_claim";

    /// Reads the written key. Absent, empty and `email` are the default: writing `email` does not
    /// switch `email_verified` off.
    ///
    /// # Errors
    ///
    /// [`InvalidInboundValue::TooLong`] past 128 bytes, [`InvalidInboundValue::NotPermitted`] for a
    /// character outside letters, digits, `_`, `-`, `.`, `:` and `/` - whitespace included, so a
    /// padded name is refused rather than trimmed into another claim.
    pub fn parse(written: Option<&str>) -> Result<Self, InvalidInboundValue> {
        let Some(name) = written.filter(|name| !name.is_empty() && *name != "email") else {
            return Ok(Self::VerifiedEmail);
        };
        if name.len() > MAX_CLAIM_LEN {
            return Err(InvalidInboundValue::TooLong {
                key: Self::KEY,
                found: name.len(),
                limit: MAX_CLAIM_LEN,
            });
        }
        if let Some(position) = name
            .chars()
            .position(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '/')))
        {
            return Err(InvalidInboundValue::NotPermitted {
                key: Self::KEY,
                position,
                accepted: CLAIM_CHARACTERS,
            });
        }
        Ok(Self::Declared(ClaimName(String::from(name))))
    }

    /// The declared claim, or `None` for the default.
    #[must_use]
    pub const fn declared(&self) -> Option<&ClaimName> {
        match *self {
            Self::VerifiedEmail => None,
            Self::Declared(ref name) => Some(name),
        }
    }
}

impl ClaimName {
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for ClaimName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{AddressClaim, InvalidInboundValue};

    #[test]
    fn absent_empty_and_email_are_the_verified_email_default() {
        for written in [None, Some(""), Some("email")] {
            assert_eq!(AddressClaim::parse(written), Ok(AddressClaim::VerifiedEmail), "{written:?}");
        }
    }

    #[test]
    fn a_declared_claim_is_kept_as_written() {
        for written in ["upn", "sub", "https://example.com/claims/google_email"] {
            let parsed = AddressClaim::parse(Some(written)).expect("a claim name");
            assert_eq!(parsed.declared().map(super::ClaimName::as_str), Some(written));
        }
    }

    #[test]
    fn a_claim_name_with_whitespace_or_past_128_bytes_is_refused() {
        for (written, position) in [(" upn", 0), ("upn ", 3), ("up\u{200b}n", 2), ("e\"mail", 1)] {
            assert!(
                matches!(
                    AddressClaim::parse(Some(written)),
                    Err(InvalidInboundValue::NotPermitted { position: at, .. }) if at == position
                ),
                "{written:?}"
            );
        }
        let long = "a".repeat(129);
        assert!(matches!(
            AddressClaim::parse(Some(&long)),
            Err(InvalidInboundValue::TooLong {
                found: 129,
                limit: 128,
                ..
            })
        ));
    }
}
