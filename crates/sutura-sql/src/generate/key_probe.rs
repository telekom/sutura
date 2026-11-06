//! One declared join key's uniqueness probe: the two counts a `many_to_one` declaration is held to.

use polyglot_sql::builder::{self, Expr};
use polyglot_sql::expressions::{Expression, Tuple};
use sutura_domain::warehouse::cardinality::{DISTINCT_LABEL, DeclaredKey, ROWS_LABEL};

use super::{GenerateError, aliased, assert_placeholder_count, qualified, render, table_path};
use crate::GeneratedQuery;
use crate::dialect::Dialect;

/// The name an Oracle compound probe's per-tuple counts are read under.
const GROUPS: &str = "key_groups";

/// The label of one group's row count inside [`GROUPS`].
const GROUP_ROWS: &str = "group_rows";

/// Renders one declared join key's uniqueness probe as one statement.
///
/// **Two counts over the target side of a whole key set, and nothing else.** A row count beside
/// `COUNT(DISTINCT key_1, key_2, …)` is the whole question a `many_to_one` declaration can be
/// contradicted by, and the pair is equal exactly when the declaration holds over the WHOLE set -
/// the one shape a compound key can be proved by, because no single column of a compound key need
/// identify a row on its own. There is no `WHERE`, no `GROUP BY`, no `HAVING` and no `LIMIT` of the
/// probe's own choosing - Oracle's compound shape below is the one exception, and it only takes the
/// null exclusion the counts already make into its `WHERE` - because the declaration is
/// unconditional, so a probe carrying a filter would answer a narrower question than the one the
/// join path spends.
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
/// for), and `BigQuery` refuses a compound probe by name instead. Only `sutura-domain`'s own
/// parse-check family measures the rendered tuple text, over those three dialects. The row count
/// stays `COUNT(col)` for one key, unchanged, so an existing single-pair golden keeps its rendered
/// text, and for a compound key becomes `COUNT(CASE WHEN a IS NOT NULL AND b IS NOT NULL THEN 1
/// END)`: a row null in ANY column of the set cannot match on either side of a join, the same
/// reason a single null key is excluded, so `rows` and `distinct` stay comparable under
/// `COUNT(DISTINCT …)`'s own per-tuple null exclusion.
///
/// **Oracle's `DISTINCT` takes one expression, so a compound key is counted over a grouped
/// subquery**: `rows` is the sum of the sizes of the groups of tuples with no null column and
/// `distinct` the number of those groups - the two counts above, by the same exclusion, spelled
/// without a multi-column `DISTINCT`. Not executed here: the `oracle-tier` job is its only venue.
///
/// Shared with [`super::generate`] and [`super::generate_leg`]: [`qualified`], [`aliased`],
/// [`table_path`] and [`render`], so identifier quoting, column qualification and path depth cannot be one thing here
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
    let first = over.first().ok_or(GenerateError::NoPredicate)?;
    if over.len() > 1 && dialect == Dialect::Oracle {
        return finished(key, &oracle_compound_probe(key, over, dialect)?, dialect);
    }
    // A row's key is non-null only when every column of the whole set is non-null - the same
    // "excluded from both counts" decision the module header states, extended to a compound key:
    // a row where one column of the pair is null cannot match on either side of any join, so it
    // is not a key row. One column keeps `COUNT(col)`'s existing shape; a compound key folds the
    // per-column nullness checks into the `CASE` that `COUNT` then counts non-null results of, so
    // the two counts stay comparable under `COUNT(DISTINCT …)`'s own null-exclusion.
    let rows = if over.len() == 1 {
        builder::count(first.clone())
    } else {
        builder::count(builder::case().when(all_not_null(&over)?, builder::lit(1)).build())
    };
    let distinct = if over.len() == 1 {
        builder::count_distinct(first.clone())
    } else {
        // An allowlist, not a denylist: DuckDB and Postgres get polyglot's null-safe
        // `CASE WHEN a IS NULL THEN NULL … ELSE (a, b) END` rewrite (`multi_arg_distinct: false`
        // in polyglot 0.12.0's generator), and ClickHouse's own multi-argument `DISTINCT` already
        // skips a row where any argument is null, natively. Every other dialect this crate renders
        // a `Tuple` for keeps `DISTINCT` a single-expression form - BigQuery's `COUNT(DISTINCT …)`
        // is documented as one expression - so a `Tuple` there renders a plain argument list the
        // dialect does not mean by it. Refused rather than rendered: see
        // `GenerateError::CompoundKeyProbeUnsupported`. Oracle's own shape returned above.
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
    finished(key, &ast, dialect)
}

/// Every column of the key set non-null, as one condition.
fn all_not_null(over: &[Expr]) -> Result<Expr, GenerateError> {
    over.iter()
        .cloned()
        .map(Expr::is_not_null)
        .reduce(Expr::and)
        .ok_or(GenerateError::NoPredicate)
}

/// A compound key's two counts for Oracle, whose `DISTINCT` takes one expression.
///
/// ```sql
/// SELECT COALESCE(SUM("key_groups"."group_rows"), 0) AS "0_key_rows", COUNT(*) AS "0_key_distinct"
/// FROM (SELECT COUNT(*) AS "group_rows" FROM "t"
///       WHERE NOT "t"."a" IS NULL AND NOT "t"."b" IS NULL GROUP BY "t"."a", "t"."b") "key_groups"
/// ```
///
/// (polyglot's `is_not_null` is `NOT x IS NULL`, which the generator writes as the prefix form
/// above: the same predicate as `x IS NOT NULL`.)
///
/// **Null tuples, in each dialect.** `DuckDB` and Postgres count the tuple through polyglot's
/// `CASE WHEN a IS NULL THEN NULL … ELSE (a, b) END`, so `COUNT(DISTINCT ..)` skips a tuple with
/// any null column; `ClickHouse`'s multi-argument `DISTINCT` skips it natively; `rows` is
/// `COUNT(CASE WHEN a IS NOT NULL AND b IS NOT NULL THEN 1 END)` on all three. Here the `WHERE`
/// drops the same tuples before grouping, so a group is a distinct non-null tuple and the sum of
/// the groups is the non-null row count. (The first two are read from this crate's own account of
/// the rewrite, not re-measured on 0.14.) **`COALESCE`**: over an empty table `SUM` is null where the
/// other dialects' `COUNT` is zero, and a probe that answered null would read as no counts at all.
fn oracle_compound_probe(key: &DeclaredKey<'_>, over: Vec<Expr>, dialect: Dialect) -> Result<Expression, GenerateError> {
    let groups = builder::select([aliased(builder::count_star(), GROUP_ROWS)?])
        .from(&table_path(key.table(), dialect)?)
        .where_(all_not_null(&over)?)
        .group_by(over);
    let sizes = builder::col(&format!("{GROUPS}.{GROUP_ROWS}"));
    let projection = vec![
        aliased(builder::coalesce([builder::sum(sizes), builder::lit(0)]), ROWS_LABEL)?,
        aliased(builder::count_star(), DISTINCT_LABEL)?,
    ];
    Ok(builder::select(projection)
        .from_expr(builder::subquery(groups, GROUPS))
        .build())
}

/// `ast` rendered as `key`'s probe: no parameter, and the placeholder count held to that.
fn finished(key: &DeclaredKey<'_>, ast: &Expression, dialect: Dialect) -> Result<GeneratedQuery, GenerateError> {
    let params = Vec::new();
    let sql = render(ast, dialect)?;
    assert_placeholder_count(ast, dialect, &params)?;
    Ok(GeneratedQuery::new(key.source().clone(), sql, params))
}
