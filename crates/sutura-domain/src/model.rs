//! The vocabulary a semantic model is written in: the names it uses, and the closed sets it
//! chooses from.
//!
//! Every name here ends up inside a quoted identifier in generated SQL, and every aggregate and
//! grain ends up as a keyword the generator emits. So the parsing is deliberately narrow: a value
//! that would need escaping, or that names an operation we cannot spell, must not exist to be
//! passed to the generator in the first place.
//!
//! **There is no free-text expression type in this module, and that is the point.** A measure is an
//! aggregate over a column, never the string `sum(amount)`; a relationship is a pair of columns,
//! never the string `a.x = b.y`. `docs/adr/0001-first-party-semantic-models.md` argues why: a string
//! field is an escape hatch, and an escape hatch on the query path is the thing being defended
//! against.

/// The longest identifier we accept.
///
/// 63 is the tightest limit among the data systems we target, and it is a *silent* limit there:
/// a longer name is truncated rather than rejected, which turns "the column does not exist" into
/// "the query read a different column". Rejecting here is the only place that failure is visible.
const MAX_IDENTIFIER_LEN: usize = 63;

/// Why a name was rejected.
///
/// The variants carry the offending input as typed fields rather than a formatted sentence: the
/// variant is the contract and the `#[error]` text is a convenience for a human.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidIdentifier {
    /// Empty or whitespace-only. An unnamed column is a modelling mistake, not a wildcard.
    #[error("a name must not be empty")]
    Empty,
    /// Starts with something other than a letter or underscore. A leading digit is legal in some
    /// dialects and not others, so accepting it would make a model portable by luck.
    #[error("a name must start with a letter or underscore: {value:?} starts with {first:?}")]
    BadFirstCharacter { value: String, first: char },
    /// Contains a character that is not `[A-Za-z0-9_]`. `offending` is the first one, which is the
    /// one worth reporting: a message naming all of them tells the reader less.
    #[error("a name may contain only letters, digits and underscore: {value:?} contains {offending:?}")]
    IllegalCharacter { value: String, offending: char },
    /// Longer than a target data system will keep. The limit is 63 characters, the tightest among
    /// the data systems targeted here.
    #[error("a name may be at most {limit} characters, {value:?} has {len}")]
    TooLong { value: String, len: usize, limit: usize },
}

/// Parses one identifier, rejecting anything that is not one.
///
/// **Case is preserved, deliberately.** The generator always quotes identifiers, and a quoted
/// identifier is case-sensitive in every dialect we target. Normalising to lower case here would
/// make `"orderdate"` the name we emit for a column the table calls `OrderDate`, which fails at the
/// data system with a message about a column that does not exist. This is the opposite of
/// [`crate::definitions::DefinitionDigest`], where normalising is right because the value is a hash
/// rather than a reference to something else.
///
/// `pub(crate)` rather than private because [`identifier_newtype`] is used from
/// [`crate::knowledge`] as well as from here, and a macro's body resolves its names where it is
/// expanded. The alternative was a second copy of this parser in the other module, which is the one
/// thing the macro exists to prevent.
pub(crate) fn parse_identifier(raw: &str) -> Result<String, InvalidIdentifier> {
    let trimmed = raw.trim();
    let Some(first) = trimmed.chars().next() else {
        return Err(InvalidIdentifier::Empty);
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(InvalidIdentifier::BadFirstCharacter {
            value: String::from(trimmed),
            first,
        });
    }
    if let Some(offending) = trimmed.chars().find(|c| !(c.is_ascii_alphanumeric() || *c == '_')) {
        return Err(InvalidIdentifier::IllegalCharacter {
            value: String::from(trimmed),
            offending,
        });
    }
    // Every character is ASCII by now, so byte length is character length.
    if trimmed.len() > MAX_IDENTIFIER_LEN {
        return Err(InvalidIdentifier::TooLong {
            value: String::from(trimmed),
            len: trimmed.len(),
            limit: MAX_IDENTIFIER_LEN,
        });
    }
    Ok(String::from(trimmed))
}

/// Declares one identifier newtype over [`parse_identifier`].
///
/// A macro rather than five hand-written copies, and not only to save lines: the five names are
/// used interchangeably by the resolver, so any drift between their parsers would be a bug that
/// only shows up for one of them. One implementation cannot drift from itself.
///
/// **Every name inside the expansion is `$crate`-qualified, which is what lets it be used outside
/// this module.** `crate::knowledge::NoteName` is declared with it, and a `macro_rules!` body
/// resolves item names at the EXPANSION site rather than at the definition site - so an unqualified
/// `parse_identifier` there would have been a second parser to keep in step, which is the drift this
/// macro exists to make impossible.
macro_rules! identifier_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        ///
        /// Construct it with `parse`. There is no other way in: the field is private and
        /// `Deserialize` is routed through the same constructor, so a value that is not a legal
        /// identifier does not exist to be passed anywhere.
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
        // Without this the derived `Deserialize` writes straight into the private field, and the
        // one path that carries a catalog file bypasses every check above.
        #[serde(try_from = "String")]
        pub struct $name(String);

        impl $name {
            /// Parses a name, rejecting anything that is not one.
            pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier> {
                $crate::model::parse_identifier(raw.as_ref()).map(Self)
            }

            #[inline]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        /// Delegates to `parse` rather than repeating it: `serde(try_from)` above is what makes
        /// this the deserialization path, and one constructor stays the source of truth.
        impl TryFrom<String> for $name {
            type Error = $crate::model::InvalidIdentifier;

            fn try_from(raw: String) -> Result<Self, Self::Error> {
                Self::parse(raw)
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

// Path-based, so `crate::knowledge` can `use` it. `#[macro_export]` would put it on the crate root
// and in the public API, which is a wider surface than one sibling module needs.
pub(crate) use identifier_newtype;

identifier_newtype! {
    /// The name of a model: one physical table plus what we know about it.
    ModelName
}

identifier_newtype! {
    /// The name of a physical table, as the data system knows it.
    TableName
}

identifier_newtype! {
    /// The name of a column on a physical table.
    ColumnName
}

identifier_newtype! {
    /// The name of a metric: something somebody certified, such as revenue or active subscribers.
    MetricName
}

identifier_newtype! {
    /// The name of a dimension a metric declares it can be broken down by.
    DimensionName
}

identifier_newtype! {
    /// The name of a declared relationship between two models.
    RelationshipName
}

identifier_newtype! {
    /// The name of a data system a model's table lives in.
    ///
    /// A plan resolves to exactly one of these, so it is the value that decides whether a question
    /// is answerable at all rather than a routing hint.
    SourceName
}

/// The aggregates a measure may use.
///
/// A closed set, and the reason is the whole of
/// `docs/adr/0001-first-party-semantic-models.md`: an open set would be a string, and a string is
/// SQL somebody wrote. Adding a variant is a visible diff plus a generator arm plus a golden, which
/// is the review it deserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregate {
    Sum,
    Count,
    CountDistinct,
    Avg,
    Min,
    Max,
}

impl Aggregate {
    /// The name this aggregate is written with in a catalog, and in a refusal.
    ///
    /// Not the SQL spelling: how an aggregate is spelled differs per dialect and belongs to the
    /// generator. A domain type that knew the SQL would be a domain type that had opinions about
    /// `ClickHouse`.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Count => "count",
            Self::CountDistinct => "count_distinct",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
        }
    }
}

impl core::fmt::Display for Aggregate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The time resolutions an answer may be aggregated to.
///
/// Daily revenue and monthly revenue are the same metric at two grains, not two metrics, which is
/// why this is a parameter of a question rather than part of a metric's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grain {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

impl Grain {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Quarter => "quarter",
            Self::Year => "year",
        }
    }
}

impl core::fmt::Display for Grain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How many rows on each side of a relationship a join may match.
///
/// Recorded because it decides whether a join can change a measure's value. Joining to a
/// `ManyToOne` side cannot duplicate a fact row; joining to a `OneToMany` side can, which turns a
/// `sum` into a different number without any error anywhere. The resolver uses this to refuse
/// rather than to optimise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinType {
    OneToOne,
    ManyToOne,
    OneToMany,
}

impl JoinType {
    /// Can a join along this relationship duplicate rows of the model it starts from?
    ///
    /// A `sum` over duplicated rows is a wrong number that looks like a right one, so the answer
    /// decides a refusal rather than a plan detail.
    #[inline]
    pub const fn may_duplicate_rows(self) -> bool {
        matches!(self, Self::OneToMany)
    }

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OneToOne => "one_to_one",
            Self::ManyToOne => "many_to_one",
            Self::OneToMany => "one_to_many",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Aggregate, ColumnName, DimensionName, Grain, InvalidIdentifier, JoinType, MAX_IDENTIFIER_LEN, MetricName, ModelName,
        RelationshipName, SourceName, TableName,
    };

    #[test]
    fn an_ordinary_name_parses_and_round_trips() {
        let name = ColumnName::parse("order_date").expect("a plain identifier is one");
        assert_eq!(name.as_str(), "order_date");
    }

    #[test]
    fn empty_is_rejected() {
        assert_eq!(ColumnName::parse("").unwrap_err(), InvalidIdentifier::Empty);
        assert_eq!(ColumnName::parse("   ").unwrap_err(), InvalidIdentifier::Empty);
    }

    #[test]
    fn case_is_preserved_because_the_generator_quotes_identifiers() {
        // The bug this prevents: lower-casing here emits `"orderdate"` for a column the table
        // calls `OrderDate`, and a quoted identifier is case-sensitive in every target dialect.
        // The failure arrives from the data system as "column does not exist", far from here.
        let name = ColumnName::parse("OrderDate").expect("mixed case is still an identifier");
        assert_eq!(name.as_str(), "OrderDate");
        assert_ne!(name, ColumnName::parse("orderdate").expect("also an identifier"));
    }

    #[test]
    fn a_quote_is_not_an_identifier() {
        // The injection this closes. Every name reaches SQL inside a quoted identifier, so a name
        // that can carry a quote can end the quoting and start being syntax.
        assert_eq!(
            ColumnName::parse("a\"; DROP TABLE t; --").unwrap_err(),
            InvalidIdentifier::IllegalCharacter {
                value: String::from("a\"; DROP TABLE t; --"),
                offending: '"',
            }
        );
    }

    #[test]
    fn a_dot_is_not_part_of_a_name() {
        // `orders.amount` is two names and a relationship, not one column. Accepting it here would
        // let a caller name a table the model never declared.
        assert_eq!(
            ColumnName::parse("orders.amount").unwrap_err(),
            InvalidIdentifier::IllegalCharacter {
                value: String::from("orders.amount"),
                offending: '.',
            }
        );
    }

    #[test]
    fn a_leading_digit_is_rejected() {
        // Legal in ClickHouse, not in Postgres. Accepting it makes a model portable by luck.
        assert_eq!(
            ModelName::parse("1st_orders").unwrap_err(),
            InvalidIdentifier::BadFirstCharacter {
                value: String::from("1st_orders"),
                first: '1',
            }
        );
    }

    #[test]
    fn a_name_longer_than_a_target_will_keep_is_rejected() {
        // Postgres truncates at 63 rather than failing, so an over-long name silently becomes a
        // different name. This is the only place that is visible.
        let long = "a".repeat(MAX_IDENTIFIER_LEN + 1);
        assert_eq!(
            MetricName::parse(&long).unwrap_err(),
            InvalidIdentifier::TooLong {
                value: long.clone(),
                len: long.len(),
                limit: MAX_IDENTIFIER_LEN,
            }
        );
        drop(MetricName::parse("a".repeat(MAX_IDENTIFIER_LEN)).expect("exactly the limit is fine"));
    }

    #[test]
    fn surrounding_whitespace_is_not_part_of_a_name() {
        let name = MetricName::parse("  revenue\n").expect("a trimmed identifier is one");
        assert_eq!(name.as_str(), "revenue");
    }

    /// Deserialize without pulling a format crate into the domain's dependency tree: the boundary
    /// gate allowlists none, and this tests the wiring rather than a YAML parser.
    fn deserialize_column(raw: &str) -> Result<ColumnName, serde::de::value::Error> {
        use serde::Deserialize as _;
        use serde::de::IntoDeserializer as _;

        ColumnName::deserialize(String::from(raw).into_deserializer())
    }

    #[test]
    fn deserialization_goes_through_the_constructor() {
        // The hole a derived `Deserialize` leaves open, and the one that matters most here: a
        // catalog file is exactly the untrusted input this type exists to parse.
        drop(deserialize_column("a\"b").expect_err("a quote must not deserialize into a name"));
        drop(deserialize_column("").expect_err("empty must not deserialize into a name"));
        let name = deserialize_column("region").expect("a real name still deserializes");
        assert_eq!(name.as_str(), "region");
    }

    #[test]
    fn every_identifier_type_shares_one_parser() {
        // The macro exists for this: the resolver treats these names interchangeably, so a parser
        // that differed for one of them would be a bug visible only through that one.
        let bad = "a b";
        let expected = InvalidIdentifier::IllegalCharacter {
            value: String::from(bad),
            offending: ' ',
        };
        assert_eq!(ModelName::parse(bad).unwrap_err(), expected);
        assert_eq!(TableName::parse(bad).unwrap_err(), expected);
        assert_eq!(ColumnName::parse(bad).unwrap_err(), expected);
        assert_eq!(MetricName::parse(bad).unwrap_err(), expected);
        assert_eq!(DimensionName::parse(bad).unwrap_err(), expected);
        assert_eq!(RelationshipName::parse(bad).unwrap_err(), expected);
        assert_eq!(SourceName::parse(bad).unwrap_err(), expected);
    }

    #[test]
    fn an_aggregate_name_is_the_catalog_spelling_not_the_sql_one() {
        // If this ever returns `COUNT(DISTINCT ...)` the domain has started to know about SQL, and
        // the boundary gate cannot see that happen.
        assert_eq!(Aggregate::CountDistinct.as_str(), "count_distinct");
        assert_eq!(Aggregate::Sum.to_string(), "sum");
    }

    #[test]
    fn every_grain_spells_itself_the_way_a_catalog_writes_it() {
        // Every variant, not a sample, because the CRAP gate found this function at 0% coverage
        // from this crate's own tests: it is called only by the dialect renderers a crate away, so
        // a per-crate coverage run cannot see them. That is a real gap rather than an artefact.
        // These five strings are what a `grains:` list in a catalog document contains, and what a
        // refusal names when a question asks for a grain a metric never declared - so a typo here
        // is a document that stops loading and a refusal that names something nobody wrote.
        assert_eq!(Grain::Day.as_str(), "day");
        assert_eq!(Grain::Week.as_str(), "week");
        assert_eq!(Grain::Month.as_str(), "month");
        assert_eq!(Grain::Quarter.as_str(), "quarter");
        assert_eq!(Grain::Year.as_str(), "year");
        // `Display` delegates, and asserting it separately is what keeps the two from drifting
        // if somebody writes a second spelling into the formatter.
        assert_eq!(Grain::Quarter.to_string(), "quarter");
    }

    #[test]
    fn grains_are_ordered_coarsest_last() {
        // The resolver compares grains, so the derived `Ord` has to mean something. Day is finer
        // than year, and a reordering of the variants would silently invert every comparison.
        assert!(Grain::Day < Grain::Month);
        assert!(Grain::Month < Grain::Year);
    }

    #[test]
    fn only_a_one_to_many_join_can_duplicate_rows() {
        // This is what decides a refusal: a `sum` over rows a join duplicated is a wrong number
        // that raises no error anywhere.
        assert!(JoinType::OneToMany.may_duplicate_rows());
        assert!(!JoinType::ManyToOne.may_duplicate_rows());
        assert!(!JoinType::OneToOne.may_duplicate_rows());
    }
}
