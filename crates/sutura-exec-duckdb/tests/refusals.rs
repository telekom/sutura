#![forbid(unsafe_code)]
//! Refusals of the file-facing entries and of rendering, provoked through the public constructors,
//! `attach_csv`, `attach_fixture_csv` and the execution port - never by constructing the variant.

#[cfg(test)]
mod refusals {
    use std::path::{Path, PathBuf};

    use sutura_conformance::corpus;
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, QualifiedTable, TableName};
    use sutura_domain::plan::{
        Executable, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm,
        PredicateOrigin, QueryPlan, ResultLabel, StatementTables,
    };
    use sutura_domain::warehouse::{ParamValue, ResultBudget, Warehouse as _};
    use sutura_exec_duckdb::{DuckDbError, DuckDbWarehouse};

    fn budget() -> ResultBudget {
        ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024 * 1024).expect("a mebibyte is positive"))
    }

    fn warehouse() -> DuckDbWarehouse {
        DuckDbWarehouse::in_memory(corpus::source(), corpus::posture(), budget()).expect("an in-memory database opens")
    }

    /// A path under a directory that does not exist, unique to this test process.
    fn nowhere(file: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("sutura-duckdb-absent-{}", std::process::id()))
            .join(file)
    }

    fn column(name: &str) -> PlanColumn {
        PlanColumn::new(
            corpus::table(),
            ColumnName::parse(name).expect("a column name is a column name"),
        )
    }

    fn day(of_january: u8) -> Date {
        Date::new(2026, 1, of_january).expect("a January date in 2026 is a date")
    }

    /// The corpus's shape over a TWO-part table name, which `DuckDB`'s table-only qualification
    /// cannot render.
    fn plan_over_a_qualified_table() -> QueryPlan {
        let metric = MetricName::parse("total").expect("a metric name is a metric name");
        let filters = vec![
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
        ];
        QueryPlan::new(
            corpus::source(),
            metric.clone(),
            StatementTables::only(QualifiedTable::parse("dataset.conformance_events").expect("a two-part name parses")),
            PlanBucket::new(ResultLabel::bucket(), Grain::Day, column("day")),
            vec![PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse("region").expect("a dimension name")),
                column("region"),
            )],
            PlanMeasure::Simple {
                term: PlanTerm::Aggregate {
                    aggregate: Aggregate::Sum,
                    column: column("amount_cents"),
                },
            },
            ResultLabel::measure(&metric),
            PlanBindings::parse(filters, vec![ParamValue::Date(day(1)), ParamValue::Date(day(3))])
                .expect("the range binds its two bounds in placeholder order"),
            TimeRange::new(day(1), day(3)).expect("the range is not empty"),
        )
    }

    #[test]
    fn a_database_under_a_missing_directory_is_refused_as_open() {
        let path = nowhere("db.duckdb");
        let outcome = DuckDbWarehouse::open(corpus::source(), corpus::posture(), &path, budget());
        assert!(
            matches!(outcome, Err(DuckDbError::Open { path: ref named, .. }) if *named == path.display().to_string()),
            "a database file the driver cannot create is refused as Open: {outcome:?}"
        );
    }

    #[test]
    fn a_csv_that_does_not_exist_is_refused_as_attach() {
        let path = nowhere("absent.csv");
        let table = TableName::parse("t").expect("a table name is a table name");
        let outcome = warehouse().attach_csv(&table, &path);
        assert!(
            matches!(outcome, Err(DuckDbError::Attach { ref table, path: ref named, .. })
                if table == "t" && *named == path.display().to_string()),
            "a CSV the driver cannot read is refused as Attach: {outcome:?}"
        );
    }

    #[cfg(feature = "fixtures")]
    #[test]
    fn a_missing_fixture_is_refused_as_fixture_read() {
        let missing = Path::new("/no/such/file.csv");
        let outcome = warehouse().attach_fixture_csv(&corpus::table(), missing);
        assert!(
            matches!(outcome, Err(DuckDbError::FixtureRead { ref path, .. }) if path == "/no/such/file.csv"),
            "a missing fixture file is refused as FixtureRead: {outcome:?}"
        );
    }

    /// Quoted syntax is refused by the shared inference before any reader sees it.
    #[cfg(feature = "fixtures")]
    #[test]
    fn a_quoted_fixture_is_refused_as_fixture_schema() {
        let path = std::env::temp_dir().join(format!("sutura-duckdb-quoted-{}.csv", std::process::id()));
        std::fs::write(&path, "x\n\"quoted\"\n").expect("a temp file is writable");
        let outcome = warehouse().attach_fixture_csv(&corpus::table(), &path);
        drop(std::fs::remove_file(&path));
        assert!(
            matches!(outcome, Err(DuckDbError::FixtureSchema { .. })),
            "a fixture the shared inference rejects is refused as FixtureSchema: {outcome:?}"
        );
    }

    /// The kill (rendering in the Postgres dialect) makes `render` SUCCEED. The cell then goes red
    /// on a downstream `Prepare` catalog error, because this empty database has no `dataset`
    /// schema, and not at the render step. The assertion names `Render` itself, so an `Ok` - which
    /// a database holding that schema would answer - is red too, and the kill does not depend on
    /// what the fixture lacks.
    #[test]
    fn a_plan_over_a_qualified_table_is_refused_as_render() {
        let plan = plan_over_a_qualified_table();
        let outcome = warehouse().execute(Executable::Query(&plan), &corpus::presented(), corpus::deadline());
        assert!(
            matches!(outcome, Err(DuckDbError::Render { .. })),
            "a plan this dialect cannot render is refused as Render: {outcome:?}"
        );
    }
}
