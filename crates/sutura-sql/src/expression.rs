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
//! It guarantees the fragment is one expression, over columns the metric's own model declares -
//! **every one of them carrying that model's table**, asserted after the rewrite rather than assumed
//! from it - reaching no table and no query it was not given; that every function it calls is one of
//! the names in the allowlist [`Construct::UnknownFunction`] names; that it nests no deeper than the
//! checks can walk without the stack; that none of the constructs in [`Construct`] is present; and
//! that the rendering for each target is well-formed SQL that parses in that target's dialect.
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
use polyglot_sql::tokens::TokenType;
use polyglot_sql::traversal::{self, ExpressionWalk as _};
use sutura_domain::expression::{AuthoredSql, DialectTag, SqlFragment};
use sutura_domain::model::{ColumnName, TableName};

use crate::dialect::{ALL, Dialect};
use crate::generate::dialect_type;

/// The name lists a refusal is decided by. Data, kept out of this file's way.
mod vocabulary;

/// What a refusal says and what it carries: [`refusal::Construct`], [`refusal::Shape`] and
/// [`refusal::ExpressionError`].
///
/// Public, and a module rather than a `pub use` here, for two reasons that happen to agree.
/// `cargo xtask max-lines` caps a file under `crates/` at a thousand lines and cannot exempt
/// anything, and this file was at the cap; and the refusal vocabulary is the half of the compile a
/// reader consults rather than follows, so it reads better as its own page than as the first third
/// of this one. `crate::ExpressionError` and `crate::Construct` still name the same types, from the
/// crate root, which is the path everything outside this crate uses.
pub mod refusal;

#[cfg(test)]
mod tests;

use crate::expression::refusal::{Construct, ExpressionError, Shape, not_one, refused, unparsable, unrenderable};
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

/// The deepest tree the checks will walk.
///
/// **A stack bound, not a style judgement**, and the number is the small half of the decision.
/// Measured, counting the wrapper's `SELECT` as a level: the conditional sum this hatch exists for
/// nests four, and the deepest fragment any test in this repository needs is five -
/// `SUM(x) / (NULLIF(COUNT(DISTINCT k), 0))`. Thirty-two leaves room for something nobody has
/// written yet, and it is far below the depth at which the two unguarded walks run out of stack -
/// see the guard in [`parse`] for which walks those are and what they cost.
const MAX_DEPTH: usize = 32;

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
                tag: tag.clone(),
                choices: ALL.to_vec(),
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
            authored_for: authored.tags().into_iter().cloned().collect(),
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
    // Asked of the TEXT rather than of the tree, for the same tokenizer reason as the comment
    // check below: a lossy-decode or authored non-ASCII character would otherwise travel into the
    // AST as a string literal, and the dialect layer's generator panics byte-slicing it with no
    // error to map. Refusing here keeps the crash class off the process entirely.
    if let Some(offending) = fragment.as_str().chars().find(|c| !c.is_ascii()) {
        return Err(ExpressionError::NonAscii {
            tag: tag.clone(),
            code: u32::from(offending),
            text: offending,
        });
    }
    // First, and asked of the TEXT rather than of the tree, because the tokenizer is where the
    // evidence is destroyed. See `holds_comment_delimiter`.
    if holds_comment_delimiter(fragment.as_str()) {
        return Err(refused(tag, Construct::Comment));
    }
    // Last of the three questions asked before the text is handed over, and the only one that has
    // to be: the parse it guards does not return. See `unclosed_parenthesis`.
    if let Some(column) = unclosed_parenthesis(fragment.as_str()) {
        return Err(ExpressionError::UnclosedParenthesis {
            tag: tag.clone(),
            column,
        });
    }
    let expression = parse(fragment, tag)?;
    // Before the node walk, because a FILTER's predicate IS walked as an ordinary child: without
    // this the refusal for `SUM(a) FILTER (WHERE b = 1)` would be about `b`.
    if carries(&expression, "filter") {
        return Err(refused(tag, Construct::AggregateFilter));
    }
    if carries_comment(&expression) {
        return Err(refused(tag, Construct::Comment));
    }
    refuse_nodes(&expression, tag, table, columns)?;
    if let Some(construct) = dialect_layer_refusal(&expression) {
        return Err(refused(tag, construct));
    }
    if !aggregates(&expression) {
        return Err(refused(tag, Construct::NotAggregated));
    }
    let qualified = qualify(expression, tag, table)?;
    require_qualified(&qualified, tag, table)?;
    Ok(qualified)
}

/// What the DIALECT LAYER's own classifiers say about a tree, if anything.
///
/// Asked instead of only our own name lists, so a node kind missing from `QUERY_KINDS` or
/// `TABLE_KINDS` is still refused. Its own function rather than two `if`s inside [`check`] because
/// one of the two answers is not reachable from a fragment and this is what lets a test provoke it
/// anyway - see below, and the test named for it.
///
/// **[`Construct::SchemaStatement`] cannot be produced by any fragment today, and the guard stays.**
/// Measured against the authoring dialect: `CREATE`, `ALTER` and `DROP` are all rejected outright in
/// expression position - as a bare fragment, inside a `CASE`, inside `EXISTS`, and parenthesised
/// under an operator - so no DDL node reaches a projection. The two spellings that do put one into a
/// parsed tree, `(SELECT 1 FROM (CREATE TABLE t (a INT)))` and
/// `(WITH x AS (CREATE TABLE t (a INT)) SELECT 1)`, both wrap it in a `subquery`, which
/// [`refuse_nodes`] refuses as [`Construct::Query`] one guard earlier. That makes this arm
/// defence-in-depth over an upstream parser that could widen, and **not** something to delete on the
/// strength of today's parser being strict - which is the fail-open this module exists to avoid.
fn dialect_layer_refusal(expression: &Expression) -> Option<Construct> {
    if expression.contains(traversal::is_query) {
        Some(Construct::Query)
    } else if expression.contains(traversal::is_ddl) {
        Some(Construct::SchemaStatement)
    } else {
        None
    }
}

/// Does the TEXT hold a comment delimiter anywhere at all?
///
/// Asked of the text and not of the tree, because the tokenizer is where the evidence is destroyed.
/// `SUM(mrr_eur) /* note */` is refused as a comment by the AST question; `SUM(mrr_eur) /* note` was
/// **accepted**, with everything after the `/*` swallowed by the tokenizer and nothing in the AST
/// recording that it was ever there. That is the same shape as the dropped `WHERE` that
/// [`Shape::CarriedClause`] exists for - text an author wrote, silently removed, and the remainder
/// certified under the metric's name.
///
/// **PRESENCE, and it used to be a count of `/*` against a count of `*/`. The count FAILED OPEN, and
/// this is the reproduction rather than a tidier way to write the same check.** A `*/` inside a
/// string literal balances a later unterminated `/*`, so the counts agreed and the fragment was
/// accepted with its tail gone:
///
/// ```text
/// SUM(CASE WHEN status = '*/' THEN mrr_eur END) /* SUM(customer_key) is what runs
///   -> ACCEPTED, rendered as
///      SUM(CASE WHEN "fact_subscription"."status" = '*/' THEN "fact_subscription"."mrr_eur" END)
/// ```
///
/// Every other guard passes it too, and by construction: the trailing text is not in the tree, so
/// `carries_comment` cannot see it, and the whole statement and the projection render identically, so
/// [`Shape::CarriedClause`] cannot either. `'a*/b'` does the same with the delimiter buried mid-word.
///
/// The two fixes offered were a string-literal skip and asking the tokenizer. Both were rejected in
/// favour of presence, and the reason is that presence needs no agreement with anybody. A skip has to
/// decide where a string literal *ends* exactly as the authoring dialect's tokenizer does, and
/// `DuckDB` has dollar-quoting - `$$ .. $$`, `$tag$ .. $tag$` - and escape-string forms, so
/// `SUM(mrr_eur) || $$'$$ /* tail` is text the tokenizer reads as a literal holding a quote followed
/// by an unterminated comment, and a one-state skip reads as a literal that never closes: the
/// delimiter is hidden again, by a construct the scanner has not been taught. That is the second
/// tokenizer this file exists not to have. Presence has no such disagreement to lose - a comment
/// delimiter the tokenizer can see has to appear in the bytes, so refusing every byte pair `/*`, `*/`
/// and `--` refuses every comment, terminated or not, wherever the tokenizer thinks it begins.
///
/// What it costs is the same class of false positive the count already had, one digraph wider: a
/// string literal holding `/*`, `*/` or `--` is refused. Stated in `docs/adr/0004` and accepted for
/// the same reason - a measure has no reason to compare a column against a comment delimiter, and
/// none of the three has any other meaning in SQL, because `--` between two operands is a comment
/// too. [`carries_comment`] stays, over the AST: this covers the delimiters and that covers the seven
/// field names, and neither is the other's proof.
fn holds_comment_delimiter(text: &str) -> bool {
    text.contains("/*") || text.contains("*/") || text.contains("--")
}

/// Where the fragment opens a parenthesis it never closes, if it does.
///
/// **The parse this guards does not fail, it does not RETURN**, which is what makes this the one
/// text-level question that is load-bearing rather than a refusal about quality. Measured against
/// the pinned 0.9.2 and reduced from a fuzz artifact to six characters, `a.:S1(`: the argument loop
/// in `Parser::parse_data_type` breaks only on `check(TokenType::RParen)`, that answers `false` at
/// the end of the token stream, and `advance()` past the end returns the last token WITHOUT moving
/// the cursor - so the loop runs forever while `*last = format!("{} {}", last, token.text)` grows a
/// string a byte at a time. Timeout and out-of-memory are the same defect at two ages, and
/// `MAX_DEPTH` cannot see it: the tree is never built. The condition refused here is the one every
/// scan-to-a-closer loop in that parser needs, so it is a bound on the class rather than on the
/// route the first artifact happened to take: `.:` reads the next word as a custom data type, and
/// `CAST(mrr_eur AS S1(9` reaches the same argument loop with no `.:` in it at all.
///
/// **Which is why the bound is not on the construct - `.:` is neither necessary nor sufficient,
/// measured on the pinned 0.9.2 rather than reasoned about.** `mrr_eur.:S1(9)` parses and returns,
/// so refusing the construct would refuse a harmless spelling; `CAST(mrr_eur AS S1(9` loops with
/// no `.:` present, so it would still miss one. The earlier wording here named `::` beside `CAST`
/// and was wrong about that half: `mrr_eur::S1(`, `mrr_eur::DECIMAL(` and `mrr_eur::STRUCT(a` all
/// error and return.
///
/// **Asked of the TOKENS the authoring dialect produces, and that is the whole of why this is not a
/// count of `(` against `)`.** A count fails open exactly the way the count in
/// [`holds_comment_delimiter`] did, one character class over: in `mrr_eur.:S1(')'` the two
/// characters balance, while the `)` is a string literal the tokenizer hands over as one token and
/// never as an `RParen` - so the count agrees and the parser is left with a parenthesis that has no
/// closer. Asking the tokenizer costs nothing that was not going to be spent, because it is the
/// tokenizer [`parse`] is about to run: there is no second scanner here to disagree with it about
/// dollar-quoting, which is the disagreement `holds_comment_delimiter` exists not to have. The
/// dialect layer's own `guard::token_guard_tests` holds this token stream and the parser's own to
/// the same parenthesis depth.
///
/// A tokenizer failure is deliberately **not** this guard's to report: [`parse`] runs the same
/// tokenizer one step later and returns its error as [`ExpressionError::Unparsable`], which
/// terminates. Measured rather than reasoned - `mrr_eur.:S1( '` does not tokenize, so this guard
/// says nothing about it and the parse it falls through to errors instead of looping. **Nothing
/// mechanical holds that**: it is true because the parse tokenizes before it descends, which is a
/// property of the pinned version rather than of its API, and no cell here can be red against a
/// base tree that has no guard at all.
fn unclosed_parenthesis(text: &str) -> Option<usize> {
    let authoring = polyglot_sql::dialects::Dialect::get(AUTHORING);
    let tokens = authoring.tokenize(text).ok()?;
    // The columns of the parentheses still open, innermost last. A depth counter would answer
    // *whether* and this answers *where*, for a refusal an author can act on.
    //
    // From the token's own `span.start`, which the dialect layer documents as a byte offset, and
    // NOT from its `span.column`: that field is one past the token here - measured, `(` at offset
    // eleven reports thirteen - so a refusal built on it would point an author at the wrong
    // character. Byte offset and character position are the same number because the guard above
    // has already refused every non-ASCII fragment.
    let mut opened: Vec<usize> = Vec::new();
    for token in &tokens {
        match token.token_type {
            TokenType::LParen => opened.push(token.span.start + 1),
            TokenType::RParen => {
                opened.pop();
            }
            _ => {}
        }
    }
    // A `)` with no opener leaves this empty, and is left to the parser: `SUM(mrr_eur))` is a parse
    // error in the authoring dialect, which returns.
    opened.first().copied()
}

/// Does this fragment aggregate anything?
///
/// Two questions, and the second one exists because the first classifies by node KIND.
/// `contains_aggregate` is true for `SUM(x)` and for `sumIf(x, p)` - each of those gets a node the
/// dialect layer recognises - and **false** for `uniqExact(k)`, which arrives as a plain `Function`.
/// `uniqExact` is a real `ClickHouse` aggregate and precisely the case a per-dialect variant exists
/// for, so refusing it as "no aggregate" was wrong. The allowlist's own mark answers for the names
/// that classifier has no node for, and it is the only place an aggregate name is written down: the
/// kinds the dialect layer already knows are not restated.
fn aggregates(expression: &Expression) -> bool {
    traversal::contains_aggregate(expression)
        || traversal::contains_window_function(expression)
        || expression.contains(|node| called_name(node).is_some_and(vocabulary::aggregates))
}

/// The name of a generic call, or `None` for a node that is not one.
///
/// **Both spellings, and that is not belt-and-braces.** An arbitrary name becomes an
/// `AggregateFunction` rather than a `Function` as soon as the call carries a `FILTER`, an
/// `IGNORE NULLS` or a `WITHIN GROUP` - measured in the authoring dialect's parser - so a check that
/// looked only at `Function` would have a bypass spelled `getenv('X') IGNORE NULLS`.
fn called_name(node: &Expression) -> Option<&str> {
    match *node {
        Expression::Function(ref call) => Some(call.name.as_str()),
        Expression::AggregateFunction(ref call) => Some(call.name.as_str()),
        _ => None,
    }
}

/// Every node walked once: what it is, and whether the column it names is declared.
fn refuse_nodes(
    expression: &Expression,
    tag: &DialectTag,
    table: &TableName,
    columns: &BTreeSet<ColumnName>,
) -> Result<(), ExpressionError> {
    for node in expression.dfs() {
        if let Some(construct) = node_refusal(node) {
            return Err(node_error(tag, construct, node));
        }
        if let Expression::Column(column) = node {
            let name = column.name.name.as_str();
            if !columns.iter().any(|declared| declared.as_str() == name) {
                return Err(ExpressionError::UnknownColumn {
                    tag: tag.clone(),
                    column: String::from(name),
                    table: table.clone(),
                });
            }
        }
    }
    Ok(())
}

/// The refusal for one node: the construct, and for an unlisted call the name as well.
fn node_error(tag: &DialectTag, construct: Construct, node: &Expression) -> ExpressionError {
    match (construct, called_name(node)) {
        (Construct::UnknownFunction, Some(name)) => ExpressionError::UnknownFunction {
            tag: tag.clone(),
            name: String::from(name),
        },
        _ => refused(tag, construct),
    }
}

/// Every column reference carries the model's table - the POSTCONDITION of [`qualify`], asserted.
///
/// Checked rather than trusted, because the rewrite cannot be relied on to have run.
/// `traversal::transform_map` dispatches on a hardcoded list of node kinds and a kind it has no arm
/// for is returned untouched, children and all - its own comment says so, and `WithinGroup`'s adds
/// that it does not descend into `this`. The unknown-column check is a `dfs` walk, which uses the
/// WIDER coverage of the traversal API. So the check saw columns the rewrite did not touch, and eight
/// measured fragments passed every guard with a bare column in the output: `MAX(x COLLATE ..)`,
/// `SUM(x) SIMILAR TO ..`, `SUM(x) WITHIN GROUP (..)`, `ARRAY_AGG`/`LIST`/`GROUP_CONCAT` with an
/// `ORDER BY`, and both placements of `IGNORE NULLS`.
///
/// What that costs is not a syntax error at load. Measured in `DuckDB` 1.5.5 for the `COLLATE` case
/// against a joined dimension carrying the same column name: `Binder Error: Ambiguous reference to
/// column name "region"` - a metric whose load SUCCEEDED, answering the questions that join nothing
/// and failing the ones that do, which is exactly what load-time checking exists to prevent. On an
/// engine that resolves by precedence rather than erroring it is silently the wrong number, which is
/// the reason [`qualify`] exists at all.
///
/// Asserting the postcondition rather than widening the rewrite is the choice worth recording: the
/// walk belongs to the dialect layer, so a version of it that gains an arm makes more fragments
/// compile, and no fragment gets out of here with a bare column either way.
fn require_qualified(expression: &Expression, tag: &DialectTag, table: &TableName) -> Result<(), ExpressionError> {
    let unqualified = expression.dfs().find_map(|node| match *node {
        Expression::Column(ref column) if column.table.is_none() => Some(column.name.name.as_str()),
        _ => None,
    });
    if let Some(column) = unqualified {
        return Err(ExpressionError::NotQualified {
            tag: tag.clone(),
            column: String::from(column),
            table: table.clone(),
        });
    }
    Ok(())
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
    // HERE, and it has to be here: before anything CLONES or SERIALIZES this tree.
    //
    // Two of the walks over a parsed fragment are ours and neither is guarded. `Expression`'s
    // derived `Clone` recurses once per node, and `projection.clone()` at the end of this function
    // runs it; `serde_json::to_value` inside `carries` recurses once per node too, and `has_field`
    // recurses again over the `Value` it produces. The dialect layer guards its own - the parser
    // enforces `ComplexityGuardOptions`, the generator wraps generation in `stacker::maybe_grow`,
    // and its `Drop` is iterative - so these two are the whole exposure.
    //
    // A stack overflow is not a panic. `panic = "abort"` is beside the point: there is no unwinding
    // to catch and the process simply dies, so a guard in `check` would be too late because the
    // clone is in here. Measured in a debug build on a 2 MiB stack - which is what a tokio worker
    // thread and a spawned std thread both have - four ordinary fragments under half of
    // `MAX_FRAGMENT_LEN` each ABORTED the process: 500 `+ 1` terms, a 500-deep list literal, 250
    // `NOT`s and 505 parentheses. In a release build the same abort needs only 128 KiB, which is
    // musl's default main-thread stack.
    //
    // `tree_depth` is iterative, so asking the question costs no stack at all.
    let depth = statement.tree_depth();
    if depth > MAX_DEPTH {
        return Err(ExpressionError::TooDeep {
            tag: tag.clone(),
            depth,
            limit: MAX_DEPTH,
        });
    }
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
        tag: tag.clone(),
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
            tag: tag.clone(),
            dialect,
            cause,
        })?;
    polyglot_sql::dialects::Dialect::get(target)
        .parse(&format!("{WRAPPER}{sql}"))
        .map_err(|cause| ExpressionError::RenderedDoesNotParse {
            tag: tag.clone(),
            dialect,
            sql: sql.clone(),
            cause,
        })?;
    Ok(sql)
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
    // `is_none_or`, and the `None` half is the whole of a fix: this was `is_ok_and`, so a tree that
    // would not serialize was reported as carrying NOTHING - which is the one answer a refusal must
    // never guess. Latent, because the AST holds no `f64` and so nothing in it can fail to become
    // JSON today; a guard whose failure mode is "allow" is a guard that stops being one the moment
    // upstream adds a field that can.
    serialized(expression).is_none_or(|value| has_field(&value, field))
}

/// The tree as JSON, or `None` if it would not serialize. See [`carries`] for why `None` is not
/// treated as an absence of anything.
fn serialized(expression: &Expression) -> Option<serde_json::Value> {
    serde_json::to_value(expression).ok()
}

/// Does any node carry a comment, under any of the names the AST spells one?
///
/// **Asked by SUFFIX, because `trailing_comments` is one of seven.** The AST also spells a comment
/// `leading_comments`, `comments`, `pre_alias_comments`, `post_select_comments`, `operator_comments`
/// and `left_comments`, and three of those six are re-emitted INTO the statement - measured, all
/// three accepted before this: `SUM(x) /* c */ + 1` keeps its comment through `left_comments`,
/// `SUM(x) + /* c */ 1` through `operator_comments`, and `CASE /* c */ WHEN ..` moves its comment to
/// the end of the `CASE` through `comments`. Up to a thousand characters of catalog prose between our
/// own generated tokens, under a refusal that reads as if it held.
///
/// Not an injection - `*/` and `/*` are both escaped on these paths, tried and confirmed - so what
/// was broken is the refusal, not the quoting. Matching the suffix covers the four spellings nothing
/// has been measured for and a name upstream has not added yet, which is the same argument
/// [`carries`] makes for asking the serialization rather than the type.
fn carries_comment(expression: &Expression) -> bool {
    serialized(expression).is_none_or(|value| has_comment(&value))
}

fn has_comment(value: &serde_json::Value) -> bool {
    match *value {
        serde_json::Value::Object(ref map) => map
            .iter()
            .any(|(key, child)| (key.ends_with("comments") && !empty(child)) || has_comment(child)),
        serde_json::Value::Array(ref items) => items.iter().any(has_comment),
        _ => false,
    }
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
