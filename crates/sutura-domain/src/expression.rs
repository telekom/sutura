//! The escape hatch: SQL a catalog author wrote, for the metrics the closed vocabulary cannot say.
//!
//! [`measure`](crate::measure) is closed and stays closed. A window function, a percentile, an
//! expression over two columns - `SUM(price * quantity)` - has no [`Measure`] and cannot get one
//! without turning that vocabulary into an expression language. Two things forced a second path
//! anyway. Metrics people actually certify use those constructs; and a provider whose catalog
//! **already holds SQL per metric** - wren's cubes carry
//! `SUM(CASE WHEN status = 'active' THEN mrr_eur END)` in the file - has nothing to map onto a
//! closed vocabulary and would arrive as "unsupported" for its entire metric set.
//!
//! So this module is the hatch, and everything about its shape is arranged so that it cannot be
//! used by accident or unnoticed:
//!
//! **It is a sibling of the closed vocabulary, not a field on it.** [`Computation`] has two
//! variants, [`Computation::Measure`] is the ordinary one, and a metric that uses SQL says so in a
//! word - `authored_sql` - that a reviewer greps for and an operator can list. There is no
//! `expression:` key on a measure, no `Option<String>` beside one, and no shape in which "this
//! metric is free-text SQL" is invisible in a diff.
//!
//! **Nothing here parses.** A [`SqlFragment`] is checked for being *a plausible fragment* - present,
//! bounded, and free of the characters that make the text a reviewer reads differ from the text
//! that compiles - and nothing more. Whether it is one SQL expression, over
//! columns this model declares, reaching no table it was not given, is decided by `sutura_sql`, at
//! catalog-compile time, and a fragment that fails is a **load failure naming line and column**. The
//! domain may not do that work: it holds no SQL parser and `cargo xtask check-boundaries` keeps it
//! that way. The consequence is worth stating plainly - **a `Computation::AuthoredSql` that has not
//! been through `sutura_sql::expression::compile` is unvalidated**, and the composition root is what
//! must not skip it.
//!
//! **It is a provider CAPABILITY, not a feature every provider has.** A wren-style directory has
//! authored SQL because a person wrote the file. A metadata service that stores no executable SQL
//! per metric, and an RDF vocabulary that never will, produce [`Computation::Measure`] for every
//! metric and are complete rather than degraded. That is why the closed vocabulary is a *variant*
//! and not the `None` arm of an `Option`: "no expression" is the ordinary shape of the type.
//!
//! **Nothing here is wren-shaped.** No `base_object`, no result `type:`, no assumption that the text
//! came out of a `cubes/*.yml`. What a provider read, and out of what file, is the adapter's
//! business; what arrives here is an authored fragment per dialect and nothing else. A provider that
//! already stores per-dialect SQL maps onto [`AuthoredSql`]'s map directly, which is the strongest
//! argument for that shape over a single string.

use std::collections::BTreeMap;

use crate::measure::Measure;
use crate::text::first_invisible;

/// The longest authored fragment accepted.
///
/// A bound rather than a judgement about style: the fragment is handed to a recursive-descent parser
/// at load, and an unbounded string out of a file is an unbounded amount of work and stack.
/// Generous enough for the conditional sums and guarded ratios this exists for; anything longer is a
/// derived column that belongs upstream, which is what `docs/adr/0001` says about the whole class.
pub const MAX_FRAGMENT_LEN: usize = 1024;

/// The longest dialect word accepted.
const MAX_TAG_LEN: usize = 32;

/// Why a fragment is not one.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidFragment {
    /// Empty or whitespace-only. This is the input that made the obvious fragment API unusable:
    /// the dialect layer's `Parser::parse_expressions` panics on an empty token list, and under
    /// `panic = "abort"` a blank line in a catalog file would end the process. It is refused here,
    /// before anything can be asked of it.
    #[error("an authored expression must not be empty")]
    Empty,
    #[error("an authored expression may be at most {limit} characters, this one has {len}")]
    TooLong { len: usize, limit: usize },
    /// A control character other than tab and newline. Those two are formatting a person might use
    /// inside a long `CASE`; the rest are not text, and their likeliest origin is a paste accident
    /// or an attempt to hide part of a fragment from a reviewer's terminal.
    #[error("an authored expression may not contain the control character {code:#04x}")]
    ControlCharacter { code: u32 },
    /// A character a terminal, a diff and a browser do not render, or render in the wrong order.
    ///
    /// A second refusal beside [`Self::ControlCharacter`] rather than a wider version of it,
    /// because it is a second character class: `char::is_control` is **false** for every one of
    /// these - they are general category `Cf`, not `Cc` - so the refusal above let them through
    /// while its stated reason, "an attempt to hide part of a fragment from a reviewer's
    /// terminal", is precisely what they do.
    ///
    /// This is Trojan Source, CVE-2021-42574, pointed at a metric definition. A fragment holding a
    /// right-to-left override inside a string literal reads in a terminal, in a diff and in a pull
    /// request as `status = 'active'` and compiles to a comparison against something else, so the
    /// branch never fires and the number certified under the name a reviewer approved is zero. The
    /// digest covers text, faithfully, and the text is not what the reviewer read.
    ///
    /// **The set is [`crate::text::is_invisible`] and is not restated here.** It used to be, as a
    /// private `const fn` two hundred lines below this variant, and the second copy was missing
    /// three of the seven ranges - which is the whole argument for the module that now owns it.
    #[error("an authored expression may not contain the invisible or direction-changing character {code:#06x}")]
    InvisibleCharacter { code: u32 },
}

/// Why a dialect word is not one.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidDialectTag {
    #[error("a dialect must be named")]
    Empty,
    #[error("a dialect name may be at most {limit} characters, {value:?} has {len}")]
    TooLong { value: String, len: usize, limit: usize },
    #[error("a dialect name may hold only lower-case letters, digits and underscore: {value:?} holds {offending:?}")]
    IllegalCharacter { value: String, offending: char },
}

/// One authored SQL fragment, as text and nothing more.
///
/// Its own type rather than a `String` field, so the checks happen once and a value that reached
/// them cannot be confused with a string that did not. Deliberately **not** an identifier newtype:
/// the character set of SQL is not the character set of a name, and narrowing it here would reject
/// the quotes, parentheses and commas the whole feature exists to allow.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct SqlFragment(String);

impl SqlFragment {
    /// Checks that this is a plausible fragment. It does **not** check that it is SQL.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidFragment> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidFragment::Empty);
        }
        let len = trimmed.chars().count();
        if len > MAX_FRAGMENT_LEN {
            return Err(InvalidFragment::TooLong {
                len,
                limit: MAX_FRAGMENT_LEN,
            });
        }
        if let Some(offending) = trimmed.chars().find(|c| c.is_control() && *c != '\n' && *c != '\t') {
            return Err(InvalidFragment::ControlCharacter {
                code: u32::from(offending),
            });
        }
        if let Some(offending) = first_invisible(trimmed) {
            return Err(InvalidFragment::InvisibleCharacter {
                code: u32::from(offending),
            });
        }
        Ok(Self(String::from(trimmed)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Delegates to `parse`, so `serde(try_from)` above and a direct call are one code path.
impl TryFrom<String> for SqlFragment {
    type Error = InvalidFragment;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

/// Which dialect a fragment was authored for, as a word at rest.
///
/// **The domain does not own the list of data systems we render for, and that is not an oversight.**
/// `sutura_sql::dialect::Dialect` owns it, because each entry there is a claim that we generate
/// correct SQL for that system and have a golden that says so - and a second copy of the set here
/// would be one that has to be kept in step with nothing checking it, which is exactly what
/// [`crate::measure::Term`] declines to do for aggregates. So a tag is a *word* until the compile
/// step, which resolves it against the list that build actually renders for and refuses an unknown
/// one naming the choices. A `postgresql:` where `postgres:` was meant is therefore a load failure
/// and not a variant that is silently never chosen.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct DialectTag(String);

impl DialectTag {
    /// The reserved word for "every dialect this build renders for".
    ///
    /// Wren's importer calls this `ANSI_SQL`. The word here is `portable`, because the claim being
    /// made is not that the fragment is in a standard - `PERCENTILE_CONT` is in the standard and
    /// `ClickHouse` does not have it - but that its author expects it to work on every target. It is
    /// the author's claim, and the compile step checks the half of it that is checkable.
    pub const PORTABLE: &'static str = "portable";

    /// Checks that this is one dialect word. **Surrounding whitespace is a load failure, not
    /// something trimmed away**, and that is the half worth writing down.
    ///
    /// A tag is a key in [`AuthoredSql`]'s map. Trimming made `duckdb` and ` duckdb ` the same tag,
    /// and `BTreeMap`'s deserialize keeps the LAST value for a repeated key - so a document writing
    /// both had one of its two authored fragments silently discarded and the other certified, with
    /// the definition digest taken over the survivor. That is the outcome
    /// [`Computation::assemble`] refuses when a metric writes `measure` beside `authored_sql`, for
    /// the same reason: a document that writes two means one of them, and choosing certifies a
    /// number its author did not ask for. Refusing the whitespace costs the author one character.
    ///
    /// [`SqlFragment::parse`] still trims, and the asymmetry is deliberate: a fragment that differs
    /// from another only by surrounding whitespace is the same fragment, so trimming there loses
    /// nothing. Two map keys that differ only by whitespace are two keys.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidDialectTag> {
        let raw = raw.as_ref();
        // The emptiness check is the one place whitespace is still folded, so that a key holding
        // only spaces is reported as the missing name it is rather than as an illegal space.
        if raw.trim().is_empty() {
            return Err(InvalidDialectTag::Empty);
        }
        if let Some(offending) = raw
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_'))
        {
            return Err(InvalidDialectTag::IllegalCharacter {
                value: String::from(raw),
                offending,
            });
        }
        if raw.len() > MAX_TAG_LEN {
            return Err(InvalidDialectTag::TooLong {
                value: String::from(raw),
                len: raw.len(),
                limit: MAX_TAG_LEN,
            });
        }
        Ok(Self(String::from(raw)))
    }

    /// The word resolution falls back to, and the only word it falls back to.
    ///
    /// **The one constructor in this module that writes the private field without going through
    /// [`Self::parse`]**, which is the shape a newtype is supposed to make impossible - and it is
    /// written down here rather than left as an oddity a reader has to notice. It cannot go through
    /// `parse`, because `parse` is fallible and this is not: the alternatives are an `unwrap`, which
    /// the workspace denies outright, or an `Err` arm that would write the same field by another
    /// route and prove nothing.
    ///
    /// So the two agreeing is pinned by a test instead of by the type - see
    /// `portable_is_the_word_parse_would_have_produced`. What that test catches is the reachable
    /// mistake: a future tightening of `parse` - a shorter length bound, a narrower character set -
    /// that would refuse `portable` while this constructor kept minting it, leaving a value in a map
    /// key position that no catalog file could ever have written.
    pub fn portable() -> Self {
        Self(String::from(Self::PORTABLE))
    }

    #[inline]
    pub fn is_portable(&self) -> bool {
        self.0 == Self::PORTABLE
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DialectTag {
    type Error = InvalidDialectTag;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

/// The word itself, so a refusal can name the dialect a fragment was authored for.
///
/// Worth having rather than `{:?}` at each site: eleven refusals in `sutura_sql::expression` carry a
/// tag, and `DialectTag("duckdb")` is the derived `Debug` those would otherwise print into a message
/// an operator reads.
impl core::fmt::Display for DialectTag {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a set of authored fragments is not usable.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidAuthoredSql {
    /// The key was written and no fragment was given under it.
    #[error("`authored_sql` must carry at least one fragment: write `portable`, or one dialect per variant")]
    NoFragments,
}

/// Catalog-authored SQL for one metric: one fragment per dialect, and a `portable` fallback.
///
/// **A map and not a single string, because the honest answer to "it does not translate" is to say
/// so per dialect.** Wren's own `Measure` has no expression field at all, and its cube path carries
/// one string with no dialect attached to it; its OSI importer is the part that got this right, with
/// `{dialects: [{dialect: SNOWFLAKE, expression: ..}, {dialect: ANSI_SQL, ..}]}`. This is that
/// shape, as a map, so the key is unique by construction rather than by a duplicate check.
///
/// **Resolution is exact dialect, then `portable`, then refuse - and the third step is where this
/// departs from the importer it copies.** Wren falls back to the first non-empty variant. That
/// hands a Postgres query a Snowflake expression because it happened to be listed first, which is a
/// number computed by a definition nobody chose, under a certified name. Refusing names the dialect
/// and costs an operator one line in a file.
///
/// **On disk it is the map itself and not a struct holding one**, so a document writes
/// `authored_sql: { portable: .. }` rather than `authored_sql: { fragments: { portable: .. } }`. The
/// serde route is `try_from`/`into` rather than `transparent`, for the reason
/// [`crate::measure::Term`]'s on-disk representation gives: `transparent` writes straight past
/// [`AuthoredSql::new`], so the empty-map refusal would hold for a constructor call and not for the
/// one path that actually carries a catalog file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "BTreeMap<DialectTag, SqlFragment>", into = "BTreeMap<DialectTag, SqlFragment>")]
pub struct AuthoredSql {
    fragments: BTreeMap<DialectTag, SqlFragment>,
}

impl AuthoredSql {
    /// Takes the authored fragments, refusing an empty set.
    ///
    /// `BTreeMap` for the reason [`crate::catalog::Definitions`] uses one: the definition digest is
    /// taken over the serialized form, and a map that serialized in hash order would move the digest
    /// without the catalog moving.
    pub fn new(fragments: BTreeMap<DialectTag, SqlFragment>) -> Result<Self, InvalidAuthoredSql> {
        if fragments.is_empty() {
            return Err(InvalidAuthoredSql::NoFragments);
        }
        Ok(Self { fragments })
    }

    /// One authored fragment per dialect word, in a canonical order.
    #[inline]
    pub const fn fragments(&self) -> &BTreeMap<DialectTag, SqlFragment> {
        &self.fragments
    }

    /// The fragment authored for exactly this dialect, if there is one.
    #[inline]
    pub fn exact(&self, dialect: &DialectTag) -> Option<&SqlFragment> {
        self.fragments.get(dialect)
    }

    /// The `portable` fragment, if there is one.
    pub fn portable(&self) -> Option<&SqlFragment> {
        self.fragments.get(&DialectTag::portable())
    }

    /// Which dialects this metric was authored for, for a refusal that lists them.
    pub fn tags(&self) -> Vec<&DialectTag> {
        self.fragments.keys().collect()
    }
}

/// The deserialize path, routed through [`AuthoredSql::new`] so the empty-map refusal holds for a
/// catalog file and not only for a constructor call.
impl TryFrom<BTreeMap<DialectTag, SqlFragment>> for AuthoredSql {
    type Error = InvalidAuthoredSql;

    fn try_from(fragments: BTreeMap<DialectTag, SqlFragment>) -> Result<Self, Self::Error> {
        Self::new(fragments)
    }
}

/// The other direction, so what the digest hashes and what a catalog wrote are the same text - the
/// reason [`crate::measure::Term`] has the same pair.
impl From<AuthoredSql> for BTreeMap<DialectTag, SqlFragment> {
    fn from(authored: AuthoredSql) -> Self {
        authored.fragments
    }
}

/// How authored SQL reads to a person: which dialects it was written for, and **never the SQL**.
///
/// The omission is the invariant rather than an economy. A metric's prose reaches the agent prompt
/// and the catalog surface, where "no SQL, table name or filter expression" is a governance boundary
/// and not a preference. A reviewer reads the fragment in the catalog file or in the compiled
/// statement; a caller learns only that this metric is authored rather than composed.
impl core::fmt::Display for AuthoredSql {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("authored sql for ")?;
        for (index, tag) in self.fragments.keys().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            f.write_str(tag.as_str())?;
        }
        Ok(())
    }
}

/// Why a metric does not say what it computes.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidComputation {
    /// Neither key. Refused rather than defaulted: a metric with no measure has no number.
    #[error("a metric must say what it computes: write `measure`, or `authored_sql` for SQL the vocabulary cannot express")]
    Nothing,
    /// Both keys. Refused rather than resolved by precedence, for the reason
    /// [`crate::measure::InvalidTerm::TwoTerms`] gives: a document that writes both means one of
    /// them, and choosing would certify a number the author did not ask for. It matters more here
    /// than there, because the two would not merely differ - one is composed by the generator and
    /// the other is text somebody wrote.
    #[error("a metric computes one thing, and this one writes two: `authored_sql` cannot appear beside `measure`")]
    Both,
}

/// What a metric computes, and which of the two ways it says so.
///
/// **Externally tagged and meant to be flattened into the metric document, which is what keeps every
/// existing catalog byte-identical.** [`Computation::Measure`] serializes as `{"measure": ..}`,
/// exactly the field a metric already had, so a closed-vocabulary metric's canonical form - and so
/// its definition digest - does not move for gaining this type. An `authored_sql` metric is a new
/// key, visible in the diff, which is the whole point.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields, try_from = "ComputationInput")]
pub enum Computation {
    /// The closed vocabulary, and the ordinary case. Every metadata provider can produce this, and
    /// nothing about it is optional or degraded.
    Measure(Measure),
    /// SQL somebody wrote in the catalog, compiled at load. The exception, named so that it reads as
    /// one.
    AuthoredSql(AuthoredSql),
}

/// The same externally tagged wire form, with construction delegated to `assemble`.
/// The enum already excludes both/neither; routing keeps future constructor policy in one place.
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum ComputationInput {
    Measure(Measure),
    AuthoredSql(AuthoredSql),
}

impl Computation {
    /// Builds a computation from the two sibling keys a metric document may carry.
    ///
    /// Two `Option`s in, and a refusal for each wrong combination - the shape
    /// [`crate::measure::Term`] uses, for the same reason and one more. `deny_unknown_fields` cannot
    /// coexist with `serde(flatten)`, so the adapter declares the two keys and this decides what
    /// they mean; and putting the decision here means a second catalog adapter cannot disagree about
    /// whether writing both is an error.
    pub fn assemble(measure: Option<Measure>, authored_sql: Option<AuthoredSql>) -> Result<Self, InvalidComputation> {
        match (measure, authored_sql) {
            (Some(measure), None) => Ok(Self::Measure(measure)),
            (None, Some(authored)) => Ok(Self::AuthoredSql(authored)),
            (None, None) => Err(InvalidComputation::Nothing),
            (Some(_), Some(_)) => Err(InvalidComputation::Both),
        }
    }

    /// The closed measure, if this metric uses the closed vocabulary.
    ///
    /// Every consumer that walks columns, resolves terms or renders an aggregate reads this, and a
    /// `None` is the signal that the number comes from a compiled fragment instead. An adapter that
    /// cannot execute one has to **refuse** on that `None` rather than skip the metric.
    #[inline]
    pub const fn measure(&self) -> Option<&Measure> {
        match *self {
            Self::Measure(ref measure) => Some(measure),
            Self::AuthoredSql(_) => None,
        }
    }

    /// The authored SQL, if this metric uses the escape hatch.
    #[inline]
    pub const fn authored_sql(&self) -> Option<&AuthoredSql> {
        match *self {
            Self::Measure(_) => None,
            Self::AuthoredSql(ref authored) => Some(authored),
        }
    }

    /// The word a catalog writes, and the word an operator lists metrics by.
    ///
    /// This is the mechanism behind "a reviewer and an operator must be able to see which metrics
    /// use the hatch": one accessor over the pinned definitions, rather than a grep over files.
    #[inline]
    pub const fn kind(&self) -> &'static str {
        match *self {
            Self::Measure(_) => "measure",
            Self::AuthoredSql(_) => "authored_sql",
        }
    }
}

impl TryFrom<ComputationInput> for Computation {
    type Error = InvalidComputation;

    fn try_from(input: ComputationInput) -> Result<Self, Self::Error> {
        match input {
            ComputationInput::Measure(measure) => Self::assemble(Some(measure), None),
            ComputationInput::AuthoredSql(authored) => Self::assemble(None, Some(authored)),
        }
    }
}

/// Vocabulary for the closed case, dialect words for the hatch, and SQL in neither.
impl core::fmt::Display for Computation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Measure(ref measure) => write!(f, "{measure}"),
            Self::AuthoredSql(ref authored) => write!(f, "{authored}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        AuthoredSql, Computation, DialectTag, InvalidAuthoredSql, InvalidComputation, InvalidDialectTag, InvalidFragment,
        MAX_FRAGMENT_LEN, SqlFragment,
    };
    use crate::measure::{AggregatedColumn, Measure, Term};
    use crate::model::{Aggregate, ColumnName};
    use crate::text::is_invisible;

    fn fragment(raw: &str) -> SqlFragment {
        SqlFragment::parse(raw).expect("a test fragment is a fragment")
    }

    fn tag(raw: &str) -> DialectTag {
        DialectTag::parse(raw).expect("a test tag is a tag")
    }

    fn authored(pairs: &[(&str, &str)]) -> AuthoredSql {
        let map: BTreeMap<DialectTag, SqlFragment> = pairs.iter().map(|&(d, s)| (tag(d), fragment(s))).collect();
        AuthoredSql::new(map).expect("a non-empty map is authored sql")
    }

    #[test]
    fn an_empty_fragment_is_refused_before_anything_can_parse_it() {
        // The abort risk, closed at the earliest possible point. The dialect layer's fragment API
        // panics on an empty token list, and `panic = "abort"` turns that into a dead process
        // triggered by a blank line in a catalog file.
        for raw in ["", " ", "   ", "\t", "\n", " \t\n "] {
            assert_eq!(SqlFragment::parse(raw), Err(InvalidFragment::Empty), "{raw:?}");
        }
    }

    #[test]
    fn a_fragment_is_bounded_and_holds_no_control_characters() {
        let long = "a".repeat(MAX_FRAGMENT_LEN + 1);
        assert_eq!(
            SqlFragment::parse(&long),
            Err(InvalidFragment::TooLong {
                len: MAX_FRAGMENT_LEN + 1,
                limit: MAX_FRAGMENT_LEN,
            })
        );
        // Tab and newline survive: a long `CASE` is more readable across lines, and neither can
        // hide anything from a reviewer that the file itself does not already show.
        assert_eq!(fragment("SUM(\n  mrr_eur\n)").as_str(), "SUM(\n  mrr_eur\n)");
        assert_eq!(
            SqlFragment::parse("SUM(mrr_eur)\u{7}"),
            Err(InvalidFragment::ControlCharacter { code: 0x07 })
        );
        // A carriage return is the one worth naming: it is what makes a terminal overwrite the
        // line it just printed, so a fragment carrying one reads as something it is not.
        assert_eq!(
            SqlFragment::parse("SUM(a)\rSUM(b)"),
            Err(InvalidFragment::ControlCharacter { code: 0x0d })
        );
    }

    #[test]
    fn an_invisible_or_direction_changing_character_is_refused_although_it_is_not_a_control_one() {
        // Trojan Source, CVE-2021-42574, against a metric definition. `char::is_control` is FALSE
        // for every character here - they are `Cf`, not `Cc` - so the control-character refusal
        // cannot see them, while the reason it gives ("an attempt to hide part of a fragment from a
        // reviewer's terminal") is exactly what they do.
        //
        // The first fragment renders in a terminal, in a diff and in a pull request as
        // `SUM(CASE WHEN status = 'active' THEN mrr_eur END)` and compares against something that
        // is not `active`, so the branch never fires and the metric certifies zero under a name a
        // reviewer approved. The definition digest covers the text faithfully; the text is not what
        // the reviewer read.
        assert_eq!(
            SqlFragment::parse("SUM(CASE WHEN status = '\u{202E}evitca\u{202C}' THEN mrr_eur END)"),
            Err(InvalidFragment::InvisibleCharacter { code: 0x202E })
        );
        // A bidirectional ISOLATE does the same job as the override and is a different code point,
        // which is why the refusal is a set of ranges rather than one character.
        assert_eq!(
            SqlFragment::parse("SUM(CASE WHEN status = '\u{2066}evitca\u{2069}' THEN mrr_eur END)"),
            Err(InvalidFragment::InvisibleCharacter { code: 0x2066 })
        );
        // And a zero-width space, which needs no reordering: two fragments a reviewer cannot tell
        // apart compare against two different strings.
        assert_eq!(
            SqlFragment::parse("SUM(CASE WHEN status = 'act\u{200B}ive' THEN mrr_eur END)"),
            Err(InvalidFragment::InvisibleCharacter { code: 0x200B })
        );
        // Every listed range, at both ends, so a typo in one of them fails here rather than at a
        // review a year from now.
        for code in [
            0x00AD_u32, 0x200B, 0x200F, 0x202A, 0x202E, 0x2060, 0x2064, 0x2066, 0x2069, 0xFEFF, 0xFFF9, 0xFFFB,
        ] {
            let offending = char::from_u32(code).expect("a listed code point is a character");
            assert_eq!(
                SqlFragment::parse(format!("SUM({offending}mrr_eur)")),
                Err(InvalidFragment::InvisibleCharacter { code }),
                "{code:#06x}"
            );
        }
        // And the neighbours of each range are NOT refused, so the refusal is the set written down
        // rather than a wider sweep that would reject ordinary text.
        for code in [
            0x00AC_u32, 0x00AE, 0x200A, 0x2010, 0x2029, 0x202F, 0x205F, 0x2065, 0x206A, 0xFEFE, 0xFF00, 0xFFFC,
        ] {
            let benign = char::from_u32(code).expect("a listed code point is a character");
            assert!(!is_invisible(benign), "{code:#06x} is not one of the invisible ones");
        }
    }

    #[test]
    fn a_dialect_tag_is_one_lower_case_word() {
        assert_eq!(tag("duckdb").as_str(), "duckdb");
        assert!(tag("portable").is_portable());
        assert!(!tag("duckdb").is_portable());
        assert_eq!(DialectTag::parse("  ").err(), Some(InvalidDialectTag::Empty));
        // Upper case is refused rather than folded. The tag is compared against
        // `sutura_sql::Dialect::as_str`, which is lower case, and folding here would mean two
        // spellings of one dialect reaching that comparison.
        assert_eq!(
            DialectTag::parse("DuckDB"),
            Err(InvalidDialectTag::IllegalCharacter {
                value: String::from("DuckDB"),
                offending: 'D',
            })
        );
        assert_eq!(
            DialectTag::parse("duck-db"),
            Err(InvalidDialectTag::IllegalCharacter {
                value: String::from("duck-db"),
                offending: '-',
            })
        );
    }

    #[test]
    fn portable_is_the_word_parse_would_have_produced() {
        // `DialectTag::portable` is the one constructor in this module that writes the private field
        // without going through `parse`, and it is the only newtype here that does. It cannot go
        // through it - `parse` is fallible and this is not - so what would otherwise be the type's
        // job is this assertion's.
        //
        // The reachable mistake it catches is a future tightening of `parse`: a shorter length
        // bound, or a character set that no longer holds every letter in the word. `portable` would
        // then be a value no catalog file could write, still being minted here and still being used
        // as the fallback key `resolve` looks up.
        assert_eq!(DialectTag::parse(DialectTag::PORTABLE), Ok(DialectTag::portable()));
        assert_eq!(DialectTag::portable().as_str(), DialectTag::PORTABLE);
        assert!(DialectTag::portable().is_portable());
        // And the same word through the only path a catalog file takes.
        let deserialized: DialectTag =
            serde_json::from_str("\"portable\"").expect("the reserved word is one a document can write");
        assert_eq!(deserialized, DialectTag::portable());
        // `Display` is the word and not the derived `Debug`, which is what eleven refusals in
        // `sutura_sql::expression` interpolate.
        assert_eq!(DialectTag::portable().to_string(), "portable");
        assert_eq!(tag("clickhouse").to_string(), "clickhouse");
    }

    #[test]
    fn two_dialect_keys_that_differ_only_by_whitespace_do_not_collapse_into_one() {
        // `parse` used to trim, which made `duckdb` and ` duckdb ` the same tag - and `BTreeMap`'s
        // deserialize keeps the LAST value for a repeated key. One of two authored fragments was
        // therefore silently discarded and the other certified, with the definition digest taken
        // over the survivor: the outcome `Computation::assemble` refuses for `measure` beside
        // `authored_sql`, arrived at without anybody writing two keys on purpose.
        assert_eq!(
            DialectTag::parse(" duckdb "),
            Err(InvalidDialectTag::IllegalCharacter {
                value: String::from(" duckdb "),
                offending: ' ',
            })
        );
        assert_eq!(
            DialectTag::parse("duckdb\t"),
            Err(InvalidDialectTag::IllegalCharacter {
                value: String::from("duckdb\t"),
                offending: '\t',
            })
        );
        // So the pair that used to collapse is a load failure on the deserialize path, which is the
        // only path a catalog file takes.
        let err = serde_json::from_str::<AuthoredSql>("{\"duckdb\":\"SUM(mrr_eur)\",\" duckdb \":\"SUM(customer_key)\"}")
            .expect_err("two keys differing by whitespace are not one dialect");
        assert!(err.to_string().contains("lower-case"), "{err}");
        // A key holding nothing but whitespace is still reported as the missing name it is rather
        // than as an illegal space, which is the one place whitespace is still folded.
        assert_eq!(DialectTag::parse("  "), Err(InvalidDialectTag::Empty));
        assert_eq!(DialectTag::parse("\t\n"), Err(InvalidDialectTag::Empty));
        // And a fragment is still trimmed, deliberately: two fragments differing only by
        // surrounding whitespace are the same fragment, where two map keys are two keys.
        assert_eq!(fragment("  SUM(mrr_eur)  ").as_str(), "SUM(mrr_eur)");
    }

    #[test]
    fn authored_sql_resolves_exact_then_portable_and_never_guesses() {
        // The departure from wren's importer, asserted: there is no "first non-empty" fallback, so
        // a dialect with neither an exact fragment nor a portable one resolves to nothing and the
        // compile step refuses naming it.
        let both = authored(&[
            ("portable", "SUM(CASE WHEN status = 'active' THEN mrr_eur END)"),
            ("clickhouse", "sumIf(mrr_eur, status = 'active')"),
        ]);
        assert_eq!(
            both.exact(&tag("clickhouse")).map(SqlFragment::as_str),
            Some("sumIf(mrr_eur, status = 'active')")
        );
        assert_eq!(both.exact(&tag("postgres")), None);
        assert!(both.portable().is_some());

        let only_one = authored(&[("clickhouse", "sumIf(mrr_eur, status = 'active')")]);
        assert_eq!(only_one.portable(), None);
        assert_eq!(only_one.exact(&tag("postgres")), None);

        assert_eq!(AuthoredSql::new(BTreeMap::new()), Err(InvalidAuthoredSql::NoFragments));
    }

    #[test]
    fn display_names_the_dialects_and_never_the_sql() {
        // The governance boundary: a metric's prose reaches the agent prompt, where no SQL may
        // appear. A `Display` that rendered the fragment would put it there by accident.
        let authored = authored(&[
            ("clickhouse", "sumIf(mrr_eur, status = 'active')"),
            ("portable", "SUM(CASE WHEN status = 'active' THEN mrr_eur END)"),
        ]);
        let rendered = authored.to_string();
        assert_eq!(rendered, "authored sql for clickhouse, portable");
        assert!(!rendered.contains("sumIf"), "{rendered}");
        assert!(!rendered.contains("CASE"), "{rendered}");
        assert!(!Computation::AuthoredSql(authored).to_string().contains("CASE"));
    }

    #[test]
    fn a_computation_is_one_of_the_two_and_says_which() {
        let measure = Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Sum,
            ColumnName::parse("mrr_eur").expect("a test column is a column"),
        )));
        let closed = Computation::assemble(Some(measure.clone()), None).expect("one key is a computation");
        assert_eq!(closed.kind(), "measure");
        assert_eq!(closed.measure(), Some(&measure));
        assert_eq!(closed.authored_sql(), None);

        let hatch =
            Computation::assemble(None, Some(authored(&[("portable", "SUM(mrr_eur)")]))).expect("one key is a computation");
        assert_eq!(hatch.kind(), "authored_sql");
        assert_eq!(hatch.measure(), None);
        assert!(hatch.authored_sql().is_some());

        assert_eq!(Computation::assemble(None, None), Err(InvalidComputation::Nothing));
        assert_eq!(
            Computation::assemble(Some(measure), Some(authored(&[("portable", "SUM(mrr_eur)")]))),
            Err(InvalidComputation::Both)
        );
    }

    #[test]
    fn a_closed_metric_serializes_exactly_as_it_did_before_this_type_existed() {
        // The digest property, asserted rather than asserted about. `Computation` is externally
        // tagged so that flattened into a metric document it produces the same `measure:` key a
        // metric already had - which is what keeps every existing definition digest where it is.
        let measure = Measure::Simple(Term::CountIf {
            column: ColumnName::parse("churned_in_month").expect("a test column is a column"),
        });
        let direct = serde_json::to_string(&measure).expect("a measure serializes");
        let wrapped = serde_json::to_string(&Computation::Measure(measure)).expect("a computation serializes");
        assert_eq!(wrapped, format!("{{\"measure\":{direct}}}"));
    }

    #[test]
    fn authored_sql_round_trips_through_its_on_disk_shape() {
        let authored = authored(&[("portable", "SUM(mrr_eur)"), ("clickhouse", "sum(mrr_eur)")]);
        let json = serde_json::to_string(&Computation::AuthoredSql(authored.clone())).expect("serializes");
        // The on-disk shape: `authored_sql` holds the dialect-to-fragment map itself, with no
        // intermediate key, so a document reads the way a wren cube's dialect list reads.
        assert_eq!(
            json,
            "{\"authored_sql\":{\"clickhouse\":\"sum(mrr_eur)\",\"portable\":\"SUM(mrr_eur)\"}}"
        );
        let back: Computation = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, Computation::AuthoredSql(authored));
    }

    #[test]
    fn a_fragment_that_is_not_text_is_refused_on_the_deserialize_path_too() {
        // The `serde(try_from)` route matters more than the constructor: a catalog file is the only
        // way a fragment gets here in production, and a derived `Deserialize` would write straight
        // into the private field.
        let err = serde_json::from_str::<SqlFragment>("\"   \"").expect_err("whitespace is not a fragment");
        assert!(err.to_string().contains("must not be empty"), "{err}");
        let err = serde_json::from_str::<DialectTag>("\"Postgres\"").expect_err("upper case is not a tag");
        assert!(err.to_string().contains("lower-case"), "{err}");
        // And the empty-map refusal holds on the deserialize path, which `serde(transparent)` would
        // have written straight past.
        let err = serde_json::from_str::<AuthoredSql>("{}").expect_err("an empty map is not authored sql");
        assert!(err.to_string().contains("at least one fragment"), "{err}");
    }
}
