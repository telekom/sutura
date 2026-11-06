//! The server settings sutura pins, sent twice: as driver options on every connection, and as the
//! own `SETTINGS` clause of each statement the adapter runs for an answer. The fixture loader's
//! statements carry no clause.
//!
//! **The clause is what holds under `EXECUTE AS`.** The server switches to the named user's profile
//! and clamps the request's settings to it, dropping without an error each one that profile
//! forbids - a `readonly = 1` profile forbids them all, and a left join then answers `''` for an
//! unmatched `String` key. A statement's own `SETTINGS` is checked against the same profile and
//! refused instead: `READONLY` (164) under `readonly = 1`, `SETTING_CONSTRAINT_VIOLATION` (452) for
//! a constraint - so a profile that caps `max_execution_time` below the deadline is refused on
//! every request, not clamped.
//!
//! **The clause is the statement's own only if it is the first `SETTINGS` after a bare `SELECT`.**
//! The server makes a `SETTINGS` output-level when it follows a `FORMAT`, a first `SETTINGS`, an
//! `INTO OUTFILE`, or a parenthesised last arm - a statement that is one parenthesised `SELECT` is
//! one - and `EXECUTE AS` lifts an output-level one onto the outer statement, past the switch.
//! [`with_clause`] refuses a statement that opens with a parenthesis or carries any of those
//! keywords at its top level. Read from the server's source (26.9); measured by the `readonly`
//! tier cells in `tests/adbc_execute_as.rs`, in CI only, for a bare `SELECT`.
//!
//! Limits: the server skips the check for a value the profile already holds, which leaves the
//! setting as sutura needs it. Whether the outer statement still enforces the deadline under the
//! switch is not measured; the clause sends it to the switched statement too. The caller's profile
//! still decides every setting sutura does not pin - `limit`, `max_result_rows` with
//! `result_overflow_mode = 'break'`, `session_timezone`, `count_distinct_implementation` among
//! them - and any of those can change the rows with no error; not measured.

/// Each setting and the value sutura needs, as the driver option and the clause both send it.
const ANSWER_SHAPING: [(&str, &str); 6] = [
    ("join_use_nulls", "1"),
    ("output_format_json_quote_denormals", "1"),
    ("output_format_json_quote_64bit_integers", "1"),
    ("output_format_json_quote_decimals", "1"),
    ("output_format_decimal_trailing_zeros", "1"),
    ("timeout_overflow_mode", "throw"),
];

/// The top-level keywords after which a `SETTINGS` would be output-level, or would follow a
/// possibly parenthesised last arm.
const LIFTING: [&str; 6] = ["SETTINGS", "FORMAT", "INTO", "UNION", "INTERSECT", "EXCEPT"];

/// What [`Lifted`] names for a statement that opens with a parenthesis: its whole body is then one
/// parenthesised arm, and a clause after it is output-level.
const OPENING_PARENTHESIS: &str = "(";

/// A statement whose top level carries this keyword, or that opens with a parenthesis, so a clause
/// after it could be lifted past the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lifted(pub(crate) &'static str);

/// The driver's option key for each setting, and its value.
pub(crate) fn driver_options() -> impl Iterator<Item = (String, &'static str)> {
    ANSWER_SHAPING
        .iter()
        .map(|&(name, value)| (format!("clickhouse.setting.{name}"), value))
}

/// `select` with its own `SETTINGS` clause: the deadline's `max_execution_time` and every setting
/// above. A value that is not digits is a constant word here, quoted as a string literal.
///
/// # Errors
///
/// [`Lifted`] for a `select` that opens with a parenthesis, or whose top level, outside parentheses
/// and quotes, carries a keyword of [`LIFTING`].
pub(crate) fn with_clause(select: &str, max_execution_time: u64) -> Result<String, Lifted> {
    if let Some(keyword) = top_level_lifting(select) {
        return Err(Lifted(keyword));
    }
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
    Ok(format!(
        "{select} SETTINGS max_execution_time = {max_execution_time}, {}",
        pinned.join(", ")
    ))
}

/// [`OPENING_PARENTHESIS`] for a `select` that opens with one, else the first keyword of [`LIFTING`]
/// that `select` spells as a word at depth zero, skipping string literals and quoted identifiers.
fn top_level_lifting(select: &str) -> Option<&'static str> {
    if select.trim_start().starts_with('(') {
        return Some(OPENING_PARENTHESIS);
    }
    let mut depth = 0_usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut word = String::new();
    for character in select.chars().chain(core::iter::once(' ')) {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == open {
                quote = None;
            }
            continue;
        }
        if character.is_ascii_alphanumeric() || character == '_' {
            word.push(character);
            continue;
        }
        if depth == 0
            && let Some(&keyword) = LIFTING.iter().find(|keyword| keyword.eq_ignore_ascii_case(&word))
        {
            return Some(keyword);
        }
        word.clear();
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
