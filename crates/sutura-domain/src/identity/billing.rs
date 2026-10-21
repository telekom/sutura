//! Who pays for a caller's work, at a source that bills per job.

use std::collections::BTreeSet;

/// The longest billing account name accepted, in bytes.
const MAX_ACCOUNT_LEN: usize = 64;

/// The name of an account a caller's work is charged to: 1 to 64 bytes of printable ASCII.
///
/// Neutral on purpose: an adapter re-parses it into its own account type and refuses one it cannot
/// use, so nothing here names a data system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingAccount(String);

/// Why a written billing account name is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnusableBillingAccount {
    /// No characters.
    #[error("a billing account name is not empty")]
    Empty,
    /// Longer than 64 bytes.
    #[error("a billing account name is at most 64 bytes, found {len}")]
    TooLong { len: usize },
    /// A space, a control character or a non-ASCII letter.
    #[error("the character at position {at} is not printable ASCII")]
    Character { at: usize },
}

impl BillingAccount {
    /// Parses an account name as written.
    ///
    /// # Errors
    ///
    /// [`UnusableBillingAccount`] for an empty or too long name, or one with a character outside
    /// printable ASCII.
    pub fn parse(written: &str) -> Result<Self, UnusableBillingAccount> {
        if written.is_empty() {
            return Err(UnusableBillingAccount::Empty);
        }
        if written.len() > MAX_ACCOUNT_LEN {
            return Err(UnusableBillingAccount::TooLong { len: written.len() });
        }
        if let Some(at) = written.chars().position(|c| !c.is_ascii_graphic()) {
            return Err(UnusableBillingAccount::Character { at });
        }
        Ok(Self(String::from(written)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the broker knew about who pays for one leg.
///
/// One enum rather than two `Option`s: a deployment's group mapping comes first, so a leg that has a
/// group's account never also carries the caller's own choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillTo {
    /// The account the deployment mapped one of the caller's verified groups onto.
    Group(BillingAccount),
    /// The account the caller named on the request, on a source that lets callers name one.
    Named(BillingAccount),
    /// Neither: the adapter decides from what the leg reads, or refuses.
    Unstated,
}

/// The values of the caller's verified `groups` claim.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallerGroups(BTreeSet<String>);

impl CallerGroups {
    #[inline]
    #[must_use]
    pub fn of<'a>(groups: impl IntoIterator<Item = &'a str>) -> Self {
        Self(groups.into_iter().map(String::from).collect())
    }

    #[inline]
    #[must_use]
    pub fn holds(&self, group: &str) -> bool {
        self.0.contains(group)
    }
}

#[cfg(test)]
mod tests {
    use super::{BillingAccount, UnusableBillingAccount};

    #[test]
    fn a_billing_account_that_is_not_1_to_64_printable_ascii_bytes_is_refused() {
        for (written, expected) in [
            ("", UnusableBillingAccount::Empty),
            (&*"a".repeat(65), UnusableBillingAccount::TooLong { len: 65 }),
            ("sales dev", UnusableBillingAccount::Character { at: 5 }),
            ("sal\u{e9}s", UnusableBillingAccount::Character { at: 3 }),
        ] {
            assert_eq!(BillingAccount::parse(written), Err(expected), "{written:?}");
        }
        assert_eq!(BillingAccount::parse(&"a".repeat(64)).map(|a| a.as_str().len()), Ok(64));
    }
}
