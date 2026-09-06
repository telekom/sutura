//! The two alphabets a name reaches a nextest filter through.
//!
//! Both PARSE, and the reason is the same for each: a name from a diff is interpolated into a
//! filter expression, so one carrying a metacharacter would widen the run or break it rather than
//! fail visibly. They stay two types rather than one lenient one because the alphabets genuinely
//! differ - cargo allows `-`, Rust does not - and `super::scoped` picks per position.

/// One Rust identifier: a test function's name, or one segment of a module path.
///
/// A newtype that PARSES, and `super::scoped::AddedTest::term` is the reason: this reaches nextest inside a
/// regular expression, so one carrying a metacharacter would widen the filter or break it rather
/// than fail visibly. A Rust identifier cannot carry one; anything that is not one does not get
/// through this constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ident(String);

impl Ident {
    /// The identifier, if `raw` is an ASCII Rust one.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let mut chars = raw.chars();
        let leading = chars.next()?;
        if !leading.is_ascii_alphabetic() && leading != '_' {
            return None;
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        Some(Self(String::from(raw)))
    }

    /// The identifier as written.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A cargo package or target name.
///
/// Parses for the same reason [`Ident`] does - it reaches nextest inside a filter expression -
/// but it is a different alphabet: cargo allows `-`, which Rust does not, and `sutura-domain` and
/// `multi_player` are both real names here. It is NOT a superset of `Ident` in intent, so the two
/// stay separate types rather than one lenient one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CargoName(String);

impl CargoName {
    /// The name, if `raw` is one cargo could have accepted.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let mut chars = raw.chars();
        let leading = chars.next()?;
        if !leading.is_ascii_alphanumeric() && leading != '_' {
            return None;
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return None;
        }
        Some(Self(String::from(raw)))
    }

    /// The name as written.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{CargoName, Ident};

    #[test]
    fn a_name_that_is_not_an_identifier_is_refused() {
        // What keeps a regular expression out of the filter expression.
        assert!(Ident::parse("sums_by_month").is_some());
        assert!(Ident::parse("_private").is_some());
        assert!(Ident::parse("").is_none());
        assert!(Ident::parse("9lives").is_none());
        assert!(Ident::parse("sums|.*").is_none());
        assert!(Ident::parse("two words").is_none());
        // A cargo name is a different alphabet: `-` is legal there and not in Rust.
        assert!(CargoName::parse("sutura-domain").is_some());
        assert!(CargoName::parse("multi_player").is_some());
        assert!(CargoName::parse("bad)name").is_none());
        assert!(CargoName::parse("has space").is_none());
    }
}
