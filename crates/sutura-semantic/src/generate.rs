//! Generate: the plan becomes one statement in one dialect.
//!
//! This is the only module that names the dialect layer, so a pre-1.0 API change upstream touches
//! one file. It is also the only module that produces SQL.
//!
//! Four things about how the dialect layer is used, each measured rather than assumed:
//!
//! **The fluent builder, not the AST structs.** The `Expression` enum's `Select` has upwards of
//! thirty fields and no `Default`, so hand-building one is a list nobody can review. The builder
//! panics on misuse rather than returning an error, which is why every call here is on a shape that
//! was proved to work; nothing a catalog or a caller supplies changes which builder method runs.
//!
//! **Identifiers are force-quoted.** The generator quotes an identifier only if it was quoted in the
//! source, is a reserved word, or the config says always. Ours were never in any source, so a column
//! called `order` would be emitted bare. `always_quote_identifiers` fixes that for identifiers and
//! does **not** cover aliases, which the generator writes from a separate `Identifier` whose `quoted`
//! flag we set ourselves.
//!
//! **`GROUP BY` gets the unaliased expressions.** Passing the aliased ones emits `GROUP BY x AS y`,
//! which is not valid in any of the three targets. It looked right until it was rendered.
//!
//! **Placeholders are ours.** The dialect layer renders every placeholder as `?` whatever the target,
//! and carries a per-dialect `parameter_token` it never reads, so Postgres would get `?` and reject
//! it. [`crate::dialect::PlaceholderStyle`] is what decides.
//!
//! **`transpile` is never called, and is not even compiled.** See `clippy.toml` and the feature list
//! in the workspace manifest.

use polyglot_sql::DialectType;
use polyglot_sql::builder::{self, Expr};
use polyglot_sql::expressions::{Expression, Parameter, ParameterStyle, Placeholder};
use sutura_domain::model::{Aggregate, Grain, JoinType};
use sutura_domain::warehouse::GeneratedQuery;

use crate::dialect::{Dialect, PlaceholderStyle};
use crate::plan::{Plan, PlanColumn};

/// Why a statement could not be rendered.
///
/// Not a refusal: a caller cannot cause one of these, and there is nothing they could ask
/// differently. A plan that cannot be rendered is a bug here or upstream.
#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("the dialect layer could not render the statement for {dialect}")]
    Render {
        dialect: Dialect,
        #[source]
        cause: polyglot_sql::Error,
    },
    /// The builder produced something that is not an alias, so the alias identifier could not be
    /// quoted. Reported rather than ignored: silently emitting an unquoted alias is how a metric
    /// named `order` becomes a syntax error at the data system.
    #[error("an alias for {label:?} did not come back as an alias, so it could not be quoted")]
    UnquotableAlias { label: String },
}

/// The dialect layer.s name for a data system.
///
/// A free function rather than a second inherent `impl Dialect`: the type is declared in
/// `dialect.rs`, which does not name this crate, and two inherent impls for one type is a lint here
/// for the good reason that it hides half of a type.s methods from whoever opens the other file.
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
/// and reads the wrong parameter, which is the reason the two are converted in exactly one place.
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
///
/// The dotted string form is what the builder parses into a qualified column reference; the quoting
/// is applied at render time from the generator config.
fn column(plan_column: &PlanColumn) -> Expr {
    builder::col(&format!("{}.{}", plan_column.table.as_str(), plan_column.column.as_str()))
}

/// `expr AS "label"`, with the alias quoted.
///
/// The builder leaves an alias identifier unquoted, and `always_quote_identifiers` does not reach
/// it. A metric or dimension named `order`, `select` or `end` would then emit `AS order`, which is a
/// syntax error in all three targets. The fields are public upstream, so setting `quoted` after
/// construction is the fix; a shape that is not an alias is an error rather than a silent pass.
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
/// Lower case because that is what every target accepts as the unit string. The function name is
/// written as `DATE_TRUNC` and the dialect layer rewrites it per target, which is the one place we
/// genuinely rely on it knowing more than we do.
const fn unit(grain: Grain) -> &'static str {
    match grain {
        Grain::Day => "day",
        Grain::Week => "week",
        Grain::Month => "month",
        Grain::Quarter => "quarter",
        Grain::Year => "year",
    }
}

/// The aggregate, as a builder call.
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

/// Renders a plan as one statement, and pairs it with its parameters.
pub fn generate(plan: &Plan, dialect: Dialect) -> Result<GeneratedQuery, GenerateError> {
    let bucket = plan.bucket();
    // Truncated, then CAST TO DATE, and the cast is the load-bearing half. `DATE_TRUNC` over a date
    // returns a TIMESTAMP in `DuckDB` and in Postgres, so without it the type of the `period` column
    // would be whatever each dialect happened to choose and every adapter would need to know which.
    // A period is a calendar date; saying so in the statement makes the result schema a contract
    // rather than a consequence of the target.
    let bucket_expr = builder::cast(
        builder::func("DATE_TRUNC", vec![builder::lit(unit(bucket.grain)), column(&bucket.column)]),
        "DATE",
    );

    // The projection is dimensions first, then the time bucket, then the measure. A stable order,
    // because it is the result schema a caller reads by position and a golden pins by text.
    let mut projection = Vec::with_capacity(plan.keys().len().saturating_add(2));
    let mut grouping = Vec::with_capacity(plan.keys().len().saturating_add(1));
    for key in plan.keys() {
        projection.push(aliased(column(&key.column), &key.label)?);
        grouping.push(column(&key.column));
    }
    projection.push(aliased(bucket_expr.clone(), &bucket.label)?);
    grouping.push(bucket_expr);

    let measure = plan.measure();
    projection.push(aliased(
        aggregate(measure.aggregate, column(&measure.column)),
        &measure.label,
    )?);

    let mut statement = builder::select(projection).from(plan.table().as_str());
    for join in plan.joins() {
        let on = column(&join.origin).eq(column(&join.target));
        // `OneToMany` never reaches here: a join that can duplicate the metric's rows is refused
        // when the definitions are assembled, because it would change the measure without erroring.
        statement = match join.join_type {
            JoinType::OneToOne | JoinType::ManyToOne | JoinType::OneToMany => statement.join(join.table.as_str(), on),
        };
    }

    // The bounded date predicate, then one equality per filter. Half-open, matching `TimeRange`, so
    // a month is one predicate pair at every grain.
    let time = column(plan.time_column());
    let mut predicate = time
        .clone()
        .gte(placeholder(dialect, 0))
        .and(time.lt(placeholder(dialect, 1)));
    for filter in plan.filters() {
        predicate = predicate.and(column(&filter.column).eq(placeholder(dialect, filter.param)));
    }

    let ordering: Vec<Expr> = grouping.clone();
    let ast = statement
        .where_(predicate)
        .group_by(grouping)
        // Ordered by the same expressions it groups by, so two runs of the same question return
        // rows in the same order. Without it row order is unspecified, and a golden over results
        // flaps for reasons that have nothing to do with the change under review.
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
