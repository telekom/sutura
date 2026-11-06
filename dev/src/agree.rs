//! `sutura-dev agree` - are two `sutura serve` answers the same answer?
//!
//! The command line, the typed errors and the entry points are here; **reading an answer and judging
//! it is not built yet**, and every input is refused with [`AgreeError::NotBuilt`] rather than
//! answered. A comparison that said *agree* before it compared anything would be the worst thing this
//! command could do, so the absence is an error and not a pass.
//!
//! What it will be: both answers read into the domain's `RowSet` and judged by
//! `sutura_domain::warehouse::agreement`, which owns what *the same* means. This module adds no
//! comparison of its own.

#![expect(
    dead_code,
    reason = "the API is declared ahead of the reading and judging that construct it"
)]

use std::path::{Path, PathBuf};

use sutura_domain::warehouse::agreement::{ContentDisagreement, OrderDisagreement};
use sutura_domain::warehouse::{MalformedRowSet, NotFinite, RowSet};

#[cfg(test)]
mod tests;

/// The one line of usage, which is also what a malformed command line answers.
pub(crate) const USAGE: &str = "sutura-dev agree <a.json> <b.json> [--ordered] [--real <column>]...";

/// Why two answers could not be called the same answer.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AgreeError {
    /// Reading and judging an answer is not built, so no input is accepted.
    #[error("sutura-dev agree does not read answers yet")]
    NotBuilt,
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
    pub(crate) const fn parse(_args: &[String]) -> Result<Self, AgreeError> {
        Err(AgreeError::NotBuilt)
    }
}

/// What agreeing compared, for the line printed on success.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Agreed {
    pub(crate) rows: usize,
    pub(crate) ordered: bool,
}

/// Reads both answers and judges them.
pub(crate) const fn run(_args: &[String]) -> Result<Agreed, AgreeError> {
    Err(AgreeError::NotBuilt)
}

/// The two policy assertions, content first and order only when it was promised.
const fn agree(_left: &RowSet, _right: &RowSet, _ordered: bool) -> Result<Agreed, AgreeError> {
    Err(AgreeError::NotBuilt)
}

/// One answer body as a [`RowSet`]. `path` only names the file in an error.
const fn answer_rows(_path: &Path, _text: &str, _real: &[String]) -> Result<RowSet, AgreeError> {
    Err(AgreeError::NotBuilt)
}
