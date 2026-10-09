//! Per-caller execution: each statement is sent as `EXECUTE AS "<user>" <statement>`.
//!
//! **The per-statement form only.** `ClickHouse` also has `EXECUTE AS <user>` with no statement,
//! which switches the user for the rest of a session. This adapter never renders it:
//! [`ClickHouseUser::execute_as`] is the one place the keyword is written, and it always carries the
//! statement. Each request is also stateless - `transport::Http` sends no `session_id` - so no
//! switch can outlive the request that asked for it, and two callers on one pooled connection
//! cannot inherit each other's user.
//!
//! **The user is never caller text.** It comes from the source's declared subject-to-user map, is
//! parsed here to a closed character set, and is quoted with the dialect's identifier quote. The
//! character set has no quote, backslash or whitespace, so the quoted span cannot be closed early.
//!
//! The server half is two operator settings this adapter cannot set: the server setting
//! `access_control_improvements.allow_impersonate_user = 1`, and `GRANT IMPERSONATE ON <user>` to
//! the service user. `ClickHouseWarehouse::refuse_unless_executes_as` checks both at boot. Not
//! available on `ClickHouse` Cloud.

use sutura_sql::Dialect;

/// A `ClickHouse` user a declared subject executes as.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClickHouseUser(String);

/// Why a declared value is not a `ClickHouse` user this adapter can name in `EXECUTE AS`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotAClickHouseUser {
    #[error("a ClickHouse user name is empty")]
    Empty,
    #[error("a ClickHouse user name is {found} characters long, and at most {most} are accepted")]
    TooLong { found: usize, most: usize },
    /// Outside letters, digits and `_ . - @`, so it could close the quoted identifier.
    #[error("a ClickHouse user name carries a character other than letters, digits and `_ . - @` at byte {at}")]
    Character { at: usize },
}

impl ClickHouseUser {
    const MOST: usize = 128;

    /// Parses a declared user name.
    ///
    /// # Errors
    ///
    /// An empty name, one longer than 128 characters, or one with a character outside letters,
    /// digits and `_ . - @`.
    pub fn parse(raw: &str) -> Result<Self, NotAClickHouseUser> {
        if raw.is_empty() {
            return Err(NotAClickHouseUser::Empty);
        }
        if raw.len() > Self::MOST {
            return Err(NotAClickHouseUser::TooLong {
                found: raw.len(),
                most: Self::MOST,
            });
        }
        if let Some(at) = raw.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '@'))) {
            return Err(NotAClickHouseUser::Character { at });
        }
        Ok(Self(String::from(raw)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `statement`, run as this user for that one statement.
    #[must_use]
    pub fn execute_as(&self, statement: &str) -> String {
        let quote = Dialect::ClickHouse.identifier_quote().character();
        format!("EXECUTE AS {quote}{}{quote} {statement}", self.0)
    }
}

impl core::fmt::Display for ClickHouseUser {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{ClickHouseUser, NotAClickHouseUser};

    #[test]
    fn a_user_name_that_could_close_its_quote_is_refused() {
        for raw in ["a\"b", "a`b", "a\\b", "a b", "a;b", "a\nb", "a'b"] {
            assert_eq!(
                ClickHouseUser::parse(raw),
                Err(NotAClickHouseUser::Character { at: 1 }),
                "{raw:?}"
            );
        }
        assert_eq!(ClickHouseUser::parse(""), Err(NotAClickHouseUser::Empty));
        assert!(matches!(
            ClickHouseUser::parse(&"a".repeat(129)),
            Err(NotAClickHouseUser::TooLong { found: 129, .. })
        ));
    }

    #[test]
    fn the_statement_is_always_carried_and_the_user_is_quoted() {
        let user = ClickHouseUser::parse("analyst.one@example.com").expect("a declared user parses");
        assert_eq!(user.execute_as("SELECT 1"), "EXECUTE AS \"analyst.one@example.com\" SELECT 1");
    }
}
