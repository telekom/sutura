//! What a refusal from the compile says, and what it carries: the construct it found, the shape
//! guard it failed, and the error itself.
//!
//! Its own module for two reasons. The plain one is size: `expression.rs` is a thousand-line file by
//! the gate that measures it, and the refusal vocabulary is the half of it a reader consults rather
//! than follows.
//!
//! The one worth stating is that this is where the shape rule lives. **Every field here is the value
//! and never prose about it.** The dialect word is a [`DialectTag`], which is what every construction
//! site already holds; the two refusals that name a *set* carry the set. The joins below are how a
//! message reads and not what it is - a caller wanting the sentence has `Display`, and a caller
//! wanting the list has the list, where before it had to split a message on `", "`.

use sutura_domain::expression::DialectTag;
use sutura_domain::model::TableName;

use super::WRAPPER;
use super::vocabulary::UNKNOWN_FUNCTION_WHY;
use crate::dialect::Dialect;

/// A construct an authored fragment may not contain, and why.
///
/// Every variant is a refusal a compile can produce, and the reason is carried with it rather than
/// left in a design document: a refusal that names a construct without saying why sends an author to
/// read this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Construct {
    /// A `SELECT`, a subquery, a set operation.
    Query,
    /// A named table.
    TableReference,
    /// A schema or data statement inside an expression.
    ///
    /// **The one construct here no fragment reaches**, and `super::dialect_layer_refusal` carries
    /// the measurement and the argument for keeping the guard.
    SchemaStatement,
    /// `*`, either as a node or as `COUNT(*)`'s flag.
    Star,
    /// `?` or `$1`.
    BindParameter,
    /// A node the generator emits with no handling at all.
    Opaque,
    /// `t.column`.
    QualifiedColumn,
    /// A function name with a schema on it.
    QualifiedFunctionName,
    /// Anything that reads a date or a time.
    DateTimeFunction,
    /// A called function whose name is not in the allowlist.
    UnknownFunction,
    /// `FILTER (WHERE ..)` on an aggregate.
    AggregateFilter,
    /// A comment inside the fragment.
    Comment,
    /// A row constructor, which is also how `COUNT(DISTINCT a, b)` parses.
    RowConstructor,
    /// `/` whose divisor is not a `NULLIF`.
    UnguardedDivision,
    /// `//`, or any integer division node.
    IntegerDivision,
    /// `x IS TRUE`, `x IS FALSE`, `x IS <expr>`.
    IsTrue,
    /// A fragment that aggregates nothing.
    NotAggregated,
}

impl Construct {
    /// The name a refusal prints.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Query => "a subquery",
            Self::TableReference => "a table reference",
            Self::SchemaStatement => "a schema statement",
            Self::Star => "a star",
            Self::BindParameter => "a bind placeholder",
            Self::Opaque => "an unhandled node",
            Self::QualifiedColumn => "a qualified column reference",
            Self::QualifiedFunctionName => "a schema-qualified function name",
            Self::DateTimeFunction => "a date or time function",
            Self::UnknownFunction => "a function outside the allowed set",
            Self::AggregateFilter => "FILTER (WHERE ..)",
            Self::Comment => "a comment",
            Self::RowConstructor => "a row constructor, or a multi-argument DISTINCT",
            Self::UnguardedDivision => "an unguarded division",
            Self::IntegerDivision => "an integer division",
            Self::IsTrue => "IS TRUE",
            Self::NotAggregated => "no aggregate",
        }
    }

    /// Why it is refused. One sentence, and it is the whole value of the refusal.
    #[inline]
    pub const fn why(self) -> &'static str {
        match self {
            Self::Query | Self::TableReference => {
                "a measure expression may read only the columns its own model declares; a query or a table \
                 name inside one reads data the plan never granted"
            }
            Self::SchemaStatement => "a statement is not an expression, and this one is neither",
            Self::Star => {
                "a star counts join products rather than facts once a dimension is joined, which is why the \
                 closed vocabulary makes a count name its column"
            }
            Self::BindParameter => {
                "the plan owns parameter positions, and a placeholder from a catalog file would shift every \
                 value bound after it"
            }
            Self::Opaque => "the generator writes this node out unchanged, so nothing checks what reaches the target",
            Self::QualifiedColumn => {
                "the fragment names columns of the metric's own model and the compile qualifies them; a \
                 qualifier written by hand can name a table the plan did not join"
            }
            Self::QualifiedFunctionName => "a schema on a function name reaches a schema the model does not declare",
            Self::DateTimeFunction => {
                "truncation is the generator's, from the question's grain, so no metric needs one - and every \
                 dialect spells the rest differently: `generate_date_trunc` special-cases exactly two dialect \
                 families, which means a third compiled dialect would silently get its argument order wrong"
            }
            Self::UnknownFunction => UNKNOWN_FUNCTION_WHY,
            Self::AggregateFilter => {
                "`aggregate_filter_supported` is set false by six dialects and is never read anywhere in the \
                 dialect layer, so FILTER is emitted unconditionally for every target - including the six \
                 that cannot run it"
            }
            Self::Comment => {
                "a `-- ..` comment is re-emitted as a `/* .. */` one INTO the statement, so text a \
                 catalog wrote ends up between our own generated tokens; the generator does escape a \
                 closing `*/` into `* /`, measured, and a measure has no reason to carry prose that \
                 the document around it cannot hold instead - and an UNTERMINATED `/*` is refused as \
                 a comment too, because the tokenizer discards it together with everything the author \
                 wrote after it and leaves nothing in the tree to say so. The check is over the TEXT \
                 and asks only whether `/*`, `*/` or `--` appears, so a string literal holding one of \
                 the three is refused as well: that is deliberate, and the alternative is a second \
                 tokenizer here that would have to agree with the first about dollar-quoting"
            }
            Self::RowConstructor => {
                "`COUNT(DISTINCT a, b)` is rewritten into a `CASE WHEN .. IS NULL` form for `DuckDB` and \
                 Postgres and left alone for `ClickHouse` - measured, and DuckDB to DuckDB is one of the \
                 pairs that rewrites - so the same fragment counts different things per target"
            }
            Self::UnguardedDivision => {
                "a bare `/` between aggregates gains a cast for one target and not another depending on which \
                 dialect is read as the source, so the NUMBER depends on the source dialect; wrap the divisor \
                 in `NULLIF(divisor, 0)`, which suppresses the rewrite and says what a zero denominator means"
            }
            Self::IntegerDivision => "integer division truncates, and a ratio that returns a whole number is wrong",
            Self::IsTrue => "`IS TRUE` is not portable across the targets this build renders for",
            Self::NotAggregated => {
                "every question about a metric is grouped, so a fragment that aggregates nothing is a bare \
                 column in a grouped projection"
            }
        }
    }
}

impl core::fmt::Display for Construct {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which of the four shape guards a fragment failed.
///
/// Named individually because each one is a different mistake: two projections is a comma somebody
/// meant as an argument separator, a `FROM` is a whole query pasted into a measure, and an alias is
/// a habit from writing `SELECT` lists. "Not one expression" would send all three to read a grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// More than one statement: a `;` in the fragment.
    ManyStatements,
    /// The wrapper did not come back as a `SELECT`. A set operation is the reachable case:
    /// `SUM(x) UNION SELECT 1` parses as a `Union`, not as a projection.
    NotASelect,
    /// Not exactly one projected expression.
    ManyExpressions,
    /// A `FROM` clause.
    CarriedFrom,
    /// An `AS name`.
    CarriedAlias,
    /// Any other clause on the wrapper's `SELECT`, which taking the projection would DISCARD.
    ///
    /// The one that needed measuring rather than reasoning. `SELECT 1 WHERE true` is legal in the
    /// authoring dialect with no `FROM` at all, so `SUM(x) WHERE secret = 1` parses as one statement
    /// with one projection and no `FROM` - it passes every other guard here - and taking
    /// `expressions[0]` throws the `WHERE` away. The metric would then be certified as `SUM(x)`,
    /// silently, over a predicate its author wrote and nobody removed on purpose. The same holds for
    /// `GROUP BY`, `HAVING`, `QUALIFY`, `ORDER BY`, `LIMIT`, `WINDOW` and a leading `DISTINCT`, all
    /// confirmed to parse and all confirmed to be dropped.
    CarriedClause,
}

impl Shape {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ManyStatements => "more than one statement",
            Self::NotASelect => "not a projection",
            Self::ManyExpressions => "more than one expression",
            Self::CarriedFrom => "a FROM clause",
            Self::CarriedAlias => "an alias",
            Self::CarriedClause => "a clause that is not the expression",
        }
    }
}

/// Why an authored expression could not be compiled.
///
/// **All of these are load failures.** A catalog that produces one does not serve; there is no
/// degraded mode in which the metric is skipped and the rest is answered, because a metric that is
/// present in a bundle and unanswerable is a metric an agent will ask about.
///
/// **Four of them cannot be produced by any fragment, and each says so on itself rather than here:**
/// [`Self::Qualify`], [`Self::Unrenderable`], [`Self::Render`] and [`Self::RenderedDoesNotParse`]
/// each need a defect in the dialect layer, and **not the same defect** - which is why the argument
/// is on the variant and not summarised here. [`Self::Qualify`] needs that layer's own transformer to
/// violate one of its own invariants; [`Self::Unrenderable`] and [`Self::Render`] are ruled out by
/// construction, because this module's caps sit under the layer's complexity guard and no dialect
/// configuration raises its unsupported level; and [`Self::RenderedDoesNotParse`] is **not** ruled
/// out by construction at all - it is the load-time net for a generator that emits text its own
/// parser rejects, which is the reason it is a check here rather than a test. They exist
/// because the calls they wrap return a `Result` and this crate may not `unwrap` one, and what is
/// pinned about them is the wiring - the fields, and that the cause survives `#[source]` - not a
/// refusal a catalog can provoke. `tests::the_four_refusals_only_a_dialect_layer_defect_can_produce`
/// is that test, and it is named for what it is so that nobody reads it as coverage of an input.
///
/// **Every field is the value, never prose about it**, and that is this enum's one shape rule. The
/// dialect word a refusal is about is a [`DialectTag`] and not a `String`, because that is what every
/// construction site already holds; the two refusals that name a *set* carry the set rather than a
/// sentence built from it. A caller that wants the sentence gets it from `Display`, and a caller that
/// wants the list has it - where before, recovering "which dialects was this authored for" meant
/// splitting a message on `", "`, which is a contract nothing checks and a format edit breaks.
#[derive(Debug, thiserror::Error)]
pub enum ExpressionError {
    /// A dialect word that is not one this build renders for. Refused rather than ignored: a
    /// `postgresql:` beside a `portable:` would otherwise be a variant that is silently never
    /// chosen, and the author would never learn that Postgres got the portable fragment.
    ///
    /// `{:?}` on the word rather than `{}`, and it is the only variant that does: a tag differing
    /// from a real one by a trailing space is the mistake this refusal is most often about, and
    /// unquoted it reads as though the name were right.
    #[error("{:?} is not a data system this build renders for; the choices are {}, or {portable}", tag.as_str(), joined_dialects(choices))]
    UnknownDialect {
        tag: DialectTag,
        choices: Vec<Dialect>,
        portable: &'static str,
    },
    /// No exact fragment and no `portable` one. The refusal wren's importer does not have.
    #[error(
        "no fragment for {dialect}: authored for {}, and none of those is {dialect} or {portable}",
        joined_tags(authored_for)
    )]
    NoFragment {
        dialect: Dialect,
        authored_for: Vec<DialectTag>,
        portable: &'static str,
    },
    #[error("the {tag} fragment is not SQL: line {line}, column {column}")]
    Unparsable {
        tag: DialectTag,
        line: usize,
        column: usize,
        #[source]
        cause: polyglot_sql::Error,
    },
    #[error("the {tag} fragment is not one expression: it carries {}", shape.as_str())]
    NotOneExpression { tag: DialectTag, shape: Shape },
    /// The parse succeeded and the result could not be written back out, so it cannot be shown to be
    /// the projection and nothing else. Its own variant rather than a [`Shape`], because a `Shape`
    /// carries no cause and this one has one worth keeping.
    ///
    /// **No fragment produces this.** `Generator::generate` has exactly two failure paths in the
    /// features this build compiles: the AST complexity guard, and `UnsupportedLevel::Raise` or
    /// `Immediate`. The guard's limits are a million nodes and a depth of 512 or more, and
    /// `super::parse` refuses anything past `MAX_DEPTH` - thirty-two - before it renders, over a
    /// fragment `sutura_domain` has already capped at 1024 characters; none of the three dialect
    /// configurations sets the level above `Warn`. The third path, a template re-parse inside the
    /// `DuckDB` dialect, is behind the `transpile` feature and is not compiled. Measured as well as
    /// argued: 300 fragments over the whole allowlist, in every argument shape this crate accepts,
    /// produced none of it.
    #[error("the {tag} fragment parsed and could not be rendered back, so it cannot be checked")]
    Unrenderable {
        tag: DialectTag,
        #[source]
        cause: polyglot_sql::Error,
    },
    #[error("the {tag} fragment uses {construct}, which is refused: {}", construct.why())]
    Refused { tag: DialectTag, construct: Construct },
    /// A character the pinned dialect layer's generator cannot survive.
    ///
    /// **The dialect layer panics, it does not refuse.** Measured with the fuzz harness: the
    /// pinned `polyglot-sql` 0.9.2 generator byte-slices a string without respecting character
    /// boundaries, so any fragment carrying a multi-byte UTF-8 character - `é`, a full-width
    /// identifier, or the replacement character `\u{fffd}` a lossy decode produces - reaches a
    /// `&s[..]` cut through that byte and panics with *"start byte index N is not a char
    /// boundary"*. There is no third-party error to map: it aborts, which under
    /// `panic = "abort"` is the process dying. So this crate refuses non-ASCII text before it is
    /// handed over, and the bound is the chunk this build renders for: the aggregation subset and
    /// its identifiers are ASCII by construction, and a Unicode string literal is refused rather
    /// than trusted to a generator that slices it by byte. If a future pin fixes the slicing, the
    /// bound can widen; stated as a limit now because this is a control over what the dependency
    /// can carry, not a judgement that authored SQL is ASCII.
    #[error(
        "the {tag} fragment contains {text:?}, a non-ASCII character (U+{code:04X}), which the dialect \
         layer's generator cannot render without aborting; write the expression in ASCII"
    )]
    NonAscii { tag: DialectTag, code: u32, text: char },
    #[error("the {tag} fragment reads column {column:?}, which model table {table} does not declare")]
    UnknownColumn {
        tag: DialectTag,
        column: String,
        table: TableName,
    },
    /// A called function that is not one of the names a measure may call.
    ///
    /// Its own variant rather than a bare [`Self::Refused`], because this is the one refusal whose
    /// value is a *pair*: the allowed set, which [`Construct::UnknownFunction`] carries, and the name
    /// that is not in it. A fragment may hold a dozen calls, and telling an author that one of them
    /// is unlisted without saying which sends them to read this file.
    #[error("the {tag} fragment calls {name}, which is refused: {}", Construct::UnknownFunction.why())]
    UnknownFunction { tag: DialectTag, name: String },
    /// A fragment nesting deeper than the checks can walk. See the guard in `super::parse`, in the parent module.
    #[error("the {tag} fragment nests {depth} levels deep, and at most {limit} is checked")]
    TooDeep { tag: DialectTag, depth: usize, limit: usize },
    /// A column the qualification rewrite did not reach. See `super::require_qualified`, in the parent module.
    #[error(
        "the {tag} fragment leaves column {column:?} of model table {table} unqualified, so it would bind to \
         whichever joined table has it"
    )]
    NotQualified {
        tag: DialectTag,
        column: String,
        table: TableName,
    },
    /// The qualification rewrite failed.
    ///
    /// **No fragment produces this, and the reason is exhaustive rather than empirical.**
    /// `traversal::transform_map` returns an error from two places: the closure, and three
    /// `Error::Internal` checks inside the dialect layer's own explicit-stack transformer - a result
    /// stack underflow, a child-restoration mismatch, and a final stack size that is not one. The
    /// closure `super::qualify` passes is two arms and both return `Ok`, so every remaining path is
    /// that transformer breaking its own invariant. The plumbing stays fallible because absorbing it
    /// would mean substituting something for a tree that did not rewrite, which is a measure that is
    /// not the one the catalog declares.
    #[error("the {tag} fragment could not be qualified against model table {table}")]
    Qualify {
        tag: DialectTag,
        table: TableName,
        #[source]
        cause: polyglot_sql::Error,
    },
    /// The target's generator refused the tree.
    ///
    /// **No fragment produces this**, for the reason [`Self::Unrenderable`] sets out - the two are
    /// the same call with a different generator configuration, and `Unrenderable` runs first over
    /// the same tree.
    #[error("the {tag} fragment could not be rendered for {dialect}")]
    Render {
        tag: DialectTag,
        dialect: Dialect,
        #[source]
        cause: polyglot_sql::Error,
    },
    /// The rendering came back as something its own target cannot parse. The same check the golden
    /// suite applies to every generated statement, applied here at load rather than in a test,
    /// because this is the one statement fragment whose text came from a file.
    ///
    /// **No fragment has been found that produces this, and unlike the three above it is not ruled
    /// out by construction.** It fires when one dialect's generator emits text that the same
    /// dialect's parser rejects, which is a round-trip defect in the dialect layer rather than
    /// anything a catalog controls - and this variant is the net for it, which is why it is a check
    /// at load and not a test. What was tried: 300 fragments over the whole allowlist in every
    /// argument shape this crate accepts, rendered and re-parsed for all three targets. None failed.
    /// A test could only assert it by shipping a hostile dialect, so what is asserted instead is the
    /// wiring - the fields, and that the parser's own error survives as the source.
    #[error("the {tag} fragment rendered for {dialect} as SQL that {dialect} cannot parse: {sql}")]
    RenderedDoesNotParse {
        tag: DialectTag,
        dialect: Dialect,
        sql: String,
        #[source]
        cause: polyglot_sql::Error,
    },
}

/// The dialect words a refusal lists, as one sentence.
///
/// The LIST is what the variant carries; this is only how it reads. Both refusals that name a set
/// join it here rather than at the construction site, which is the whole difference between an error
/// whose fields are the contract and one whose message is.
fn joined_dialects(dialects: &[Dialect]) -> String {
    dialects
        .iter()
        .map(|dialect| dialect.as_str())
        .collect::<Vec<&str>>()
        .join(", ")
}

/// The same, for the words a catalog wrote rather than the ones this build renders for.
fn joined_tags(tags: &[DialectTag]) -> String {
    tags.iter().map(DialectTag::as_str).collect::<Vec<&str>>().join(", ")
}

pub(super) fn refused(tag: &DialectTag, construct: Construct) -> ExpressionError {
    ExpressionError::Refused {
        tag: tag.clone(),
        construct,
    }
}

pub(super) fn not_one(tag: &DialectTag, shape: Shape) -> ExpressionError {
    ExpressionError::NotOneExpression { tag: tag.clone(), shape }
}

pub(super) fn unrenderable(tag: &DialectTag, cause: polyglot_sql::Error) -> ExpressionError {
    ExpressionError::Unrenderable { tag: tag.clone(), cause }
}

/// A parse failure, with the reported position moved back onto the author's own text.
///
/// The parser saw `SELECT {fragment}`, so on the first line its column is seven further along than
/// the author's. Reporting the wrapper's column would point at the wrong character in the file,
/// which for a one-line fragment is every fragment.
pub(super) fn unparsable(tag: &DialectTag, cause: polyglot_sql::Error) -> ExpressionError {
    let line = cause.line().unwrap_or(1);
    let reported = cause.column().unwrap_or(1);
    let column = if line == 1 {
        reported.saturating_sub(WRAPPER.len())
    } else {
        reported
    };
    ExpressionError::Unparsable {
        tag: tag.clone(),
        line,
        column,
        cause,
    }
}
