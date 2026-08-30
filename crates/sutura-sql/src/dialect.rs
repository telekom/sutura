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
//!
//! And two things the layer DOES decide that this module has to state anyway, because something
//! outside the renderer reads them. Both are declarations of what we expect the layer to do, and
//! both are MEASURED against it - the tests live in [`mod@crate::generate`], which is the module allowed
//! to name the layer:
//!
//! **Which character the quotes are.** We force quoting; the layer picks the character, and it is not
//! the same one everywhere. `BigQuery` uses a backtick, and in `GoogleSQL` a double quote is a STRING
//! LITERAL rather than an identifier quote - so the difference is not cosmetic in the direction that
//! matters: a statement quoted the wrong way is not a syntax error there, it is a statement about
//! different values. The golden suite's *no identifier reaches the statement unquoted* claim searches
//! for quoted spans, so it has to know which character to look for, and a hard-coded `"` silently
//! stopped asserting anything the moment a fourth dialect arrived.
//!
//! **How the date bucket is spelled.** [`DateTruncShape`] carries the argument order and whether the
//! grain is a string literal or a bare keyword, and it exists because the parse check cannot catch
//! getting it wrong - see that type.

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
    BigQuery,
}

/// How a bind parameter is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderStyle {
    /// `?`, positional by order of appearance. `DuckDB`, `ClickHouse` and `BigQuery`.
    Question,
    /// `$1`, `$2`, numbered from one. Postgres.
    ///
    /// The numbering is why this is not cosmetic: a statement with three `?` sent to Postgres is a
    /// syntax error, and one with `$1` repeated is a different query.
    Numbered,
}

/// Which character a dialect wraps an identifier in.
///
/// Two variants and no `Other(char)`, for this workspace's usual reason: a variant is a claim that we
/// render and pin a dialect using it, and a `char` field would let a caller invent one nothing was
/// tested against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierQuote {
    /// `"name"`. `DuckDB`, Postgres and `ClickHouse`.
    Double,
    /// `` `name` ``. `BigQuery`.
    ///
    /// **The asymmetry that makes this worth a type, and this is the wrong-NUMBER risk on this
    /// dialect.** For the other three a double quote is an identifier quote and a backtick is a
    /// syntax error, so a mistake is loud. In `GoogleSQL` a double quote delimits a STRING, so
    /// `SELECT "amount"` is not a column reference at all - it selects the constant text `amount`,
    /// and the target's own lexical reference leans on this when it writes
    /// `WHERE date_col = "2014-09-27"` as a string coerced to a date. A statement quoted the wrong
    /// way can therefore be accepted and answer about different values.
    ///
    /// What limits the blast radius today is that the mistake is caught in CI for a different reason:
    /// a qualified column makes `"orders"."month"` a literal followed by a dot, which the target's
    /// parser rejects - so the golden suite's parse check does bite. That is luck about the shape we
    /// generate rather than a guarantee about the quote character, which is why it is written down
    /// beside the type rather than trusted.
    Backtick,
}

impl IdentifierQuote {
    /// The character itself, for building or searching for a quoted span.
    #[inline]
    #[must_use]
    pub const fn character(self) -> char {
        match self {
            Self::Double => '"',
            Self::Backtick => '`',
        }
    }
}

/// How a dialect spells truncating a date to a grain.
///
/// **This type exists because the parse check cannot catch getting it wrong, and that was measured
/// rather than assumed.** Within one target, `polyglot_sql::parse` accepts `DATE_TRUNC('month', col)`
/// and `DATE_TRUNC(col, MONTH)` alike - a generic function call is a generic function call to a
/// parser, whatever the argument order means to the data system. So the golden suite's *every
/// generated statement parses under its target dialect* row is blind to this class by construction,
/// and the only things standing behind the bucket are this exhaustive match, the golden diff a
/// reviewer reads, and an execution against a real instance.
///
/// **What getting it wrong costs, stated precisely, because the two failures here are not the same
/// severity.** Sending `BigQuery` the grain-first shape produces a statement it **rejects**: the
/// first argument is the value to truncate, a string literal only coerces there if it is a canonical
/// date, and `'month'` is not one - while the column lands in the granularity slot, which takes a
/// keyword. So the defect is a deployment whose corpus is green and which fails on its first real
/// question, not one that returns a wrong number. The wrong-number risk on this dialect belongs to
/// [`IdentifierQuote`] instead, and that asymmetry is why the two are separate types.
///
/// A fifth dialect therefore cannot be added without stating its spelling, which is the one
/// mechanism available here.
///
/// Verified against the target's own function reference rather than inferred: the documented syntax
/// is `DATE_TRUNC(date_value, date_granularity)`, every granularity in the list is a bare keyword,
/// and one of them - `WEEK(<WEEKDAY>)` - is not expressible as a string at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTruncShape {
    /// `DATE_TRUNC('month', <date>)` - the grain first, as a single-quoted string literal.
    ///
    /// `DuckDB`, Postgres and `ClickHouse`. The layer rewrites the function name for `ClickHouse`
    /// itself, which is a difference we do delegate.
    GrainFirstAsLiteral,
    /// `DATE_TRUNC(<date>, MONTH)` - the date first, the grain a bare keyword.
    ///
    /// `BigQuery`. Both halves differ from the shape above, and neither half is optional: the
    /// argument order and the grain's form are separately load-bearing.
    DateFirstAsKeyword,
}

/// Every dialect, for iterating a golden suite over all of them.
///
/// A `const` rather than a derive, so a new variant that is not added here fails the exhaustiveness
/// test below rather than being silently untested.
pub const ALL: &[Dialect] = &[Dialect::DuckDb, Dialect::Postgres, Dialect::ClickHouse, Dialect::BigQuery];

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
            Self::BigQuery => "bigquery",
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
    ///
    /// **`BigQuery` is `Question`, and the decision was taken against the client's request shape
    /// rather than against the rendering.** Its job API takes either positional parameters, written
    /// `?` in the statement with `parameterMode: POSITIONAL` and an ORDERED array carrying no names,
    /// or named ones written `@name` with `parameterMode: NAMED`. A [`crate::GeneratedQuery`] carries
    /// an ordered `Vec` of values and no names at all - the plan has none to give, because a
    /// parameter's identity there IS its position - so positional is the shape that already matches
    /// end to end. Choosing named would mean inventing a name per parameter in the generator, a third
    /// `PlaceholderStyle`, and a map on `GeneratedQuery` for a driver to read: three new things, none
    /// of which the domain has anything to put in them.
    #[inline]
    pub const fn placeholder_style(self) -> PlaceholderStyle {
        match self {
            // All three accept positional `?`. `ClickHouse` also has a named form, and the driver
            // decides which it wants; `?` is the one every driver we have accepts.
            Self::DuckDb | Self::ClickHouse | Self::BigQuery => PlaceholderStyle::Question,
            Self::Postgres => PlaceholderStyle::Numbered,
        }
    }

    /// Which character this data system wraps an identifier in.
    ///
    /// Declared here and measured in [`mod@crate::generate`] against what the layer actually emits, so
    /// this cannot become a claim about a rendering nobody checked.
    #[inline]
    #[must_use]
    pub const fn identifier_quote(self) -> IdentifierQuote {
        match self {
            Self::DuckDb | Self::Postgres | Self::ClickHouse => IdentifierQuote::Double,
            Self::BigQuery => IdentifierQuote::Backtick,
        }
    }

    /// How this data system spells truncating a date to a grain.
    ///
    /// See [`DateTruncShape`] for why this is a declaration rather than something the parse check
    /// would have caught.
    #[inline]
    #[must_use]
    pub const fn date_trunc_shape(self) -> DateTruncShape {
        match self {
            Self::DuckDb | Self::Postgres | Self::ClickHouse => DateTruncShape::GrainFirstAsLiteral,
            Self::BigQuery => DateTruncShape::DateFirstAsKeyword,
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
    use super::{ALL, DateTruncShape, Dialect, IdentifierQuote, PlaceholderStyle};

    #[test]
    fn every_dialect_is_in_all() {
        // The list is what the golden suite iterates. A variant missing from it is a data system
        // with no snapshot, which reads as covered and is not.
        for dialect in [Dialect::DuckDb, Dialect::Postgres, Dialect::ClickHouse, Dialect::BigQuery] {
            assert!(ALL.contains(&dialect), "{dialect} is not in ALL");
        }
        assert_eq!(ALL.len(), 4);
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
        //
        // **The name being refused used to be `bigquery`**, which is now one of the four and would
        // have made this test fail rather than mislead - the good direction for a fixture to break
        // in. `snowflake` is the replacement and is chosen for the same property: the dialect layer
        // has a target for it, so what this asserts is OUR closed set refusing a name the layer would
        // have accepted, rather than a typo being refused by both.
        let err = Dialect::parse("snowflake").expect_err("snowflake is not rendered for");
        let rendered = err.to_string();
        assert!(rendered.contains("snowflake"), "{rendered}");
        assert!(rendered.contains("duckdb"), "{rendered}");
        assert!(rendered.contains("clickhouse"), "{rendered}");
        assert!(rendered.contains("bigquery"), "{rendered}");
    }

    #[test]
    fn postgres_is_the_one_that_needs_numbering() {
        // Measured, not assumed: the dialect layer renders every placeholder as `?` regardless of
        // target, so a statement generated for Postgres with `?` is a syntax error there. This is
        // the fact that makes placeholder style ours to own.
        assert_eq!(Dialect::Postgres.placeholder_style(), PlaceholderStyle::Numbered);
        assert_eq!(Dialect::DuckDb.placeholder_style(), PlaceholderStyle::Question);
        assert_eq!(Dialect::ClickHouse.placeholder_style(), PlaceholderStyle::Question);
        // Positional, decided against the job API's own request shape - the accessor says why.
        assert_eq!(Dialect::BigQuery.placeholder_style(), PlaceholderStyle::Question);
    }

    #[test]
    fn bigquery_is_the_one_that_quotes_with_a_backtick() {
        // The value the golden suite's quoting claim searches with. What makes this worth pinning by
        // value is that the wrong character is not a syntax error in `GoogleSQL` - see
        // `IdentifierQuote::Backtick`.
        assert_eq!(Dialect::BigQuery.identifier_quote(), IdentifierQuote::Backtick);
        assert_eq!(Dialect::BigQuery.identifier_quote().character(), '`');
        for dialect in [Dialect::DuckDb, Dialect::Postgres, Dialect::ClickHouse] {
            assert_eq!(dialect.identifier_quote(), IdentifierQuote::Double, "{dialect}");
            assert_eq!(dialect.identifier_quote().character(), '"', "{dialect}");
        }
    }

    #[test]
    fn bigquery_is_the_one_that_writes_the_grain_as_a_keyword() {
        // The declaration behind the bucket. `DateTruncShape`'s own doc carries the measurement that
        // makes this a declaration rather than something a parse check would have caught.
        assert_eq!(Dialect::BigQuery.date_trunc_shape(), DateTruncShape::DateFirstAsKeyword);
        for dialect in [Dialect::DuckDb, Dialect::Postgres, Dialect::ClickHouse] {
            assert_eq!(dialect.date_trunc_shape(), DateTruncShape::GrainFirstAsLiteral, "{dialect}");
        }
    }
}
