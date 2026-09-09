//! The names, the environment fixture and the plan the acceptance leg asks.
//!
//! **Split out of `acceptance.rs` because a merge took that file past the 1000-line ceiling
//! `cargo xtask max-lines` enforces**, and the gate's instruction is to split rather than exempt:
//! nothing under `crates/` can be listed in `devco/max-lines-ignore`. What moved is the *setup* -
//! the table a developer supplies, the name no dataset holds, and the one plan every leg asks - so
//! what is left beside the assertions is the assertions.
//!
//! Declared by `acceptance.rs` with `#[path = "fixture/fixture.rs"] mod fixture;` and living in a
//! self-named `tests/fixture/fixture.rs`, which is the difference from `tests/support/mod.rs`: that
//! one is shared with the corpus leg, so `dead_code = "deny"` means nothing target-specific can live
//! in it. Everything here is target-specific, which is exactly why it is not there.

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{
    Aggregate, ColumnName, DatasetName, Grain, MetricName, ProjectName, QualifiedTable, SourceName, TableName, TableQualifier,
};
use sutura_domain::plan::federated::InternalLabel;
use sutura_domain::plan::{
    PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan, ResultLabel,
    StatementTables,
};
use sutura_domain::warehouse::ParamValue;

use crate::support::{Connection, named};

/// A table name no dataset holds, and the one name in this file that needs no masking.
///
/// **A constant rather than four literals**, because four legs ask the same question - *what does
/// this dataset do with a name it does not hold* - and a copy that drifted by a character would be
/// a leg passing for the wrong reason: `preflight` reports an unknown name absent whatever it is,
/// so nothing here would go red.
pub(crate) const NO_SUCH_TABLE: &str = "sutura_acceptance_no_such_table";

/// What this leg needs from the environment: the shared [`Connection`], plus the one variable only
/// this leg reads.
///
/// **The environment reading itself moved to `tests/support/mod.rs` when the corpus leg arrived**,
/// and the argument for FAILING rather than skipping moved with it - it applies to both legs
/// identically and there must be one copy of it. `SUTURA_BQ_TABLE` stayed here, because the corpus
/// leg creates its own tables and has no use for it: a shared module is compiled once per target,
/// and `dead_code` is `deny`.
pub(crate) struct Fixture {
    pub(crate) connection: Connection,
    pub(crate) table: TableName,
}

impl Fixture {
    /// The credential and the three names, or a panic saying exactly what is missing.
    pub(crate) fn required() -> Self {
        Self {
            connection: Connection::required(),
            table: TableName::parse(named(
                "SUTURA_BQ_TABLE",
                "a table with a DATE column `day` and an INT64 column `amount`",
            ))
            .expect("a table name parses"),
        }
    }

    /// The table as an unqualified name, resolved by the job's `defaultDataset`.
    ///
    /// The shape every leg in this file used before qualification existed, kept so the qualified
    /// legs have something to be COMPARED against: a qualified read that returns the right numbers
    /// is only evidence beside an unqualified read that returns the same ones.
    pub(crate) fn unqualified(&self) -> QualifiedTable {
        QualifiedTable::from(self.table.clone())
    }

    /// `dataset.table` - the same table, named without relying on the job's default.
    pub(crate) fn in_dataset(&self) -> QualifiedTable {
        QualifiedTable::new(
            Some(TableQualifier::in_dataset(
                DatasetName::parse(self.connection.dataset.as_str()).expect("a dataset id is also a dataset name"),
            )),
            self.table.clone(),
        )
    }

    /// `project.dataset.table` - the same table, fully qualified.
    ///
    /// **The claim this file was extended for.** The path is built from the fixture's OWN project
    /// and dataset, so no value here is written into the repository - which is the same rule the
    /// two variables above are read under.
    pub(crate) fn in_project(&self) -> QualifiedTable {
        self.qualified_in_project(self.connection.dataset.as_str(), self.table.clone())
    }

    /// `project.dataset.table` in the fixture's OWN project, for any dataset and table.
    ///
    /// **One place parses the project id, which is why this is a method and not a second literal
    /// in a test body.** [`Self::in_project`] asks it about the real dataset; the soft-edge leg
    /// asks it about one the project does not hold. Two copies of the same
    /// `ProjectName::parse(billing_project)` would be two answers to *which project pays*, which
    /// is the live bug `x-goog-user-project` already cost this adapter once.
    pub(crate) fn qualified_in_project(&self, dataset: &str, table: TableName) -> QualifiedTable {
        QualifiedTable::new(
            Some(TableQualifier::in_project(
                ProjectName::parse(self.connection.billing_project.as_str()).expect("a project id is also a project name"),
                DatasetName::parse(dataset).expect("a dataset id is also a dataset name"),
            )),
            table,
        )
    }
}

pub(crate) fn source() -> SourceName {
    SourceName::parse("warehouse").expect("a source name is a source name")
}

/// [`NO_SUCH_TABLE`], parsed - the bare name, for a caller that qualifies it itself.
pub(crate) fn no_such_table() -> TableName {
    TableName::parse(NO_SUCH_TABLE).expect("a table name parses")
}

/// [`NO_SUCH_TABLE`] as an unqualified path, which is the shape four legs ask about.
///
/// **Unqualified on purpose, in every one of them.** `preflight` partitions an unaddressable path
/// into the absent set BEFORE anything is listed, so a name qualified into a dataset that is not
/// there could answer *absent* without a call ever being made. Resolved by the source's own
/// default dataset, the answer comes back from a real listing.
pub(crate) fn absent_table() -> QualifiedTable {
    QualifiedTable::from(no_such_table())
}

/// A plan over the developer's table, built the way the compiler builds one.
///
/// One bucket, one measure, and the two range bounds as predicates - because a `TimeRange` has no
/// unbounded form, so every real plan carries them and the generator refuses one with no
/// predicate.
pub(crate) fn plan(table: &QualifiedTable) -> QueryPlan {
    labelled_plan(
        table,
        ResultLabel::bucket(),
        ResultLabel::measure(&MetricName::parse("total_amount").expect("a metric name parses")),
    )
}

/// The same plan, with its two projected labels taken from the internal federation namespace.
///
/// **The one thing a parse check cannot answer, put in a shape the service can.** Every internal
/// label starts with a digit, which is the character `InvalidIdentifier::BadFirstCharacter` refuses
/// *because* it is legal in some dialects and not others - and `BigQuery` documents a **column name**
/// as starting with a letter or an underscore. Whether that reaches a backtick-quoted select ALIAS
/// is the question, and `sutura_sql::generate`'s `aliased` puts the label in exactly that one
/// position: `GROUP BY` and `ORDER BY` carry the expression.
///
/// A whole-answer plan rather than a leg, deliberately: this adapter takes the default and declares
/// no `EXECUTES_LEGS`, so it is never handed one - and since `telekom/sutura#441` the adapter that
/// IS handed one renders no SQL at all. The alias is rendered by the same `aliased` either way, so
/// this asks the service the alias question without pretending to execute federation.
pub(crate) fn plan_in_the_internal_namespace(table: &QualifiedTable) -> QueryPlan {
    labelled_plan(
        table,
        ResultLabel::internal(InternalLabel::Link),
        ResultLabel::internal(InternalLabel::Leaf(0)),
    )
}

fn labelled_plan(table: &QualifiedTable, bucket_label: ResultLabel, measure_label: ResultLabel) -> QueryPlan {
    // Every column is qualified by the table's BARE name, because `FROM a.b.c` gives the reference
    // an implicit alias of `c`. That is a claim about GoogleSQL that no local test can check, and
    // `the_same_table_read_by_its_fully_qualified_name_answers_the_same_numbers` is what checks it.
    let column = |name: &str| PlanColumn::new(table.name().clone(), ColumnName::parse(name).expect("a column name parses"));
    // **Under `MAX_RANGE_DAYS`, which the previous version was not.** A hundred-year span is a
    // question this surface REFUSES as `TimeRangeTooLong` before an adapter ever sees it, so
    // asking a real endpoint one was asking something no caller could ask - which made
    // "a statement this repository generated" generous. Ten years less a day is the widest a
    // question can legitimately be.
    let from = Date::parse("2016-09-01").expect("an ISO date parses");
    let until = Date::parse("2026-08-30").expect("an ISO date parses");
    QueryPlan::new(
        source(),
        MetricName::parse("total_amount").expect("a metric name parses"),
        StatementTables::only(table.clone()),
        PlanBucket::new(bucket_label, Grain::Month, column("day")),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("amount"),
            },
        },
        measure_label,
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: column("day"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: column("day"),
                    param: 1,
                },
            ),
        ],
        vec![ParamValue::Date(from), ParamValue::Date(until)],
        TimeRange::new(from, until).expect("a bounded range is a range"),
    )
}
