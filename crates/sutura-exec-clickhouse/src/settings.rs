//! The server settings that decide what the rows say, sent twice: as driver options on every
//! connection, and as each statement's own `SETTINGS` clause.
//!
//! **The clause is what holds under `EXECUTE AS`.** The server switches to the named user's profile
//! and clamps the request's settings to it, dropping without an error each one that profile
//! forbids - a `readonly = 1` profile forbids them all, and a left join then answers `''` for an
//! unmatched `String` key. A statement's own `SETTINGS` is checked against the same profile and
//! refused instead: `READONLY` (164) under `readonly = 1`, `SETTING_CONSTRAINT_VIOLATION` (452) for
//! a constraint. The clause follows the `SELECT` and precedes no `FORMAT`, so it is the switched
//! statement's own rather than one `EXECUTE AS` lifts onto the outer statement. Read from the
//! server's source (26.9) and measured by the `readonly` tier cells in `tests/adbc_execute_as.rs`.
//!
//! Limits: the server skips the check for a value the profile already holds, which leaves the
//! setting as sutura needs it. Whether the outer statement still enforces the deadline under the
//! switch is not measured; the clause sends it to the switched statement too.

/// Each setting and the value sutura needs, as the driver option and the clause both send it.
const ANSWER_SHAPING: [(&str, &str); 6] = [
    ("join_use_nulls", "1"),
    ("output_format_json_quote_denormals", "1"),
    ("output_format_json_quote_64bit_integers", "1"),
    ("output_format_json_quote_decimals", "1"),
    ("output_format_decimal_trailing_zeros", "1"),
    ("timeout_overflow_mode", "throw"),
];

/// The driver's option key for each setting, and its value.
pub(crate) fn driver_options() -> impl Iterator<Item = (String, &'static str)> {
    ANSWER_SHAPING
        .iter()
        .map(|&(name, value)| (format!("clickhouse.setting.{name}"), value))
}

/// `select` with its own `SETTINGS` clause: the deadline's `max_execution_time` and every setting
/// above. A value that is not digits is a constant word here, quoted as a string literal.
pub(crate) fn with_clause(select: &str, max_execution_time: u64) -> String {
    let pinned: Vec<String> = ANSWER_SHAPING
        .iter()
        .map(|&(name, value)| {
            if value.bytes().all(|byte| byte.is_ascii_digit()) {
                format!("{name} = {value}")
            } else {
                format!("{name} = '{value}'")
            }
        })
        .collect();
    format!(
        "{select} SETTINGS max_execution_time = {max_execution_time}, {}",
        pinned.join(", ")
    )
}
