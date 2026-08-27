//! The name lists a refusal is decided by: which node kinds, and which called function names.
//!
//! Split out of [`super`] because it is data rather than reasoning, and because `cargo xtask
//! max-lines` caps a file under `crates/` at a thousand lines and cannot exempt anything - the
//! module that argues about SQL should not be competing for room with sixty date-function names.
//!
//! # Two directions, and only one of them is a denylist
//!
//! **Node KINDS are a denylist.** [`kind_refusal`] names the kinds that are a query, a relation, a
//! placeholder or an unhandled node, and a typo in one of those lists fails CLOSED: the entry never
//! matches, the construct is not refused for *that* reason, and it falls through to the parent
//! module's structural guards - `traversal::is_query`, `traversal::is_ddl`, the shape checks - which
//! are what decide. `docs/adr/0004` argues that direction on the count, and the argument holds:
//! there are some six hundred kinds and only so many ways to name a relation.
//!
//! **Called function NAMES are an allowlist**, and that is the reversal `docs/adr/0004` records.
//! The count argument does not transfer to names: the space is every function every target has plus
//! every user-defined one, so no denylist bounds it, while the set a *measure* needs is short enough
//! to read in one screen. So [`name_refusal`] refuses every name that is not in
//! [`ALLOWED_FUNCTION_NAMES`], and a per-dialect variant that needs one more is a one-line,
//! review-visible edit to that list - which is exactly the property the authored-SQL hatch exists to
//! have.
//!
//! A typo in the ALLOWLIST therefore fails closed in the other direction: it refuses a name that
//! should have been allowed. That is a load failure naming the function, at the moment the catalog is
//! read, which is the failure this repository prefers over a name reaching a data system unchecked.

use super::Construct;

/// A generic function call is emitted verbatim into every target, so its name is checked rather
/// than trusted. These are the date and time names, refused for the reason
/// [`Construct::DateTimeFunction`] gives.
///
/// Not exhaustive over every dialect's date vocabulary, and it no longer needs to be for the reason
/// it used to give. Truncation is the generator's, from `Grain`, so no metric has a reason to write
/// a date function; and an unlisted date name is now refused anyway, as a name outside
/// [`ALLOWED_FUNCTION_NAMES`]. This list survives because it gives that name a refusal that says
/// *date function* and points at the argument-order defect, which is a better sentence for an author
/// than "not on the list" - not because it is what closes the hole.
const DATE_FUNCTION_NAMES: &[&str] = &[
    "AGE",
    "CURRENT_DATE",
    "CURRENT_DATETIME",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "DATEADD",
    "DATEDIFF",
    "DATEPART",
    "DATETIME",
    "DATETRUNC",
    "DATE_ADD",
    "DATE_BIN",
    "DATE_DIFF",
    "DATE_PART",
    "DATE_SUB",
    "DATE_TRUNC",
    "GETDATE",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "NOW",
    "STRFTIME",
    "STRPTIME",
    "SYSDATE",
    "SYSTIMESTAMP",
    "TIMESTAMPADD",
    "TIMESTAMPDIFF",
    "TIMESTAMP_TRUNC",
    "TODAY",
    "TO_DATE",
    "TO_TIMESTAMP",
    "TRUNC_DATE",
    "UTC_TIMESTAMP",
];

/// AST node kinds that are a date or time function, by `Expression::variant_name`.
const DATE_KINDS: &[&str] = &[
    "add_months",
    "at_time_zone",
    "current_date",
    "current_datetime",
    "current_time",
    "current_timestamp",
    "current_timestamp_l_t_z",
    "date",
    "date_add",
    "date_bin",
    "date_diff",
    "date_format",
    "date_from_parts",
    "date_from_unix_date",
    "date_sub",
    "date_to_di",
    "date_trunc",
    "datetime",
    "datetime_add",
    "datetime_diff",
    "datetime_sub",
    "datetime_trunc",
    "day",
    "day_of_month",
    "day_of_week",
    "day_of_week_iso",
    "day_of_year",
    "dayname",
    "di_to_date",
    "epoch",
    "epoch_ms",
    "extract",
    "format_date",
    "from_time_zone",
    "from_unixtime",
    "gap_fill",
    "hour",
    "interval",
    "interval_op",
    "interval_span",
    "last_day",
    "localtime",
    "localtimestamp",
    "make_date",
    "make_interval",
    "make_timestamp",
    "minute",
    "month",
    "months_between",
    "next_day",
    "previous_day",
    "quarter",
    "second",
    "systimestamp",
    "time",
    "time_add",
    "time_diff",
    "time_from_parts",
    "time_slice",
    "time_str_to_date",
    "time_str_to_unix",
    "time_sub",
    "time_to_unix",
    "time_trunc",
    "timestamp",
    "timestamp_add",
    "timestamp_diff",
    "timestamp_sub",
    "timestamp_trunc",
    "to_date",
    "to_timestamp",
    "ts_or_di_to_di",
    "ts_or_ds_to_datetime",
    "ts_or_ds_to_timestamp",
    "unix_date",
    "unix_micros",
    "unix_millis",
    "unix_seconds",
    "unix_timestamp",
    "unix_to_time_str",
    "week_of_year",
    "year",
    "year_of_week",
    "year_of_week_iso",
];

/// Node kinds that are a query of their own.
///
/// A short list rather than an allowlist of the six hundred kinds that are fine, and it is backed up
/// rather than trusted: `traversal::is_query` and `traversal::is_ddl` are asked as well, so a kind
/// missing from here is still refused if the dialect layer classifies it.
const QUERY_KINDS: &[&str] = &[
    "cte",
    "except",
    "exists",
    "from",
    "intersect",
    "join",
    "joined_table",
    "pipe_operator",
    "select",
    "set_operation",
    "subquery",
    "union",
    "values",
    "with",
];

/// Node kinds that name or unfold a relation. `table` and `exists` are the ones the dialect layer's
/// own `is_query` and `is_ddl` predicates miss.
const TABLE_KINDS: &[&str] = &[
    "braced_wildcard",
    "columns",
    "explode",
    "explode_outer",
    "lateral",
    "lateral_view",
    "match_recognize",
    "pivot",
    "pivot_alias",
    "rows_from",
    "semantic_view",
    "star_map",
    "table",
    "table_argument",
    "table_from_rows",
    "table_sample",
    "unnest",
    "unpivot",
];

/// Node kinds that would put a bind placeholder in the statement.
const PARAMETER_KINDS: &[&str] = &["named_argument", "parameter", "placeholder", "var", "variadic"];

/// Node kinds the generator writes out with no dialect handling of any kind.
const OPAQUE_KINDS: &[&str] = &[
    "command",
    "dot",
    "heredoc",
    "lambda",
    "method_call",
    "raw",
    "sql_comment",
    "subscript",
];

/// What calling a name does, for the two questions asked about a generic call.
///
/// The mark exists because the second question has no other answer. `traversal::contains_aggregate`
/// classifies by node KIND, so it is true for `SUM(x)` and for `sumIf(x, p)` - each of those gets a
/// node the dialect layer recognises - and false for `uniqExact(k)`, which arrives as a plain
/// `Function`. `uniqExact` is a real `ClickHouse` aggregate and precisely what a per-dialect variant
/// is for, so refusing it as "no aggregate" was wrong. The mark is the answer, and it is written
/// down once: the aggregate node kinds the dialect layer already knows are **not** restated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Called {
    /// One value per row, from its arguments. Allowed, and it aggregates nothing.
    Scalar,
    /// A group collapsed to one value. Allowed, and a fragment that calls one aggregates something.
    Aggregate,
}

/// Declares the allowlist once and derives the sentence a refusal prints from it.
///
/// A macro and not two `const`s, because the refusal has to NAME the allowed set and a hand-written
/// copy of it in a message is a copy that goes stale silently - which for the one refusal whose
/// whole value is "here is what you may write instead" is the worst place to keep a second list.
/// `concat!` folds the names into a `&'static str` at compile time, so `Construct::why` stays a
/// `const fn` and there is exactly one place a name is typed.
macro_rules! allowlist {
    ($($name:literal => $called:ident,)+) => {
        /// The function names a generic call may have. **Everything else is refused.**
        ///
        /// Short on purpose, and short is what makes the allowlist possible: a measure needs
        /// percentiles, a null guard, a rounding, a min/max pair and the aggregates. It does not
        /// need the twelve hundred other functions each target ships, and it is those that carry
        /// `getenv`, `pg_read_file`, `query_to_xml`, `lo_import`, `nextval`, `pg_sleep`, `dblink`,
        /// `dictGet` and `getSetting`.
        ///
        /// Alphabetical, and asserted to be: the order is what makes a diff to this list readable,
        /// and a duplicate with two different marks would be decided by whichever came first.
        ///
        /// Matched with `eq_ignore_ascii_case`, so one entry covers a dialect's own casing -
        /// `SUMIF` is how `sumIf` is written here. A spelling that differs by more than case, such
        /// as `stddevPop` beside `STDDEV_POP`, needs its own line, and needing one is the
        /// review-visible edit this design is for.
        pub(super) const ALLOWED_FUNCTION_NAMES: &[(&str, Called)] = &[$(($name, Called::$called),)+];

        /// Why a name outside the allowlist is refused, with the allowed set spelled out.
        ///
        /// The list half is generated from [`ALLOWED_FUNCTION_NAMES`] above, so the sentence an
        /// author reads cannot disagree with the check that refused them.
        pub(super) const UNKNOWN_FUNCTION_WHY: &str = concat!(
            "a generic call carries no lowering at all - the dialect layer has no typed node for it, so the \
             name is written verbatim into every target - which makes a function name unbounded reach: \
             measured against DuckDB 1.5.5, `MAX(getenv('X'))` RETURNS the value of an environment variable \
             the sutura process holds, and the compiled targets also carry names that read a file, write \
             one, mutate a sequence and execute a query of their own. No denylist bounds that space, so the \
             set a measure may call is enumerated instead, and a variant needing one more name is a \
             review-visible edit to it. Those names are:",
            $(" ", $name,)+
        );
    };
}

allowlist! {
    "ABS" => Scalar,
    "ARRAY_AGG" => Aggregate,
    "AVG" => Aggregate,
    "CAST" => Scalar,
    "COALESCE" => Scalar,
    "COUNT" => Aggregate,
    "COUNTIF" => Aggregate,
    "COUNT_IF" => Aggregate,
    "GREATEST" => Scalar,
    "GROUP_CONCAT" => Aggregate,
    "LEAST" => Scalar,
    "LIST" => Aggregate,
    "MAX" => Aggregate,
    "MEDIAN" => Aggregate,
    "MIN" => Aggregate,
    "NULLIF" => Scalar,
    "PERCENTILE_CONT" => Aggregate,
    "PERCENTILE_DISC" => Aggregate,
    "ROUND" => Scalar,
    "STDDEV" => Aggregate,
    "STDDEV_POP" => Aggregate,
    "STDDEV_SAMP" => Aggregate,
    "STRING_AGG" => Aggregate,
    "SUM" => Aggregate,
    "SUMIF" => Aggregate,
    "SUM_IF" => Aggregate,
    "TRY_CAST" => Scalar,
    "UNIQEXACT" => Aggregate,
    "VARIANCE" => Aggregate,
    "VAR_POP" => Aggregate,
    "VAR_SAMP" => Aggregate,
}

/// What the allowlist says calling this name does, or `None` if it does not list it.
pub(super) fn called(name: &str) -> Option<Called> {
    ALLOWED_FUNCTION_NAMES
        .iter()
        .find(|&&(known, _)| name.eq_ignore_ascii_case(known))
        .map(|&(_, called)| called)
}

/// Does calling this name aggregate a group?
///
/// Asked only of a name the dialect layer gave a generic node, so this is not a second copy of
/// `traversal::is_aggregate`: it is the answer for the names that classifier has no node for.
pub(super) fn aggregates(name: &str) -> bool {
    matches!(called(name), Some(Called::Aggregate))
}

/// What is wrong with a called function's name, if anything.
///
/// A generic call is the case where the dialect layer has no typed node, so it emits the name
/// verbatim into every target with no lowering whatsoever - `STRFTIME`, `NOW` and an unknown UDF all
/// come out unchanged, measured. **A UDF is no longer allowed**, and that is the reversal: the
/// author naming a dialect is a claim about portability, not a grant to call anything the target
/// happens to have. A date name keeps its own refusal, because "a date function, and here is the
/// argument-order defect" is a better sentence than "not on the list".
///
/// The dotted-name branch fires for no input the authoring dialect produces today - it parses
/// `secret.udf(x)` into a `dot` node, which [`Construct::Opaque`] refuses instead. It stays because
/// how a qualified call parses is an upstream detail and the branch costs one line, and it is asked
/// FIRST so that a dotted name is not reported as an unknown one when the schema is the problem.
pub(super) fn name_refusal(name: &str) -> Option<Construct> {
    if name.contains('.') {
        return Some(Construct::QualifiedFunctionName);
    }
    if DATE_FUNCTION_NAMES.iter().any(|known| name.eq_ignore_ascii_case(known)) {
        return Some(Construct::DateTimeFunction);
    }
    if called(name).is_none() {
        return Some(Construct::UnknownFunction);
    }
    None
}

/// What is wrong with a node kind, if anything.
pub(super) fn kind_refusal(kind: &str) -> Option<Construct> {
    if QUERY_KINDS.contains(&kind) {
        return Some(Construct::Query);
    }
    if TABLE_KINDS.contains(&kind) {
        return Some(Construct::TableReference);
    }
    if PARAMETER_KINDS.contains(&kind) {
        return Some(Construct::BindParameter);
    }
    if OPAQUE_KINDS.contains(&kind) {
        return Some(Construct::Opaque);
    }
    if DATE_KINDS.contains(&kind) {
        return Some(Construct::DateTimeFunction);
    }
    match kind {
        "is" | "is_true" | "is_false" => Some(Construct::IsTrue),
        "int_div" => Some(Construct::IntegerDivision),
        _ => None,
    }
}
