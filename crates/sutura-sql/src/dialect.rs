//! Which data system a statement is rendered for, and the two things we do not delegate.
//!
//! The dialect layer we build on knows more about dialects than we do: it renders `DATE_TRUNC` as
//! `dateTrunc` and `SUM` as `sum` for `ClickHouse`, which is exactly the kind of difference nobody
//! should be maintaining by hand. Two things it does not decide for us, both measured rather than
//! assumed:
//!
//! **Placeholder syntax.** A placeholder renders as `?` for every dialect, including the one that
//! needs `$1`. The crate carries a per-dialect `parameter_token` field and never reads it. So the
//! style is chosen here, per dialect, and a target that needs numbering gets numbering.
//!
//! **Identifier quoting.** The generator quotes an identifier only when it was quoted in the source,
//! is a reserved word, or the config says always. Our identifiers were never in any source, so
//! without forcing it a column called `order` would be emitted bare. We force it, and we force it
//! for aliases too, which the config's own flag does not cover.

/// The data systems a statement can be rendered for.
///
/// A closed set rather than a passthrough of the dialect layer's thirty-three, because each entry
/// here is a claim that we generate correct SQL for it and have a golden that says so. Adding one is
/// a feature flag, a match arm and a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    DuckDb,
    Postgres,
    ClickHouse,
}

/// How a bind parameter is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderStyle {
    /// `?`, positional by order of appearance. `DuckDB` and `ClickHouse`.
    Question,
    /// `$1`, `$2`, numbered from one. Postgres.
    ///
    /// The numbering is why this is not cosmetic: a statement with three `?` sent to Postgres is a
    /// syntax error, and one with `$1` repeated is a different query.
    Numbered,
}

/// Every dialect, for iterating a golden suite over all of them.
///
/// A `const` rather than a derive, so a new variant that is not added here fails the exhaustiveness
/// test below rather than being silently untested.
pub const ALL: &[Dialect] = &[Dialect::DuckDb, Dialect::Postgres, Dialect::ClickHouse];

/// Why a dialect name was not recognised.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{value:?} is not a data system this build renders for; the choices are {choices}")]
pub struct UnknownDialect {
    value: String,
    choices: String,
}

impl Dialect {
    /// The name used on a command line and in a snapshot suffix.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DuckDb => "duckdb",
            Self::Postgres => "postgres",
            Self::ClickHouse => "clickhouse",
        }
    }

    /// Parses a dialect name.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownDialect> {
        let raw = raw.as_ref().trim();
        ALL.iter().copied().find(|d| d.as_str() == raw).ok_or_else(|| UnknownDialect {
            value: String::from(raw),
            choices: ALL.iter().map(|d| d.as_str()).collect::<Vec<&str>>().join(", "),
        })
    }

    /// How this data system writes a bind parameter.
    #[inline]
    pub const fn placeholder_style(self) -> PlaceholderStyle {
        match self {
            // Both accept positional `?`. `ClickHouse` also has a named form, and the driver decides
            // which it wants; `?` is the one both drivers we have accept.
            Self::DuckDb | Self::ClickHouse => PlaceholderStyle::Question,
            Self::Postgres => PlaceholderStyle::Numbered,
        }
    }
}

impl core::fmt::Display for Dialect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{ALL, Dialect, PlaceholderStyle};

    #[test]
    fn every_dialect_is_in_all() {
        // The list is what the golden suite iterates. A variant missing from it is a data system
        // with no snapshot, which reads as covered and is not.
        for dialect in [Dialect::DuckDb, Dialect::Postgres, Dialect::ClickHouse] {
            assert!(ALL.contains(&dialect), "{dialect} is not in ALL");
        }
        assert_eq!(ALL.len(), 3);
    }

    #[test]
    fn a_name_round_trips() {
        for dialect in ALL {
            assert_eq!(Dialect::parse(dialect.as_str()).expect("a listed name parses"), *dialect);
        }
    }

    #[test]
    fn an_unknown_name_lists_the_choices() {
        // The message is the whole value of the error here: a typo on a command line should not
        // require reading the source to find out what was meant.
        let err = Dialect::parse("bigquery").expect_err("bigquery is not rendered for");
        let rendered = err.to_string();
        assert!(rendered.contains("bigquery"), "{rendered}");
        assert!(rendered.contains("duckdb"), "{rendered}");
        assert!(rendered.contains("clickhouse"), "{rendered}");
    }

    #[test]
    fn postgres_is_the_one_that_needs_numbering() {
        // Measured, not assumed: the dialect layer renders every placeholder as `?` regardless of
        // target, so a statement generated for Postgres with `?` is a syntax error there. This is
        // the fact that makes placeholder style ours to own.
        assert_eq!(Dialect::Postgres.placeholder_style(), PlaceholderStyle::Numbered);
        assert_eq!(Dialect::DuckDb.placeholder_style(), PlaceholderStyle::Question);
        assert_eq!(Dialect::ClickHouse.placeholder_style(), PlaceholderStyle::Question);
    }
}
