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
//! Five things about how the dialect layer is used, every one of them measured rather than assumed,
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
//! three targets, so without the cast the type of the `period` column is whatever each dialect chose
//! and every adapter would need to know which.
//!
//! **Placeholders are ours.** The dialect layer renders every placeholder as `?` whatever the target,
//! and carries a per-dialect `parameter_token` it never reads - so Postgres would be sent `?` and
//! reject it. [`crate::dialect::PlaceholderStyle`] decides.
//!
//! `transpile` is never called and is not compiled. See `clippy.toml` and the feature list in the
//! workspace manifest.

use polyglot_sql::DialectType;
use polyglot_sql::builder::{self, Expr};
use polyglot_sql::expressions::{Expression, Parameter, ParameterStyle, Placeholder};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::{Aggregate, Grain, JoinType};
use sutura_domain::plan::{PlanColumn, PlanMeasure, PlanPredicate, PlanTerm, QueryPlan};
use sutura_domain::warehouse::GeneratedQuery;

use crate::dialect::{Dialect, PlaceholderStyle};

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
    /// A plan that carries no predicate at all.
    ///
    /// Unreachable: a plan always carries the two bounds of its `TimeRange`, which cannot be
    /// unbounded. Its own variant rather than a sentinel string inside another one, because the
    /// variant is what a caller matches on - and worded exactly as `sutura_exec_datafusion`'s
    /// `NoPredicate`, so the SQL path and the engine name one condition identically rather than
    /// describing it twice.
    #[error("a plan must carry the two bounds of its range, and this one carries no predicate")]
    NoPredicate,
}

/// The dialect layer's name for a data system.
///
/// A free function rather than a second inherent `impl Dialect`: the type is declared in
/// `dialect.rs`, which does not name this crate, and two inherent impls for one type hide half of a
/// type's methods from whoever opens the other file.
const fn dialect_type(dialect: Dialect) -> DialectType {
    match dialect {
        Dialect::DuckDb => DialectType::DuckDB,
        Dialect::Postgres => DialectType::PostgreSQL,
        Dialect::ClickHouse => DialectType::ClickHouse,
    }
}

/// A placeholder, written the way this dialect writes one.
///
/// `position` is zero-based because it indexes the parameter list; the numbered form is one-based
/// because that is what Postgres counts from. Getting that off by one produces a statement that runs
/// and reads the wrong parameter, which is why the two are converted in exactly one place.
fn placeholder(dialect: Dialect, position: usize) -> Expr {
    match dialect.placeholder_style() {
        PlaceholderStyle::Question => Expr(Expression::Placeholder(Placeholder { index: None })),
        PlaceholderStyle::Numbered => {
            let index = u32::try_from(position.saturating_add(1)).unwrap_or(u32::MAX);
            Expr(Expression::Parameter(Box::new(Parameter {
                name: None,
                index: Some(index),
                style: ParameterStyle::Dollar,
                quoted: false,
                string_quoted: false,
                expression: None,
            })))
        }
    }
}

/// `"table"."column"`, as an expression the builder understands.
fn column(plan_column: &PlanColumn) -> Expr {
    builder::col(&format!("{}.{}", plan_column.table().as_str(), plan_column.column().as_str()))
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
const fn unit(grain: Grain) -> &'static str {
    match grain {
        Grain::Day => "day",
        Grain::Week => "week",
        Grain::Month => "month",
        Grain::Quarter => "quarter",
        Grain::Year => "year",
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

/// One term, as one expression.
///
/// A conditional count is `SUM(CASE WHEN col THEN 1 ELSE 0 END)` rather than the dialect layer's own
/// `CountIf` node. That node does lower correctly for all three of our targets, unlike `SafeDivide`
/// below, so this is the weaker of the two decisions - but it keeps every term rendered by one
/// mechanism we can read, and it counts 0 rather than null for a false row, so a period with no
/// matches answers 0 instead of nothing.
fn term_expression(term: &PlanTerm) -> Expr {
    match *term {
        PlanTerm::Aggregate {
            aggregate: kind,
            column: ref col,
        } => aggregate(kind, column(col)),
        PlanTerm::CountIf { column: ref col } => builder::sum(
            builder::case()
                .when(column(col), builder::lit(1))
                .else_(builder::lit(0))
                .build(),
        ),
    }
}

/// The measure, as one expression.
///
/// A ratio is rendered as a division with a `NULLIF` on the denominator, rather than through the
/// dialect layer's own `SafeDivide` node - and that is a measured decision rather than ignorance of
/// the node.
///
/// The dialect layer does carry typed `SafeDivide` and `CountIf` nodes, and they lower correctly for
/// some targets: `SAFE_DIVIDE` for one, a `CASE` for another, `COUNTIF` and `countIf` for two more.
/// But `SafeDivide` has **no Postgres lowering** - the generator falls through to writing the literal
/// text `SAFE_DIVIDE(x, y)`, which is not a function Postgres has - and Postgres is a target we
/// render for. Using the node would produce a statement that is valid in two of our three dialects
/// and a call to a non-existent function in the third.
///
/// `NULLIF` and `/` exist in all three, and a division by null is null in all three, so this form is
/// identical in behaviour and portable by construction. Revisit it if the node gains that lowering;
/// until then the golden that parses every statement in its target dialect is what would catch the
/// regression.
///
/// The numerator is cast to a floating type first. Integer division truncates in Postgres and in
/// `DuckDB` - `SUM(cents) / COUNT(*)` would silently return a whole number - which is the wrong answer
/// for every ratio anybody actually wants.
fn measure_expression(measure: &PlanMeasure) -> Expr {
    match *measure {
        PlanMeasure::Simple { ref term } => term_expression(term),
        PlanMeasure::Ratio {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            let top = term_expression(numerator).cast("DOUBLE");
            let bottom = term_expression(denominator);
            let bottom = match zero_denominator {
                ZeroDenominator::Null => builder::null_if(bottom, builder::lit(0)),
                ZeroDenominator::Fail => bottom,
            };
            top.div(bottom)
        }
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
        PlanPredicate::IsTrue { .. } => col.is(builder::boolean(true)),
        PlanPredicate::IsNotNull { .. } => col.is_not_null(),
    }
}

/// Renders a plan as one statement, paired with its parameters.
pub fn generate(plan: &QueryPlan, dialect: Dialect) -> Result<GeneratedQuery, GenerateError> {
    let bucket = plan.bucket();
    let bucket_expr = builder::func(
        "DATE_TRUNC",
        vec![builder::lit(unit(bucket.grain())), column(bucket.column())],
    )
    .cast("DATE");

    // Dimensions, then the time bucket, then the measure. A stable order, because it is the result
    // schema a caller reads by position and a golden pins by text - and `QueryPlan::result_labels`
    // states the same order for the adapter that builds a schema instead of a projection.
    let mut projection = Vec::with_capacity(plan.keys().len().saturating_add(2));
    let mut grouping = Vec::with_capacity(plan.keys().len().saturating_add(1));
    for key in plan.keys() {
        projection.push(aliased(column(key.column()), key.label())?);
        grouping.push(column(key.column()));
    }
    projection.push(aliased(bucket_expr.clone(), bucket.label())?);
    grouping.push(bucket_expr);
    projection.push(aliased(measure_expression(plan.measure()), plan.measure_label())?);
    let mut statement = builder::select(projection).from(plan.table().as_str());
    for join in plan.joins() {
        let on = column(join.origin()).eq(column(join.target()));
        // A LEFT join, always, and this was a bug before it was a decision.
        //
        // The metric's own model is the grain being measured; a dimension is a lookup beside it. An
        // INNER join drops every fact row with no matching dimension row - an order whose customer
        // is missing from the customer table - so `revenue by region` would total less than
        // `revenue`, with nothing raising an error anywhere. That is the same failure the catalog
        // already refuses a row-DUPLICATING join for, arrived at from the other direction, and the
        // duplication check could not see it: `may_duplicate_rows` is about fan-out, not about
        // elimination.
        //
        // Left-joining makes an unmatched row group under a null key instead of vanishing, so the
        // grouped total always reconciles with the ungrouped one - which is what
        // `a_dimension_join_does_not_change_the_measure` asserts over real data.
        //
        // `OneToMany` never reaches here: a join that can duplicate the metric's rows is refused
        // when the definitions are assembled. The cardinality therefore does not change the join
        // KIND today; it is matched on so that adding a variant is a compile error here rather than
        // a silently wrong statement.
        statement = match join.join_type() {
            JoinType::OneToOne | JoinType::ManyToOne | JoinType::OneToMany => statement.left_join(join.table().as_str(), on),
        };
    }

    // Folded in plan order, which is parameter order: the range bounds, then the metric's required
    // filters, then the caller's. For a dialect that writes `?` the position in the statement is the
    // parameter's identity, so this fold and the plan's parameter list have to walk together.
    let mut clauses = plan.filters().iter().map(|f| predicate(dialect, f.predicate()));
    let Some(first) = clauses.next() else {
        // Unreachable: a plan always carries its two range bounds, because a `TimeRange` cannot be
        // unbounded. Written as a branch rather than an `expect` because a panic here would be
        // reachable from a catalog file.
        return Err(GenerateError::NoPredicate);
    };
    let where_clause = clauses.fold(first, Expr::and);

    let ordering: Vec<Expr> = grouping.clone();
    let ast = statement
        .where_(where_clause)
        .group_by(grouping)
        // Ordered by what it groups by, so two runs of one question return rows in one order.
        // Without it row order is unspecified and a golden over results flaps for reasons that have
        // nothing to do with the change under review.
        .order_by(ordering)
        .limit(usize::try_from(plan.row_limit()).unwrap_or(usize::MAX))
        .build();

    let mut config = polyglot_sql::dialects::Dialect::get(dialect_type(dialect))
        .generator_config()
        .clone();
    config.always_quote_identifiers = true;
    let sql = polyglot_sql::Generator::with_config(config)
        .generate(&ast)
        .map_err(|cause| GenerateError::Render { dialect, cause })?;

    Ok(GeneratedQuery::new(plan.source().clone(), sql, plan.params().to_vec()))
}
