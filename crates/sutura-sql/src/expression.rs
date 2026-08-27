//! Compile: an authored SQL fragment becomes one checked expression per target dialect.
//!
//! [`sutura_domain::expression`] holds the fragment as text and knows nothing about SQL, because the
//! domain has no parser and `cargo xtask check-boundaries` keeps it that way. This module is the
//! other half: it parses what a catalog author wrote, refuses what it will not carry, qualifies the
//! columns against the model, and renders the result for **every** dialect this build renders for -
//! all of it at catalog-compile time, none of it on the query path.
//!
//! # Parse and generate, never transpile
//!
//! `Dialect::parse` in one dialect, `Generator::generate` in another. `Dialect::transpile` is not
//! called and its feature is not compiled, and that is a stronger reason than tidiness. Under
//! `TranspileOptions::default()` the unsupported level is `Warn`: an unsupported construct returns
//! `Ok(sql)` and pushes a diagnostic into `unsupported_messages`, which `Dialect::transpile` then
//! **discards**. The default failure mode of the convenient call is therefore silent wrong output.
//! Setting the level to `Raise` is not a usable net either - measured, it errors on every non-count
//! aggregate targeting `ClickHouse` while staying silent on all four of the real breakages
//! [`Construct`] refuses below.
//!
//! Parse-and-generate also turns out to be *more* faithful for the aggregation subset: measured
//! byte-identical across all sixteen (read, write) pairs over `DuckDB`, Postgres, `ClickHouse` and
//! `BigQuery` for the conditional sum, the guarded ratio, `COUNT(DISTINCT k)`, `AVG`, `COALESCE`, a
//! bare `CASE`, `MIN`/`MAX` and `ARRAY_AGG(DISTINCT .. ORDER BY ..)`, with `CAST(.. AS DOUBLE)`
//! retargeting correctly.
//!
//! # Why a `SELECT` wrapper and not the fragment API
//!
//! The obvious way to parse a fragment - `Parser::new(dialect.tokenize(x))` then
//! `parse_expressions()` - **panics** on an empty token list, which is what `""`, whitespace-only
//! and comment-only input all produce, in every one of our dialects. Under `panic = "abort"` that is
//! a catalog file ending the process. So the fragment is parsed as `SELECT {fragment}` and the
//! projection is taken back out, with a guard on each way that can go wrong: one statement, one
//! projection, no `FROM`, no alias. [`sutura_domain::expression::SqlFragment`] refuses the empty
//! cases before they get this far, and the guards here hold for everything else.
//!
//! **The authoring dialect is `DuckDB` and may not be `ClickHouse`.** Measured: `ClickHouse`'s
//! parser accepts `SUM(x))` and `x) FROM secret --`, silently dropping the tail. That is
//! injection-shaped input passing validation. `DuckDB` rejects both.
//!
//! # What the compile guarantees, and what it does not
//!
//! It guarantees the fragment is one expression, over columns the metric's own model declares,
//! reaching no table and no query it was not given; that none of the constructs in [`Construct`] is
//! present; and that the rendering for each target is well-formed SQL that parses in that target's
//! dialect.
//!
//! It does **not** guarantee the target has the function. `MEDIAN(x)`, `COUNT_IF(x)` and
//! `PERCENTILE_CONT(..) WITHIN GROUP (..)` are emitted verbatim into Postgres and `ClickHouse` by
//! the dialect layer, and two of those three do not exist there. No per-dialect function catalogue
//! is compiled into this build, so nothing here can tell. That is precisely what the per-dialect
//! variants of [`AuthoredSql`] are for: the author names the dialect and takes the claim, and a
//! dialect with neither an exact fragment nor a `portable` one is refused rather than guessed at.

use std::collections::{BTreeMap, BTreeSet};

use polyglot_sql::DialectType;
use polyglot_sql::builder::Expr;
use polyglot_sql::expressions::{Expression, Identifier, Paren, Raw};
use polyglot_sql::traversal::{self, ExpressionWalk as _};
use sutura_domain::expression::{AuthoredSql, DialectTag, SqlFragment};
use sutura_domain::model::{ColumnName, TableName};

use crate::dialect::{ALL, Dialect};

/// The name lists a refusal is decided by. Data, kept out of this file's way.
mod vocabulary;

#[cfg(test)]
mod tests;

use crate::expression::vocabulary::{kind_refusal, name_refusal};

/// The dialect a fragment is READ in, whatever it is rendered into.
///
/// `DuckDB` and not the target, for two measured reasons. Its parser is the strict one: it rejects
/// `SUM(x))` and `x) FROM secret_table --`, which `ClickHouse`'s accepts by discarding the tail. And
/// a single authoring dialect means one fragment has one meaning - reading it in the target dialect
/// would make `SUM(a) / COUNT(b)` compile to a different number per target, which is the
/// source-dialect dependency [`Construct::UnguardedDivision`] exists to close.
const AUTHORING: DialectType = DialectType::DuckDB;

/// What the fragment is wrapped in to be parsed, and how far that shifts a reported column.
const WRAPPER: &str = "SELECT ";

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
            Self::AggregateFilter => {
                "`aggregate_filter_supported` is set false by six dialects and is never read anywhere in the \
                 dialect layer, so FILTER is emitted unconditionally for every target - including the six \
                 that cannot run it"
            }
            Self::Comment => {
                "a `-- ..` comment is re-emitted as a `/* .. */` one INTO the statement, so text a \
                 catalog wrote ends up between our own generated tokens; the generator does escape a \
                 closing `*/` into `* /`, measured, and a measure has no reason to carry prose that \
                 the document around it cannot hold instead"
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
#[derive(Debug, thiserror::Error)]
pub enum ExpressionError {
    /// A dialect word that is not one this build renders for. Refused rather than ignored: a
    /// `postgresql:` beside a `portable:` would otherwise be a variant that is silently never
    /// chosen, and the author would never learn that Postgres got the portable fragment.
    #[error("{tag:?} is not a data system this build renders for; the choices are {choices}, or {portable}")]
    UnknownDialect {
        tag: String,
        choices: String,
        portable: &'static str,
    },
    /// No exact fragment and no `portable` one. The refusal wren's importer does not have.
    #[error("no fragment for {dialect}: authored for {authored_for}, and none of those is {dialect} or {portable}")]
    NoFragment {
        dialect: Dialect,
        authored_for: String,
        portable: &'static str,
    },
    #[error("the {tag} fragment is not SQL: line {line}, column {column}")]
    Unparsable {
        tag: String,
        line: usize,
        column: usize,
        #[source]
        cause: polyglot_sql::Error,
    },
    #[error("the {tag} fragment is not one expression: it carries {}", shape.as_str())]
    NotOneExpression { tag: String, shape: Shape },
    /// The parse succeeded and the result could not be written back out, so it cannot be shown to be
    /// the projection and nothing else. Its own variant rather than a [`Shape`], because a `Shape`
    /// carries no cause and this one has one worth keeping.
    #[error("the {tag} fragment parsed and could not be rendered back, so it cannot be checked")]
    Unrenderable {
        tag: String,
        #[source]
        cause: polyglot_sql::Error,
    },
    #[error("the {tag} fragment uses {construct}, which is refused: {}", construct.why())]
    Refused { tag: String, construct: Construct },
    #[error("the {tag} fragment reads column {column:?}, which model table {table} does not declare")]
    UnknownColumn { tag: String, column: String, table: TableName },
    #[error("the {tag} fragment could not be qualified against model table {table}")]
    Qualify {
        tag: String,
        table: TableName,
        #[source]
        cause: polyglot_sql::Error,
    },
    #[error("the {tag} fragment could not be rendered for {dialect}")]
    Render {
        tag: String,
        dialect: Dialect,
        #[source]
        cause: polyglot_sql::Error,
    },
    /// The rendering came back as something its own target cannot parse. The same check the golden
    /// suite applies to every generated statement, applied here at load rather than in a test,
    /// because this is the one statement fragment whose text came from a file.
    #[error("the {tag} fragment rendered for {dialect} as SQL that {dialect} cannot parse: {sql}")]
    RenderedDoesNotParse {
        tag: String,
        dialect: Dialect,
        sql: String,
        #[source]
        cause: polyglot_sql::Error,
    },
}

/// One authored fragment, compiled for one dialect.
///
/// `authored_for` is the traceability half and is not cosmetic: with a `portable` fragment and a
/// `clickhouse` one in the same metric, "which one did this statement use" is otherwise a question
/// answered by re-deriving the resolution rule in your head.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Rendering {
    authored_for: DialectTag,
    sql: String,
}

impl Rendering {
    /// The dialect word whose fragment was chosen: the target's own, or `portable`.
    #[inline]
    pub const fn authored_for(&self) -> &DialectTag {
        &self.authored_for
    }

    /// The rendered expression, with every identifier already quoted.
    #[inline]
    pub fn sql(&self) -> &str {
        &self.sql
    }
}

/// An authored expression, compiled for every dialect this build renders for.
///
/// Complete by construction: [`compile`] returns one of these only when every entry in
/// [`crate::dialect::ALL`] resolved and rendered. So there is no per-query moment at which a target
/// turns out to have no expression - that failure has already happened, at load, naming the dialect.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CompiledExpression {
    renderings: BTreeMap<Dialect, Rendering>,
}

impl CompiledExpression {
    /// The rendering for one target. Always `Some` for a dialect in [`crate::dialect::ALL`].
    #[inline]
    pub fn for_dialect(&self, dialect: Dialect) -> Option<&Rendering> {
        self.renderings.get(&dialect)
    }

    /// Every rendering, for a snapshot a reviewer reads.
    #[inline]
    pub const fn renderings(&self) -> &BTreeMap<Dialect, Rendering> {
        &self.renderings
    }
}

/// Compiles an authored expression for every dialect this build renders for.
///
/// `columns` is the metric's own model's declared column set and `table` its table. Both are
/// required rather than optional, which is the decision worth writing down: **an unknown column
/// fails the load.** Wren's cube path does not check them - its own documentation tells the agent to
/// expect a runtime error from the warehouse - while its model path does, through a schema-driven
/// AST rewrite. The model path is right. A metric whose fragment names a column that does not exist
/// is broken whether or not anybody asks about it, and the difference between finding out at load
/// and finding out at query time is the difference between a refusal an operator can fix and a stack
/// trace an agent shows a user.
pub fn compile(
    authored: &AuthoredSql,
    table: &TableName,
    columns: &BTreeSet<ColumnName>,
) -> Result<CompiledExpression, ExpressionError> {
    for tag in authored.fragments().keys() {
        if !tag.is_portable() && Dialect::parse(tag.as_str()).is_err() {
            return Err(ExpressionError::UnknownDialect {
                tag: String::from(tag.as_str()),
                choices: choices(),
                portable: DialectTag::PORTABLE,
            });
        }
    }

    let mut renderings: BTreeMap<Dialect, Rendering> = BTreeMap::new();
    for dialect in ALL.iter().copied() {
        let chosen = resolve(authored, dialect)?;
        let checked = check(chosen.fragment, chosen.tag, table, columns)?;
        let sql = render(&checked, chosen.tag, dialect)?;
        renderings.insert(
            dialect,
            Rendering {
                authored_for: chosen.tag.clone(),
                sql,
            },
        );
    }
    Ok(CompiledExpression { renderings })
}

/// The compiled expression, as something the statement builder will accept.
///
/// `Expression::Raw` is a true verbatim passthrough - the generator writes `raw.sql` and consults no
/// dialect flag - which is what makes it right here and wrong almost everywhere else: the text was
/// produced by [`compile`] for this exact target, with identifiers already quoted, so re-parsing it
/// would be re-parsing our own output for nothing.
///
/// Wrapped in a `Paren`. `Raw` reports `is_statement()`, carries no precedence and has no children,
/// so an unparenthesised one placed under an operator would bind by text rather than by structure.
/// The parentheses cost two characters and make the embedding position-independent.
pub fn embed(rendering: &Rendering) -> Expr {
    Expr(Expression::Paren(Box::new(Paren {
        this: Expression::Raw(Raw {
            sql: String::from(rendering.sql()),
        }),
        trailing_comments: Vec::new(),
    })))
}

/// The dialect words this build renders for, for a refusal that lists them.
fn choices() -> String {
    ALL.iter().map(|d| d.as_str()).collect::<Vec<&str>>().join(", ")
}

/// Which fragment one dialect resolved to, and under which word.
///
/// A named pair rather than a tuple, because the word is the traceability half and `.0` says
/// nothing about which of the two things it is.
struct Chosen<'authored> {
    tag: &'authored DialectTag,
    fragment: &'authored SqlFragment,
}

/// Exact dialect, then `portable`, then refuse. There is no third fallback, deliberately.
fn resolve(authored: &AuthoredSql, dialect: Dialect) -> Result<Chosen<'_>, ExpressionError> {
    let exact = DialectTag::parse(dialect.as_str()).ok();
    if let Some((tag, fragment)) = exact.as_ref().and_then(|tag| authored.fragments().get_key_value(tag)) {
        return Ok(Chosen { tag, fragment });
    }
    let portable = DialectTag::portable();
    authored
        .fragments()
        .get_key_value(&portable)
        .map(|(tag, fragment)| Chosen { tag, fragment })
        .ok_or_else(|| ExpressionError::NoFragment {
            dialect,
            authored_for: authored.tags().iter().map(|t| t.as_str()).collect::<Vec<&str>>().join(", "),
            portable: DialectTag::PORTABLE,
        })
}

/// Parses one fragment and refuses everything this build will not carry.
///
/// Returns the expression with its columns qualified against the model's table, which is the only
/// rewrite performed: the fragment's meaning is otherwise exactly what was written.
fn check(
    fragment: &SqlFragment,
    tag: &DialectTag,
    table: &TableName,
    columns: &BTreeSet<ColumnName>,
) -> Result<Expression, ExpressionError> {
    let expression = parse(fragment, tag)?;
    // Before the node walk, because a FILTER's predicate IS walked as an ordinary child: without
    // this the refusal for `SUM(a) FILTER (WHERE b = 1)` would be about `b`.
    if carries(&expression, "filter") {
        return Err(refused(tag, Construct::AggregateFilter));
    }
    if carries(&expression, "trailing_comments") {
        return Err(refused(tag, Construct::Comment));
    }
    for node in expression.dfs() {
        if let Some(construct) = node_refusal(node) {
            return Err(refused(tag, construct));
        }
        if let Expression::Column(column) = node {
            let name = column.name.name.as_str();
            if !columns.iter().any(|declared| declared.as_str() == name) {
                return Err(ExpressionError::UnknownColumn {
                    tag: String::from(tag.as_str()),
                    column: String::from(name),
                    table: table.clone(),
                });
            }
        }
    }
    // Asked of the dialect layer rather than of our own lists, so a node kind missing from
    // `QUERY_KINDS` or `TABLE_KINDS` is still refused.
    if expression.contains(traversal::is_query) {
        return Err(refused(tag, Construct::Query));
    }
    if expression.contains(traversal::is_ddl) {
        return Err(refused(tag, Construct::SchemaStatement));
    }
    if !traversal::contains_aggregate(&expression) && !traversal::contains_window_function(&expression) {
        return Err(refused(tag, Construct::NotAggregated));
    }
    qualify(expression, tag, table)
}

/// `SELECT {fragment}`, with one guard for each way that can come back as something else.
///
/// The last guard is the one that makes the others sufficient. Taking `expressions[0]` out of the
/// wrapper silently discards every other clause the parser attached to that `SELECT`, and the
/// authoring dialect accepts most of them with no `FROM` - so the whole statement is rendered, the
/// projection is rendered, and the two must differ by exactly the wrapper. Checking the *rendering*
/// rather than a list of `Select` fields is what makes the guard hold for a clause nobody thought of,
/// and it compares our own generator's output against our own generator's output rather than
/// matching text against the input.
fn parse(fragment: &SqlFragment, tag: &DialectTag) -> Result<Expression, ExpressionError> {
    let wrapped = format!("{WRAPPER}{}", fragment.as_str());
    let authoring = polyglot_sql::dialects::Dialect::get(AUTHORING);
    let mut statements = authoring.parse(&wrapped).map_err(|cause| unparsable(tag, cause))?;
    if statements.len() != 1 {
        return Err(not_one(tag, Shape::ManyStatements));
    }
    let statement = statements.remove(0);
    let Expression::Select(ref select) = statement else {
        return Err(not_one(tag, Shape::NotASelect));
    };
    if select.expressions.len() != 1 {
        return Err(not_one(tag, Shape::ManyExpressions));
    }
    if select.from.is_some() {
        return Err(not_one(tag, Shape::CarriedFrom));
    }
    let Some(projection) = select.expressions.first() else {
        return Err(not_one(tag, Shape::ManyExpressions));
    };
    if matches!(*projection, Expression::Alias(_)) {
        return Err(not_one(tag, Shape::CarriedAlias));
    }
    // A failure to render either side is its own variant rather than the mismatch below, and it
    // keeps the cause: the two conclusions are the same (this cannot be shown to be one expression)
    // and the diagnosis is not.
    let whole = authoring.generate(&statement).map_err(|cause| unrenderable(tag, cause))?;
    let alone = authoring.generate(projection).map_err(|cause| unrenderable(tag, cause))?;
    if whole != format!("{WRAPPER}{alone}") {
        return Err(not_one(tag, Shape::CarriedClause));
    }
    Ok(projection.clone())
}

/// Every column reference qualified with the model's table.
///
/// The plan qualifies every column it emits for a stated reason - an unqualified column in a
/// statement that later grows a join binds to whichever table happens to have it, and that is a
/// wrong number rather than an error - and an authored fragment is not exempt from it.
///
/// Our own closure cannot fail, and the rewrite is still fallible, because the walk it runs inside
/// is the dialect layer's. A failure there is reported rather than absorbed: substituting anything
/// for a tree that did not rewrite would emit a measure that is not the one the catalog declares.
fn qualify(expression: Expression, tag: &DialectTag, table: &TableName) -> Result<Expression, ExpressionError> {
    traversal::transform_map(expression, &|node| match node {
        Expression::Column(mut column) => {
            column.table = Some(Identifier::new(table.as_str()));
            Ok(Expression::Column(column))
        }
        other => Ok(other),
    })
    .map_err(|cause| ExpressionError::Qualify {
        tag: String::from(tag.as_str()),
        table: table.clone(),
        cause,
    })
}

/// Renders for one target, then parses the result back in that target to prove it is well formed.
fn render(expression: &Expression, tag: &DialectTag, dialect: Dialect) -> Result<String, ExpressionError> {
    let target = dialect_type(dialect);
    let mut config = polyglot_sql::dialects::Dialect::get(target).generator_config().clone();
    // The same forcing `generate` applies, and for the same reason: our identifiers were never in
    // any source, so without it a column called `order` is emitted bare.
    config.always_quote_identifiers = true;
    let sql = polyglot_sql::Generator::with_config(config)
        .generate(expression)
        .map_err(|cause| ExpressionError::Render {
            tag: String::from(tag.as_str()),
            dialect,
            cause,
        })?;
    polyglot_sql::dialects::Dialect::get(target)
        .parse(&format!("{WRAPPER}{sql}"))
        .map_err(|cause| ExpressionError::RenderedDoesNotParse {
            tag: String::from(tag.as_str()),
            dialect,
            sql: sql.clone(),
            cause,
        })?;
    Ok(sql)
}

/// The dialect layer's name for a data system.
const fn dialect_type(dialect: Dialect) -> DialectType {
    match dialect {
        Dialect::DuckDb => DialectType::DuckDB,
        Dialect::Postgres => DialectType::PostgreSQL,
        Dialect::ClickHouse => DialectType::ClickHouse,
    }
}

/// Does any node in the tree carry a non-empty `field`?
///
/// **Through the serialized AST, and that is not cleverness for its own sake.** The two things asked
/// of it are FIELDS rather than nodes. `FILTER (WHERE ..)` is a `filter: Option<Expression>` on
/// eleven different aggregate structs, so the DFS walk yields its predicate as an ordinary child and
/// nothing distinguishes it - measured, and it is why the refusal for
/// `SUM(a) FILTER (WHERE b = 1)` would otherwise be about `b`. A comment is a
/// `trailing_comments: Vec<String>` on most nodes. The typed way to ask either question would be the
/// child-path API, and `ChildPathSegment` is `pub(crate)`. So the questions are asked of the
/// serialization, where a field name is visible - which also means an aggregate that gains a filter
/// field upstream is covered with no edit here.
fn carries(expression: &Expression, field: &str) -> bool {
    serde_json::to_value(expression).is_ok_and(|value| has_field(&value, field))
}

fn has_field(value: &serde_json::Value, field: &str) -> bool {
    match *value {
        serde_json::Value::Object(ref map) => map
            .iter()
            .any(|(key, child)| (key == field && !empty(child)) || has_field(child, field)),
        serde_json::Value::Array(ref items) => items.iter().any(|item| has_field(item, field)),
        _ => false,
    }
}

/// Absent, for the two shapes the fields above use: `Option` serializes as null, `Vec` as `[]`.
const fn empty(value: &serde_json::Value) -> bool {
    match *value {
        serde_json::Value::Null => true,
        serde_json::Value::Array(ref items) => items.is_empty(),
        _ => false,
    }
}

/// What is wrong with one node, if anything.
fn node_refusal(node: &Expression) -> Option<Construct> {
    match *node {
        Expression::Column(ref column) if column.table.is_some() => Some(Construct::QualifiedColumn),
        Expression::Star(_) => Some(Construct::Star),
        // `COUNT(*)` carries its star as a `bool` and projects no `Star` node, measured. Without
        // this arm the star check passes over the one spelling anybody writes.
        Expression::Count(ref count) if count.star => Some(Construct::Star),
        Expression::Tuple(_) => Some(Construct::RowConstructor),
        Expression::Div(ref division) if !guarded(&division.right) => Some(Construct::UnguardedDivision),
        Expression::Function(ref call) => name_refusal(&call.name),
        Expression::AggregateFunction(ref call) => name_refusal(&call.name),
        _ => kind_refusal(node.variant_name()),
    }
}

/// Is this divisor a `NULLIF`, through any number of parentheses?
///
/// `NULLIF` parses to a generic `Function` in the authoring dialect rather than to the typed
/// `NullIf` node, measured, so both are accepted.
fn guarded(divisor: &Expression) -> bool {
    match *divisor {
        Expression::NullIf(_) => true,
        Expression::Function(ref call) => call.name.eq_ignore_ascii_case("NULLIF"),
        Expression::Paren(ref paren) => guarded(&paren.this),
        _ => false,
    }
}

fn refused(tag: &DialectTag, construct: Construct) -> ExpressionError {
    ExpressionError::Refused {
        tag: String::from(tag.as_str()),
        construct,
    }
}

fn not_one(tag: &DialectTag, shape: Shape) -> ExpressionError {
    ExpressionError::NotOneExpression {
        tag: String::from(tag.as_str()),
        shape,
    }
}

fn unrenderable(tag: &DialectTag, cause: polyglot_sql::Error) -> ExpressionError {
    ExpressionError::Unrenderable {
        tag: String::from(tag.as_str()),
        cause,
    }
}

/// A parse failure, with the reported position moved back onto the author's own text.
///
/// The parser saw `SELECT {fragment}`, so on the first line its column is seven further along than
/// the author's. Reporting the wrapper's column would point at the wrong character in the file,
/// which for a one-line fragment is every fragment.
fn unparsable(tag: &DialectTag, cause: polyglot_sql::Error) -> ExpressionError {
    let line = cause.line().unwrap_or(1);
    let reported = cause.column().unwrap_or(1);
    let column = if line == 1 {
        reported.saturating_sub(WRAPPER.len())
    } else {
        reported
    };
    ExpressionError::Unparsable {
        tag: String::from(tag.as_str()),
        line,
        column,
        cause,
    }
}
