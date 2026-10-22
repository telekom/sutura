//! `domain_delegation.address_domains`: the only domains a `domain_delegation` source runs a caller
//! as, whatever claim the address came from.

use std::collections::BTreeSet;

use sutura_domain::identity::VerifiedAddress;

/// The longest domain accepted, in bytes: RFC 1035's name limit less its trailing dot.
const MAX_DOMAIN_LEN: usize = 253;

/// The longest label, in bytes.
const MAX_LABEL_LEN: usize = 63;

/// The domains a `domain_delegation` source delegates for. Never empty.
///
/// **Compared whole, ASCII case ignored, and never by suffix.** `a@sub.example.com` and
/// `a@evil-example.com` are both outside `example.com`. Case is ignored because DNS names that differ
/// only in ASCII case are one domain (RFC 4343), so no other party can own `EXAMPLE.com`. Only the
/// ASCII form of a name can match: an address holding a non-ASCII character never parses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressDomains(BTreeSet<String>);

/// Why a written `address_domains` list is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAddressDomains {
    /// An empty list, which would refuse every caller.
    #[error("the list is empty - name at least one domain, or remove the key")]
    Empty,
    /// An entry that is not a domain name.
    #[error(
        "entry {index} is not a domain: two or more labels of 1 to 63 ASCII letters, digits and inner \
         `-`, joined by `.`, at most 253 bytes"
    )]
    NotADomain { index: usize },
}

impl AddressDomains {
    /// Parses the written list, folding each domain to ASCII lowercase.
    ///
    /// # Errors
    ///
    /// [`InvalidAddressDomains`] for an empty list or an entry that is not a domain name.
    pub fn parse(written: &[String]) -> Result<Self, InvalidAddressDomains> {
        if written.is_empty() {
            return Err(InvalidAddressDomains::Empty);
        }
        written
            .iter()
            .enumerate()
            .map(|(index, domain)| {
                is_domain(domain)
                    .then(|| domain.to_ascii_lowercase())
                    .ok_or(InvalidAddressDomains::NotADomain { index })
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// Is `address` in one of these domains?
    #[must_use]
    pub fn admits(&self, address: &VerifiedAddress) -> bool {
        self.0.iter().any(|domain| domain.eq_ignore_ascii_case(address.domain()))
    }
}

fn is_domain(written: &str) -> bool {
    let label = |label: &str| {
        (1..=MAX_LABEL_LEN).contains(&label.len())
            && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    };
    written.len() <= MAX_DOMAIN_LEN && written.contains('.') && written.split('.').all(label)
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::VerifiedAddress;

    use super::{AddressDomains, InvalidAddressDomains};

    fn domains(written: &[&str]) -> Result<AddressDomains, InvalidAddressDomains> {
        AddressDomains::parse(&written.iter().map(|d| String::from(*d)).collect::<Vec<_>>())
    }

    fn admits(domains: &AddressDomains, address: &str) -> bool {
        domains.admits(&VerifiedAddress::parse(address).expect("a test address is an address"))
    }

    #[test]
    fn an_address_is_admitted_only_in_a_listed_domain_whole() {
        let listed = domains(&["example.com", "Example.org"]).expect("two domains");
        for address in ["a@example.com", "a@EXAMPLE.com", "First.Last@example.org"] {
            assert!(admits(&listed, address), "{address}");
        }
        for address in [
            "a@evil-example.com",
            "a@sub.example.com",
            "a@example.com.evil.net",
            "a@example.co",
            "a@example.net",
            "a@example.com.",
            "<a@example.com>",
        ] {
            assert!(!admits(&listed, address), "{address}");
        }
    }

    #[test]
    fn an_empty_list_or_an_entry_that_is_not_a_domain_is_refused() {
        assert_eq!(domains(&[]), Err(InvalidAddressDomains::Empty));
        let long = format!("{}.com", "a".repeat(250));
        for (index, written) in [
            "com",
            "@example.com",
            "*.example.com",
            ".example.com",
            "example..com",
            "-example.com",
            "example-.com",
            "example.com.",
            "exam ple.com",
            "b\u{fc}cher.de",
            long.as_str(),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                domains(&["example.com", written]),
                Err(InvalidAddressDomains::NotADomain { index: 1 }),
                "{index}: {written:?}"
            );
        }
        let _ascii_form = domains(&["xn--bcher-kva.de"]).expect("the ASCII form of a domain is a domain");
    }
}
