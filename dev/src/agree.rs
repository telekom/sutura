//! `sutura-dev agree` - are two `sutura serve` answers the same answer?
//!
//! It parses the JSON body of two `/v1/query` answers into the domain's [`RowSet`] and hands them to
//! `sutura_domain::warehouse::agreement`, which owns what *the same* means: the variant is part of
//! the value, rows are a multiset, a [`Value::Real`] is compared at
//! [`RealTolerance::DIFFERENTIAL`] and nothing else is approximate, and order is a separate
//! assertion. **This module adds no comparison.** It decides one thing the policy cannot - which
//! variant a wire cell is - and reports what the policy found in the policy's own words.
//!
//! # The wire erases the variant, so the caller declares the Real columns
//!
//! An answer renders every cell as text through `Value::render` (`sutura-http`'s `wire::render`):
//! `Null` is the word `null`, an integer its digits, a real its shortest round-trip form, and a
//! decimal whatever text the adapter gave it. Nothing on the wire says which was which, so a cell is
//! read as follows, and **only the first rule loosens anything**:
//!
//! 1. in a column named by `--real <label>`: `null` is [`Value::Null`], anything else must be a
//!    finite number and is a [`Value::Real`] - an `avg` that happens to land on `12` still reads as
//!    the real it is, which no spelling-based guess could tell from an integer;
//! 2. elsewhere: `null` is [`Value::Null`], digits that round-trip through `i64` are
//!    [`Value::Integer`], and everything else is [`Value::Text`], compared exactly.
//!
//! So a tolerance is never applied by inference. A column that holds a real and was not declared is
//! compared exactly, which can only report a disagreement that is not one - the direction that fails
//! loud - and never hides one. A decimal sum is left out of `--real` for the same reason: at 13
//! significant digits a one-cent error in a figure over 10^11 is below the tolerance.
//!
//! # What this cannot see
//!
//! Two variants that render to the same text. `Integer(5)` and `Text("5")` are one cell here, and
//! `Null` and the word `null` are too, which is the pair `agreement`'s header is written about. They
//! are one cell on both sides of every comparison, so this reports no false disagreement; it reports
//! no real one either, and a decoder that answered the wrong variant with the same spelling is not
//! found by this tool. A variant difference that DOES change the spelling (`5` against `5.00`) is a
//! disagreement, which is the case a decimal-scale defect takes.
//!
//! Nor does a shared compiler show: both answers come from the same semantic plan, so a defect in the
//! plan they share agrees with itself.

use std::path::{Path, PathBuf};

use sutura_domain::warehouse::agreement::{
    ContentDisagreement, OrderDisagreement, RealTolerance, agree_on_content, agree_on_order,
};
use sutura_domain::warehouse::{MalformedRowSet, NotFinite, Real, RowSet, Value};

/// The tolerance every comparison is made at - the differential legs' own, and the only one.
const TOLERANCE: RealTolerance = RealTolerance::DIFFERENTIAL;

#[cfg(test)]
mod tests;

/// The one line of usage, which is also what a malformed command line answers.
pub(crate) const USAGE: &str = "sutura-dev agree <a.json> <b.json> [--ordered] [--real <column>]...";

/// Why two answers could not be called the same answer.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AgreeError {
    /// The command line is not `agree <a> <b> [flags]`.
    #[error("{reason}\nusage: {}", USAGE)]
    Usage { reason: &'static str },
    /// An answer file could not be read.
    #[error("could not read {}: {source}", .path.display())]
    Unreadable { path: PathBuf, source: std::io::Error },
    /// An answer file is not JSON.
    #[error("{} is not JSON: {source}", .path.display())]
    NotJson { path: PathBuf, source: serde_json::Error },
    /// The file holds a refusal, so there is no answer to compare.
    #[error("{} holds a refusal ({code}), not an answer", .path.display())]
    Refused { path: PathBuf, code: String },
    /// The file is JSON but not the body of an answer.
    #[error("{} is not the body of an answer: {what}", .path.display())]
    NotAnAnswer { path: PathBuf, what: &'static str },
    /// The answer's rows are not as wide as its columns.
    #[error("{} is ragged: {source}", .path.display())]
    Ragged { path: PathBuf, source: MalformedRowSet },
    /// `--real` named a column the answer does not have.
    #[error("{} has no column {label:?}, which --real names", .path.display())]
    NoSuchRealColumn { path: PathBuf, label: String },
    /// A cell in a `--real` column is not a finite number.
    #[error("{}: row {row}, column {column:?} is {text:?}, which --real declares a number", .path.display())]
    NotAReal {
        path: PathBuf,
        row: usize,
        column: String,
        text: String,
        source: Option<NotFinite>,
    },
    /// The two answers do not carry the same content.
    #[error(transparent)]
    Content(#[from] ContentDisagreement),
    /// The two answers carry the same content in different orders.
    #[error(transparent)]
    Order(#[from] OrderDisagreement),
}

impl AgreeError {
    /// Whether this is a malformed command line rather than a finding.
    pub(crate) const fn is_usage(&self) -> bool {
        matches!(*self, Self::Usage { .. })
    }
}

/// A parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Arguments {
    left: PathBuf,
    right: PathBuf,
    ordered: bool,
    real: Vec<String>,
}

impl Arguments {
    /// Parses what follows `agree`: two paths, then `--ordered` and any number of `--real <column>`.
    pub(crate) fn parse(args: &[String]) -> Result<Self, AgreeError> {
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut ordered = false;
        let mut real: Vec<String> = Vec::new();
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "--ordered" => ordered = true,
                "--real" => real.push(rest.next().cloned().ok_or(AgreeError::Usage {
                    reason: "--real needs a column label",
                })?),
                flag if flag.starts_with("--") => {
                    return Err(AgreeError::Usage { reason: "unknown flag" });
                }
                path => paths.push(PathBuf::from(path)),
            }
        }
        let [left, right] = <[PathBuf; 2]>::try_from(paths).map_err(|_other| AgreeError::Usage {
            reason: "two answer files are needed",
        })?;
        Ok(Self {
            left,
            right,
            ordered,
            real,
        })
    }
}

/// What agreeing compared, for the line printed on success.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Agreed {
    pub(crate) rows: usize,
    pub(crate) ordered: bool,
}

/// Reads both answers and judges them.
pub(crate) fn run(args: &[String]) -> Result<Agreed, AgreeError> {
    let arguments = Arguments::parse(args)?;
    let left = read(&arguments.left, &arguments.real)?;
    let right = read(&arguments.right, &arguments.real)?;
    agree(&left, &right, arguments.ordered)
}

/// The two policy assertions, content first and order only when it was promised.
fn agree(left: &RowSet, right: &RowSet, ordered: bool) -> Result<Agreed, AgreeError> {
    agree_on_content(left, right, TOLERANCE)?;
    if ordered {
        agree_on_order(left, right, TOLERANCE)?;
    }
    Ok(Agreed {
        rows: left.rows().len(),
        ordered,
    })
}

fn read(path: &Path, real: &[String]) -> Result<RowSet, AgreeError> {
    let text = std::fs::read_to_string(path).map_err(|source| AgreeError::Unreadable {
        path: path.to_path_buf(),
        source,
    })?;
    answer_rows(path, &text, real)
}

/// One answer body as a [`RowSet`]. `path` only names the file in an error.
fn answer_rows(path: &Path, text: &str, real: &[String]) -> Result<RowSet, AgreeError> {
    let not_an_answer = |what: &'static str| AgreeError::NotAnAnswer {
        path: path.to_path_buf(),
        what,
    };
    let body: serde_json::Value = serde_json::from_str(text).map_err(|source| AgreeError::NotJson {
        path: path.to_path_buf(),
        source,
    })?;
    match body.get("outcome").and_then(serde_json::Value::as_str) {
        Some("answer") => {}
        Some("refusal") => {
            return Err(AgreeError::Refused {
                path: path.to_path_buf(),
                code: body
                    .pointer("/reason/code")
                    .and_then(serde_json::Value::as_str)
                    .map_or_else(|| String::from("no code"), String::from),
            });
        }
        _ => return Err(not_an_answer("`outcome` is neither `answer` nor `refusal`")),
    }

    let columns: Vec<String> = body
        .get("columns")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| not_an_answer("no `columns` array"))?
        .iter()
        .map(|label| label.as_str().map(String::from))
        .collect::<Option<_>>()
        .ok_or_else(|| not_an_answer("a column label is not text"))?;

    let real_at: Vec<bool> = columns.iter().map(|label| real.contains(label)).collect();
    if let Some(label) = real.iter().find(|label| !columns.contains(label)) {
        return Err(AgreeError::NoSuchRealColumn {
            path: path.to_path_buf(),
            label: label.clone(),
        });
    }

    let mut rows: Vec<Vec<Value>> = Vec::new();
    for (index, row) in body
        .get("rows")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| not_an_answer("no `rows` array"))?
        .iter()
        .enumerate()
    {
        let cells = row.as_array().ok_or_else(|| not_an_answer("a row is not an array"))?;
        let mut parsed = Vec::with_capacity(cells.len());
        for (position, cell) in cells.iter().enumerate() {
            let text = cell.as_str().ok_or_else(|| not_an_answer("a cell is not text"))?;
            let is_real = real_at.get(position).copied().unwrap_or(false);
            parsed.push(if is_real {
                real_cell(text).map_err(|source| AgreeError::NotAReal {
                    path: path.to_path_buf(),
                    row: index,
                    column: columns.get(position).cloned().unwrap_or_default(),
                    text: String::from(text),
                    source,
                })?
            } else {
                exact_cell(text)
            });
        }
        rows.push(parsed);
    }
    RowSet::new(columns, rows).map_err(|source| AgreeError::Ragged {
        path: path.to_path_buf(),
        source,
    })
}

/// A cell of a column the caller declared real. `Err(None)` is text that is no number at all.
fn real_cell(text: &str) -> Result<Value, Option<NotFinite>> {
    if text == "null" {
        return Ok(Value::Null);
    }
    let number: f64 = text.parse().map_err(|_unparsed| None)?;
    Real::parse(number).map(Value::Real).map_err(Some)
}

/// A cell of any other column: null, an integer that round-trips, or the text as given.
fn exact_cell(text: &str) -> Value {
    if text == "null" {
        return Value::Null;
    }
    match text.parse::<i64>() {
        Ok(number) if number.to_string() == text => Value::Integer(number),
        _ => Value::Text(String::from(text)),
    }
}
