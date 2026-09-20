//! Case definitions read from tracked data files, not Rust.
//!
//! Each case is a `.case` file under `corpus/cases/`, embedded at compile time with
//! [`include_str!`] and parsed here by a typed loader. Adding a case is a data edit -
//! a new file plus one `include_str!` line in [`FILES`] - and no Rust function changes.
//!
//! # The format
//!
//! A line-oriented text file, not a serialisation format, because the domain types this crate
//! reaches (`QueryPlan`, `Value`, `RowSet`) carry `Serialize` but deliberately not `Deserialize`.
//! A format an already-present dependency could read would need `Deserialize` impls that the
//! domain does not provide, and adding a parser dependency would break the portability contract
//! `xtask/src/boundaries/harness.rs` holds. So the loader parses text and constructs each domain
//! type through its own `parse` constructor, the same path a catalog file takes.
//!
//! # What a malformed file costs
//!
//! A parse failure is a typed `CaseError` surfaced through `expect` in [`super::cases`], which
//! turns it into a panic at the call site the way every other literal in this module does. A
//! silently skipped case is what this loader exists to prevent, so every entry in `FILES` is
//! parsed or the whole call fails - there is no `continue` past a broken file.

use sutura_domain::model::{Aggregate, ColumnName, DimensionName, MetricName};
use sutura_domain::plan::{PlanColumn, PlanKey, PlanMeasure, PlanTerm, QueryPlan, ResultLabel, StatementTables};
use sutura_domain::warehouse::{Real, RowSet, Value};

use super::{bucket, range, range_bindings, source, table};
use crate::corpus::Case;

/// Every case file, embedded at compile time.
///
/// The entry's first element is the case's name (the `name:` field in the file, repeated here so
/// a failure report can name the file without parsing it first) and the second is the file's
/// bytes. A file that is removed from disk but not from this list is a **compile error** -
/// `include_str!` cannot find it - which is the strongest guard against a case disappearing. A
/// file removed from this list but left on disk is caught by `the_loader_reads_every_case_file`,
/// which counts the directory at test time and compares.
static FILES: &[(&str, &str)] = &[
    (
        "total-by-region-and-day",
        include_str!("../../corpus/cases/total_by_region_and_day.case"),
    ),
    ("mean-by-day", include_str!("../../corpus/cases/mean_by_day.case")),
    ("total-wide-by-day", include_str!("../../corpus/cases/total_wide_by_day.case")),
    ("total-rate-by-day", include_str!("../../corpus/cases/total_rate_by_day.case")),
    ("wide-total-by-day", include_str!("../../corpus/cases/wide_total_by_day.case")),
    (
        "overflowing-integer-total-by-day",
        include_str!("../../corpus/cases/overflowing_integer_total_by_day.case"),
    ),
    (
        "decimal-total-by-day",
        include_str!("../../corpus/cases/decimal_total_by_day.case"),
    ),
    (
        "total-by-collation-sensitive-key-and-day",
        include_str!("../../corpus/cases/total_by_collation_sensitive_key_and_day.case"),
    ),
];

/// Why a case file could not be read.
///
/// Typed rather than a string, for the reason every refusal in this workspace is: a reader that
/// matched on the text would be depending on the text. Each variant names what was wrong and
/// where, so a broken fixture says which file and which field.
#[derive(Debug, thiserror::Error)]
pub(super) enum CaseError {
    /// A header field was missing from the file.
    #[error("case `{file}` is missing field `{field}`")]
    MissingField { file: &'static str, field: &'static str },
    /// A header field had a value that could not be parsed.
    #[error("case `{file}` field `{field}` could not be parsed: {message}")]
    BadField {
        file: &'static str,
        field: &'static str,
        message: String,
    },
    /// A header field had an unrecognized value.
    #[error("case `{file}` field `{field}` has unknown value `{value}`")]
    UnknownValue {
        file: &'static str,
        field: &'static str,
        value: String,
    },
    /// A header field appeared more than once.
    #[error("case `{file}` field `{field}` appeared more than once")]
    DuplicateField { file: &'static str, field: &'static str },
    /// The `rows:` section was missing or empty.
    #[error("case `{file}` has no expected rows")]
    NoRows { file: &'static str },
    /// A row cell could not be parsed.
    #[error("case `{file}` row {row} cell {cell} could not be parsed: {message}")]
    BadCell {
        file: &'static str,
        row: usize,
        cell: usize,
        message: String,
    },
    /// A row had the wrong number of cells for the plan's projection.
    #[error("case `{file}` row {row} has {cells} cells, expected {expected}")]
    RowWidth {
        file: &'static str,
        row: usize,
        cells: usize,
        expected: usize,
    },
}

/// Parses every case file and returns the cases in order.
///
/// Called by [`super::cases`], which panics on any error - the same posture the rest of this
/// module takes about its literals. The count is the length of `FILES`, so a corpus that lost a
/// file is a compile error and a corpus that lost an entry is caught by the test that counts the
/// directory.
pub(super) fn load() -> Vec<Case> {
    FILES
        .iter()
        .map(|(file, content)| parse(file, content).unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

/// Parses one case file into a [`Case`].
fn parse(file: &'static str, content: &str) -> Result<Case, CaseError> {
    let mut name: Option<String> = None;
    let mut metric: Option<MetricName> = None;
    let mut aggregate: Option<Aggregate> = None;
    let mut column: Option<ColumnName> = None;
    let mut keys: Option<Vec<String>> = None;
    let mut order_is_asserted: Option<bool> = None;
    let mut row_lines: Option<&[&str]> = None;

    let lines: Vec<&str> = content.lines().collect();
    let mut iter = lines.iter().copied().enumerate().peekable();
    while let Some((_, line)) = iter.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed == "rows:" {
            let start = iter.peek().map_or(lines.len(), |(i, _)| *i);
            row_lines = lines.get(start..);
            break;
        }

        let (field, value) = split_field(file, line)?;
        match field {
            "name" => set_once(&mut name, value.to_owned(), file, "name")?,
            "metric" => set_once(
                &mut metric,
                MetricName::parse(value).map_err(|e| bad_field(file, "metric", &e.to_string()))?,
                file,
                "metric",
            )?,
            "aggregate" => set_once(&mut aggregate, parse_aggregate(file, value)?, file, "aggregate")?,
            "column" => set_once(
                &mut column,
                ColumnName::parse(value).map_err(|e| bad_field(file, "column", &e.to_string()))?,
                file,
                "column",
            )?,
            "keys" => set_once(
                &mut keys,
                if value.is_empty() {
                    Vec::new()
                } else {
                    value.split(',').map(str::trim).map(String::from).collect()
                },
                file,
                "keys",
            )?,
            "order" => set_once(&mut order_is_asserted, parse_order(file, value)?, file, "order")?,
            _ => {
                return Err(CaseError::UnknownValue {
                    file,
                    field: "header",
                    value: field.to_owned(),
                });
            }
        }
    }

    let name = name.ok_or(CaseError::MissingField { file, field: "name" })?;
    let metric = metric.ok_or(CaseError::MissingField { file, field: "metric" })?;
    let aggregate = aggregate.ok_or(CaseError::MissingField {
        file,
        field: "aggregate",
    })?;
    let column = column.ok_or(CaseError::MissingField { file, field: "column" })?;
    let keys = keys.ok_or(CaseError::MissingField { file, field: "keys" })?;
    let order_is_asserted = order_is_asserted.ok_or(CaseError::MissingField { file, field: "order" })?;

    let row_lines = row_lines.ok_or(CaseError::NoRows { file })?;
    let non_empty: Vec<&str> = row_lines
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
        .collect();
    if non_empty.is_empty() {
        return Err(CaseError::NoRows { file });
    }

    let plan_keys = build_keys(&keys, file)?;
    let measure = PlanMeasure::Simple {
        term: PlanTerm::Aggregate {
            aggregate,
            column: plan_column(column.as_str()),
        },
    };
    let plan = QueryPlan::new(
        source(),
        metric.clone(),
        StatementTables::only(table()),
        bucket(),
        plan_keys.clone(),
        measure,
        ResultLabel::measure(&metric),
        range_bindings(),
        range(),
    );

    let expected_width = plan_keys.len() + 1 + 1;
    let cells: Vec<Vec<Value>> = non_empty
        .iter()
        .enumerate()
        .map(|(row_idx, line)| parse_row(file, line, row_idx, expected_width))
        .collect::<Result<Vec<_>, _>>()?;

    let expected = RowSet::new(plan.result_labels(), cells).map_err(|e| CaseError::BadField {
        file,
        field: "rows",
        message: e.to_string(),
    })?;

    Ok(Case {
        name,
        plan,
        expected,
        order_is_asserted,
    })
}

/// A header field name and its raw value, parsed from one line.
type Field<'a> = (&'a str, &'a str);

/// Parses `field: value` from a header line.
fn split_field<'a>(file: &'static str, line: &'a str) -> Result<Field<'a>, CaseError> {
    let trimmed = line.trim();
    let colon = trimmed.find(':').ok_or_else(|| CaseError::BadField {
        file,
        field: "header",
        message: format!("expected `field: value`, found `{trimmed}`"),
    })?;
    let (field, rest) = trimmed.split_at(colon);
    let value = rest.strip_prefix(':').unwrap_or(rest).trim();
    Ok((field, value))
}

/// Sets a field once; rejects duplicates.
fn set_once<T>(slot: &mut Option<T>, value: T, file: &'static str, field: &'static str) -> Result<(), CaseError> {
    if slot.is_some() {
        return Err(CaseError::DuplicateField { file, field });
    }
    *slot = Some(value);
    Ok(())
}

fn bad_field(file: &'static str, field: &'static str, message: &str) -> CaseError {
    CaseError::BadField {
        file,
        field,
        message: message.to_owned(),
    }
}

fn parse_aggregate(file: &'static str, value: &str) -> Result<Aggregate, CaseError> {
    match value {
        "sum" => Ok(Aggregate::Sum),
        "avg" => Ok(Aggregate::Avg),
        _ => Err(CaseError::UnknownValue {
            file,
            field: "aggregate",
            value: value.to_owned(),
        }),
    }
}

fn parse_order(file: &'static str, value: &str) -> Result<bool, CaseError> {
    match value {
        "asserted" => Ok(true),
        "not_asserted" => Ok(false),
        _ => Err(CaseError::UnknownValue {
            file,
            field: "order",
            value: value.to_owned(),
        }),
    }
}

/// Builds the `PlanKey` list from dimension names.
fn build_keys(names: &[String], file: &'static str) -> Result<Vec<PlanKey>, CaseError> {
    names
        .iter()
        .map(|name| {
            let dim = DimensionName::parse(name).map_err(|e| bad_field(file, "keys", &e.to_string()))?;
            Ok(PlanKey::new(ResultLabel::dimension(&dim), plan_column_from(name)))
        })
        .collect()
}

/// Makes a `PlanColumn` from a column name string.
fn plan_column(name: &str) -> PlanColumn {
    PlanColumn::new(table(), ColumnName::parse(name).expect("a corpus column name is a name"))
}

/// Makes a `PlanColumn` from a dimension name string (same as `plan_column` - the key name is
/// both the dimension name and the column name, by construction in this corpus).
fn plan_column_from(name: &str) -> PlanColumn {
    plan_column(name)
}

/// Parses one tab-separated row into a list of `Value` cells.
fn parse_row(file: &'static str, line: &str, row: usize, expected: usize) -> Result<Vec<Value>, CaseError> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() != expected {
        return Err(CaseError::RowWidth {
            file,
            row,
            cells: parts.len(),
            expected,
        });
    }
    parts
        .iter()
        .enumerate()
        .map(|(cell_idx, raw)| parse_cell(file, raw.trim(), row, cell_idx))
        .collect()
}

/// Parses one cell from its prefixed text form.
fn parse_cell(file: &'static str, raw: &str, row: usize, cell: usize) -> Result<Value, CaseError> {
    if raw == "null" {
        return Ok(Value::Null);
    }
    let (prefix, rest) = raw.split_once(':').ok_or_else(|| CaseError::BadCell {
        file,
        row,
        cell,
        message: format!("expected `type:value`, found `{raw}`"),
    })?;
    match prefix {
        "int" => rest.parse::<i64>().map(Value::Integer).map_err(|e| CaseError::BadCell {
            file,
            row,
            cell,
            message: e.to_string(),
        }),
        "real" => {
            let value = rest.parse::<f64>().map_err(|e| CaseError::BadCell {
                file,
                row,
                cell,
                message: e.to_string(),
            })?;
            Ok(Value::Real(Real::parse(value).map_err(|e| CaseError::BadCell {
                file,
                row,
                cell,
                message: e.to_string(),
            })?))
        }
        "text" => Ok(Value::Text(String::from(rest))),
        _ => Err(CaseError::BadCell {
            file,
            row,
            cell,
            message: format!("unknown cell type `{prefix}`"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{FILES, load};

    /// The loader reads every `.case` file the directory holds.
    ///
    /// `include_str!` makes a file removed from disk a compile error, which is the strongest guard.
    /// A file removed from [`FILES`] but left on disk is the gap this closes: the directory holds
    /// more than the loader reads, and the packs would go green over a corpus that is silently
    /// smaller.
    #[test]
    fn the_loader_reads_every_case_file() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus").join("cases");
        let on_disk = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
            .filter_map(|entry| {
                let entry = entry.expect("a directory entry");
                let path = entry.path();
                path.extension()
                    .is_some_and(|ext| ext == "case")
                    .then(|| path.file_stem().unwrap().to_string_lossy().into_owned())
            })
            .count();
        let loaded = FILES.len();
        assert_eq!(
            on_disk, loaded,
            "the corpus directory holds {on_disk} `.case` file(s) but the loader reads {loaded} - \
             a file the directory holds is not being read, so the packs would go green over a \
             corpus that is silently smaller",
        );
    }

    /// Every case parses and the count matches what [`FILES`] declares.
    #[test]
    fn every_case_file_parses() {
        let cases = load();
        assert_eq!(cases.len(), FILES.len(), "the loader returned fewer cases than files");
    }

    /// The `order` field is explicit in every case - no default.
    #[test]
    fn every_case_states_its_order_assertion() {
        for (file, content) in FILES {
            let has_order = content.lines().any(|line| line.trim().starts_with("order:"));
            assert!(
                has_order,
                "case `{file}` has no `order:` field - a missing order must not default to the permissive value"
            );
        }
    }

    /// A missing `order` field is a parse error, not a default.
    #[test]
    fn a_missing_order_field_is_rejected() {
        let malformed = "name: broken\nmetric: m\naggregate: sum\ncolumn: c\nkeys:\nrows:\ntext:a\tint:1\n";
        let result = super::parse("broken", malformed);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("order"),
            "the error should name the missing `order` field: {err}"
        );
    }

    /// A malformed cell is a typed error, not a silently skipped case.
    #[test]
    fn a_malformed_cell_is_rejected() {
        let malformed = "name: broken\nmetric: m\naggregate: sum\ncolumn: c\nkeys:\norder: asserted\nrows:\ntext:a\tbogus:1\n";
        let result = super::parse("broken", malformed);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bogus"), "the error should name the bad cell type: {err}");
    }
}
