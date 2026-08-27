//! The name lists a refusal is decided by: which node kinds, and which called function names.
//!
//! Split out of [`super`] because it is data rather than reasoning, and because `cargo xtask
//! max-lines` caps a file under `crates/` at a thousand lines and cannot exempt anything - the
//! module that argues about SQL should not be competing for room with sixty date-function names.
//!
//! **A typo in any list here fails CLOSED, which is the direction that matters.** Every entry is
//! compared against `Expression::variant_name` output or against a called function's name, so a
//! misspelling simply never matches - and never matching means the construct is not refused for
//! *that* reason and falls through to the next check, where the parent module's structural guards
//! are what decide. Nothing in here can accidentally refuse something that should have been allowed.

use super::Construct;

/// A generic function call is emitted verbatim into every target, so its name is checked rather
/// than trusted. These are the date and time names, refused for the reason
/// [`Construct::DateTimeFunction`] gives.
///
/// Not exhaustive over every dialect's date vocabulary, and it does not need to be: truncation is
/// the generator's, from `Grain`, so no metric has a reason to write one. An unlisted name that is
/// nonetheless a date function renders verbatim, which is the residual risk and is stated in
/// `docs/adr/0004`.
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

/// What is wrong with a called function's name, if anything.
///
/// A generic call is the case where the dialect layer has no typed node, so it emits the name
/// verbatim into every target with no lowering whatsoever - `STRFTIME`, `NOW` and an unknown UDF all
/// come out unchanged, measured. A UDF is allowed: the author named a dialect or claimed portability
/// and took that claim. A date name is not, for the reason [`Construct::DateTimeFunction`] gives.
///
/// The dotted-name branch fires for no input the authoring dialect produces today - it parses
/// `secret.udf(x)` into a `dot` node, which [`Construct::Opaque`] refuses instead. It stays because
/// how a qualified call parses is an upstream detail and the branch costs one line.
pub(super) fn name_refusal(name: &str) -> Option<Construct> {
    if name.contains('.') {
        return Some(Construct::QualifiedFunctionName);
    }
    if DATE_FUNCTION_NAMES.iter().any(|known| name.eq_ignore_ascii_case(known)) {
        return Some(Construct::DateTimeFunction);
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
