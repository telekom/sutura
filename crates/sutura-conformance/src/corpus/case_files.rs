//! Case definitions read from tracked data files, not Rust.
//!
//! Each case is a `.case` file under `corpus/cases/`, embedded at compile time with
//! [`include_str!`] and parsed here by a typed loader. Adding a case is a data edit -
//! a new file plus one `include_str!` line in [`FILES`] or [`FEDERATED_FILES`] - and no Rust
//! function changes.
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
//! # A federated file
//!
//! The same header and `rows:`, with `order:` gone (a federated plan claims no order) and four
//! fields added. `lookup:` names the lookup leg's table, and the corpus has one. `keys:` are read
//! off that table, joined to the fact rows on `region`. `lookup_filter: <column> = <value>` is a
//! filter in the lookup leg, and its presence drops an unmatched fact row, as the planner's
//! `include_unmatched` does. A ratio is `aggregate: ratio` with `numerator:`/`denominator:` as
//! `<aggregate> <column>` and `zero_denominator: yields_null|fails`. Any other field is refused.
//!
//! # What a malformed file costs
//!
//! A parse failure is a typed `CaseError` surfaced through `expect` in [`super::cases`], which
//! turns it into a panic at the call site the way every other literal in this module does. A
//! silently skipped case is what this loader exists to prevent, so every entry in `FILES` is
//! parsed or the whole call fails - there is no `continue` past a broken file.

use sutura_domain::federation::{Carried, Federation};
use sutura_domain::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, MetricName, QualifiedTable};
use sutura_domain::plan::{
    AnswerKey, FederatedPlan, FederatedPlanError, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanColumn, PlanFilter, PlanKey,
    PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan, ResultLabel, StatementTables, labels,
};
use sutura_domain::warehouse::{ParamValue, Real, RowSet, Value};

use super::{LOOKUP_TABLE, bucket, lookup_source, range, range_bindings, source, table};
use crate::corpus::{Case, FederatedCase};

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

/// Every federated case file with an answer, embedded and guarded exactly as [`FILES`] is.
///
/// The third federated file ADR 0012 names has no answer: its plan is refused at construction, so
/// it is registered beside the test that asserts the refusal rather than here.
static FEDERATED_FILES: &[(&str, &str)] = &[
    (
        "two-source-remote-filter-with-an-orphan-key",
        include_str!("../../corpus/cases/two_source_remote_filter_with_an_orphan_key.case"),
    ),
    (
        "two-source-zero-denominator-in-one-subgroup",
        include_str!("../../corpus/cases/two_source_zero_denominator_in_one_subgroup.case"),
    ),
];

/// The column both legs of a federated case join on, under [`InternalLabel::Link`].
const LINK_COLUMN: &str = "region";

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
    /// The file's own `name:` disagreed with the name it is registered under in `FILES`.
    ///
    /// The two are the same string written twice - once in the entry, once in the file - and
    /// nothing compared them, so a misspelling in either left a corpus that loaded and reported a
    /// green run under a name no file carries.
    #[error("case `{file}` is registered under that name but its own `name:` field says `{name}`")]
    NameDisagrees { file: &'static str, name: String },
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
    /// [`FederatedPlan::new`] refused the plan a federated file describes.
    #[error("case `{file}` describes a plan the domain refuses: {error}")]
    PlanRefused {
        file: &'static str,
        #[source]
        error: FederatedPlanError,
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

/// Parses every federated case file with an answer, with [`load`]'s posture.
pub(super) fn load_federated() -> Vec<FederatedCase> {
    FEDERATED_FILES
        .iter()
        .map(|(file, content)| parse_federated(file, content).unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

/// Parses one case file into a [`Case`].
fn parse(file: &'static str, content: &str) -> Result<Case, CaseError> {
    let sections = Sections::read(file, content, &["name", "metric", "aggregate", "column", "keys", "order"])?;
    let metric = sections.metric()?;
    let aggregate = parse_aggregate(file, sections.required("aggregate")?)?;
    let column = PlanColumn::new(table(), sections.column("column")?);
    let keys = sections
        .keys()?
        .iter()
        .map(|key| PlanKey::new(ResultLabel::dimension(key), plan_column(key.as_str())))
        .collect();
    let order_is_asserted = parse_order(file, sections.required("order")?)?;
    let plan = QueryPlan::new(
        source(),
        metric.clone(),
        StatementTables::only(table()),
        bucket(),
        keys,
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate { aggregate, column },
        },
        ResultLabel::measure(&metric),
        range_bindings(),
        range(),
    );
    let expected = sections.expected(plan.result_labels())?;
    Ok(Case {
        name: String::from(file),
        plan,
        expected,
        order_is_asserted,
    })
}

/// Parses one federated case file into a [`FederatedCase`], or the refusal that stopped it.
fn parse_federated(file: &'static str, content: &str) -> Result<FederatedCase, CaseError> {
    let sections = Sections::read(
        file,
        content,
        &[
            "name",
            "metric",
            "aggregate",
            "column",
            "keys",
            "lookup",
            "lookup_filter",
            "numerator",
            "denominator",
            "zero_denominator",
        ],
    )?;
    let metric = sections.metric()?;
    let keys = sections.keys()?;
    let lookup_table = match sections.required("lookup")? {
        LOOKUP_TABLE => QualifiedTable::parse(LOOKUP_TABLE).map_err(|e| bad_field(file, "lookup", &e.to_string()))?,
        other => {
            return Err(CaseError::UnknownValue {
                file,
                field: "lookup",
                value: other.to_owned(),
            });
        }
    };
    let lookup_column = |field: &'static str, name: &str| {
        ColumnName::parse(name)
            .map(|column| PlanColumn::new(lookup_table.name().clone(), column))
            .map_err(|e| bad_field(file, field, &e.to_string()))
    };
    let measure = match sections.required("aggregate")? {
        "ratio" => Measure::Ratio {
            numerator: sections.term("numerator")?,
            denominator: sections.term("denominator")?,
            zero_denominator: parse_zero_denominator(file, sections.required("zero_denominator")?)?,
        },
        aggregate => Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            parse_aggregate(file, aggregate)?,
            sections.column("column")?,
        ))),
    };
    let federation = Federation::of(&measure);
    let terms = federation
        .carried()
        .into_iter()
        .zip(labels(&federation))
        .filter_map(|(leaf, label)| match *leaf {
            Carried::Aggregated { pushed, ref column, .. } => Some(LegTerm::new(
                PlanTerm::Aggregate {
                    aggregate: pushed.push(),
                    column: PlanColumn::new(table(), column.clone()),
                },
                ResultLabel::internal(label),
            )),
            // No field spells a conditional count, and a pulled-up column is a key rather than a
            // term - `FederatedPlan::new` refuses the leaf that needs one.
            Carried::CountIf { .. } | Carried::Keys { .. } => None,
        })
        .collect();
    let link = ResultLabel::internal(InternalLabel::Link);
    let fact = LegPlan::Fact {
        source: source(),
        metric: metric.clone(),
        tables: StatementTables::only(table()),
        bucket: bucket(),
        keys: vec![PlanKey::new(link.clone(), plan_column(LINK_COLUMN))],
        terms,
        bindings: range_bindings(),
        range: range(),
    };
    let mut lookup_keys = vec![PlanKey::new(link, lookup_column("lookup", LINK_COLUMN)?)];
    for key in &keys {
        lookup_keys.push(PlanKey::new(
            ResultLabel::dimension(key),
            lookup_column("keys", key.as_str())?,
        ));
    }
    let (bindings, include_unmatched) = match sections.get("lookup_filter") {
        None => (PlanBindings::none(), true),
        Some(filter) => {
            let (column, value) = filter
                .split_once('=')
                .map(|(column, value)| (column.trim(), value.trim()))
                .filter(|&(column, value)| !column.is_empty() && !value.is_empty())
                .ok_or_else(|| bad_field(file, "lookup_filter", "expected `<column> = <value>`"))?;
            let equals = PlanFilter::new(
                PredicateOrigin::Requested,
                PlanPredicate::Equals {
                    column: lookup_column("lookup_filter", column)?,
                    param: 0,
                },
            );
            let bindings = PlanBindings::parse(vec![equals], vec![ParamValue::Text(String::from(value))])
                .map_err(|e| bad_field(file, "lookup_filter", &e.to_string()))?;
            (bindings, false)
        }
    };
    let lookup = LegPlan::Lookup {
        source: lookup_source(),
        table: lookup_table,
        keys: lookup_keys,
        bindings,
    };
    let answer_keys = keys
        .iter()
        .map(|key| AnswerKey::lookup(ResultLabel::dimension(key)))
        .collect();
    let measure_label = ResultLabel::measure(&metric);
    // Built before the rows are read: a refused plan has no answer, so its refusal IS the case.
    let plan = FederatedPlan::new(
        metric,
        measure_label,
        bucket(),
        fact,
        None,
        lookup,
        include_unmatched,
        federation,
        answer_keys,
    )
    .map_err(|error| CaseError::PlanRefused { file, error })?;
    let mut labels: Vec<String> = plan.keys().iter().map(|key| String::from(key.label())).collect();
    labels.push(String::from(plan.bucket_label()));
    labels.push(String::from(plan.measure_label()));
    let expected = sections.expected(labels)?;
    Ok(FederatedCase {
        name: file,
        plan,
        expected,
    })
}

/// A file's header - every field one the caller knows, none named twice - and its row lines.
struct Sections<'a> {
    file: &'static str,
    fields: Vec<(&'static str, &'a str)>,
    rows: Option<Vec<&'a str>>,
}

impl<'a> Sections<'a> {
    /// Splits `content` at `rows:`, refusing an unknown or repeated field and a `name:` that
    /// disagrees with `file`.
    fn read(file: &'static str, content: &'a str, known: &[&'static str]) -> Result<Self, CaseError> {
        let blank = |line: &str| line.trim().is_empty() || line.trim().starts_with('#');
        let mut fields: Vec<(&'static str, &'a str)> = Vec::new();
        let mut rows = None;
        let mut lines = content.lines();
        while let Some(line) = lines.next() {
            if blank(line) {
                continue;
            }
            if line.trim() == "rows:" {
                rows = Some(lines.by_ref().filter(|line| !blank(line)).collect());
                break;
            }
            let (field, value) = split_field(file, line)?;
            let Some(&field) = known.iter().find(|&&known| known == field) else {
                return Err(CaseError::UnknownValue {
                    file,
                    field: "header",
                    value: field.to_owned(),
                });
            };
            if fields.iter().any(|&(seen, _)| seen == field) {
                return Err(CaseError::DuplicateField { file, field });
            }
            fields.push((field, value));
        }
        let sections = Self { file, fields, rows };
        let name = sections.required("name")?;
        if name != file {
            return Err(CaseError::NameDisagrees {
                file,
                name: name.to_owned(),
            });
        }
        Ok(sections)
    }

    fn get(&self, field: &str) -> Option<&'a str> {
        self.fields.iter().find(|&&(seen, _)| seen == field).map(|&(_, value)| value)
    }

    fn required(&self, field: &'static str) -> Result<&'a str, CaseError> {
        self.get(field).ok_or(CaseError::MissingField { file: self.file, field })
    }

    fn metric(&self) -> Result<MetricName, CaseError> {
        MetricName::parse(self.required("metric")?).map_err(|e| bad_field(self.file, "metric", &e.to_string()))
    }

    fn column(&self, field: &'static str) -> Result<ColumnName, CaseError> {
        ColumnName::parse(self.required(field)?).map_err(|e| bad_field(self.file, field, &e.to_string()))
    }

    fn keys(&self) -> Result<Vec<DimensionName>, CaseError> {
        let keys = self.required("keys")?;
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        keys.split(',')
            .map(|key| DimensionName::parse(key.trim()).map_err(|e| bad_field(self.file, "keys", &e.to_string())))
            .collect()
    }

    /// A `numerator:`/`denominator:` term: `<aggregate> <column>`.
    fn term(&self, field: &'static str) -> Result<Term, CaseError> {
        let (aggregate, column) = self
            .required(field)?
            .split_once(' ')
            .ok_or_else(|| bad_field(self.file, field, "expected `<aggregate> <column>`"))?;
        let column = ColumnName::parse(column.trim()).map_err(|e| bad_field(self.file, field, &e.to_string()))?;
        Ok(Term::Aggregate(AggregatedColumn::new(
            parse_aggregate(self.file, aggregate)?,
            column,
        )))
    }

    /// The rows under `labels`, one cell per label.
    fn expected(&self, labels: Vec<String>) -> Result<RowSet, CaseError> {
        let rows = match self.rows.as_deref() {
            Some(rows) if !rows.is_empty() => rows,
            _ => return Err(CaseError::NoRows { file: self.file }),
        };
        let cells = rows
            .iter()
            .enumerate()
            .map(|(row, line)| parse_row(self.file, line, row, labels.len()))
            .collect::<Result<Vec<_>, _>>()?;
        RowSet::new(labels, cells).map_err(|e| CaseError::BadField {
            file: self.file,
            field: "rows",
            message: e.to_string(),
        })
    }
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
        "count_distinct" => Ok(Aggregate::CountDistinct),
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

fn parse_zero_denominator(file: &'static str, value: &str) -> Result<ZeroDenominator, CaseError> {
    match value {
        "yields_null" => Ok(ZeroDenominator::Null),
        "fails" => Ok(ZeroDenominator::Fail),
        _ => Err(CaseError::UnknownValue {
            file,
            field: "zero_denominator",
            value: value.to_owned(),
        }),
    }
}

/// Makes a `PlanColumn` on the fact table from a column name string.
fn plan_column(name: &str) -> PlanColumn {
    PlanColumn::new(table(), ColumnName::parse(name).expect("a corpus column name is a name"))
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
    use sutura_domain::model::Aggregate;
    use sutura_domain::plan::FederatedPlanError;

    use super::{CaseError, FEDERATED_FILES, FILES, load};

    /// The federated case ADR 0012 names whose plan the domain refuses, so it has no answer and no
    /// place in [`FEDERATED_FILES`]; the refusal is what [`a_count_distinct_across_the_join_is_refused`]
    /// holds it to.
    static REFUSED: (&str, &str) = (
        "two-source-distinct-value-spanning-join-keys",
        include_str!("../../corpus/cases/two_source_distinct_value_spanning_join_keys.case"),
    );

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
        let loaded = FILES.len() + FEDERATED_FILES.len() + 1;
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
    fn a_name_that_disagrees_with_its_registration_is_rejected() {
        // The gap this closes: `FILES` repeats each case's name so a failure report can name the
        // file without parsing it, and nothing compared the two copies. A misspelling in either
        // gave `133 passed` under a name no file carries.
        let disagreeing =
            "name: not-the-registered-name\nmetric: m\naggregate: sum\ncolumn: c\nkeys:\norder: asserted\nrows:\ntext:a\tint:1\n";
        let err = super::parse("total-by-region-and-day", disagreeing)
            .expect_err("a file whose own name disagrees with its registration is refused")
            .to_string();
        assert!(
            err.contains("not-the-registered-name") && err.contains("total-by-region-and-day"),
            "the error should name both the registration and the file's own name: {err}"
        );
    }

    #[test]
    fn every_case_file_agrees_with_the_name_it_is_registered_under() {
        // Over the real corpus, not a fixture: the check above proves the refusal exists, this
        // proves the shipped corpus satisfies it.
        for (file, content) in FILES {
            super::parse(file, content).unwrap_or_else(|e| panic!("{e}"));
        }
    }

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

    /// ADR 0012's third case: a `CountDistinct` leaf does not re-aggregate, so the plan is refused
    /// at construction rather than summed into an over-count.
    #[test]
    fn a_count_distinct_across_the_join_is_refused() {
        let (file, content) = REFUSED;
        let refused = super::parse_federated(file, content);
        assert!(
            matches!(
                refused,
                Err(CaseError::PlanRefused {
                    error: FederatedPlanError::LeafDoesNotReaggregate {
                        aggregate: Aggregate::CountDistinct
                    },
                    ..
                })
            ),
            "{refused:?}"
        );
    }

    /// A federated file names the lookup table it reads, and one the corpus does not hold is
    /// refused rather than read as the one it does.
    #[test]
    fn a_federated_file_naming_another_lookup_table_is_refused() {
        let (file, content) = FEDERATED_FILES[0];
        let elsewhere = content.replace("lookup: conformance_regions", "lookup: somewhere_else");
        let refused = super::parse_federated(file, &elsewhere);
        assert!(
            matches!(refused, Err(CaseError::UnknownValue { field: "lookup", ref value, .. }) if value == "somewhere_else"),
            "{refused:?}"
        );
    }

    /// A header field the reader does not know is refused, not dropped: a misspelled `lookup:`
    /// would otherwise surface as a missing field, or pass silently where the field is optional.
    #[test]
    fn an_unknown_header_field_is_refused() {
        let (file, content) = FEDERATED_FILES[0];
        let misspelled = content.replace("lookup: conformance_regions", "lookuup: conformance_regions");
        let refused = super::parse_federated(file, &misspelled);
        assert!(
            matches!(refused, Err(CaseError::UnknownValue { field: "header", ref value, .. }) if value == "lookuup"),
            "{refused:?}"
        );
    }

    /// A header line without a `:` is a `BadField` naming `header`, not a skipped line.
    #[test]
    fn a_malformed_header_line_is_rejected() {
        let malformed =
            "name: broken\nmetric: m\ngarbage_line\naggregate: sum\ncolumn: c\nkeys:\norder: asserted\nrows:\ntext:a\tint:1\n";
        let result = super::parse("broken", malformed);
        assert!(
            matches!(result, Err(CaseError::BadField { field: "header", .. })),
            "{result:?}"
        );
    }

    /// A header field named twice is a `DuplicateField` naming it.
    #[test]
    fn a_duplicate_header_field_is_rejected() {
        let malformed =
            "name: broken\nmetric: m\naggregate: sum\naggregate: avg\ncolumn: c\nkeys:\norder: asserted\nrows:\ntext:a\tint:1\n";
        let result = super::parse("broken", malformed);
        assert!(
            matches!(result, Err(CaseError::DuplicateField { field: "aggregate", .. })),
            "{result:?}"
        );
    }

    /// A row with the wrong number of cells is a `RowWidth` naming the row and both counts.
    #[test]
    fn a_row_with_the_wrong_width_is_rejected() {
        // `keys: region` gives three labels - region, bucket, measure - and the row has two cells.
        let malformed =
            "name: broken\nmetric: m\naggregate: sum\ncolumn: c\nkeys: region\norder: asserted\nrows:\ntext:east\tint:1\n";
        let result = super::parse("broken", malformed);
        assert!(
            matches!(
                result,
                Err(CaseError::RowWidth {
                    row: 0,
                    cells: 2,
                    expected: 3,
                    ..
                })
            ),
            "{result:?}"
        );
    }

    /// A federated file with no `rows:` is a `NoRows`, not an empty answer. The file is otherwise
    /// valid, so the plan is built and the refusal is the row reader's.
    #[test]
    fn a_federated_file_with_no_rows_is_rejected() {
        let malformed = "name: broken-federated\nmetric: amount_total\naggregate: sum\ncolumn: amount_cents\nkeys: region_name\nlookup: conformance_regions\n";
        let result = super::parse_federated("broken-federated", malformed);
        assert!(
            matches!(
                result,
                Err(CaseError::NoRows {
                    file: "broken-federated"
                })
            ),
            "{result:?}"
        );
    }
}
