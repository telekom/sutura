#![forbid(unsafe_code)]
//! Per-caller execution over the ADBC transport, the one `sutura serve` composes: a switched user is
//! named in the statement under the deployment's own sign-in, and a caller presented with a token
//! signs in with it. The first cell reads what the driver sends to a loopback listener; the second
//! asks the tier's server, whose users `nix/clickhouse-tier-provision.sh` declares.

#[cfg(test)]
mod adbc_execute_as {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::path::Path;
    use std::thread;

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

    /// Runs the case through the driver against a listener that refuses it, and returns the one
    /// request the listener read.
    fn sent(presented: &Presented) -> Sent {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let endpoint = format!("http://{}", listener.local_addr().expect("a bound listener has an address"));
        let listening = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the driver dials the listener");
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
            stream
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("the refusal writes");
            let head = String::from_utf8_lossy(received.get(..head_end).expect("the head is received")).into_owned();
            let body = String::from_utf8_lossy(received.get(head_end..head_end + length).expect("the body is received"));
            // A statement with parameters travels as the `query` field of a multipart body.
            let sql = body
                .split_once("name=\"query\"\r\n\r\n")
                .map_or(&*body, |(_, field)| field.split_once("\r\n--").map_or(field, |(sql, _)| sql))
                .to_owned();
            Sent { head, sql }
        });
        let answered = warehouse(endpoint, String::from("sutura_service"), String::from("deployment-secret")).execute(
            Executable::Query(case().plan()),
            presented,
            corpus::deadline(),
        );
        assert!(answered.is_err(), "the listener answers no rows");
        listening.join().expect("the listener read one request")
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

    fn refused_for_lack_of(refused: &AdbcError, grant: &str) {
        let message = refused.to_string();
        assert!(message.contains(grant), "refused for another reason: {message}");
    }

    /// `sutura_switched` has no password, reads `adbc_switched` only, and the service user may
    /// become it: the question is answered under that user's grants, and refused on a table only
    /// the service user may read. `sutura_switched_ungranted` has the same grants and no
    /// `IMPERSONATE` on it, so the server refuses the switch. A token still signs in as the caller.
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
        refused_for_lack_of(&refused, "ACCESS_DENIED");
        let refused = run("adbc_switched", &switched_to("sutura_switched_ungranted"))
            .expect_err("the service user may not become a user it holds no IMPERSONATE on");
        refused_for_lack_of(&refused, "IMPERSONATE");

        let token = variable("SUTURA_CLICKHOUSE_TIER_TOKEN");
        let rows =
            run("adbc_switched", &signed_in_as("sutura_caller_a", &token)).unwrap_or_else(|e| panic!("sutura_caller_a: {e}"));
        assert_eq!(rows.rows(), case.expected().rows().len());
        run("adbc_switched", &signed_in_as("sutura_caller_b", &token)).expect_err("a token signs in only as its own `sub`");
    }
}
