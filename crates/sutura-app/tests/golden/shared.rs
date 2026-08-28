//! Helpers every axis of the golden suite shares.
//!
//! In `tests/golden/` rather than beside the target, because `tests/*.rs` at the top level is a
//! test target and this is not one.

use sutura_domain::query::Query;
use sutura_domain::warehouse::{RowSet, Value};
use sutura_sql::Dialect;

use crate::adapters::{questions, read_question};

/// Settings every snapshot in this suite uses.
///
/// The snapshot path is set explicitly so the files land in `tests/snapshots/` next to the corpus
/// rather than wherever the macro would guess from the module path - the axes are modules under
/// `tests/golden/`, so the path is relative to there - and the module prefix is
/// dropped so a snapshot is named after the question and the adapter rather than after the nesting
/// the matrix produces.
///
/// An empty suffix is left unset rather than set to nothing, because insta writes the separator
/// whenever a suffix is present and `anchor_mismatch@.snap` is not a name anybody wants to read.
pub(crate) fn settings(suffix: &str) -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../snapshots");
    settings.set_prepend_module_to_snapshot(false);
    if !suffix.is_empty() {
        settings.set_snapshot_suffix(suffix);
    }
    settings
}

/// Renders a plan, because compiling no longer does.
///
/// The dialect is named at the call site now rather than passed into `compile`, which is the point
/// of the split: a golden that pins SQL is asking for a rendering, and says so. It is also a
/// different CRATE now - `sutura-sql`, a dev-dependency here - so a test that wants SQL declares
/// that it wants SQL and nothing in `src/` pulls a generator in on its behalf.
pub(crate) fn sql_for(plan: &sutura_domain::plan::QueryPlan, dialect: Dialect) -> sutura_sql::GeneratedQuery {
    sutura_sql::generate(plan, dialect).expect("a planned question renders")
}

/// An error and every cause beneath it, outermost first, as one block.
///
/// Snapshotted rather than asserted with `contains`, for the reason the service walks the chain at
/// all: `Display` on a `thiserror` enum prints the outermost message and stops, and the outermost
/// message here is "the data system did not answer", which names nothing. What a reader needs is
/// the column and which of the three non-finite values it was, and both of those live one and two
/// levels down.
pub(crate) fn chain(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

/// One cell, with its type kept and a float cut to twelve significant digits.
///
/// **The truncation is what makes a row snapshot per data system worth having.** Summing the same
/// rows in a different order changes the last place of an `f64`, so pinning the full binary
/// expansion asserts WHICH ENGINE RAN - and the snapshot then goes red on an upstream version bump
/// that altered no number anybody reports. Twelve digits is far beyond any figure a metric carries
/// and far short of the noise. The variant is kept, so a measure that changed from an exact integer
/// to a float is still a diff.
fn cell(value: &Value) -> String {
    match *value {
        Value::Null => String::from("Null"),
        Value::Integer(v) => format!("Integer({v})"),
        Value::Real(v) => format!("Real({v:.12e})"),
        Value::Text(ref v) => format!("Text({v})"),
    }
}

/// A result set as a snapshot: the labels, then one line per row.
///
/// Text rather than the serialized `RowSet`, because what a reader checks is the shape and the
/// numbers, and a tab-separated block is the form the CLI already prints them in.
pub(crate) fn stable(rows: &RowSet) -> String {
    let mut out = rows.columns().join("\t");
    for row in rows.rows() {
        out.push('\n');
        out.push_str(&row.iter().map(cell).collect::<Vec<String>>().join("\t"));
    }
    out
}

/// One question by the name of its file.
///
/// Found in `questions` rather than joined onto a path, so a fixture that was renamed fails saying
/// so instead of failing as a missing file two frames further in.
pub(crate) fn question(file: &str) -> Query {
    let path = questions()
        .into_iter()
        .find(|path| path.file_name().is_some_and(|name| name == file))
        .unwrap_or_else(|| panic!("there is no question fixture called {file}"));
    read_question(&path)
}

/// Every refusal variant a question file can provoke, and the fixture that provokes it.
///
/// A table rather than a test each, so the exhaustiveness assertion can be written against it.
///
/// Three variants are absent on purpose, because no question file can reach one, and each has its
/// own test in `golden/service.rs` instead. `PlanSpansTwoSources` needs a catalog naming two data
/// systems and `SourceUnavailable` a plan for a data system nobody opened - both decided above the
/// port, by the service rather than by the compiler. `ResultTooLarge` is the third and is absent for
/// a different reason worth keeping straight: it is decided AFTER a data system has answered, and
/// `a_refused_question_never_reaches_the_data_system` asserts of every entry in this table that
/// nothing ran. A fixture that provoked it here would make that assertion false.
pub(crate) const PROVOKED: &[(&str, &str)] = &[
    ("refused-metric-unknown", "MetricUnknown"),
    ("refused-grain-not-supported", "GrainNotSupported"),
    ("refused-dimension-not-permitted", "DimensionNotPermitted"),
    ("refused-dimension-not-filterable", "DimensionNotFilterable"),
    ("refused-value-not-allowed", "DimensionValueNotAllowed"),
    ("refused-duplicate-dimension", "DuplicateDimension"),
    ("refused-too-many-dimensions", "TooManyDimensions"),
    ("refused-range-too-long", "TimeRangeTooLong"),
];
