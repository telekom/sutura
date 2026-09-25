//! Recognising a wren expression as one of sutura's closed-vocabulary shapes.
//!
//! **This is not a SQL parser, and every function here is deliberately narrow rather than
//! defensively broad.** Each recognises exactly one textual shape - a bare column name, one
//! aggregate over one column, two such terms divided, an equality between two declared columns -
//! and returns `None` for anything else, including a shape a real parser would happily read. That
//! is the point: `sutura_domain::measure`'s own module header states the security property as *no
//! free-text SQL reaches a statement unexamined*, and a recogniser that occasionally guessed right
//! about a wider grammar would be exactly the free-text path this converter exists to avoid. What
//! [`super::convert`] does with a `None` is refuse the item by name, never fall back to storing the
//! raw text.
//!
//! Also why this lives beside the CLI's import command and not in `sutura_domain`: it is offline,
//! authorship-time text recognition over a foreign format's strings, not a domain rule, and
//! `AGENTS.md`'s "the boot path never parses foreign SQL" is about the boot path - nothing here
//! runs there.

use sutura_domain::model::Aggregate;

/// Every character of `text` is legal in a bare identifier and the first is not a digit - the
/// shape `[A-Za-z_][A-Za-z0-9_]*`, `sutura_domain`'s own identifier grammar, checked here without
/// constructing one: a column name that fails this is reported as free-SQL rather than as an
/// unparseable identifier, which is the more useful thing for a reader of the report to see.
fn is_bare_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `expr` read as a bare column name, for a cube dimension or time dimension whose `expression` is
/// nothing more than the column itself.
pub(crate) fn bare_column(expr: &str) -> Option<&str> {
    let trimmed = expr.trim();
    is_bare_identifier(trimmed).then_some(trimmed)
}

/// Splits `FUNC(ARG)` into the function name and its argument, both trimmed - or `None` if `text`
/// is not exactly one call with no nesting.
fn function_call(text: &str) -> Option<(&str, &str)> {
    let (name, rest) = text.trim().split_once('(')?;
    let arg = rest.strip_suffix(')')?;
    let name = name.trim();
    if name.is_empty() || arg.contains('(') || arg.contains(')') {
        return None;
    }
    Some((name, arg.trim()))
}

/// `arg` read as `DISTINCT <column>` - the one shape [`term`] admits inside `COUNT(..)`.
fn distinct_column(arg: &str) -> Option<&str> {
    let (keyword, column) = arg.split_once(char::is_whitespace)?;
    keyword.eq_ignore_ascii_case("distinct").then(|| column.trim())
}

/// One recognised aggregate over one column - the wren-side reading of
/// [`sutura_domain::measure::AggregatedColumn`], before a [`sutura_domain::model::ModelName`] or
/// [`sutura_domain::model::ColumnName`] has parsed it.
pub(crate) struct Term {
    pub(crate) aggregate: Aggregate,
    pub(crate) column: String,
}

/// `expr` read as one aggregate over one column: `SUM(x)`, `AVG(x)`, `MIN(x)`, `MAX(x)`,
/// `COUNT(x)`, or `COUNT(DISTINCT x)`. Case-insensitive on the function name and on the `DISTINCT`
/// keyword, because a wren author writes both freely; the column itself is not case-folded, for the
/// reason `sutura_domain::model::parse_identifier` gives - a quoted identifier is case-sensitive at
/// the data system, and folding it here would recognise a term that then fails to attach at load.
pub(crate) fn term(expr: &str) -> Option<Term> {
    let (name, arg) = function_call(expr)?;
    if name.eq_ignore_ascii_case("count") {
        if let Some(column) = distinct_column(arg) {
            return is_bare_identifier(column).then(|| Term {
                aggregate: Aggregate::CountDistinct,
                column: column.to_owned(),
            });
        }
        return is_bare_identifier(arg).then(|| Term {
            aggregate: Aggregate::Count,
            column: arg.to_owned(),
        });
    }
    let aggregate = if name.eq_ignore_ascii_case("sum") {
        Aggregate::Sum
    } else if name.eq_ignore_ascii_case("avg") {
        Aggregate::Avg
    } else if name.eq_ignore_ascii_case("min") {
        Aggregate::Min
    } else if name.eq_ignore_ascii_case("max") {
        Aggregate::Max
    } else {
        return None;
    };
    is_bare_identifier(arg).then(|| Term {
        aggregate,
        column: arg.to_owned(),
    })
}

/// One measure, in the two shapes [`sutura_domain::measure::Measure`] admits.
pub(crate) enum Measure {
    Simple(Term),
    Ratio { numerator: Term, denominator: Term },
}

/// Splits `expr` on a `/` that sits outside every pair of parentheses - the one place a ratio's two
/// terms meet - or `None` if there is none.
fn split_top_level_slash(expr: &str) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (index, character) in expr.char_indices() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => {
                let (left, rest) = expr.split_at(index);
                let (_slash, right) = rest.split_at(1);
                return Some((left, right));
            }
            _ => {}
        }
    }
    None
}

/// `expr` read as a wren cube measure: one term, or one term divided by another.
pub(crate) fn measure(expr: &str) -> Option<Measure> {
    if let Some((left, right)) = split_top_level_slash(expr) {
        let numerator = term(left)?;
        let denominator = term(right)?;
        return Some(Measure::Ratio { numerator, denominator });
    }
    term(expr).map(Measure::Simple)
}

/// One side of a recognised equi-join: which model, which column.
pub(crate) struct Side {
    pub(crate) model: String,
    pub(crate) column: String,
}

/// `text` read as `model.column`, with exactly one `.` and both halves bare identifiers.
fn qualified(text: &str) -> Option<Side> {
    let (model, column) = text.trim().split_once('.')?;
    (is_bare_identifier(model) && is_bare_identifier(column)).then(|| Side {
        model: model.to_owned(),
        column: column.to_owned(),
    })
}

/// `condition` read as a plain equality between two declared columns - the one shape
/// [`sutura_domain::catalog::Relationship`]'s own module header licenses, over the escape hatch a
/// free condition string is: `a.x = b.y OR 1 = 1` is a valid wren condition and is refused here
/// because a second `=` anywhere makes this `None`, not because the disjunction is parsed and
/// rejected.
pub(crate) fn equi_join(condition: &str) -> Option<(Side, Side)> {
    let (left, right) = condition.split_once('=')?;
    if right.contains('=') {
        return None;
    }
    Some((qualified(left)?, qualified(right)?))
}

#[cfg(test)]
mod tests;
