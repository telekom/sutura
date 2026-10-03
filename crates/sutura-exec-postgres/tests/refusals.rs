#![forbid(unsafe_code)]
//! Refusals that fire before any caller SQL, provoked through the adapter's public entries with no
//! database tier.
//!
//! The adapter under test names a driver no path holds, so a request every guard lets through
//! fails at the driver's LOAD - and a missing guard answers that load failure instead of the
//! refusal, which is what each assertion names. The last cell is that control. A refusal that needs
//! the server to answer a query is out of reach here and belongs with the tier-backed cells.
//!
//! **Except the two missing-file cells.** Their kill reads the absent file as empty text, and the
//! column validation refuses that empty header before any SQL (`InvalidColumnName` and
//! `FixtureSchema`), so the kill swaps which refusal answers and never reaches the driver.

#[cfg(test)]
mod refusals {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus;
    use sutura_domain::identity::{Presented, PrincipalName, Secret};
    use sutura_domain::model::TableName;
    use sutura_domain::plan::Executable;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{AdbcError, AdbcPostgres, Channel, Conninfo, PostgresDriver, UnusableChannel};
    use sutura_exec_postgres::connection::ConnectionTarget;

    fn warehouse_under(posture: SourcePosture) -> AdbcPostgres {
        let driver = PostgresDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("an absolute path parses");
        let conninfo = Conninfo::new(
            &corpus::source(),
            ConnectionTarget::Host("127.0.0.1"),
            1,
            "sutura",
            "sutura",
            &Secret::new("unused"),
            Channel::Plaintext,
        )
        .expect("plaintext builds");
        AdbcPostgres::new(corpus::source(), posture, driver, conninfo).expect("the default ceiling parses")
    }

    fn warehouse() -> AdbcPostgres {
        warehouse_under(corpus::posture())
    }

    fn subjects() -> [Presented; 2] {
        [
            Presented::SubjectToken {
                material: Secret::new("an-exchanged-token"),
                impersonate: None,
            },
            Presented::SubjectPrincipal {
                name: PrincipalName::parse("analyst_role").expect("a test name is a name"),
            },
        ]
    }

    fn statement() -> RawStatement {
        RawStatement::parse("select 1").expect("a test statement is a statement")
    }

    fn spent() -> Deadline {
        let opened = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("this host's clock is more than a minute past its epoch");
        Deadline::opened_at(
            opened,
            Budget::parse(Duration::from_secs(1)).expect("a positive budget parses"),
        )
    }

    /// The three port calls that take a request, as one refusal each - its variant, nothing else.
    fn refusals(warehouse: &AdbcPostgres, presented: &Presented, deadline: Deadline) -> [&'static str; 3] {
        let case = corpus::cases().into_iter().next().expect("the corpus has a case");
        [
            kind(
                warehouse
                    .dry_run(Executable::Query(case.plan()), presented, deadline)
                    .err()
                    .as_ref(),
            ),
            kind(
                warehouse
                    .execute(Executable::Query(case.plan()), presented, deadline)
                    .err()
                    .as_ref(),
            ),
            kind(
                warehouse
                    .execute_raw(&statement(), presented, deadline)
                    .and_then(Result::err)
                    .as_ref(),
            ),
        ]
    }

    fn kind(refused: Option<&PostgresError>) -> &'static str {
        match refused {
            None => "answered",
            Some(PostgresError::NoPlaceForASubject { .. }) => "no place for a subject",
            Some(PostgresError::PresentedDisagreesWithPosture { .. }) => "a disagreeing witness",
            Some(PostgresError::DeadlineSpent) => "the deadline spent",
            Some(_) => "something past the guards",
        }
    }

    /// A CSV file unique to this test process, holding `text`.
    fn csv(name: &str, text: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("sutura-pg-{name}-{}.csv", std::process::id()));
        std::fs::write(&path, text).expect("a temp file is writable");
        path
    }

    fn table() -> TableName {
        TableName::parse("orders").expect("a test table is a table")
    }

    /// Refused before any connection string exists, so nothing is ever dialled.
    #[test]
    fn a_schema_name_with_a_dot_is_refused() {
        let outcome = Conninfo::in_schema(
            "bad.schema",
            &corpus::source(),
            ConnectionTarget::Host("127.0.0.1"),
            1,
            "sutura",
            "sutura",
            &Secret::new("unused"),
        );
        assert!(
            matches!(outcome, Err(UnusableChannel::NotASchema { ref schema }) if schema == "bad.schema"),
            "a dot is not a word character: {outcome:?}"
        );
    }

    #[test]
    fn a_subject_token_is_refused_by_dry_run_before_anything_is_prepared() {
        let warehouse = warehouse();
        let case = corpus::cases().into_iter().next().expect("the corpus has a case");
        for presented in subjects() {
            let outcome = warehouse.dry_run(Executable::Query(case.plan()), &presented, corpus::deadline());
            assert!(
                matches!(outcome, Err(PostgresError::NoPlaceForASubject { .. })),
                "a subject credential has nowhere to arrive on this adapter: {outcome:?}"
            );
        }
    }

    #[test]
    fn a_subject_token_is_refused_by_execute_before_anything_is_run() {
        let warehouse = warehouse();
        let case = corpus::cases().into_iter().next().expect("the corpus has a case");
        for presented in subjects() {
            let outcome = warehouse.execute(Executable::Query(case.plan()), &presented, corpus::deadline());
            assert!(
                matches!(outcome, Err(PostgresError::NoPlaceForASubject { .. })),
                "a subject credential has nowhere to arrive on this adapter: {outcome:?}"
            );
        }
    }

    /// Refused before the driver loads, so before `SET TRANSACTION READ ONLY` could be sent.
    #[test]
    fn a_subject_token_is_refused_by_execute_raw_before_anything_is_run() {
        let warehouse = warehouse();
        for presented in subjects() {
            let outcome = warehouse.execute_raw(&statement(), &presented, corpus::deadline());
            assert!(
                matches!(outcome, Some(Err(PostgresError::NoPlaceForASubject { .. }))),
                "a subject credential has nowhere to arrive on this adapter: {outcome:?}"
            );
        }
    }

    /// The corpus's shared leg, handed to a source declared under another acknowledgement.
    #[test]
    fn a_disagreeing_witness_is_refused_by_execute() {
        let warehouse = warehouse_under(SourcePosture::SharedServiceUser {
            declared: SharedIdentityDeclared::of(
                AcknowledgementReason::parse("a different operator's acknowledgement").expect("a test reason is a reason"),
            ),
        });
        assert_eq!(
            refusals(&warehouse, &corpus::presented(), corpus::deadline()),
            ["a disagreeing witness"; 3]
        );
    }

    #[test]
    fn a_spent_deadline_is_refused_before_the_driver_loads() {
        assert_eq!(
            refusals(&warehouse(), &corpus::presented(), spent()),
            ["the deadline spent"; 3]
        );
    }

    #[test]
    fn a_request_every_guard_lets_through_reaches_the_driver_load() {
        // The control the cells above rest on: past the guards, every call fails at the load - so a
        // refusal above arrived INSTEAD of it.
        let warehouse = warehouse();
        assert_eq!(
            refusals(&warehouse, &corpus::presented(), corpus::deadline()),
            ["something past the guards"; 3]
        );
        let reached = warehouse.execute_raw(&statement(), &corpus::presented(), corpus::deadline());
        assert!(
            matches!(
                reached,
                Some(Err(PostgresError::Adbc {
                    cause: AdbcError::Load(_)
                }))
            ),
            "{reached:?}"
        );
    }

    #[test]
    fn a_missing_fixture_csv_is_refused_naming_the_path() {
        let missing = Path::new("/definitely/not/here.csv");
        let outcome = warehouse().load_csv(&table(), missing);
        assert!(
            matches!(outcome, Err(PostgresError::FixtureRead { ref path, .. }) if path == "/definitely/not/here.csv"),
            "a file that cannot be read is refused by its path: {outcome:?}"
        );
    }

    #[test]
    fn load_csv_refuses_an_invalid_column_name() {
        let path = csv("bad-header", "orders.amount\n1\n");
        let outcome = warehouse().load_csv(&table(), &path);
        drop(std::fs::remove_file(&path));
        assert!(
            matches!(outcome, Err(PostgresError::InvalidColumnName { .. })),
            "a dotted header is not a column name: {outcome:?}"
        );
    }

    #[test]
    fn a_missing_fixture_csv_is_refused_by_load_fixture_csv() {
        let missing = Path::new("/definitely/not/here-either.csv");
        let outcome = warehouse().load_fixture_csv(&corpus::table(), missing);
        assert!(
            matches!(outcome, Err(PostgresError::FixtureRead { ref path, .. }) if path == "/definitely/not/here-either.csv"),
            "a fixture that cannot be read is refused by its path: {outcome:?}"
        );
    }

    /// Two headers, three cells: the shared fixture inference refuses the ragged row.
    #[test]
    fn load_fixture_csv_refuses_a_row_with_the_wrong_width() {
        let path = csv("ragged-fixture", "region,amount\nnorth,1,2\n");
        let outcome = warehouse().load_fixture_csv(&corpus::table(), &path);
        drop(std::fs::remove_file(&path));
        assert!(
            matches!(outcome, Err(PostgresError::FixtureSchema { .. })),
            "a ragged fixture row is refused before any SQL: {outcome:?}"
        );
    }
}
