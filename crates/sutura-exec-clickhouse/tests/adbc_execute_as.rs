#![forbid(unsafe_code)]
//! Per-caller execution over the ADBC transport, the one `sutura serve` composes: a switched user is
//! named in the statement under the deployment's own sign-in, and a caller presented with a token
//! signs in with it. The first cell reads what the driver sends to a loopback listener; the second
//! asks the tier's server, whose users `nix/clickhouse-tier-provision.sh` declares.

#[cfg(test)]
mod adbc_execute_as {
    use std::io::{ErrorKind, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus::{self, Case};
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::identity::{Presented, PrincipalName, Secret};
    use sutura_domain::plan::Executable;
    use sutura_domain::source::SourcePosture;
    use sutura_domain::warehouse::{ResultBudget, Warehouse as _};
    use sutura_exec_clickhouse::ClickHouseWarehouse;
    use sutura_exec_clickhouse::adbc::{AdbcClickHouseWarehouse, AdbcError, driver_location};
    use sutura_exec_clickhouse::fixture::credential_from_env;
    use sutura_exec_clickhouse::transport::Endpoint;

    fn warehouse(endpoint: String, user: String, password: String) -> AdbcClickHouseWarehouse {
        let driver = driver_location().unwrap_or_else(|e| panic!("the dev shell and every nix check mount the driver: {e}"));
        AdbcClickHouseWarehouse::new(
            corpus::source(),
            SourcePosture::ImpersonationAtSource,
            driver,
            endpoint,
            user,
            Secret::new(password),
            None,
            ResultBudget::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test budget is positive")),
        )
    }

    fn case() -> Case {
        corpus::cases()
            .into_iter()
            .find(|case| case.name() == "decimal-total-by-day")
            .expect("the corpus has the case")
    }

    fn switched_to(name: &str) -> Presented {
        Presented::SubjectPrincipal {
            name: PrincipalName::parse(name).expect("a tier user is a principal"),
        }
    }

    fn signed_in_as(name: &str, token: &str) -> Presented {
        Presented::SubjectSignIn {
            name: PrincipalName::parse(name).expect("a tier user is a principal"),
            material: Secret::new(String::from(token)),
        }
    }

    /// One request's header block and its SQL text, as a loopback listener received it.
    struct Sent {
        head: String,
        sql: String,
    }

    impl Sent {
        fn header(&self, name: &str) -> Option<&str> {
            header(&self.head, name)
        }
    }

    fn header<'h>(head: &'h str, name: &str) -> Option<&'h str> {
        head.lines().find_map(|line| {
            line.split_once(':')
                .filter(|(found, _)| found.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.trim())
        })
    }

    /// How long the listener waits to be dialled before the cell fails.
    const DIALLED_WITHIN: Duration = Duration::from_secs(10);

    /// A 500 with no body: no rows, and no server code.
    const BARE_REFUSAL: &str = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    /// `ClickHouse`'s error answer for `code`: the code in its header and at the head of its text.
    fn server_error(code: u32, text: &str) -> String {
        let body = format!("Code: {code}. DB::Exception: {text}. (version 26.9.15.5 (official build))");
        format!(
            "HTTP/1.1 500 Internal Server Error\r\nX-ClickHouse-Exception-Code: {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// The listener's first connection, or `None` if nothing dialled it within [`DIALLED_WITHIN`].
    fn accepted(listener: &TcpListener) -> Option<TcpStream> {
        let deadline = Instant::now() + DIALLED_WITHIN;
        loop {
            match listener.accept() {
                Ok((stream, _)) => return Some(stream),
                Err(error) if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        }
    }

    /// Reads one request off `stream` and answers it with `answer`.
    fn answer_one(mut stream: TcpStream, answer: &str) -> Sent {
        stream.set_nonblocking(false).expect("the stream blocks");
        stream
            .set_read_timeout(Some(DIALLED_WITHIN))
            .expect("the stream takes a read timeout");
        let mut received = Vec::new();
        let mut chunk = [0_u8; 4096];
        let (head_end, length) = loop {
            let read = stream.read(&mut chunk).expect("the request reads");
            assert_ne!(read, 0, "the request ended before its headers");
            received.extend_from_slice(chunk.get(..read).expect("read within the chunk"));
            if let Some(at) = received.windows(4).position(|window| window == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(received.get(..at).expect("the head is received"));
                let length = header(&head, "content-length").map(str::parse::<usize>);
                break (
                    at + 4,
                    length.expect("the driver sends a sized body").expect("a length is a number"),
                );
            }
        };
        while received.len() < head_end + length {
            let read = stream.read(&mut chunk).expect("the body reads");
            assert_ne!(read, 0, "the request ended before its body");
            received.extend_from_slice(chunk.get(..read).expect("read within the chunk"));
        }
        stream.write_all(answer.as_bytes()).expect("the answer writes");
        let head = String::from_utf8_lossy(received.get(..head_end).expect("the head is received")).into_owned();
        let body = String::from_utf8_lossy(received.get(head_end..head_end + length).expect("the body is received"));
        // A statement with parameters travels as the `query` field of a multipart body.
        let sql = body
            .split_once("name=\"query\"\r\n\r\n")
            .map_or(&*body, |(_, field)| field.split_once("\r\n--").map_or(field, |(sql, _)| sql))
            .to_owned();
        Sent { head, sql }
    }

    /// What the warehouse answered when the listener answered `answer`, and the request it read.
    fn exchange(presented: &Presented, answer: String) -> (AdbcClickHouseWarehouse, AdbcError, Sent) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let endpoint = format!("http://{}", listener.local_addr().expect("a bound listener has an address"));
        listener.set_nonblocking(true).expect("the listener goes non-blocking");
        let listening = thread::spawn(move || accepted(&listener).map(|stream| answer_one(stream, &answer)));
        let warehouse = warehouse(endpoint, String::from("sutura_service"), String::from("deployment-secret"));
        let answered = warehouse
            .execute(Executable::Query(case().plan()), presented, corpus::deadline())
            .err();
        let sent = listening.join().expect("the listener thread finishes");
        let Some(sent) = sent else {
            panic!("nothing dialled the listener within {DIALLED_WITHIN:?}; the warehouse answered {answered:?}");
        };
        let refused = answered.unwrap_or_else(|| panic!("the listener answers no rows"));
        (warehouse, refused, sent)
    }

    fn sent(presented: &Presented) -> Sent {
        exchange(presented, String::from(BARE_REFUSAL)).2
    }

    /// A switched user is named in the statement, never in the handshake: the driver signs in as the
    /// deployment's own user with its own password and sends `EXECUTE AS "<user>" <statement>`. A
    /// caller presented with a token signs in as that user with it, and the statement is unchanged.
    #[test]
    fn a_switched_user_is_named_in_the_statement_and_a_token_signs_in() {
        let switched = sent(&switched_to("sutura_switched"));
        assert_eq!(
            switched.header("x-clickhouse-user"),
            Some("sutura_service"),
            "{}",
            switched.head
        );
        assert_eq!(switched.header("x-clickhouse-key"), Some("deployment-secret"));
        let statement = switched.sql.strip_prefix("EXECUTE AS \"sutura_switched\" ");
        assert!(statement.is_some_and(|rest| !rest.trim().is_empty()), "{}", switched.sql);

        let signed_in = sent(&signed_in_as("sutura_caller_a", "caller-token"));
        assert_eq!(
            signed_in.header("x-clickhouse-user"),
            Some("sutura_caller_a"),
            "{}",
            signed_in.head
        );
        assert_eq!(signed_in.header("x-clickhouse-key"), Some("caller-token"));
        assert!(!signed_in.sql.contains("EXECUTE AS"), "{}", signed_in.sql);
    }

    /// The driver reports every server error with one status, so the server's code decides: a
    /// refused identity reads as refused, a spent `max_execution_time` as a spent deadline, and any
    /// other code as neither.
    #[test]
    fn a_server_refusal_reads_as_refused_and_a_server_timeout_as_spent() {
        let switched = switched_to("sutura_switched");
        for (code, text) in [
            (
                497,
                "sutura_service: Not enough privileges. To execute this query, it's necessary to have the grant IMPERSONATE ON sutura_switched. (ACCESS_DENIED)",
            ),
            (516, "sutura_service: Authentication failed. (AUTHENTICATION_FAILED)"),
        ] {
            let (warehouse, refused, _) = exchange(&switched, server_error(code, text));
            assert!(warehouse.source_refused(&refused), "{code}: {refused}");
            assert!(!warehouse.deadline_exceeded(&refused), "{code}: {refused}");
        }
        let (warehouse, spent, _) = exchange(
            &switched,
            server_error(159, "Timeout exceeded: elapsed 30.1 seconds, maximum: 30. (TIMEOUT_EXCEEDED)"),
        );
        assert!(warehouse.deadline_exceeded(&spent), "{spent}");
        assert!(!warehouse.source_refused(&spent), "{spent}");
        let (warehouse, failed, _) = exchange(
            &switched,
            server_error(
                60,
                "Unknown table expression identifier 'conformance_events'. (UNKNOWN_TABLE)",
            ),
        );
        assert!(!warehouse.source_refused(&failed), "{failed}");
        assert!(!warehouse.deadline_exceeded(&failed), "{failed}");
    }

    /// A federated leg never runs under a switch: it is refused before the driver is asked for
    /// anything, so nothing listens at the endpoint.
    #[test]
    fn a_federated_leg_is_refused_under_a_switch_before_anything_is_dialled() {
        let refused = warehouse(
            String::from("http://127.0.0.1:1"),
            String::from("sutura_service"),
            String::from("deployment-secret"),
        )
        .execute(
            Executable::Leg(corpus::leg_case().leg()),
            &switched_to("sutura_switched"),
            corpus::deadline(),
        )
        .err();
        assert!(matches!(refused, Some(AdbcError::LegUnderExecuteAs)), "{refused:?}");
    }

    fn variable(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_absent| panic!("{name} is unset, so the tier has no fixture credential"))
    }

    /// Loads the corpus into `database` as the service user, over the fixture loader.
    fn load(host: &str, port: u16, database: &str) {
        let auth = credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        ClickHouseWarehouse::connect_in_database(
            corpus::source(),
            SourcePosture::ImpersonationAtSource,
            Endpoint::plaintext(host, port),
            auth,
            database,
            ResultBudget::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test budget is positive")),
        )
        .unwrap_or_else(|e| panic!("clickhouse did not open {database}: {e}"))
        .load_conformance_csv(&corpus::table(), &corpus::on_disk())
        .unwrap_or_else(|e| panic!("clickhouse could not load the corpus into {database}: {e}"));
    }

    /// Refused by the server for lack of `grant`, and classified as a refused identity.
    fn refused_for_lack_of(warehouse: &AdbcClickHouseWarehouse, refused: &AdbcError, grant: &str) {
        let message = refused.to_string();
        assert!(message.contains(grant), "refused for another reason: {message}");
        assert!(warehouse.source_refused(refused), "{message}");
    }

    /// `sutura_switched` has no password, reads `adbc_switched` only, and the service user may
    /// become it: the question is answered under that user's grants, and refused on a table only
    /// the service user may read. `sutura_switched_ungranted` has the same grants and no
    /// `IMPERSONATE` on it, so the server refuses the switch. A token still signs in as the caller.
    /// Each refusal reads as a refused identity.
    #[test]
    fn a_switched_user_answers_under_its_own_grants() {
        // The two databases only this cell writes to.
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse") {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("adbc_execute_as::switched: NOT RUN - {absent}");
                return;
            }
        };
        let (host, port) = (endpoint.host(), endpoint.port());
        let (user, password) = (
            variable("SUTURA_CLICKHOUSE_TIER_USER"),
            variable("SUTURA_CLICKHOUSE_TIER_PASSWORD"),
        );
        load(host, port, "adbc_switched");
        load(host, port, "adbc_service_only");
        let case = case();
        let at = |database: &str| {
            warehouse(
                format!("http://{host}:{port}/?database={database}"),
                user.clone(),
                password.clone(),
            )
        };
        let run = |database: &str, presented: &Presented| {
            at(database).execute(Executable::Query(case.plan()), presented, corpus::deadline())
        };

        let rows = run("adbc_switched", &switched_to("sutura_switched")).unwrap_or_else(|e| panic!("sutura_switched: {e}"));
        assert_eq!(rows.rows(), case.expected().rows().len());
        let refused = run("adbc_service_only", &switched_to("sutura_switched"))
            .expect_err("a switched user reads only what its own grants admit");
        refused_for_lack_of(&at("adbc_service_only"), &refused, "ACCESS_DENIED");
        let refused = run("adbc_switched", &switched_to("sutura_switched_ungranted"))
            .expect_err("the service user may not become a user it holds no IMPERSONATE on");
        refused_for_lack_of(&at("adbc_switched"), &refused, "IMPERSONATE");

        let token = variable("SUTURA_CLICKHOUSE_TIER_TOKEN");
        let rows =
            run("adbc_switched", &signed_in_as("sutura_caller_a", &token)).unwrap_or_else(|e| panic!("sutura_caller_a: {e}"));
        assert_eq!(rows.rows(), case.expected().rows().len());
        let refused =
            run("adbc_switched", &signed_in_as("sutura_caller_b", &token)).expect_err("a token signs in only as its own `sub`");
        assert!(at("adbc_switched").source_refused(&refused), "{refused}");
    }
}
