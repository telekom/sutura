//! Generate: a plan becomes one statement in one dialect.
//!
//! The module that turns a plan into SQL. An adapter that executes a plan without rendering it -
//! the in-process engine - never calls anything here.
//!
//! It used to be the only module that names the dialect layer. [`crate::expression`] names it too
//! now, because compiling a catalog-authored fragment is parsing rather than rendering and the two
//! jobs share no code: this file builds an AST from a plan, that one takes an AST apart and refuses
//! most of it. A pre-1.0 API change upstream therefore touches two files in this crate, both of them
//! here rather than anywhere else.
//!
//! Six things about how the dialect layer is used, every one of them measured rather than assumed,
//! and every one of them looking right until it was rendered:
//!
//! **The fluent builder, not the AST structs.** `Expression::Select` has upwards of thirty fields
//! and no `Default`, so hand-building one is a list nobody can review. The builder panics on misuse
//! rather than returning an error, which is why every call here is on a shape that was proved to
//! work and nothing a catalog or a caller supplies changes which builder method runs.
//!
//! **Identifiers are force-quoted, aliases included.** The generator quotes only what was quoted in
//! its source, is a reserved word, or the config forces. Ours were never in any source, so a column
//! called `order` would be emitted bare. `always_quote_identifiers` covers identifiers and does
//! **not** cover aliases, which come from a separate `Identifier` whose flag we set ourselves.
//!
//! **`GROUP BY` gets the unaliased expressions.** Passing the aliased ones emits `GROUP BY x AS y`,
//! which no target accepts.
//!
//! **The time bucket is cast to a date.** `DATE_TRUNC` over a date returns a TIMESTAMP in two of the
//! five targets, so without the cast the type of the `period` column is whatever each dialect chose
//! and every adapter would need to know which. Oracle's `TRUNC` already returns a `DATE`, so the
//! cast is redundant rather than corrective there - applied anyway, because one shared cast is what
//! keeps the type uniform without a per-dialect branch to keep in step.
//!
//! **The time bucket's SPELLING is per dialect, and it is the one difference here the layer does not
//! absorb.** Three targets take `DATE_TRUNC('month', <date>)`; `BigQuery` takes
//! `DATE_TRUNC(<date>, MONTH)` - the arguments the other way round and the grain a bare keyword
//! rather than a string; Oracle takes `TRUNC(<date>, 'MM')` - a different FUNCTION, the date first
//! like `BigQuery`, but the grain a quoted format model rather than a keyword.
//! [`crate::dialect::DateTruncShape`] holds the declaration and the reason it
//! has to be one: **within one target, the parse check cannot tell the two apart.** Both shapes
//! rendered for `BigQuery` parse as `BigQuery`, so the corpus would be green on the wrong one.
//! `the_parse_check_cannot_tell_the_two_bucket_shapes_apart` is that measurement.
//!
//! **The limit on that claim, because the first version of it was too broad and a test caught it:**
//! the check does catch a statement rendered for the WRONG target, on the quoting rather than on the
//! bucket - double quotes are string delimiters in `GoogleSQL`, so a DuckDB-rendered statement fails to
//! parse as `BigQuery` at the first qualified column. `the_parse_check_does_catch_the_wrong_quote_character`
//! pins that, and the two tests together say precisely which half is covered by a mechanism and which
//! half rests on a declaration and a reviewed golden.
//!
//! **Placeholders are ours.** The dialect layer renders every placeholder as `?` whatever the target,
//! and carries a per-dialect `parameter_token` it never reads - so Postgres would be sent `?` and
//! reject it. [`crate::dialect::PlaceholderStyle`] decides.
//!
//! `transpile` is never called and is not compiled. See `clippy.toml` and the feature list in the
//! workspace manifest.

use polyglot_sql::DialectType;
use polyglot_sql::builder::{self, Expr, SelectBuilder};
use polyglot_sql::expressions::{Expression, Fetch, Literal, Ordered, Parameter, ParameterStyle, Placeholder, Raw, Tuple};
use sutura_domain::model::{Aggregate, ColumnName, Grain, JoinType, Qualification, QualifiedTable, TableName};
use sutura_domain::plan::{LegPlan, PlanBucket, PlanColumn, PlanJoin, PlanJoinKey, PlanPredicate, QueryPlan};
use sutura_domain::warehouse::ParamValue;
use sutura_domain::warehouse::cardinality::{DISTINCT_LABEL, DeclaredKey, ROWS_LABEL};

use crate::GeneratedQuery;
use crate::dialect::{DateTruncShape, Dialect, PlaceholderStyle};

/// Why a statement could not be rendered.
///
/// Not a refusal: a caller cannot cause one of these and there is nothing they could ask
/// differently. A plan that will not render is a bug here or upstream.
#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("the dialect layer could not render the statement for {dialect}")]
    Render {
        dialect: Dialect,
        #[source]
        cause: polyglot_sql::Error,
    },
    /// The builder produced something that is not an alias, so its identifier could not be quoted.
    ///
    /// Reported rather than ignored: silently emitting an unquoted alias is how a metric named
    /// `order` becomes a syntax error at the data system instead of an error here.
    #[error("an alias for {label:?} did not come back as an alias, so it could not be quoted")]
    UnquotableAlias { label: String },
    /// A table path deeper than the target resolves.
    ///
    /// **Not a refusal, and the boundary is worth being precise about.** A caller cannot cause one:
    /// there is no field on a question that names a table, so what produced this is a catalog document
    /// naming `project.dataset.table` for a data system with nowhere to put the project - which is
    /// upstream of here, exactly as this enum's own header says.
    ///
    /// **An error and not a silently-dropped qualifier**, which is the whole reason it exists: the
    /// dialect layer renders three parts for any target, so dropping the part that does not fit would
    /// read the table of that name in whatever the connection defaults to and return a plausible
    /// number under a certified metric. That is the failure issue #83 reports, moved rather than
    /// fixed.
    ///
    /// The path travels as text because that is the only thing a message can show, and it is safe to
    /// show: every part of it is a parsed name, so it carries no quote character and no value from any
    /// question.
    #[error("{table} names a {carries} path, and {dialect} resolves at most a {resolves} one")]
    QualificationUnsupported {
        dialect: Dialect,
        table: String,
        carries: Qualification,
        resolves: Qualification,
    },
    /// A plan that carries no predicate at all.
    ///
    /// Unreachable: a plan always carries the two bounds of its `TimeRange`, which cannot be
    /// unbounded. Its own variant rather than a sentinel string inside another one, because the
    /// variant is what a caller matches on - and worded exactly as `sutura_exec_datafusion`'s
    /// `NoPredicate`, so the SQL path and the engine name one condition identically rather than
    /// describing it twice.
    #[error("a plan must carry the two bounds of its range, and this one carries no predicate")]
    NoPredicate,
    /// A compound declared key's fan-out probe, for a dialect this crate has no null-safe
    /// multi-column `DISTINCT` rendering for.
    ///
    /// **An allowlist (`DuckDb`, `Postgres`, `ClickHouse`), not a denylist.** `Tuple` renders as a
    /// plain argument list, `DISTINCT a, b`, on every dialect this crate does not special-case -
    /// which is the tuple `DuckDB` and Postgres read (polyglot 0.12.0's generator rewrites it to
    /// the null-safe `CASE WHEN a IS NULL THEN NULL … ELSE (a, b) END` for both, `multi_arg_distinct:
    /// false`) and the shape `ClickHouse`'s own multi-argument `DISTINCT` already treats as one,
    /// null-safely, without any rewrite. Oracle and `BigQuery` both document `DISTINCT` as taking one
    /// expression, and `Tuple` there is `DISTINCT a, b` - a plain argument list neither grammar
    /// means by it, not the tuple this crate intends. **Not reproduced against a live instance of
    /// either** - stated from each dialect's documented `DISTINCT` syntax, not from an observed
    /// rejection. Refused here, at generation time, rather than handed to the data system as SQL
    /// that may be rejected: the boot-time probe for a `many_to_one` relationship declaring more
    /// than one join key is the only caller, so this is reachable only from a compound key on one
    /// of these dialects, and a caller cannot narrow a catalog document out of it - an operator
    /// changes the relationship or the dialect.
    #[error("a compound declared-key probe cannot be rendered for {dialect}: it accepts one column inside DISTINCT")]
    CompoundKeyProbeUnsupported { dialect: Dialect },
}

/// The dialect layer's name for a data system.
///
/// **The one mapping, and `pub(crate)` so it stays one.** `expression.rs` held a byte-identical copy
/// until a fourth dialect made the duplication visible the way duplication usually becomes visible:
/// the compiler demanded the same new arm twice, and two matches over one enum are two places for the
/// answer to differ. Adding a dialect now touches this match once.
///
/// A free function rather than a second inherent `impl Dialect`: the type is declared in
/// `dialect.rs`, which does not name this crate, and two inherent impls for one type hide half of a
/// type's methods from whoever opens the other file.
pub(crate) const fn dialect_type(dialect: Dialect) -> DialectType {
    match dialect {
        Dialect::DuckDb => DialectType::DuckDB,
        Dialect::Postgres => DialectType::PostgreSQL,
        Dialect::ClickHouse => DialectType::ClickHouse,
        Dialect::BigQuery => DialectType::BigQuery,
        Dialect::Oracle => DialectType::Oracle,
    }
}

/// A placeholder, written the way this dialect writes one.
///
/// `position` is zero-based because it indexes the parameter list; a numbered form is one-based
/// because that is what Postgres and Oracle both count from. Getting that off by one produces a
/// statement that runs and reads the wrong parameter, which is why the two are converted in exactly
/// one place.
fn placeholder(dialect: Dialect, position: usize) -> Expr {
    // Shared by `Numbered` and `Colon`: both are one-based positional parameters, differing only in
    // which character the dialect layer writes before the digits.
    let numbered = |style: ParameterStyle| {
        let index = u32::try_from(position.saturating_add(1)).unwrap_or(u32::MAX);
        Expr(Expression::Parameter(Box::new(Parameter {
            name: None,
            index: Some(index),
            style,
            quoted: false,
            string_quoted: false,
            expression: None,
        })))
    };
    match dialect.placeholder_style() {
        PlaceholderStyle::Question => Expr(Expression::Placeholder(Placeholder { index: None })),
        PlaceholderStyle::Numbered => numbered(ParameterStyle::Dollar),
        PlaceholderStyle::Colon => numbered(ParameterStyle::Colon),
    }
}

/// `"table"."column"`, as an expression the builder understands.
fn column(plan_column: &PlanColumn) -> Expr {
    qualified(plan_column.table(), plan_column.column())
}

/// A column reference, qualified by its table's own name.
///
/// **Split out of [`column`](fn@crate::generate::column) rather than duplicated**, because the key
/// probe holds a [`QualifiedTable`] and a [`ColumnName`] rather than a [`PlanColumn`] and would
/// otherwise have built the same reference a second way - which is exactly the drift `render`,
/// `aliased` and `table_path` are shared to prevent one layer down.
fn qualified(table: &TableName, column: &ColumnName) -> Expr {
    builder::col(&format!("{}.{}", table.as_str(), column.as_str()))
}

/// `expr AS "label"`, with the alias quoted.
fn aliased(inner: Expr, label: &str) -> Result<Expr, GenerateError> {
    let mut expression = builder::alias(inner, label).into_inner();
    match expression {
        Expression::Alias(ref mut alias) => {
            alias.alias.quoted = true;
        }
        _ => {
            return Err(GenerateError::UnquotableAlias {
                label: String::from(label),
            });
        }
    }
    Ok(Expr(expression))
}

/// The grain, as the argument `DATE_TRUNC` takes.
///
/// Lowercase, which is what the three string-literal targets have always been sent and what their
/// goldens carry. The keyword target uppercases it - see [`grain_keyword`].
///
/// `week` means a **Monday**-based week on every dialect this renders for, and that is measured
/// rather than assumed for the one that looked doubtful. `ClickHouse` `26.7.5.10` answers
/// `dateTrunc('week', DATE '2026-08-30')` - a Sunday - with **2026-08-24, a Monday**, matching
/// the pinned `DuckDB` and `Postgres` 17.11 (and `BigQuery`'s Monday-based `ISOWEEK`); measured on
/// 2026-08-30. The reason is in the source rather than the spelling: `ClickHouse` routes
/// `date_trunc('week')` through `toStartOfInterval`, whose weeks start on Monday, while only the
/// bare `toStartOfWeek` defaults to Sunday. So one lowercase mapping stays shared, and should a
/// dialect ever disagree it stops being shared and becomes the per-dialect shape `grain_keyword`'s
/// `ISOWEEK` arm already is.
///
/// **The limit, because this sentence should not be trusted further than the tree checks it.**
/// What the suite executes and pins for `week` is the `DuckDB` and engine legs and their agreement
/// (`differential`); the `Postgres` and `ClickHouse` legs are rendered and parse-checked only, the
/// weight `docs/adr/0003` gives the goldens, so the Monday claim for those two rests on this
/// measurement and not on a gate. The other four grains are asserted, not measured: `day`, `month`,
/// `quarter` and `year` mean the same thing in every dialect this crate renders for, and no question
/// in the corpus asks for `quarter` or `year`, so no golden renders either.
const fn unit(grain: Grain) -> &'static str {
    match grain {
        Grain::Day => "day",
        Grain::Week => "week",
        Grain::Month => "month",
        Grain::Quarter => "quarter",
        Grain::Year => "year",
    }
}

/// The grain as a BARE KEYWORD, for the target that wants one rather than a string.
///
/// **This is the one place text is written into a statement rather than bound, so the reason it is
/// not an injection is stated here rather than left to be re-derived.** `Raw` inserts its `sql` field
/// verbatim, with no quoting and no escaping, which is exactly what a keyword argument needs and
/// exactly what nothing reachable from input may be allowed to reach. What reaches it is the return
/// of an exhaustive `const` match over [`Grain`] - a closed five-variant enum in the domain - so the
/// set of strings this function can ever produce is five compile-time literals, none of which
/// contains a quote, a space or a parenthesis. A caller cannot widen it: [`Grain`] has no variant
/// carrying text, and a question names a grain by choosing one of the five.
///
/// Uppercase because that is how the target's own documentation writes a date part, and because a
/// bare lowercase `month` beside a column called `month` is needlessly hard to read in a golden.
///
/// # `Week` maps to `ISOWEEK`, and this is the arm that would otherwise be a wrong number
///
/// **Not a translation of the word, a translation of the SEMANTICS**, and the difference was measured
/// rather than reasoned about. `BigQuery`'s `WEEK` begins on **Sunday** - its own reference says
/// `WEEK` is equivalent to `WEEK(SUNDAY)` - while the pinned `DuckDB` answers
/// `DATE_TRUNC('week', DATE '2026-08-30')` (a Sunday) with **2026-08-24, a Monday**, putting that
/// Sunday in the PREVIOUS week. So the obvious mapping, `Week => "WEEK"`, buckets a Sunday's rows
/// under a different period on `BigQuery` than on the data system that vouches for acceptance - a
/// wrong number under a certified name, with nothing anywhere raising an error.
///
/// `ISOWEEK` is `BigQuery`'s Monday-based part, which is what agrees. The example corpus has asked
/// `week` since `examples/single-player/questions/data-per-subscription-by-week.yaml` landed, and a
/// golden holds the rendered keyword:
/// `crates/sutura-app/tests/snapshots/data-per-subscription-by-week__sql@bigquery.snap` pins
/// `ISOWEEK` against its `duckdb` sibling's `DATE_TRUNC('week', …)` in the same directory - the
/// comparison the limit below names. The question's other snapshots (`rows@`, `params@`,
/// `plan@markdown`, and `sutura-cli`'s own rendered statement) exercise the `week` grain end to end
/// without going through this arm's text at all, since every non-`BigQuery` dialect reaches
/// `DATE_TRUNC` through `bucket_expression`'s other branch and never calls this function.
///
/// The other four need no such care: `DAY`, `MONTH`, `QUARTER` and `YEAR` mean the same thing in every
/// dialect this crate renders for.
///
/// **The limit, stated because it is the honest shape of this claim:** what is compared is `BigQuery`
/// against `DuckDB`. Whether `ClickHouse`'s own week agrees with `DuckDB`'s is a pre-existing question
/// this arm does not touch and no test in this repository answers.
const fn grain_keyword(grain: Grain) -> &'static str {
    match grain {
        Grain::Day => "DAY",
        // See the header: `WEEK` here would be Sunday-based and disagree with every other dialect.
        Grain::Week => "ISOWEEK",
        Grain::Month => "MONTH",
        Grain::Quarter => "QUARTER",
        Grain::Year => "YEAR",
    }
}

/// The grain as a single-quoted Oracle format model, for `TRUNC`'s second argument.
///
/// A closed five-variant `const` match, for [`grain_keyword`]'s own reason: the string this can ever
/// produce is one of five compile-time literals, none of which contains a quote.
///
/// **`Week` maps to `IW`, and this is the arm that would otherwise be a wrong number, for exactly
/// the reason `grain_keyword`'s own `Week` arm names.** Oracle's SQL reference lists `WW` as "same
/// day of the week as first day of year" - a year-anchored week that drifts off Monday - and `IW` as
/// the ISO week, which is Monday-based and agrees with the pinned `DuckDB`. `DD`, `MM`, `Q` and
/// `YYYY` are the plain day/month/quarter/year models and need no such care.
///
/// **The limit, stated plainly: this mapping is read off Oracle's documented format models and not
/// measured against an instance.** PR 2 is where one exists to ask; until then this is a declaration
/// like [`grain_keyword`]'s `ISOWEEK` arm was before `DuckDB` and the engine differential existed to
/// check it against.
const fn oracle_format(grain: Grain) -> &'static str {
    match grain {
        Grain::Day => "DD",
        // See the header: `WW` here would drift off Monday and disagree with every other dialect.
        Grain::Week => "IW",
        Grain::Month => "MM",
        Grain::Quarter => "Q",
        Grain::Year => "YYYY",
    }
}

/// One aggregate over one column.
fn aggregate(kind: Aggregate, over: Expr) -> Expr {
    match kind {
        Aggregate::Sum => builder::sum(over),
        Aggregate::Count => builder::count(over),
        Aggregate::CountDistinct => builder::count_distinct(over),
        Aggregate::Avg => builder::avg(over),
        Aggregate::Min => builder::min_(over),
        Aggregate::Max => builder::max_(over),
    }
}

/// One predicate, as an expression.
fn predicate(dialect: Dialect, plan_predicate: &PlanPredicate) -> Expr {
    let col = column(plan_predicate.column());
    match *plan_predicate {
        PlanPredicate::AtOrAfter { param, .. } => col.gte(placeholder(dialect, param)),
        PlanPredicate::Before { param, .. } => col.lt(placeholder(dialect, param)),
        PlanPredicate::Equals { param, .. } => col.eq(placeholder(dialect, param)),
        PlanPredicate::NotEquals { param, .. } => col.neq(placeholder(dialect, param)),
        PlanPredicate::In { ref params, .. } => col.in_list(params.iter().map(|&index| placeholder(dialect, index))),
        PlanPredicate::NotIn { ref params, .. } => col.not_in(params.iter().map(|&index| placeholder(dialect, index))),
        PlanPredicate::IsTrue { .. } => col.is(builder::boolean(true)),
        PlanPredicate::IsNotNull { .. } => col.is_not_null(),
    }
}

/// The truncated time column, cast to a date.
///
/// One definition, shared by the whole-answer path and the leg path, because a bucket that truncated
/// differently in a leg than in a mono-source answer would make the two disagree about which month a
/// row belongs to - and the differential test compares rows rather than statements, so it would
/// report the disagreement as a wrong number.
///
/// **It takes the dialect now, and that is the fourth dialect's real cost.** Three targets spell this
/// one way and `BigQuery` spells it another, in both the argument order and the grain's form, so the
/// match below is the only thing between a `BigQuery` deployment and a statement that truncates by
/// the wrong argument. [`crate::dialect::DateTruncShape`] carries why nothing else can cover it.
///
/// **The fifth dialect's cost is a third shape rather than a third argument order.** Oracle's
/// `TRUNC` is not `DATE_TRUNC` under another name - the match below picks the FUNCTION as well as
/// the arguments for that arm, and `DateTruncShape::DateFirstAsQuotedFormat`'s own doc says why
/// reusing either of the first two shapes would still be wrong.
fn bucket_expression(bucket: &PlanBucket, dialect: Dialect) -> Expr {
    bucket_expr(bucket.grain(), bucket.column(), dialect)
}

/// One column truncated to a grain, the way this dialect spells date truncation.
///
/// Shared between a question's [`PlanBucket`] and a compound join's truncation key, so a join that
/// truncates an origin to a month and a time bucket at the same grain cannot render differently.
fn bucket_expr(grain: Grain, over: &PlanColumn, dialect: Dialect) -> Expr {
    let (function, arguments) = match dialect.date_trunc_shape() {
        DateTruncShape::GrainFirstAsLiteral => ("DATE_TRUNC", vec![builder::lit(unit(grain)), column(over)]),
        // The grain as a bare keyword. `grain_keyword` carries why a `Raw` node here is not a hole.
        DateTruncShape::DateFirstAsKeyword => (
            "DATE_TRUNC",
            vec![
                column(over),
                Expr(Expression::Raw(Raw {
                    sql: String::from(grain_keyword(grain)),
                })),
            ],
        ),
        // `TRUNC`, not `DATE_TRUNC` - see `DateTruncShape::DateFirstAsQuotedFormat`'s own doc for
        // why this arm builds the call directly instead of reshaping the other two's shape.
        DateTruncShape::DateFirstAsQuotedFormat => ("TRUNC", vec![column(over), builder::lit(oracle_format(grain))]),
    };
    builder::func(function, arguments).cast("DATE")
}

/// Every join a plan declared, added to the statement.
///
/// A LEFT join, always, and this was a bug before it was a decision.
///
/// The metric's own model is the grain being measured; a dimension is a lookup beside it. An INNER
/// join drops every fact row with no matching dimension row - an order whose customer is missing
/// from the customer table - so `revenue by region` would total less than `revenue`, with nothing
/// raising an error anywhere. That is the same failure the catalog already refuses a row-DUPLICATING
/// join for, arrived at from the other direction, and the duplication check could not see it:
/// `may_duplicate_rows` is about fan-out, not about elimination.
///
/// Left-joining makes an unmatched row group under a null key instead of vanishing, so the grouped
/// total always reconciles with the ungrouped one - which is what
/// `a_dimension_join_does_not_change_the_measure` asserts over real data.
///
/// `OneToMany` never reaches here: a join that can duplicate the metric's rows is refused when the
/// definitions are assembled. The cardinality therefore does not change the join KIND today; it is
/// matched on so that adding a variant is a compile error here rather than a silently wrong
/// statement.
///
/// **A federated fact leg reaches this with same-source hops only.** A dimension on another data
/// system is a [`LegPlan::Lookup`] leg and not a join, so the join kind a splitter derives for the
/// combine above - INNER for a remote dimension carrying a filter, LEFT for one that does not - is
/// decided nowhere in this file.
fn joined(statement: SelectBuilder, joins: &[PlanJoin], dialect: Dialect) -> Result<SelectBuilder, GenerateError> {
    let mut statement = statement;
    for join in joins {
        // A joined table carries its own path, which is what makes a cross-dataset join one native
        // statement rather than two legs and a combiner.
        let path = table_path(join.table(), dialect)?;
        // Every key contributes one `=` term, `AND`ed together: a compound join renders
        // `a.x = b.y AND a.day_truncated = b.month`. A truncated key buckets its origin through
        // the same per-dialect date-truncation a time bucket uses, so the join's truncation and a
        // question's time bucket cannot drift apart in spelling.
        let on = join.keys().iter().fold(None, |acc: Option<Expr>, key| {
            let term = match key {
                PlanJoinKey::Equal { origin, target } => column(origin).eq(column(target)),
                PlanJoinKey::TruncatedEqual { origin, grain, target } => bucket_expr(*grain, origin, dialect).eq(column(target)),
            };
            Some(match acc {
                None => term,
                Some(acc) => acc.and(term),
            })
        });
        let Some(on) = on else {
            return Err(GenerateError::NoPredicate);
        };
        statement = match join.join_type() {
            JoinType::OneToOne | JoinType::ManyToOne | JoinType::OneToMany => statement.left_join(&path, on),
        };
    }
    Ok(statement)
}

/// The dotted path a `FROM` or a `JOIN` names, or a refusal if this target cannot resolve it.
///
/// **The one place a table's parts are joined back into text, and the two reasons that is sound
/// here.** First, the path is handed to the builder, whose `builder_table_ref` splits it on `.` and
/// puts each piece in a `TableRef` slot of its own - a plain split, not a tokenizer, so a hyphen in a
/// project id is a character rather than an operator - and `always_quote_identifiers` then quotes
/// **each part separately**. `` `analytics-prod`.`sales`.`orders` `` is what comes out, which is what
/// *no identifier reaches the statement unquoted* means for a path. Second, no part can contain a
/// `.`, so the split reconstructs exactly the parts that went in; `sutura_domain::model::qualified`
/// is where that is parsed and where it is argued.
///
/// **No explicit alias is emitted, and that is a declaration rather than an oversight.** A column is
/// qualified by the table's BARE name - `PlanColumn` holds a `TableName` - and `FROM a.b.c` gives the
/// reference an implicit alias of `c` in all five targets - Oracle's own documented behaviour rather
/// than something this workspace has an instance to measure, the same limit every other declaration
/// about it in this PR states. An explicit `AS "c"` would say so in the
/// statement instead of relying on that, and it is not reachable through the builder: `left_join`
/// takes a `&str` and `join_with_kind` is private, so aliasing the joined table would mean
/// hand-building the AST - which this module's header rules out for a reason. So the implicit alias is
/// what holds, the live acceptance leg is what measures it on the target where a misquoted identifier
/// is not a syntax error, and this paragraph is here so the next reader knows it was a choice.
fn table_path(table: &QualifiedTable, dialect: Dialect) -> Result<String, GenerateError> {
    let carries = table.qualification();
    let resolves = dialect.qualification();
    if carries > resolves {
        return Err(GenerateError::QualificationUnsupported {
            dialect,
            table: table.to_string(),
            carries,
            resolves,
        });
    }
    Ok(table.to_string())
}

/// `expr`, nulls placed last, ascending unless `desc`.
///
/// **Why explicit, and why `NULLS LAST`:** a bare `ORDER BY` leaves null placement to the target,
/// and the dialects disagree about it - the engine orders nulls last, `BigQuery` orders them
/// first, and no golden caught that until two data systems actually EXECUTED the same bare
/// `ORDER BY` and disagreed. Naming `NULLS LAST` makes every target converge on the engine's own
/// order: the layer renders the keyword for `BigQuery`, whose default is the other way, and for
/// `DuckDB`, whose null ordering is a runtime setting, and omits it where it is the fixed default
/// (Postgres, `ClickHouse`, Oracle for an ascending sort - the layer's own default-elision table
/// groups Oracle with Postgres) - behaviour, not text, converging.
/// Nulls last regardless of `desc`, because a null means there was nothing to rank and that sorts
/// after every value either way. Rendered through the layer's own [`Ordered`] node, which
/// `engine::ordered` unwraps rather than re-wrapping.
fn ordered(expr: Expr, desc: bool) -> Expr {
    Expr(Expression::Ordered(Box::new(Ordered {
        this: expr.into_inner(),
        desc,
        nulls_first: Some(false),
        explicit_asc: false,
        with_fill: None,
    })))
}

fn ordered_nulls_last(expr: Expr) -> Expr {
    ordered(expr, false)
}

/// `top: { n, by, direction }` - `github.com/telekom/sutura#777`.
mod top;

/// One measure, as one expression - the guard/aggregate rendering.
mod measure;
use measure::{Guard, measure_expression, predicate_values, term_expression};

/// The statement, as this dialect writes it.
///
/// Identifiers force-quoted, for the reason this module's header gives at length. One function, so
/// the whole-answer path and the leg path cannot quote differently.
fn render(ast: &Expression, dialect: Dialect) -> Result<String, GenerateError> {
    let mut config = polyglot_sql::dialects::Dialect::get(dialect_type(dialect))
        .generator_config()
        .clone();
    config.always_quote_identifiers = true;
    polyglot_sql::Generator::with_config(config)
        .generate(ast)
        .map_err(|cause| GenerateError::Render { dialect, cause })
}

/// Renders a plan as one statement, paired with its parameters.
pub fn generate(plan: &QueryPlan, dialect: Dialect) -> Result<GeneratedQuery, GenerateError> {
    let bucket = plan.bucket();
    let bucket_expr = bucket_expression(bucket, dialect);

    // Dimensions, then the time bucket, then one measure column per metric. A stable order,
    // because it is the result schema a caller reads by position and a golden pins by text - and
    // `QueryPlan::result_labels` states the same order for the adapter that builds a schema instead
    // of a projection (keys, bucket, measures).
    let mut projection = Vec::with_capacity(plan.keys().len().saturating_add(plan.measures().len()).saturating_add(1));
    let mut grouping = Vec::with_capacity(plan.keys().len().saturating_add(1));
    for key in plan.keys() {
        projection.push(aliased(column(key.column()), key.label())?);
        grouping.push(column(key.column()));
    }
    projection.push(aliased(bucket_expr.clone(), bucket.label())?);
    grouping.push(bucket_expr.clone());
    // The parameters this statement actually binds, in the order their placeholders will render -
    // built up as the SELECT list and then the `WHERE` are constructed, which is text order. Used
    // verbatim below for `PlaceholderStyle::Question` and `Colon` (Oracle binds a plain statement by
    // occurrence, not by its placeholders' names); `Numbered` binds by index and keeps
    // `plan.params()` as it stands - see `measure`'s module doc for why Colon sides with Question.
    let mut emitted: Vec<ParamValue> = Vec::new();
    // Single-metric: an empty guard, so it renders exactly as it always did - a bare aggregate, no
    // `CASE`. Multi-metric: each measure's own required filters guard its column (`guarded_column`).
    let primary = plan.measures().first();
    let primary_guard = Guard::build(dialect, primary.guard(), plan.params());
    let measure_expr = measure_expression(primary.measure(), primary_guard.as_ref(), dialect, &mut emitted);
    projection.push(aliased(measure_expr.clone(), primary.label())?);
    for measure in plan.measures().iter().skip(1) {
        let guard = Guard::build(dialect, measure.guard(), plan.params());
        let expr = measure_expression(measure.measure(), guard.as_ref(), dialect, &mut emitted);
        projection.push(aliased(expr, measure.label())?);
    }
    let statement = joined(
        builder::select(projection).from(&table_path(plan.table(), dialect)?),
        plan.joins(),
        dialect,
    )?;

    // Folded in plan order, which is parameter order: the range bounds, then the caller's. A
    // single-metric plan's metric filters live here too; a multi-metric plan's are each measure's
    // own guard (folded into the SELECT) and never reach this clause. Each filter here renders
    // exactly once, so its value is pushed to `emitted` once, in the same order, right after every
    // guard above - matching the SELECT-then-WHERE order the rendered text will have.
    let mut clauses = plan.filters().iter().map(|f| predicate(dialect, f.predicate()));
    let Some(first) = clauses.next() else {
        // Unreachable: a plan always carries its two range bounds, because a `TimeRange` cannot be
        // unbounded. Written as a branch rather than an `expect` because a panic here would be
        // reachable from a catalog file.
        return Err(GenerateError::NoPredicate);
    };
    let where_clause = clauses.fold(first, Expr::and);
    for filter in plan.filters() {
        emitted.extend(predicate_values(filter.predicate(), plan.params()));
    }

    // `top` replaces both the ordering and the limit - see `top::ordering` for the tie-break
    // argument - with the caller's own count rather than one past the row cap, since a `top`
    // answer is bounded by construction and there is nothing to detect.
    let tiebreak: Vec<Expr> = grouping.iter().cloned().map(ordered_nulls_last).collect();
    let (ordering, limit) = match plan.top() {
        Some(requested) => (
            top::ordering(requested, measure_expr, bucket_expr, tiebreak),
            usize::try_from(requested.n().get()).unwrap_or(usize::MAX),
        ),
        None => (tiebreak, usize::try_from(plan.row_limit()).unwrap_or(usize::MAX)),
    };
    let mut ast = statement
        .where_(where_clause)
        .group_by(grouping)
        // Ordered by what it groups by, so two runs of one question return rows in one order.
        // Without it row order is unspecified and a golden over results flaps for reasons that have
        // nothing to do with the change under review. Each ascending, nulls last - `ordered_nulls_last`
        // carries why the placement is stated rather than left to each target's default.
        .order_by(ordering)
        .limit(limit)
        .build();
    // Oracle refuses `LIMIT n` (`ORA-03049`) and takes `FETCH FIRST n ROWS ONLY`. The dialect
    // layer's `generate_limit` writes `LIMIT` for every target - its `limit_fetch_style` setting is
    // read by neither `generate_limit` nor `generate_fetch` in the pinned release - so for Oracle the
    // limit moves onto the `Fetch` node, which that layer renders as `FETCH FIRST`. Every other
    // dialect keeps the `Limit` node. `github.com/telekom/sutura#127`
    if dialect == Dialect::Oracle
        && let Expression::Select(select) = &mut ast
    {
        select.limit = None;
        select.fetch = Some(Fetch {
            direction: "FIRST".to_owned(),
            count: Some(Expression::Literal(Box::new(Literal::Number(limit.to_string())))),
            percent: false,
            rows: true,
            with_ties: false,
        });
    }

    // `Question` and `Colon` both bind a plain statement by occurrence - the oracledb driver's
    // `add_bind` pushes one `BindInfo` per placeholder it sees rather than per distinct name
    // (`oracledb-26.0.0-beta.3/src/statement/mod.rs:72-86`), and `bind_params.rs:99-104` refuses
    // when the bound row's length does not match that occurrence count - so a guard embedded more
    // than once needs its value repeated that many times for Oracle exactly as it does for the
    // bare-`?` dialects. `emitted` is exactly that, built alongside the tree above, in text order.
    // `Numbered` is the one style that binds by the index each placeholder carries, so a repeat
    // already resolves to the right value and `plan.params()` needs no rebuilding - true for
    // Postgres's `$n`, which the driver dereferences by number rather than by occurrence.
    let params = match dialect.placeholder_style() {
        PlaceholderStyle::Question | PlaceholderStyle::Colon => emitted,
        PlaceholderStyle::Numbered => plan.params().to_vec(),
    };
    Ok(GeneratedQuery::new(plan.source().clone(), render(&ast, dialect)?, params))
}

/// Renders one leg of a federated question as one statement, paired with its parameters.
///
/// **Four differences from [`generate`], and each of them is why a second entry point exists rather
/// than a flag on the first.**
///
/// 1. **It projects a LIST of term columns**, one per descending term, instead of one measure
///    expression. That is the whole of 0009's Decision 2 at the rendering layer: a decomposed `Avg`
///    travels as a sum beside a count and a ratio travels as an undivided numerator and denominator,
///    so nothing here can emit a division. It never calls [`measure_expression`], and it could not -
///    there is no `PlanMeasure` in a [`LegPlan`] to hand it.
/// 2. **The bucket and the joins are the fact leg's alone.** A dimension lookup reads a table with
///    no time column, so it projects its keys and groups by them, which is a distinct key set.
///
/// 3. **It emits no `LIMIT`.** A leg is not an answer:
///    `sutura_domain::plan::MAX_ROWS` caps one answer's rows and
///    [`QueryPlan::row_limit`] is how an adapter asks for one more than the cap, so a cap applied per
///    leg would refuse a question no answer was too large for. What bounds a leg is the byte budget
///    at the conversion boundary, which belongs with the code that converts. A federated `top` ranks
///    after the combine (`github.com/telekom/sutura#777`'s case 2), never inside a leg.
/// 4. **The `WHERE` clause is optional.** A [`QueryPlan`] always carries the two bounds of its range
///    so [`GenerateError::NoPredicate`] is unreachable there; a lookup leg for a remote dimension
///    that carries no filter has no predicate at all, and no clause is the correct rendering rather
///    than an error.
///
/// Everything else is shared with [`generate`] on purpose - [`column`](fn@crate::generate::column),
/// [`aliased`], [`aggregate`], [`term_expression`], [`predicate`], [`bucket_expression`],
/// [`joined`] and [`render`] - so a change to identifier quoting, to placeholder style or to how a
/// term renders cannot apply to one path and not the other.
///
/// **A RELEASE calls this now**, and the two sentences it replaces each stated an absence that has
/// since been spent: first *there is no splitter* (there is - `sutura_semantic::federated_plan`),
/// then *no release links an adapter that renders a leg, because the one leg-executing adapter a
/// published binary contains is the engine, which builds a logical plan instead*.
/// `sutura-exec-postgres` declares `Warehouse::EXECUTES_LEGS` and ships behind a feature
/// `nix/shipped.nix` enables, so a published binary renders a leg here at `Dialect::Postgres`.
///
/// What pins the rendering is still the golden family under `crates/sutura-app/tests/golden`, one
/// statement per shape per dialect, parse-checked in the dialect it was generated for. **Its limit
/// changed rather than went away:** one of those five dialects is now what a release executes, and
/// a parse check is not an execution - the executed evidence is
/// `crates/sutura-exec-postgres/tests/conformance.rs`'s leg cell against a provisioned tier.
pub fn generate_leg(leg: &LegPlan, dialect: Dialect) -> Result<GeneratedQuery, GenerateError> {
    // Keys first, in leg order, and both grouped by and projected. `LegPlan::result_labels` states
    // the same order for whatever reads the rows back.
    let mut projection = Vec::with_capacity(leg.keys().len().saturating_add(1));
    let mut grouping = Vec::with_capacity(leg.keys().len().saturating_add(1));
    for key in leg.keys() {
        projection.push(aliased(column(key.column()), key.label())?);
        grouping.push(column(key.column()));
    }

    // THE rendering arm. A third leg shape does not compile until it says what it projects.
    let joins: &[PlanJoin] = match *leg {
        LegPlan::Fact {
            ref bucket,
            ref terms,
            ref tables,
            ..
        } => {
            let bucket_expr = bucket_expression(bucket, dialect);
            projection.push(aliased(bucket_expr.clone(), bucket.label())?);
            grouping.push(bucket_expr);
            // Zero to four of them. Empty is the distinct-key leg, and it is not a special case
            // here: the projection is then the key list and the bucket, grouped by itself.
            for term in terms {
                // A leg projects bare decomposed terms, never a guarded measure - guards belong to
                // the whole-answer `generate` path, so `None` keeps this byte-identical.
                // A leg projects bare decomposed terms, never a guarded measure, so `emitted` is
                // never read - `generate_leg` keeps `leg.params()` unchanged below.
                projection.push(aliased(
                    term_expression(term.term(), None, dialect, &mut Vec::new()),
                    term.label(),
                )?);
            }
            tables.joins()
        }
        // No bucket, no terms, no joins. It projects its keys and groups by them, which is the
        // distinct set of dimension rows surviving its own filters.
        LegPlan::Lookup { .. } => &[],
    };

    let mut statement = joined(
        builder::select(projection).from(&table_path(leg.table(), dialect)?),
        joins,
        dialect,
    )?;

    // Folded in leg order, which is parameter order, exactly as `generate` folds a plan's.
    let mut clauses = leg.filters().iter().map(|f| predicate(dialect, f.predicate()));
    if let Some(first) = clauses.next() {
        statement = statement.where_(clauses.fold(first, Expr::and));
    }

    let tiebreak: Vec<Expr> = grouping.iter().cloned().map(ordered_nulls_last).collect();
    // Ordered by what it groups by, for `generate`'s reason: without it a leg's row order is
    // unspecified and a golden over it flaps. Each ascending, nulls last, like `generate`.
    let ast = statement.group_by(grouping).order_by(tiebreak).build();

    Ok(GeneratedQuery::new(
        leg.source().clone(),
        render(&ast, dialect)?,
        leg.params().to_vec(),
    ))
}

/// Renders one declared join key's uniqueness probe as one statement.
///
/// **Two counts over the target side of a whole key set, and nothing else.** A row count beside
/// `COUNT(DISTINCT key_1, key_2, …)` is the whole question a `many_to_one` declaration can be
/// contradicted by, and the pair is equal exactly when the declaration holds over the WHOLE set -
/// the one shape a compound key can be proved by, because no single column of a compound key need
/// identify a row on its own. There is no `WHERE`, no `GROUP BY`, no `HAVING` and no `LIMIT`: the
/// declaration is unconditional, so a probe carrying a filter would answer a narrower question than
/// the one the join path spends.
///
/// **No parameter, and nothing from a question.** A [`DeclaredKey`] is built out of a pinned
/// bundle's own parsed names, so the statement has nowhere for a caller's value to arrive; the
/// returned [`GeneratedQuery`] carries an empty parameter list rather than one this could fill.
///
/// **No key value is projected**, which is the same decision the answer type makes and for the same
/// reason: what comes back reaches a boot log, and a duplicated dimension key printed there is
/// source data copied into a sink nobody scoped for it.
///
/// The distinct count is over a TUPLE of the target columns for `DuckDb`, `Postgres` and
/// `ClickHouse` (the allowlist [`GenerateError::CompoundKeyProbeUnsupported`]'s rustdoc argues
/// for), and Oracle and `BigQuery` refuse a compound probe by name instead. Only
/// `sutura-domain`'s own parse-check family measures the rendered tuple text, over the three
/// allowed dialects; this comment does not claim Oracle or `BigQuery`. The row count stays
/// `COUNT(col)` for one key, unchanged, so an existing single-pair golden keeps its rendered
/// text, and for a compound key becomes `COUNT(CASE WHEN a IS NOT NULL AND b IS NOT NULL THEN 1
/// END)`: a row null in ANY column of the set cannot match on either side of a join, the same
/// reason a single null key is excluded, so `rows` and `distinct` stay comparable under
/// `COUNT(DISTINCT …)`'s own per-tuple null exclusion.
///
/// Shared with [`generate`] and [`generate_leg`]: [`qualified`], [`aliased`], [`table_path`] and
/// [`render`], so identifier quoting, column qualification and path depth cannot be one thing here
/// and another there. The two aliases are `sutura-domain`'s constants rather than this crate's
/// literals, so the label an adapter reads the count back under is the label the statement asked
/// for.
pub fn generate_key_probe(key: &DeclaredKey<'_>, dialect: Dialect) -> Result<GeneratedQuery, GenerateError> {
    // The path goes in the `FROM` and the qualifier is the table's own NAME, so a two-part or
    // three-part table still renders a two-part column reference - `qualified` is the one place that
    // is decided.
    let over: Vec<Expr> = key
        .target_columns()
        .map(|column| qualified(key.table().name(), column))
        .collect();
    let first = over.first().ok_or(GenerateError::NoPredicate)?.clone();
    // A row's key is non-null only when every column of the whole set is non-null - the same
    // "excluded from both counts" decision the module header states, extended to a compound key:
    // a row where one column of the pair is null cannot match on either side of any join, so it
    // is not a key row. One column keeps `COUNT(col)`'s existing shape; a compound key folds the
    // per-column nullness checks into the `CASE` that `COUNT` then counts non-null results of, so
    // the two counts stay comparable under `COUNT(DISTINCT …)`'s own null-exclusion.
    let rows = if over.len() == 1 {
        builder::count(first.clone())
    } else {
        let all_not_null = over
            .iter()
            .cloned()
            .map(Expr::is_not_null)
            .reduce(Expr::and)
            .ok_or(GenerateError::NoPredicate)?;
        let marker = builder::case().when(all_not_null, builder::lit(1)).build();
        builder::count(marker)
    };
    let distinct = if over.len() == 1 {
        builder::count_distinct(first)
    } else {
        // An allowlist, not a denylist: DuckDB and Postgres get polyglot's null-safe
        // `CASE WHEN a IS NULL THEN NULL … ELSE (a, b) END` rewrite (`multi_arg_distinct: false`
        // in polyglot 0.12.0's generator), and ClickHouse's own multi-argument `DISTINCT` already
        // skips a row where any argument is null, natively. Every other dialect this crate renders
        // a `Tuple` for keeps `DISTINCT` a single-expression form - Oracle's grammar accepts one
        // argument (`ORA-00909` on more, per Oracle's documented syntax; not reproduced against a
        // live instance here) and BigQuery's `COUNT(DISTINCT …)` is documented as one expression
        // too - so a `Tuple` there renders a plain argument list neither dialect means by it.
        // Refused rather than rendered: see `GenerateError::CompoundKeyProbeUnsupported`.
        if !matches!(dialect, Dialect::DuckDb | Dialect::Postgres | Dialect::ClickHouse) {
            return Err(GenerateError::CompoundKeyProbeUnsupported { dialect });
        }
        let tuple = Expr(Expression::Tuple(Box::new(Tuple {
            expressions: over.into_iter().map(|e| e.0).collect(),
        })));
        builder::count_distinct(tuple)
    };
    let projection = vec![aliased(rows, ROWS_LABEL)?, aliased(distinct, DISTINCT_LABEL)?];
    let ast = builder::select(projection).from(&table_path(key.table(), dialect)?).build();
    Ok(GeneratedQuery::new(key.source().clone(), render(&ast, dialect)?, Vec::new()))
}

/// What this module claims about the dialect layer, measured against the layer itself.
///
/// **Every test here exists because a declaration in [`crate::dialect`] would otherwise be a claim
/// about a rendering nobody checked.** That module deliberately does not name `polyglot-sql`, so the
/// declarations live there and the measurements live here - and a declaration that stopped matching
/// the layer fails at this file rather than in a golden diff somebody accepts.
///
/// Its own file (`generate/tests.rs`) rather than an inline module: this file reached the
/// unexemptable 1000-line gate, and the split is a plain file move - every assertion below is
/// unchanged, and none of it is new evidence for `test-causality` to measure.
#[cfg(test)]
mod tests;
