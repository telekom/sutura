//! How a text column compares: the evidence a source gave for it, and nothing it did not.

/// The longest collation name kept. A name past it is not one a dictionary reports.
pub const MAX_COLLATION_NAME_CHARS: usize = 128;

/// The Arrow field metadata key a pinned text column carries its [`Collation`] under.
pub const COLLATION_METADATA_KEY: &str = "sutura:collation";

/// A collation as a source spells it: `und:ci`, `en_US`, `NOCASE`, `pg_catalog.default`.
///
/// A closed ASCII vocabulary (letters, digits and `_ - . : @ = ,`), so the name can sit in Arrow
/// field metadata and under a digest without a character that reorders or hides text. A name the
/// vocabulary cannot hold is no collation this crate knows, and [`Collation::named`] maps it to
/// [`Collation::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct CollationName(String);

/// Why a collation name was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidCollationName {
    #[error("a collation name must not be empty")]
    Empty,
    #[error("a collation name may be at most {limit} characters, {value:?} has {len}")]
    TooLong { value: String, len: usize, limit: usize },
    #[error("a collation name may hold only letters, digits and `_ - . : @ = ,`, {value:?} holds {offending:?}")]
    Character { value: String, offending: char },
}

impl CollationName {
    /// # Errors
    ///
    /// [`InvalidCollationName`]: empty, over [`MAX_COLLATION_NAME_CHARS`], or outside the vocabulary.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidCollationName> {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(InvalidCollationName::Empty);
        }
        let len = raw.chars().count();
        if len > MAX_COLLATION_NAME_CHARS {
            return Err(InvalidCollationName::TooLong {
                value: raw.chars().take(MAX_COLLATION_NAME_CHARS).collect(),
                len,
                limit: MAX_COLLATION_NAME_CHARS,
            });
        }
        if let Some(offending) = raw
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '@' | '=' | ',')))
        {
            return Err(InvalidCollationName::Character {
                value: raw.to_owned(),
                offending,
            });
        }
        Ok(Self(raw.to_owned()))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for CollationName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a text column's `LIKE` and equality compare, as pinned.
///
/// **[`Self::Unknown`] is the default and means "not binary"**: the source said nothing, or said
/// something this type cannot hold. Only [`Self::Binary`] licenses code that is safe solely where
/// the comparison is byte-for-byte. It is evidence a reader or a catalog author supplied, never a
/// measurement of the data system at run time, and an author's `binary` is trusted as far as the
/// author is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Collation {
    /// Comparison is byte-for-byte on the stored text.
    Binary,
    /// A collation the source names. Never treated as binary, whatever its name suggests.
    Named(CollationName),
    #[default]
    Unknown,
}

impl Collation {
    /// A source's own spelling of a collation; [`Self::Unknown`] for a name this type cannot hold.
    #[must_use]
    pub fn named(raw: &str) -> Self {
        CollationName::parse(raw).map_or(Self::Unknown, Self::Named)
    }

    /// An author's declaration: `binary` (any case) is [`Self::Binary`], any other valid name is
    /// [`Self::Named`].
    ///
    /// # Errors
    ///
    /// [`InvalidCollationName`], so a typo in a catalog document is a refusal rather than a silent
    /// downgrade.
    pub fn declared(raw: &str) -> Result<Self, InvalidCollationName> {
        let name = CollationName::parse(raw)?;
        Ok(if name.as_str().eq_ignore_ascii_case("binary") {
            Self::Binary
        } else {
            Self::Named(name)
        })
    }

    #[inline]
    #[must_use]
    pub const fn is_binary(&self) -> bool {
        matches!(self, Self::Binary)
    }

    #[inline]
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// The collation a pinned field carries; absent, or unreadable, is [`Self::Unknown`].
    #[must_use]
    pub fn of_field(field: &arrow_schema::Field) -> Self {
        field
            .metadata()
            .get(COLLATION_METADATA_KEY)
            .map_or(Self::Unknown, |text| Self::decoded(text))
    }

    /// Records this collation on a field being pinned; [`Self::of_field`] reads it back.
    /// [`Self::Unknown`] writes nothing, so a field with no collation fact hashes as it always did.
    ///
    /// Public so a cell in another crate can build a pinned field that carries a collation. The
    /// pin itself writes it only for a `Utf8` column, from the catalog's column or the dialect.
    pub fn pin_onto(&self, field: &mut arrow_schema::Field) {
        if let Some(text) = self.encoded() {
            field.metadata_mut().insert(String::from(COLLATION_METADATA_KEY), text);
        }
    }

    /// The text a pinned field carries, `None` for [`Self::Unknown`] (absence is unknown).
    pub(crate) fn encoded(&self) -> Option<String> {
        match self {
            Self::Binary => Some(String::from("binary")),
            Self::Named(name) => Some(format!("named:{name}")),
            Self::Unknown => None,
        }
    }

    /// The inverse of [`Self::encoded`]; anything unreadable is [`Self::Unknown`].
    pub(crate) fn decoded(text: &str) -> Self {
        match text.strip_prefix("named:") {
            Some(name) => Self::named(name),
            None if text == "binary" => Self::Binary,
            None => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Collation, CollationName, InvalidCollationName, MAX_COLLATION_NAME_CHARS};

    #[test]
    fn a_collation_name_is_a_bounded_ascii_token_and_anything_else_is_refused() {
        assert_eq!(CollationName::parse("und:ci").expect("a name").as_str(), "und:ci");
        assert_eq!(CollationName::parse(""), Err(InvalidCollationName::Empty));
        assert!(matches!(
            CollationName::parse("a".repeat(MAX_COLLATION_NAME_CHARS + 1)),
            Err(InvalidCollationName::TooLong { len, .. }) if len == MAX_COLLATION_NAME_CHARS + 1
        ));
        CollationName::parse("a".repeat(MAX_COLLATION_NAME_CHARS)).expect("the bound itself is kept");
        for raw in ["en US", "x\u{202e}y", "x\ny", "\"C\"", "\u{fc}n\u{ef}"] {
            assert!(
                matches!(CollationName::parse(raw), Err(InvalidCollationName::Character { .. })),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn only_a_declared_binary_is_binary_and_a_name_a_source_reports_never_is() {
        assert_eq!(Collation::default(), Collation::Unknown);
        assert!(!Collation::Unknown.is_binary());
        assert_eq!(Collation::declared("BINARY"), Ok(Collation::Binary));
        assert!(matches!(Collation::declared("NOCASE"), Ok(Collation::Named(_))));
        assert!(matches!(
            Collation::declared("no case"),
            Err(InvalidCollationName::Character { offending: ' ', .. })
        ));
        assert!(
            !Collation::named("binary").is_binary(),
            "a reported name is evidence of a name"
        );
        assert_eq!(Collation::named("not a name"), Collation::Unknown);
    }

    #[test]
    fn the_pinned_encoding_round_trips_and_unreadable_text_is_unknown() {
        for collation in [Collation::Binary, Collation::named("und:ci"), Collation::Unknown] {
            assert_eq!(
                collation
                    .encoded()
                    .map_or(Collation::Unknown, |text| Collation::decoded(&text)),
                collation
            );
        }
        for text in ["", "BINARY", "named:", "named:a b", "named:named:x ", "other"] {
            assert_eq!(Collation::decoded(text), Collation::Unknown, "{text:?}");
        }
    }
}
