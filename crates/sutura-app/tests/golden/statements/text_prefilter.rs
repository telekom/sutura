//! Native equality and timings over actual rows; a skipped cell provides no native evidence.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use sutura_domain::model::TableName;
use sutura_domain::plan::{PolicyRead, StatementStopped};
use sutura_domain::raw::InvalidRawStatement;
use sutura_exec_datafusion::CombineError;

use super::*;

// The causality base run restores the domain before this test file is compiled. Keep the fixture
// helper buildable there; branch-provided inherent methods take precedence on the real run.
trait EngineCollationFixture {
    fn engine_collation(self) -> Option<&'static sutura_domain::catalog::Collation>;
}

impl EngineCollationFixture for TypeDialect {
    fn engine_collation(self) -> Option<&'static sutura_domain::catalog::Collation> {
        None
    }
}

trait PinCollationFixture {
    fn pin_onto(&self, field: &mut Field);
}

impl PinCollationFixture for sutura_domain::catalog::Collation {
    fn pin_onto(&self, _field: &mut Field) {}
}

/// Fixture setup belongs only to the golden target that exercises it.
pub(super) trait TextFixture: DataSystemUnderTest {
    /// A private large-text fixture; cloud and fixed-model venues override the CSV loader.
    fn open_text_fixture(csv: PathBuf) -> (Self, QualifiedTable) {
        let table = TableName::parse("text_prefilter").expect("a fixture name");
        (
            Self::open_on(source(), vec![(table.clone(), csv)]),
            QualifiedTable::new(None, table),
        )
    }

    /// Cloud fixtures expire independently, and are dropped here on a completed cell.
    fn close_text_fixture(&self, _table: &QualifiedTable) {}
}

macro_rules! text_fixture {
    (bigquery, $adapter:ty) => {
        impl TextFixture for $adapter {
            fn open_text_fixture(csv: PathBuf) -> (Self, QualifiedTable) {
                static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                let (warehouse, qualifier) = crate::adapters::bigquery::bigquery_provisioned(source());
                let name = TableName::parse(format!(
                    "text_prefilter_{}_{}",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ))
                .expect("a unique fixture table name");
                let table = QualifiedTable::new(Some(qualifier), name);
                assert_eq!(
                    warehouse
                        .load_fixture(&table, &csv)
                        .expect("the atomic fixture job succeeds"),
                    1_000_000
                );
                (warehouse, table)
            }

            fn close_text_fixture(&self, table: &QualifiedTable) {
                self.drop_table(table).expect("the private fixture is removed");
            }
        }
    };
    (clickhouse, $adapter:ty) => {
        impl TextFixture for $adapter {
            fn open_text_fixture(csv: PathBuf) -> (Self, QualifiedTable) {
                let table = TableName::parse("text_prefilter").expect("a fixture name");
                (
                    crate::adapters::clickhouse_on(source(), vec![(table.clone(), csv)], <$adapter>::load_federated_csv),
                    QualifiedTable::new(None, table),
                )
            }
        }
    };
    ($_name:ident, $adapter:ty) => {
        impl TextFixture for $adapter {}
    };
}
crate::adapters::registered!(data_systems: text_fixture);

const ROWS: usize = 1_000_000;
const RARE: &[(&str, &str)] = &[
    ("secret_other", "hidden"),
    ("visible TERM", "rare"),
    ("visible TERM", "rare"),
    ("", "rare"),
    ("secret_TERM", "hidden"),
    ("prefix secret_TERM suffix", "hidden"),
    ("TERxM", "rare"),
    ("xERM", "rare"),
    ("TERx", "rare"),
    ("term", "rare"),
    ("caf\u{e9} TERM", "rare"),
    ("stra\u{df}e", "rare"),
    ("\u{130}STANBUL", "rare"),
    ("caf\u{e9}", "rare"),
    ("masked", "rare"),
];

fn fixture_csv() -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "text-prefilter-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&root).expect("a private fixture directory");
    let path = root.join("rows.csv");
    let mut file = std::io::BufWriter::new(std::fs::File::create(&path).expect("a fixture file"));
    writeln!(file, "body,cohort").expect("a header");
    for _ in 0..ROWS - RARE.len() - 2 {
        writeln!(file, "unrelated,filler").expect("a filler row");
    }
    for (body, cohort) in RARE {
        writeln!(file, "{body},{cohort}").expect("a rare row");
    }
    writeln!(file, "{} TERM,rare", "a".repeat(4100)).expect("a real wide text cell");
    writeln!(file, "{} secret_TERM,hidden", "a".repeat(4100)).expect("a late hidden term");
    file.flush().expect("all actual fixture rows reach disk");
    path
}

fn admitted(path: &QualifiedTable, dialect: TypeDialect, policy: Option<&serde_json::Value>) -> StatementTable {
    let mut metadata = HashMap::new();
    if let Some(policy) = policy {
        metadata.insert(String::from("sutura:text_policy"), policy.to_string());
    }
    if dialect == TypeDialect::Oracle {
        metadata.insert(String::from("sutura:oracle_lob"), String::from("clob"));
    }
    // What pinning records for an engine that fixes the collation itself; ClickHouse is admitted by it alone.
    let mut body = Field::new("body", DataType::Utf8, true).with_metadata(metadata);
    if let Some(collation) = dialect.engine_collation() {
        collation.pin_onto(&mut body);
    }
    StatementTable::new(
        source(),
        path.clone(),
        dialect,
        Arc::new(Schema::new(vec![body, Field::new("cohort", DataType::Utf8, true)])),
    )
}

#[derive(Debug, thiserror::Error)]
enum FixtureQueryError {
    #[error("the native combiner could not start")]
    Combiner {
        #[source]
        cause: CombineError,
    },
    #[error("the generated fixture query is invalid")]
    Query {
        #[source]
        cause: InvalidRawStatement,
    },
    #[error("the registered fixture query is unsupported")]
    Unsupported,
    #[error("the actual native scan set differs: observed {observed:?}, expected {expected:?}")]
    Scans {
        observed: Vec<BTreeSet<QualifiedTable>>,
        expected: Vec<BTreeSet<QualifiedTable>>,
    },
    #[error("the native statement stopped: {cause:?}")]
    Stopped { cause: StatementStopped<String> },
}

/// The statements the source engine really received, beside the rows it answered.
struct Recorded<'a, W> {
    inner: Executing<'a, W>,
    statements: Vec<String>,
}

impl<W: DataSystemUnderTest> StatementLegs for Recorded<'_, W> {
    type Stop = String;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.inner.open(sources)
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.inner.run(source, statement)
    }

    fn run_planned(
        &mut self,
        source: &SourceName,
        statement: &RawStatement,
        tables: &BTreeSet<QualifiedTable>,
        policy: PolicyRead,
    ) -> Result<RawRows, Self::Stop> {
        self.statements.push(String::from(statement.as_str()));
        self.inner.run_planned(source, statement, tables, policy)
    }
}

fn execute<W>(warehouse: &W, table: StatementTable, sql: &str) -> Result<RawRows, FixtureQueryError>
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    execute_recorded(warehouse, table, sql).map(|(rows, _planned)| rows)
}

type Answered = (RawRows, Vec<String>);

fn execute_recorded<W>(warehouse: &W, table: StatementTable, sql: &str) -> Result<Answered, FixtureQueryError>
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    let combiner = DataFusionCombiner::new().map_err(|cause| FixtureQueryError::Combiner { cause })?;
    let question = RawStatement::parse(sql).map_err(|cause| FixtureQueryError::Query { cause })?;
    let mut executing = Recorded {
        inner: Executing {
            warehouse,
            expected_scans: vec![BTreeSet::from([table.table().clone()])],
            observed_scans: Vec::new(),
        },
        statements: Vec::new(),
    };
    let answer = combiner
        .answer_statement(
            StatementQuestion::new(&question, &[table], false, WORKING_SET, deadline()),
            &mut executing,
        )
        .ok_or(FixtureQueryError::Unsupported)?;
    if executing.inner.observed_scans != executing.inner.expected_scans {
        return Err(FixtureQueryError::Scans {
            observed: executing.inner.observed_scans,
            expected: executing.inner.expected_scans,
        });
    }
    answer
        .map(|rows| (rows, executing.statements))
        .map_err(|cause| FixtureQueryError::Stopped { cause })
}

type OrderedRows = (Vec<String>, Vec<Vec<Value>>);

fn sorted(rows: RawRows) -> OrderedRows {
    let (columns, mut values) = rows.into_parts();
    values.sort_by_cached_key(|row| format!("{row:?}"));
    (columns, values)
}

fn observe_population<W>(warehouse: &W, path: &QualifiedTable, dialect: TypeDialect, setup: Instant)
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    match dialect {
        TypeDialect::BigQuery => println!("raw-prefilter-version: engine=bigquery managed-service-no-exposed-server-version"),
        TypeDialect::Oracle => {
            let sql = RawStatement::parse("SELECT banner FROM v$version WHERE banner LIKE 'Oracle%Database%'")
                .expect("a native version read");
            let version = warehouse
                .execute_raw(&sql, &presented(), deadline())
                .expect("the Oracle raw port")
                .unwrap_or_else(|cause| panic!("the native version reads: {cause:?}"));
            assert!(
                !version.rows().is_empty(),
                "the Oracle banner must be read, not filtered away"
            );
            println!("raw-prefilter-version: engine=oracle version={:?}", version.rows());
        }
        _ => {
            let version = execute(
                warehouse,
                admitted(path, dialect, None),
                &format!("SELECT version() AS native_version FROM {} LIMIT 1", quoted(path)),
            )
            .expect("the native engine reports its version");
            println!("raw-prefilter-version: engine={} version={:?}", W::NAME, version.rows());
        }
    }
    let count = execute(
        warehouse,
        admitted(path, dialect, None),
        &format!("SELECT count(*) AS actual_rows FROM {}", quoted(path)),
    )
    .expect("read the actual table count");
    assert_eq!(
        count.rows(),
        &[vec![Value::Integer(1_000_000)]],
        "actual native population at {}",
        W::NAME
    );
    if dialect == TypeDialect::Oracle {
        let sql = RawStatement::parse(
            "SELECT data_type FROM user_tab_columns WHERE table_name = 'text_prefilter' AND column_name = 'body'",
        )
        .expect("a dictionary read");
        let declared = warehouse
            .execute_raw(&sql, &presented(), deadline())
            .expect("Oracle exposes its raw query port")
            .unwrap_or_else(|cause| panic!("the actual native type is readable: {cause:?}"));
        assert_eq!(
            declared.rows(),
            &[vec![Value::Text(String::from("CLOB"))]],
            "the fixture must be a real native CLOB"
        );
    }
    println!(
        "raw-prefilter-native: engine={} actual_rows={} setup_ms={} scope=full-table-search prefilter={}",
        W::NAME,
        ROWS,
        setup.elapsed().as_millis(),
        if prefilter_capable(dialect) {
            "eligible-where-the-query-qualifies"
        } else {
            "never-applies-redacted-only"
        }
    );
}

/// Only these arms have comparison evidence; every other dialect keeps the redacted-only path.
const fn prefilter_capable(dialect: TypeDialect) -> bool {
    matches!(
        dialect,
        TypeDialect::DataFusion | TypeDialect::Oracle | TypeDialect::ClickHouse
    )
}

/// How many times the statements the source received search for `term`.
fn searches_in(planned: &[String], term: &str) -> usize {
    let needle = format!("LIKE '%{term}%'").to_ascii_uppercase();
    planned
        .iter()
        .map(|statement| statement.to_ascii_uppercase().matches(&needle).count())
        .sum()
}

/// How many times the statements the source received search the stored column, named through its table.
///
/// `ClickHouse` resolves a bare name to the projection alias beside it unless `prefer_column_name_to_alias`
/// is set, so an unqualified raw test reads the redacted value: every answer stays equal and no row is
/// ever skipped. Only the text of the statement tells the two apart here, and no other engine is read.
/// The view is aliased with the table's own name, so the caller's predicate in the outer `WHERE`
/// reads the same qualified column; only the view's own condition, which the view's closing alias
/// follows, counts.
fn stored_searches_in(planned: &[String], dialect: TypeDialect, path: &QualifiedTable, term: &str) -> usize {
    if dialect != TypeDialect::ClickHouse {
        return 0;
    }
    let table = path.name().as_str();
    let needle = format!("WHERE (`{table}`.`body` LIKE '%{term}%')) AS `{table}`");
    planned.iter().map(|statement| statement.matches(&needle).count()).sum()
}

/// Only an eligible arm may add one raw search, and exactly where the shape qualifies. The `UPPER`
/// barrier query narrows with a string literal on `cohort`, a column no text policy proves is text,
/// so it keeps the redacted-only statement on every dialect.
fn assert_raw_condition(dialect: TypeDialect, profile: &str, upper: bool, (baseline, opted): (usize, usize)) {
    assert!(
        opted == baseline || (prefilter_capable(dialect) && opted == baseline + 1),
        "only an eligible arm may add one raw search to the statement the source received: {dialect:?} {profile} upper={upper} baseline={baseline} opted={opted}",
    );
    match profile {
        "barrier" => assert_eq!(
            opted - baseline,
            usize::from(prefilter_capable(dialect) && !upper),
            "the raw condition is present exactly where the arm and shape qualify: {dialect:?} upper={upper}",
        ),
        "replacement-creates" | "deletion-bridge" => assert_eq!(
            opted, baseline,
            "a replacement that can create or bridge a match keeps the redacted-only statement: {dialect:?} {profile}",
        ),
        _ => {}
    }
}

pub(super) fn native<W>()
where
    W: TextFixture + Sync,
    W::Error: Send,
{
    let base = serde_json::json!({"max_text_bytes":128,"redact":[{"pattern":"secret_[^ ]+","replace":"[masked]"}]});
    let mut enabled = base.clone();
    enabled["raw_prefilter"] = serde_json::json!({"acknowledged_because":"this fixture accepts secret-dependent timing"});
    // This runs before availability: base must refuse the new opt-in rather than silently skip it.
    let _: sutura_domain::catalog::TextPolicy = serde_json::from_value(enabled).expect("the acknowledged opt-in parses");
    if !runs_here::<W>() {
        return;
    }
    let setup = Instant::now();
    let csv = fixture_csv();
    let (warehouse, path) = W::open_text_fixture(csv.clone());
    let dialect = warehouse.type_dialect().expect("a registered native dialect");
    observe_population(&warehouse, &path, dialect, setup);
    let profiles = [
        ("barrier", "TERM", vec![("secret_[^ ]+", "[masked]")]),
        ("replacement-creates", "TERM", vec![("secret_[^ ]+", "[TERM]")]),
        ("comparison-creates", "TERM", vec![("secret_[^ ]+", "term")]),
        ("deletion-bridge", "TERM", vec![("x", "")]),
        ("left-bridge", "TERM", vec![("x", "T")]),
        ("right-bridge", "TERM", vec![("x", "M")]),
        ("left-boundary", "TERM", vec![("x", "M[mask]")]),
        ("right-boundary", "TERM", vec![("x", "[mask]T")]),
        ("ordered-bridge", "TERM", vec![("secret_[^ ]+", "[masked]"), ("x", "")]),
        ("marker", "truncated", vec![("secret_[^ ]+", "[masked]")]),
        ("unicode-case", "STRASSE", vec![("secret_[^ ]+", "[masked]")]),
    ];
    for (profile, term, rules) in profiles {
        let mut disabled = base.clone();
        disabled["redact"] = serde_json::json!(
            rules
                .into_iter()
                .map(|(pattern, replace)| serde_json::json!({"pattern":pattern,"replace":replace}))
                .collect::<Vec<_>>()
        );
        let mut enabled = disabled.clone();
        enabled["raw_prefilter"] = serde_json::json!({"acknowledged_because":"this fixture accepts secret-dependent timing"});
        for upper in [false, true] {
            let expression = if upper { "UPPER(\"body\")" } else { "\"body\"" };
            let full_table = profile == "barrier" && !upper;
            let narrowed = if full_table { "" } else { " AND \"cohort\" <> 'filler'" };
            let sql = format!(
                "SELECT \"body\" FROM {} WHERE {expression} LIKE '%{term}%'{narrowed}",
                quoted(&path)
            );
            let mut answers = Vec::new();
            let mut searches = Vec::new();
            let mut stored = Vec::new();
            for (mode, policy) in [("baseline", &disabled), ("opted-in", &enabled)] {
                let start = Instant::now();
                let (rows, planned) = execute_recorded(&warehouse, admitted(&path, dialect, Some(policy)), &sql)
                    .unwrap_or_else(|cause| panic!("{} {profile}/{mode} upper={upper}: {cause}", W::NAME));
                searches.push(searches_in(&planned, term));
                stored.push(stored_searches_in(&planned, dialect, &path, term));
                println!(
                    "raw-prefilter-query: engine={} profile={profile} upper={upper} mode={mode} scope={} elapsed_ms={} returned_rows={}",
                    W::NAME,
                    if full_table { "full-table" } else { "cohort-excludes-filler" },
                    start.elapsed().as_millis(),
                    rows.rows().len()
                );
                answers.push(sorted(rows));
            }
            assert_eq!(
                answers[0],
                answers[1],
                "exact typed rows and duplicate multiplicity: {} {profile} upper={upper}",
                W::NAME
            );
            assert_raw_condition(dialect, profile, upper, (searches[0], searches[1]));

            if dialect == TypeDialect::ClickHouse {
                assert_eq!(
                    stored,
                    [0, usize::from(full_table)],
                    "the view's own raw search names the stored column, never the alias: {profile} upper={upper}"
                );
            }

            if profile == "barrier" && !upper {
                assert_eq!(
                    answers[0]
                        .1
                        .iter()
                        .filter(|row| **row == vec![Value::Text(String::from("visible TERM"))])
                        .count(),
                    2,
                    "the duplicate fixture rows must both survive at {}",
                    W::NAME
                );
            }
            if profile == "replacement-creates" && !upper {
                assert!(
                    answers[0]
                        .1
                        .iter()
                        .any(|row| *row == vec![Value::Text(String::from("[TERM]"))]),
                    "replacement-created matches must remain observable at {}",
                    W::NAME
                );
            }
        }
    }
    for enabled in [false, true] {
        let mut policy = base.clone();
        if enabled {
            policy["raw_prefilter"] = serde_json::json!({"acknowledged_because":"this fixture accepts secret-dependent timing"});
        }
        let sql = format!(
            "SELECT \"body\" FROM {} WHERE \"body\" IS NULL AND \"cohort\" = 'rare'",
            quoted(&path)
        );
        assert_eq!(
            execute(&warehouse, admitted(&path, dialect, Some(&policy)), &sql)
                .expect("a native NULL query executes")
                .rows(),
            &[vec![Value::Null]],
            "the typed NULL survives at {}",
            W::NAME
        );
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
        let mut policy = base.clone();
        policy["raw_prefilter"] = serde_json::json!({"acknowledged_because":"this fixture accepts secret-dependent timing"});
        let raw = format!(
            "SELECT count(*) FROM {} WHERE \"cohort\" = 'hidden' AND UPPER(\"body\") LIKE '%{term}%'",
            quoted(&path)
        );
        assert_eq!(
            execute(&warehouse, admitted(&path, dialect, None), &raw)
                .expect("the raw hidden rows really contain the probe")
                .rows(),
            &[vec![Value::Integer(if matches!(term, "SECRET" | "SECRET_") { 4 } else { 3 })]],
            "a probe must search actual hidden values at {}",
            W::NAME
        );
        let sql = format!(
            "SELECT \"body\" FROM {} WHERE \"cohort\" = 'hidden' AND UPPER(\"body\") LIKE '%{term}%'",
            quoted(&path)
        );
        assert!(
            execute(&warehouse, admitted(&path, dialect, Some(&policy)), &sql)
                .expect("a hidden-secret probe executes")
                .rows()
                .is_empty(),
            "{} hidden probe {term}",
            W::NAME
        );
    }
    warehouse.close_text_fixture(&path);
    std::fs::remove_dir_all(csv.parent().expect("a private directory")).expect("remove the generated CSV");
}

macro_rules! system {
    // Cube reads every raw row before any operator and never takes the prefilter: nothing to compare.
    (cube, $warehouse:ty) => {};
    ($name:ident, $warehouse:ty) => {
        mod $name {
            #[test]
            fn raw_prefilter_matches_the_baseline_over_one_million_actual_native_rows() {
                super::native::<$warehouse>();
            }
        }
    };
}
crate::adapters::registered!(data_systems: system);
