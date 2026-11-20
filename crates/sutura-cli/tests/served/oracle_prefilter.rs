//! Real CLOB/NCLOB and session comparison variants at the combiner/driver venue, not a served caller.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arrow_schema::{DataType, Field, Schema};
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::{FederationCombiner as _, PolicyRead, StatementLegs, StatementQuestion, StatementTable};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{RawRows, Value};
use sutura_exec_datafusion::DataFusionCombiner;

use super::super::Tier;

struct Native<'a> {
    connection: &'a oracledb::Connection,
    table: &'a QualifiedTable,
}

#[derive(Debug, thiserror::Error)]
enum NativeStop {
    #[error("the native fixture opened an unexpected source set: {observed:?}")]
    Sources { observed: BTreeSet<SourceName> },
    #[error("the native fixture received an unexpected source: {observed}")]
    Source { observed: SourceName },
    #[error("the native fixture received an unexpected scan set: {observed:?}, expected {expected}")]
    Tables {
        observed: BTreeSet<QualifiedTable>,
        expected: QualifiedTable,
    },
    #[error("the native fixture requires a planned statement")]
    Unplanned,
    #[error("the native driver did not answer")]
    Driver {
        #[source]
        cause: oracledb::Error,
    },
}

impl StatementLegs for Native<'_> {
    type Stop = NativeStop;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        if sources != &BTreeSet::from([source()]) {
            return Err(NativeStop::Sources {
                observed: sources.clone(),
            });
        }
        Ok(())
    }

    fn run(&mut self, _source: &SourceName, _statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        Err(NativeStop::Unplanned)
    }

    fn run_planned(
        &mut self,
        source: &SourceName,
        statement: &RawStatement,
        tables: &BTreeSet<QualifiedTable>,
        _policy: PolicyRead,
    ) -> Result<RawRows, Self::Stop> {
        if source != &self::source() {
            return Err(NativeStop::Source {
                observed: source.clone(),
            });
        }
        if tables != &BTreeSet::from([self.table.clone()]) {
            return Err(NativeStop::Tables {
                observed: tables.clone(),
                expected: self.table.clone(),
            });
        }
        let cursor = self
            .connection
            .query(statement.as_str(), &[])
            .map_err(|cause| NativeStop::Driver { cause })?;
        let columns = cursor
            .columns()
            .iter()
            .map(|field| field.name().to_owned())
            .collect::<Vec<_>>();
        let values = cursor
            .map(|row| {
                let row = row.map_err(|cause| NativeStop::Driver { cause })?;
                (0..columns.len())
                    .map(|index| {
                        row.get::<Option<String>>(index)
                            .map(|text| text.map_or(Value::Null, Value::Text))
                            .map_err(|cause| NativeStop::Driver { cause })
                    })
                    .collect::<Result<Vec<_>, NativeStop>>()
            })
            .collect::<Result<Vec<_>, NativeStop>>()?;
        Ok(RawRows::of(columns, values))
    }
}

fn source() -> SourceName {
    SourceName::parse("local").expect("a fixture source")
}

fn policy(enabled: bool, replacement: &str) -> serde_json::Value {
    let mut value = serde_json::json!({"max_text_bytes":128,"redact":[{"pattern":"secret_[^ ]+","replace":replacement}]});
    if enabled {
        value["raw_prefilter"] =
            serde_json::json!({"acknowledged_because":"this native fixture accepts secret-dependent timing"});
    }
    value
}

fn run(
    connection: &oracledb::Connection,
    path: &QualifiedTable,
    column: &str,
    kind: &str,
    policy: &serde_json::Value,
    sql: &str,
) -> RawRows {
    let metadata = HashMap::from([
        (String::from("sutura:text_policy"), policy.to_string()),
        (String::from("sutura:oracle_lob"), kind.to_owned()),
    ]);
    let table = StatementTable::new(
        source(),
        path.clone(),
        TypeDialect::Oracle,
        Arc::new(Schema::new(vec![
            Field::new(column, DataType::Utf8, true).with_metadata(metadata),
            Field::new("COHORT", DataType::Utf8, true),
        ])),
    );
    let combiner = DataFusionCombiner::new().expect("the native statement combiner starts");
    let statement = RawStatement::parse(sql).expect("a generated query");
    let deadline = Deadline::opened_at(
        Instant::now(),
        Budget::parse(Duration::from_secs(120)).expect("a bounded native budget"),
    );
    combiner
        .answer_statement(
            StatementQuestion::new(&statement, &[table], false, 1 << 30, deadline),
            &mut Native { connection, table: path },
        )
        .expect("a supported native statement")
        .unwrap_or_else(|cause| panic!("the real native query must answer: {cause:?}"))
}

fn populate(connection: &oracledb::Connection, name: &str, table: &str, scheme: &str) {
    let setup = Instant::now();
    connection
        .execute(&format!("BEGIN EXECUTE IMMEDIATE 'DROP TABLE {table} PURGE'; EXCEPTION WHEN OTHERS THEN IF SQLCODE != -942 THEN RAISE; END IF; END;"), &[])
        .expect("an old private fixture drops");
    connection
        .execute(
            &format!("CREATE TABLE {table} (\"BODY\" CLOB, \"BODY_N\" NCLOB, \"COHORT\" VARCHAR2(16))"),
            &[],
        )
        .expect("real CLOB and NCLOB columns create");
    let body = "CASE n WHEN 999984 THEN 'secret_other' WHEN 999985 THEN 'visible TERM' WHEN 999986 THEN 'visible TERM' WHEN 999987 THEN NULL WHEN 999988 THEN 'secret_TERM' WHEN 999989 THEN 'prefix secret_TERM suffix' WHEN 999990 THEN 'term' WHEN 999991 THEN 'caf\u{e9} TERM' WHEN 999992 THEN 'stra\u{df}e' WHEN 999993 THEN '\u{130}STANBUL' WHEN 999994 THEN 'caf\u{e9}' WHEN 999995 THEN 'TERxM' WHEN 999996 THEN 'xERM' WHEN 999997 THEN 'TERx' WHEN 999998 THEN 'masked' ELSE 'unrelated' END";
    let clob = format!(
        "CASE WHEN n = 999999 THEN TO_CLOB(RPAD('a', 4000, 'a')) || RPAD('a', 100, 'a') || ' TERM' WHEN n = 1000000 THEN TO_CLOB(RPAD('a', 4000, 'a')) || RPAD('a', 100, 'a') || ' secret_TERM' ELSE TO_CLOB({body}) END"
    );
    connection.execute(&format!("INSERT INTO {table} SELECT body, TO_NCLOB(body), CASE WHEN n IN (999984, 999988, 999989, 1000000) THEN 'hidden' WHEN n < 999984 THEN 'filler' ELSE 'rare' END FROM (SELECT n, {clob} body FROM (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000000))"), &[]).expect("one million actual LOB rows insert");
    connection.commit().expect("the LOB fixture commits");
    let count: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), &[])
        .expect("read actual count")
        .get(0)
        .expect("an integer count");
    assert_eq!(count, 1_000_000);
    let kinds = connection.query("SELECT column_name, data_type FROM user_tab_columns WHERE table_name = :1 AND column_name IN ('BODY', 'BODY_N') ORDER BY column_name", &[&name]).expect("read actual native types")
        .map(|row| { let row = row.expect("a dictionary row"); (row.get::<String>(0).expect("a name"), row.get::<String>(1).expect("a type")) }).collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            (String::from("BODY"), String::from("CLOB")),
            (String::from("BODY_N"), String::from("NCLOB"))
        ]
    );
    let banner: String = connection
        .query_row("SELECT banner FROM v$version WHERE banner LIKE 'Oracle%Database%'", &[])
        .expect("the native version is readable")
        .get(0)
        .expect("a version banner");
    println!(
        "raw-prefilter-oracle-setup: rows={count} setup_ms={} native_version={banner} venue=combiner-driver transport={scheme}",
        setup.elapsed().as_millis()
    );
}

#[test]
#[ignore = "requires just oracle-native-acceptance"]
#[expect(
    clippy::disallowed_methods,
    reason = "the native fixture provisions real LOBs and controls only its own test session"
)]
fn raw_prefilter_keeps_exact_lob_answers_for_binary_and_linguistic_sessions() {
    let _: sutura_domain::catalog::TextPolicy =
        serde_json::from_value(policy(true, "[masked]")).expect("the new opt-in must parse before fixture setup");
    let tier = Tier::here();
    let native = tier.channel.starts_with("transport_mode: native-encryption");
    let scheme = if native { "tcp" } else { "tcps" };
    let mut dial = oracledb::Config::default()
        .set_connect_string(&format!("{scheme}://127.0.0.1:{}/{}", tier.port, tier.service))
        .expect("the fixture endpoint parses")
        .set_credentials(&tier.user, &tier.password)
        .set_native_encryption(native);
    if !native {
        let anchors = std::fs::read_to_string(
            std::env::var("SUTURA_ORACLE_ACCEPTANCE_CA").expect("the verified venue names its authority"),
        )
        .expect("the authority reads");
        dial = dial.set_trust_anchors_pem(&anchors).expect("a valid fixture authority");
    }
    let connection = oracledb::connect(dial).expect("the native fixture session opens");
    connection
        .set_call_timeout(Some(Duration::from_secs(120)))
        .expect("the native timeout sets");
    let name = format!("PREFILTER_{}", std::process::id());
    let path = QualifiedTable::parse(format!("{}.{name}", tier.user)).expect("a real schema-qualified fixture name");
    let table = path
        .to_string()
        .split('.')
        .map(|part| format!("\"{part}\""))
        .collect::<Vec<_>>()
        .join(".");
    populate(&connection, &name, &table, scheme);
    for comp in ["BINARY", "ANSI", "LINGUISTIC"] {
        for sort in ["BINARY", "BINARY_CI", "GERMAN"] {
            connection
                .execute(&format!("ALTER SESSION SET NLS_COMP = {comp}"), &[])
                .expect("only the fixture session changes comparison mode");
            connection
                .execute(&format!("ALTER SESSION SET NLS_SORT = {sort}"), &[])
                .expect("only the fixture session changes sort");
            let observed: String = connection
                .query_row("SELECT SYS_CONTEXT('USERENV', 'NLS_SORT') FROM dual", &[])
                .expect("read the active sort")
                .get(0)
                .expect("a declared native sort");
            assert_eq!(observed, sort);
            let observed_comp: String = connection
                .query_row("SELECT value FROM nls_session_parameters WHERE parameter = 'NLS_COMP'", &[])
                .expect("read the active comparison mode")
                .get(0)
                .expect("a native comparison mode");
            assert_eq!(observed_comp, comp);
            for (column, kind) in [("BODY", "clob"), ("BODY_N", "nclob")] {
                for upper in [false, true] {
                    for (replacement, term, pattern, later_delete) in [
                        ("[masked]", "TERM", "secret_[^ ]+", false),
                        ("[TERM]", "TERM", "secret_[^ ]+", false),
                        ("term", "TERM", "secret_[^ ]+", false),
                        ("[masked]", "STRASSE", "secret_[^ ]+", false),
                        ("[masked]", "truncated", "secret_[^ ]+", false),
                        ("", "TERM", "x", false),
                        ("T", "TERM", "x", false),
                        ("M", "TERM", "x", false),
                        ("M[mask]", "TERM", "x", false),
                        ("[mask]T", "TERM", "x", false),
                        ("[masked]", "TERM", "secret_[^ ]+", true),
                    ] {
                        if (pattern == "x" || later_delete) && (comp != "BINARY" || sort != "BINARY") {
                            continue;
                        }
                        let input = if upper {
                            format!("UPPER({table}.\"{column}\")")
                        } else {
                            format!("{table}.\"{column}\"")
                        };
                        let full_table = comp == "BINARY"
                            && sort == "BINARY"
                            && !upper
                            && replacement == "[masked]"
                            && term == "TERM"
                            && pattern == "secret_[^ ]+"
                            && !later_delete;
                        let narrowed = if full_table { "" } else { " AND \"COHORT\" <> 'filler'" };
                        let sql =
                            format!("SELECT \"{column}\" AS \"result\" FROM {table} WHERE {input} LIKE '%{term}%'{narrowed}");
                        let mut answers = Vec::new();
                        for enabled in [false, true] {
                            let start = Instant::now();
                            let mut configured = policy(enabled, replacement);
                            configured["redact"][0]["pattern"] = serde_json::json!(pattern);
                            if later_delete {
                                configured["redact"]
                                    .as_array_mut()
                                    .expect("ordered redactions")
                                    .push(serde_json::json!({"pattern":"x","replace":""}));
                            }
                            let rows = run(&connection, &path, column, kind, &configured, &sql);
                            println!(
                                "raw-prefilter-oracle-query: kind={kind} comp={comp} sort={sort} upper={upper} replacement={replacement} term={term} pattern={pattern} later_delete={later_delete} opted_in={enabled} scope={} elapsed_ms={} returned_rows={}",
                                if full_table { "full-table" } else { "cohort-excludes-filler" },
                                start.elapsed().as_millis(),
                                rows.rows().len()
                            );
                            let (columns, mut values) = rows.into_parts();
                            values.sort_by_cached_key(|row| format!("{row:?}"));
                            answers.push((columns, values));
                        }
                        assert_eq!(
                            answers[0], answers[1],
                            "exact typed native results {kind}/{comp}/{sort}/{upper}/{replacement}/{term}"
                        );
                        if replacement == "[masked]" && term == "TERM" && !upper {
                            assert_eq!(
                                answers[0]
                                    .1
                                    .iter()
                                    .filter(|row| **row == vec![Value::Text(String::from("visible TERM"))])
                                    .count(),
                                2
                            );
                        }
                    }
                }
                for term in [
                    "TERM",
                    "SECRET",
                    "SECRET_",
                    "SECRET_T",
                    "SECRET_TE",
                    "SECRET_TER",
                    "SECRET_TERM",
                ] {
                    let raw: i64 = connection
                        .query_row(
                            &format!(
                                "SELECT COUNT(*) FROM {table} WHERE \"COHORT\" = 'hidden' AND UPPER(\"{column}\") LIKE '%{term}%'"
                            ),
                            &[],
                        )
                        .expect("the hidden values really contain the probe")
                        .get(0)
                        .expect("a count");
                    assert_eq!(raw, if matches!(term, "SECRET" | "SECRET_") { 4 } else { 3 });
                    let sql = format!(
                        "SELECT \"{column}\" FROM {table} WHERE \"COHORT\" = 'hidden' AND UPPER(\"{column}\") LIKE '%{term}%'"
                    );
                    assert!(
                        run(&connection, &path, column, kind, &policy(true, "[masked]"), &sql)
                            .rows()
                            .is_empty(),
                        "the hidden native probe remains empty {kind}/{comp}/{sort}/{term}"
                    );
                }
            }
        }
    }
    connection
        .execute(&format!("DROP TABLE {table} PURGE"), &[])
        .expect("the private LOB fixture drops");
}
