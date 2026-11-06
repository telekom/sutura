#![forbid(unsafe_code)]
//! A federated leg under `EXECUTE AS`, over the ADBC transport: the switched user is named in the
//! leg's one statement under the deployment's own sign-in, and the tier's server answers the leg
//! under that user's grants. The first cell reads what the driver sends to a loopback listener; the
//! others ask the tier's server, whose users `nix/clickhouse-tier-provision.sh` declares.

#[cfg(test)]
mod adbc_execute_as_legs {
    use std::io::{ErrorKind, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::definitions::DefinitionDigest;
    use sutura_domain::identity::{Presented, PrincipalName, Secret};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, RelationshipName, TableName};
    use sutura_domain::nonempty::NonEmpty;
    use sutura_domain::pinned::MetricDigest;
    use sutura_domain::plan::{
        Executable, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanJoinKey,
        PlanKey, PlanPredicate, PlanTerm, PredicateOrigin, ResultLabel, StatementTables,
    };
    use sutura_domain::source::SourcePosture;
    use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content};
    use sutura_domain::warehouse::{ParamValue, ResultBudget, RowSet, Value, Warehouse as _};
    use sutura_exec_clickhouse::adbc::{AdbcClickHouseWarehouse, AdbcError, driver_location};
    use sutura_exec_clickhouse::fixture::credential_from_env;

    /// The databases the tier grants the switched users, each written by these cells alone.
    const LEGS: &str = "adbc_switched_legs";
    const LOGGED: &str = "adbc_switched_legs_log";
    /// A database no switched user may read.
    const SERVICE_ONLY: &str = "adbc_switched_legs_service_only";

    fn budget() -> ResultBudget {
        ResultBudget::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test budget is positive"))
    }

    fn opened(endpoint: String, posture: SourcePosture, user: &str, password: &str) -> AdbcClickHouseWarehouse {
        let driver = driver_location().unwrap_or_else(|e| panic!("the dev shell and every nix check mount the driver: {e}"));
        AdbcClickHouseWarehouse::new(
            corpus::source(),
            posture,
            driver,
            endpoint,
            String::from(user),
            Secret::new(String::from(password)),
            None,
            budget(),
        )
    }

    fn switched_to(name: &str) -> Presented {
        Presented::SubjectPrincipal {
            name: PrincipalName::parse(name).expect("a tier user is a principal"),
        }
    }

    fn day(of_january: u8) -> Date {
        Date::new(2026, 1, of_january).expect("a January date in 2026 is a date")
    }

    fn column(table: &TableName, name: &str) -> PlanColumn {
        PlanColumn::new(table.clone(), ColumnName::parse(name).expect("a corpus column is a name"))
    }

    fn events(name: &str) -> PlanColumn {
        column(&corpus::table(), name)
    }

    fn regions(name: &str) -> PlanColumn {
        column(&corpus::lookup_table(), name)
    }

    fn keyed(label: &str, on: PlanColumn) -> PlanKey {
        PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse(label).expect("a corpus dimension is a name")),
            on,
        )
    }

    fn summed(name: &str, leaf: usize) -> LegTerm {
        LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: events(name),
            },
            ResultLabel::internal(InternalLabel::Leaf(leaf)),
        )
    }

    /// The corpus range, January 1st to 3rd, then `requested`, each binding its own parameter.
    fn bound(requested: Vec<(PlanPredicate, ParamValue)>) -> PlanBindings {
        let mut filters = vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: events("day"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: events("day"),
                    param: 1,
                },
            ),
        ];
        let mut params = vec![ParamValue::Date(day(1)), ParamValue::Date(day(3))];
        for (predicate, value) in requested {
            filters.push(PlanFilter::new(PredicateOrigin::Requested, predicate));
            params.push(value);
        }
        PlanBindings::parse(filters, params).expect("each filter binds its own parameter in order")
    }

    /// A fact leg over the corpus table, by day.
    fn fact(tables: StatementTables, keys: Vec<PlanKey>, terms: Vec<LegTerm>, bindings: PlanBindings) -> LegPlan {
        LegPlan::Fact {
            source: corpus::source(),
            metric: MetricDigest::new(
                MetricName::parse("amount_total").expect("a corpus metric is a name"),
                DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
            ),
            tables,
            bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Day, events("day")),
            keys,
            terms,
            bindings,
            range: TimeRange::new(day(1), day(3)).expect("the corpus range is not empty"),
        }
    }

    /// `amount_cents` by region name: the corpus table left-joined to the regions table, so the
    /// event with no region has no region name.
    fn joined() -> LegPlan {
        let tables = StatementTables::parse(
            corpus::table(),
            vec![PlanJoin::new(
                RelationshipName::parse("event_region").expect("a relationship is a name"),
                corpus::lookup_table(),
                JoinType::ManyToOne,
                NonEmpty::parse(vec![PlanJoinKey::Equal {
                    origin: events("region"),
                    target: regions("region"),
                }])
                .expect("the join declares one key"),
            )],
        )
        .expect("two differently named tables are distinguishable");
        fact(
            tables,
            vec![keyed("region_name", regions("region_name"))],
            vec![summed("amount_cents", 0)],
            bound(Vec::new()),
        )
    }

    /// One expected row: a key (`None` for NULL), the day and the cents.
    type Row<'a> = (Option<&'a str>, &'a str, i64);

    /// The warehouse a switched leg ran on, and what it answered.
    type Switched = (AdbcClickHouseWarehouse, Result<RowSet, AdbcError>);

    /// Rows under `leg`'s own labels.
    fn rows(leg: &LegPlan, rows: &[Row<'_>]) -> RowSet {
        let rows = rows
            .iter()
            .map(|&(key, on, cents)| {
                vec![
                    key.map_or(Value::Null, |key| Value::Text(String::from(key))),
                    Value::Text(String::from(on)),
                    Value::Integer(cents),
                ]
            })
            .collect();
        RowSet::new(leg.result_labels(), rows).expect("three cells under three labels")
    }

    fn agreeing(expected: &RowSet, answered: &RowSet) {
        agree_on_content(expected, answered, RealTolerance::DIFFERENTIAL).unwrap_or_else(|e| panic!("{e}"));
    }

    fn variable(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_absent| panic!("{name} is unset, so the tier has no fixture credential"))
    }

    /// The tier's server and its service user.
    struct Tier {
        host: String,
        port: u16,
        user: String,
        password: String,
    }

    impl Tier {
        fn here(cell: &str) -> Option<Self> {
            match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse") {
                Provisioned::At(endpoint) => Some(Self {
                    host: String::from(endpoint.host()),
                    port: endpoint.port(),
                    user: variable("SUTURA_CLICKHOUSE_TIER_USER"),
                    password: variable("SUTURA_CLICKHOUSE_TIER_PASSWORD"),
                }),
                Provisioned::Skipped(absent) => {
                    eprintln!("adbc_execute_as_legs::{cell}: NOT RUN - {absent}");
                    None
                }
            }
        }

        /// Loads the corpus table and the regions table into `database` as the service user.
        fn load(&self, database: &str) {
            let credential = credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
            let loader = AdbcClickHouseWarehouse::connect_in_database(
                corpus::source(),
                corpus::posture(),
                &self.host,
                self.port,
                &credential,
                database,
                budget(),
            )
            .unwrap_or_else(|e| panic!("clickhouse did not open {database}: {e}"));
            loader
                .load_conformance_csv(&corpus::table(), &corpus::on_disk())
                .unwrap_or_else(|e| panic!("clickhouse could not load the corpus into {database}: {e}"));
            loader
                .load_federated_csv(&corpus::lookup_table(), &corpus::lookup_on_disk())
                .unwrap_or_else(|e| panic!("clickhouse could not load the regions into {database}: {e}"));
        }

        fn at(&self, database: &str, posture: SourcePosture) -> AdbcClickHouseWarehouse {
            opened(
                format!("http://{}:{}/?database={database}", self.host, self.port),
                posture,
                &self.user,
                &self.password,
            )
        }

        /// `leg` in `database`, switched to `user` under the service user's own sign-in.
        fn switched(&self, database: &str, leg: &LegPlan, user: &str) -> Switched {
            let warehouse = self.at(database, SourcePosture::ImpersonationAtSource);
            let answered = warehouse
                .execute(Executable::Leg(leg), &switched_to(user), corpus::deadline())
                .map(|batches| batches.to_rows().unwrap_or_else(|e| panic!("{user}: {e}")));
            (warehouse, answered)
        }

        fn answered_as(&self, database: &str, leg: &LegPlan, user: &str) -> RowSet {
            self.switched(database, leg, user)
                .1
                .unwrap_or_else(|refused| panic!("{user}: {refused}"))
        }

        /// `leg` in `database` as the service user, at a shared source.
        fn shared(&self, database: &str, leg: &LegPlan) -> RowSet {
            self.at(database, corpus::posture())
                .execute(Executable::Leg(leg), &corpus::presented(), corpus::deadline())
                .unwrap_or_else(|e| panic!("the service user: {e}"))
                .to_rows()
                .unwrap_or_else(|e| panic!("the service user: {e}"))
        }

        /// What the server answers `sql` with, as text, asked over HTTP/1.0 as the service user.
        fn asked(&self, sql: &str) -> String {
            let mut stream = TcpStream::connect((self.host.as_str(), self.port)).expect("the tier accepts a connection");
            let request = format!(
                "POST / HTTP/1.0\r\nHost: {}\r\nX-ClickHouse-User: {}\r\nX-ClickHouse-Key: {}\r\nContent-Length: {}\r\n\r\n{sql}",
                self.host,
                self.user,
                self.password,
                sql.len()
            );
            stream.write_all(request.as_bytes()).expect("the request writes");
            let mut answer = String::new();
            stream.read_to_string(&mut answer).expect("the answer reads");
            let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((answer.as_str(), ""));
            assert!(
                head.lines().next().is_some_and(|status| status.contains(" 200 ")),
                "{sql}: {answer}"
            );
            if header(head, "transfer-encoding").is_some_and(|coding| coding.eq_ignore_ascii_case("chunked")) {
                dechunked(body)
            } else {
                String::from(body)
            }
        }
    }

    fn dechunked(mut body: &str) -> String {
        let mut out = String::new();
        while let Some((size, rest)) = body.split_once("\r\n") {
            let size = usize::from_str_radix(size.trim(), 16).expect("a chunk size is hexadecimal");
            if size == 0 {
                break;
            }
            out.push_str(rest.get(..size).expect("the chunk arrived whole"));
            body = rest.get(size + 2..).unwrap_or_default();
        }
        out
    }

    fn header<'h>(head: &'h str, name: &str) -> Option<&'h str> {
        head.lines().find_map(|line| {
            line.split_once(':')
                .filter(|(found, _)| found.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.trim())
        })
    }

    /// Refused by the server for lack of `grant`, and classified as a refused identity.
    fn refused_for_lack_of((warehouse, answered): Switched, grant: &str) {
        let Err(refused) = answered else {
            panic!("answered without {grant}");
        };
        let message = refused.to_string();
        assert!(message.contains(grant), "refused for another reason: {message}");
        assert!(warehouse.source_refused(&refused), "{message}");
    }

    /// A bare 500: no rows, and no server code.
    const BARE_REFUSAL: &[u8] = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    /// The first request `listener` receives within ten seconds, answered with [`BARE_REFUSAL`].
    fn first_request(listener: &TcpListener) -> Option<String> {
        let until = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock && Instant::now() < until => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        };
        stream.set_nonblocking(false).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
        let mut received = Vec::new();
        let mut chunk = [0_u8; 4096];
        while !whole(&String::from_utf8_lossy(&received)) {
            let read = stream.read(&mut chunk).ok()?;
            if read == 0 {
                break;
            }
            received.extend_from_slice(chunk.get(..read)?);
        }
        stream.write_all(BARE_REFUSAL).ok()?;
        Some(String::from_utf8_lossy(&received).into_owned())
    }

    /// The head is in, and as much body as it announces.
    fn whole(request: &str) -> bool {
        request.split_once("\r\n\r\n").is_some_and(|(head, body)| {
            header(head, "content-length")
                .and_then(|length| length.parse::<usize>().ok())
                .is_some_and(|length| body.len() >= length)
        })
    }

    /// A leg under a switch is one statement, sent under the deployment's own sign-in and prefixed
    /// with the switched user, and its date parameters still bind under the prefix.
    #[test]
    fn a_switched_leg_is_named_in_its_one_statement_under_the_deployment_sign_in() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let endpoint = format!("http://{}", listener.local_addr().expect("a bound listener has an address"));
        listener.set_nonblocking(true).expect("the listener goes non-blocking");
        let listening = thread::spawn(move || first_request(&listener));
        let answered = opened(
            endpoint,
            SourcePosture::ImpersonationAtSource,
            "sutura_service",
            "deployment-secret",
        )
        .execute(
            Executable::Leg(corpus::leg_case().leg()),
            &switched_to("sutura_switched"),
            corpus::deadline(),
        );
        let request = listening.join().expect("the listener thread finishes");
        let Some(request) = request else {
            panic!("nothing dialled the listener; the warehouse answered {:?}", answered.err());
        };
        let (head, body) = request.split_once("\r\n\r\n").expect("the request has a head");
        assert_eq!(header(head, "x-clickhouse-user"), Some("sutura_service"), "{head}");
        assert_eq!(header(head, "x-clickhouse-key"), Some("deployment-secret"), "{head}");
        assert!(body.contains("EXECUTE AS \"sutura_switched\" SELECT "), "{body}");
        assert!(body.contains("{p0:Date}") && body.contains("{p1:Date}"), "{body}");
    }

    /// `sutura_switched` reads its own database: the leg answers the whole case's rows. A database
    /// only the service user reads is refused to it, and the server refuses to switch to
    /// `sutura_switched_ungranted`, on which the service user holds no `IMPERSONATE`.
    #[test]
    fn a_switched_leg_answers_under_its_own_grants() {
        let Some(tier) = Tier::here("grants") else { return };
        tier.load(LEGS);
        tier.load(SERVICE_ONLY);
        let case = corpus::leg_case();
        agreeing(case.expected(), &tier.answered_as(LEGS, case.leg(), "sutura_switched"));
        refused_for_lack_of(tier.switched(SERVICE_ONLY, case.leg(), "sutura_switched"), "ACCESS_DENIED");
        refused_for_lack_of(tier.switched(LEGS, case.leg(), "sutura_switched_ungranted"), "IMPERSONATE");
    }

    /// The LEFT JOIN's unmatched key is NULL under the switch, so `join_use_nulls` reaches the
    /// switched statement: without it the server answers the empty string.
    #[test]
    fn a_switched_leg_reads_an_unmatched_left_join_key_as_null() {
        let Some(tier) = Tier::here("left_join") else { return };
        tier.load(LEGS);
        let leg = joined();
        let expected = rows(
            &leg,
            &[
                (Some("Eastern"), "2026-01-01", 350),
                (Some("Eastern"), "2026-01-02", 150),
                (Some("Northern"), "2026-01-01", 400),
                (Some("Northern"), "2026-01-02", 1300),
                (None, "2026-01-01", 50),
            ],
        );
        agreeing(&expected, &tier.answered_as(LEGS, &leg, "sutura_switched"));
    }

    /// A Text parameter and the two Date parameters bind under the prefix.
    #[test]
    fn a_switched_leg_binds_its_text_and_date_parameters() {
        let Some(tier) = Tier::here("parameters") else { return };
        tier.load(LEGS);
        let leg = fact(
            StatementTables::only(corpus::table()),
            vec![keyed("region", events("region"))],
            vec![summed("amount_cents", 0)],
            bound(vec![(
                PlanPredicate::Equals {
                    column: events("region"),
                    param: 2,
                },
                ParamValue::Text(String::from("east")),
            )]),
        );
        let expected = rows(&leg, &[(Some("east"), "2026-01-01", 350), (Some("east"), "2026-01-02", 150)]);
        agreeing(&expected, &tier.answered_as(LEGS, &leg, "sutura_switched"));
    }

    /// An integer, a decimal and a float sum each arrive in their own slot of the sum tuple, and the
    /// switched leg reads each one as the service user's leg does.
    #[test]
    fn a_switched_leg_sums_each_slot_as_the_service_user_does() {
        let Some(tier) = Tier::here("sum_slots") else { return };
        tier.load(LEGS);
        let leg = fact(
            StatementTables::only(corpus::table()),
            vec![keyed("region", events("region"))],
            vec![summed("amount_cents", 0), summed("decimal_amount", 1), summed("rate", 2)],
            bound(Vec::new()),
        );
        let shared = tier.shared(LEGS, &leg);
        assert_eq!(shared.rows().len(), 5, "{shared:?}");
        agreeing(&shared, &tier.answered_as(LEGS, &leg, "sutura_switched"));
    }

    /// RECORDS what the server does with a leg switched to a user whose profile sets `readonly = 1`.
    /// The server's source reads as though the switch clamps sutura's settings against that profile
    /// and drops them without an error, `join_use_nulls` among them; no run here has observed it.
    /// The cell holds only that the leg reaches the server, and prints what came back.
    #[test]
    fn a_switched_leg_under_a_readonly_profile_reaches_the_server() {
        let Some(tier) = Tier::here("readonly") else { return };
        tier.load(LEGS);
        let leg = joined();
        let (warehouse, answered) = tier.switched(LEGS, &leg, "sutura_switched_readonly");
        let observed = match answered {
            Ok(answered) => {
                let unmatched = answered
                    .rows()
                    .iter()
                    .find(|row| row.last() == Some(&Value::Integer(50)))
                    .and_then(|row| row.first().cloned());
                format!(
                    "answered {} rows; the unmatched key read as {unmatched:?}",
                    answered.rows().len()
                )
            }
            Err(refused @ AdbcError::Driver(_)) => format!(
                "refused by the server (source_refused: {}): {refused}",
                warehouse.source_refused(&refused)
            ),
            Err(refused) => panic!("the leg did not reach the server: {refused:?}"),
        };
        eprintln!("adbc_execute_as_legs::readonly: OBSERVED - {observed}");
    }

    /// RECORDS what `system.query_log` holds for one switched leg: every statement the leg's
    /// connection sent in its database, each with its `user` and `initial_user`. It holds that
    /// exactly one of them is the switched statement, and prints them all.
    #[test]
    fn a_switched_leg_is_logged_once_as_its_one_switched_statement() {
        let Some(tier) = Tier::here("query_log") else { return };
        tier.load(LOGGED);
        let since = tier.asked("SELECT toUnixTimestamp64Micro(now64(6))");
        let since = since.trim();
        drop(tier.answered_as(LOGGED, corpus::leg_case().leg(), "sutura_switched"));
        drop(tier.asked("SYSTEM FLUSH LOGS"));
        let logged = tier.asked(&format!(
            "SELECT user, initial_user, toString(query_kind), \
             if(startsWith(query, 'EXECUTE AS \"sutura_switched\" '), 'switched', 'unswitched') \
             FROM system.query_log \
             WHERE toUnixTimestamp64Micro(event_time_microseconds) >= {since} \
             AND current_database = '{LOGGED}' AND type = 'QueryFinish' \
             ORDER BY event_time_microseconds FORMAT TSV"
        ));
        eprintln!("adbc_execute_as_legs::query_log: OBSERVED user, initial_user, kind, prefix\n{logged}");
        let switched = logged.lines().filter(|line| line.ends_with("\tswitched")).count();
        assert_eq!(switched, 1, "{logged}");
    }
}
