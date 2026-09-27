#![forbid(unsafe_code)]
//! Refusals that fire before any caller SQL, provoked through the adapter's public entries with no
//! database tier.
//!
//! A warehouse needs a connection to exist, so the cells that take one connect to a FAKE server
//! that answers only the handshake and the one `SET statement_timeout` the adapter sends at boot,
//! then hangs up. A missing guard therefore reaches a closed connection and answers a different
//! error than the refusal, which is what each assertion names. A refusal that needs the server to
//! answer a query is out of reach here and belongs with the tier-backed cells.

#[cfg(test)]
mod refusals {
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use sutura_conformance::corpus;
    use sutura_domain::identity::{Presented, PrincipalName, Secret};
    use sutura_domain::model::TableName;
    use sutura_domain::plan::Executable;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_postgres::{PostgresError, PostgresWarehouse};

    /// Reads one length-prefixed message body; `tagged` says whether a type byte precedes it.
    #[expect(clippy::big_endian_bytes, reason = "the Postgres wire protocol frames lengths big-endian")]
    fn read_message(stream: &mut TcpStream, tagged: bool) {
        if tagged {
            let mut tag = [0_u8; 1];
            stream.read_exact(&mut tag).expect("the driver sends a message type");
        }
        let mut length = [0_u8; 4];
        stream.read_exact(&mut length).expect("the driver sends a message length");
        let body = usize::try_from(u32::from_be_bytes(length)).expect("a u32 fits a usize") - 4;
        let mut discarded = vec![0_u8; body];
        stream.read_exact(&mut discarded).expect("the driver sends the message body");
    }

    /// A loopback listener that completes one connection's handshake - `AuthenticationOk`,
    /// `ReadyForQuery`, then `CommandComplete` for the boot `SET` - and closes it.
    fn fake_postgres() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener binds");
        let port = listener.local_addr().expect("a bound listener has an address").port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the driver dials this listener");
            read_message(&mut stream, false);
            stream
                .write_all(b"R\x00\x00\x00\x08\x00\x00\x00\x00Z\x00\x00\x00\x05I")
                .expect("the handshake is answered");
            read_message(&mut stream, true);
            stream
                .write_all(b"C\x00\x00\x00\x08SET\x00Z\x00\x00\x00\x05I")
                .expect("the boot statement is answered");
        });
        port
    }

    fn config(port: u16) -> tokio_postgres::Config {
        let mut config = tokio_postgres::Config::new();
        config
            .host("127.0.0.1")
            .port(port)
            .dbname("sutura")
            .user("sutura")
            .connect_timeout(Duration::from_secs(5));
        config
    }

    fn warehouse_under(posture: SourcePosture) -> PostgresWarehouse {
        PostgresWarehouse::connect(corpus::source(), posture, &config(fake_postgres()))
            .expect("the fake server completes the handshake")
    }

    fn warehouse() -> PostgresWarehouse {
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

    /// A CSV file unique to this test process, holding `text`.
    fn csv(name: &str, text: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("sutura-pg-{name}-{}.csv", std::process::id()));
        std::fs::write(&path, text).expect("a temp file is writable");
        path
    }

    fn table() -> TableName {
        TableName::parse("orders").expect("a test table is a table")
    }

    /// Refused before any connection is attempted, so the closed port is never dialled.
    #[test]
    fn a_schema_name_with_a_dot_is_refused() {
        let mut config = tokio_postgres::Config::new();
        config.host("127.0.0.1").port(1).dbname("sutura").user("sutura");
        let outcome = PostgresWarehouse::connect_in_schema(corpus::source(), corpus::posture(), &config, "bad.schema");
        assert!(
            matches!(outcome, Err(PostgresError::InvalidSchemaName { ref schema }) if schema == "bad.schema"),
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

    /// Refused before `BEGIN READ ONLY` is sent.
    #[test]
    fn a_subject_token_is_refused_by_execute_raw_before_anything_is_run() {
        let warehouse = warehouse();
        for presented in subjects() {
            let outcome = warehouse.execute_raw(&statement(), &presented);
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
        let case = corpus::cases().into_iter().next().expect("the corpus has a case");
        let outcome = warehouse.execute(Executable::Query(case.plan()), &corpus::presented(), corpus::deadline());
        assert!(
            matches!(outcome, Err(PostgresError::PresentedDisagreesWithPosture { .. })),
            "a leg carrying another source's witness is refused: {outcome:?}"
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

    #[cfg(feature = "fixtures")]
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
    #[cfg(feature = "fixtures")]
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
