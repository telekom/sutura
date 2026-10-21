//! The email address an identity provider verified for the caller.
//!
//! Read only by a `domain_delegation` source, which runs the caller's question as the Google account
//! with this address. Parsed and never normalised: no case fold, no alias, no trimming, so the
//! account a question runs as is exactly the one the identity provider named.

/// The longest address accepted, in bytes: RFC 5321's path limit less its two angle brackets.
const MAX_ADDRESS_LEN: usize = 254;

/// An address the identity provider marked verified, as it wrote it.
///
/// The transport reads `email` into one only when `email_verified` is `true`. `Debug` does not
/// print the value: it names a person.
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedAddress(String);

/// Why a claimed address is not one. The value is never quoted: it names a person.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAddress {
    /// Longer than 254 bytes.
    #[error("an address may be at most 254 bytes, found {len}")]
    TooLong { len: usize },
    /// A character outside printable ASCII: a space, a control character or a non-ASCII letter.
    #[error("the character at position {at} is not printable ASCII")]
    Character { at: usize },
    /// Not exactly one `@` between a non-empty local part and a non-empty domain.
    #[error("an address is one `@` between a non-empty local part and a non-empty domain")]
    Shape,
}

impl VerifiedAddress {
    /// Parses an address as written.
    ///
    /// # Errors
    ///
    /// [`InvalidAddress`] for an address that is too long, holds a character outside printable
    /// ASCII, or is not one `@` between two non-empty halves.
    pub fn parse(written: &str) -> Result<Self, InvalidAddress> {
        if written.len() > MAX_ADDRESS_LEN {
            return Err(InvalidAddress::TooLong { len: written.len() });
        }
        if let Some(at) = written.chars().position(|c| !c.is_ascii_graphic()) {
            return Err(InvalidAddress::Character { at });
        }
        match written.split_once('@') {
            Some((local, domain)) if !local.is_empty() && !domain.is_empty() && !domain.contains('@') => {
                Ok(Self(String::from(written)))
            }
            _ => Err(InvalidAddress::Shape),
        }
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for VerifiedAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("VerifiedAddress(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidAddress, VerifiedAddress};

    #[test]
    fn an_address_that_is_not_one_ascii_mailbox_of_at_most_254_bytes_is_refused() {
        let long = format!("{}@example.com", "a".repeat(243));
        assert_eq!(long.len(), 255);
        for (written, expected) in [
            ("a@b@example.com", InvalidAddress::Shape),
            ("user.example.com", InvalidAddress::Shape),
            ("@example.com", InvalidAddress::Shape),
            ("user@", InvalidAddress::Shape),
            ("us\u{e9}r@example.com", InvalidAddress::Character { at: 2 }),
            (" user@example.com", InvalidAddress::Character { at: 0 }),
            ("user@example.com\n", InvalidAddress::Character { at: 16 }),
            (long.as_str(), InvalidAddress::TooLong { len: 255 }),
        ] {
            assert_eq!(VerifiedAddress::parse(written), Err(expected), "{written:?}");
        }
        let longest = format!("{}@example.com", "a".repeat(242));
        assert_eq!(VerifiedAddress::parse(&longest).map(|a| a.as_str().len()), Ok(254));
    }

    #[test]
    fn an_address_is_kept_as_written_and_not_case_folded() {
        let parsed = VerifiedAddress::parse("First.Last+tag@Example.COM").expect("an address");
        assert_eq!(parsed.as_str(), "First.Last+tag@Example.COM");
        assert_ne!(
            parsed,
            VerifiedAddress::parse("first.last+tag@example.com").expect("an address")
        );
        assert_eq!(format!("{parsed:?}"), "VerifiedAddress(..)");
    }
}
