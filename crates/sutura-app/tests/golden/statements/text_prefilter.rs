//! Native equality and timings over actual rows; a skipped cell provides no native evidence.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use sutura_domain::catalog::{COLLATION_METADATA_KEY, PrefilterCapability};
use sutura_domain::model::TableName;
use sutura_domain::plan::StatementStopped;
use sutura_domain::raw::InvalidRawStatement;
use sutura_exec_datafusion::CombineError;

use super::*;

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

    /// The same table with `body` stored under a case-insensitive collation, for an engine that has one.
    fn open_collated_text_fixture(_csv: PathBuf) -> Option<(Self, QualifiedTable)> {
        None
    }
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
    (duckdb, $adapter:ty) => {
        impl TextFixture for $adapter {
            fn open_collated_text_fixture(csv: PathBuf) -> Option<(Self, QualifiedTable)> {
                static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                let table = TableName::parse("text_prefilter").expect("a fixture name");
                let file = std::env::temp_dir().join(format!(
                    "sutura-golden-duckdb-collated-{}-{}.duckdb",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                drop(std::fs::remove_file(&file));
                sutura_exec_duckdb::write_nocase_database(&file, &[(table.clone(), csv)], "body")
                    .unwrap_or_else(|cause| panic!("duckdb could not write {}: {cause}", file.display()));
                let warehouse = <$adapter>::open(
                    source(),
                    crate::adapters::posture(),
                    &file,
                    crate::adapters::result_budget(),
                )
                .expect("the written database opens read-only");
                drop(std::fs::remove_file(&file));
                Some((warehouse, QualifiedTable::new(None, table)))
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

/// A deterministic 32-bit scramble: its hex digits cannot spell, or be rewritten into,
/// any probe, and it yields about as many distinct values as rows, which dictionary encoding cannot collapse.
const fn scrambled(index: u64) -> u64 {
    let mixed = index.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (mixed ^ (mixed >> 31)) >> 32
}

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
    for index in (0_u64..).take(ROWS - RARE.len() - 2) {
        writeln!(file, "unrelated-{:08x},filler", scrambled(index)).expect("a filler row");
    }
    for (body, cohort) in RARE {
        writeln!(file, "{body},{cohort}").expect("a rare row");
    }
    writeln!(file, "{} TERM,rare", "a".repeat(4100)).expect("a real wide text cell");
    writeln!(file, "{} secret_TERM,hidden", "a".repeat(4100)).expect("a late hidden term");
    file.flush().expect("all actual fixture rows reach disk");
    path
}

/// A few rows, one of which only the redacted value matches: `secret_other` becomes `term`.
fn collated_csv() -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "text-prefilter-collated-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&root).expect("a private fixture directory");
    let path = root.join("rows.csv");
    std::fs::write(
        &path,
        "body,cohort\nsecret_other,hidden\nsecret_TERM,hidden\nterm,rare\nvisible TERM,rare\n",
    )
    .expect("a collated fixture file");
    path
}

fn admitted(path: &QualifiedTable, dialect: TypeDialect, policy: Option<&serde_json::Value>) -> StatementTable {
    admitted_as(path, dialect, policy, Some("binary"))
}

/// `collation` is the pinned fact, as the pin writes it; `None` is a column nothing was declared for.
fn admitted_as(
    path: &QualifiedTable,
    dialect: TypeDialect,
    policy: Option<&serde_json::Value>,
    collation: Option<&str>,
) -> StatementTable {
    let mut metadata = HashMap::new();
    if let Some(collation) = collation {
        metadata.insert(String::from(COLLATION_METADATA_KEY), String::from(collation));
    }
    if let Some(policy) = policy {
        metadata.insert(String::from("sutura:text_policy"), policy.to_string());
    }
    if dialect == TypeDialect::Oracle {
        metadata.insert(String::from("sutura:oracle_lob"), String::from("clob"));
    }
    StatementTable::new(
        source(),
        path.clone(),
        dialect,
        Arc::new(Schema::new(vec![
            Field::new("body", DataType::Utf8, true).with_metadata(metadata),
            Field::new("cohort", DataType::Utf8, true),
        ])),
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
    ) -> Result<RawRows, Self::Stop> {
        self.statements.push(String::from(statement.as_str()));
        self.inner.run_planned(source, statement, tables)
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

/// The combiner's own capability, so a dialect it enables is expected here without a second list.
const fn prefilter_capable(dialect: TypeDialect) -> bool {
    PrefilterCapability::of(dialect).is_some()
}

/// How many times the statements the source received search for `term`.
fn searches_in(planned: &[String], term: &str) -> usize {
    let needle = format!("LIKE '%{term}%'").to_ascii_uppercase();
    planned
        .iter()
        .map(|statement| statement.to_ascii_uppercase().matches(&needle).count())
        .sum()
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
            for (mode, policy) in [("baseline", &disabled), ("opted-in", &enabled)] {
                let start = Instant::now();
                let (rows, planned) = execute_recorded(&warehouse, admitted(&path, dialect, Some(policy)), &sql)
                    .unwrap_or_else(|cause| panic!("{} {profile}/{mode} upper={upper}: {cause}", W::NAME));
                searches.push(searches_in(&planned, term));
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

/// A column the engine compares case-insensitively, whatever the pinned fact says it is. The raw
/// condition is a necessary one only under a binary comparison, so no declared-or-missing collation
/// may let it into a statement: the redacted `term` below matches `TERM` here, and a raw condition
/// would drop the row whose stored text lacks it.
pub(super) fn collated<W>()
where
    W: TextFixture + Sync,
    W::Error: Send,
{
    if !runs_here::<W>() {
        return;
    }
    let csv = collated_csv();
    let (warehouse, path) = W::open_collated_text_fixture(csv.clone()).expect("this engine stores a collated column");
    let dialect = warehouse.type_dialect().expect("a registered native dialect");
    let mut policy = serde_json::json!({"max_text_bytes": 128, "redact": [{"pattern": "secret_[^ ]+", "replace": "term"}]});
    let sql = format!("SELECT \"body\" FROM {} WHERE \"body\" LIKE '%TERM%'", quoted(&path));
    let answer = |policy: &serde_json::Value, collation: Option<&str>| {
        let (rows, planned) = execute_recorded(&warehouse, admitted_as(&path, dialect, Some(policy), collation), &sql)
            .unwrap_or_else(|cause| panic!("{} collated {collation:?}: {cause}", W::NAME));
        (sorted(rows), searches_in(&planned, "TERM"))
    };
    let (baseline, searches) = answer(&policy, None);
    assert_eq!(baseline.1.len(), 4, "the redacted value decides at {}", W::NAME);
    assert_eq!(
        baseline
            .1
            .iter()
            .filter(|row| **row == vec![Value::Text(String::from("term"))])
            .count(),
        3,
        "a row only the redacted value matches survives at {}",
        W::NAME
    );
    policy["raw_prefilter"] = serde_json::json!({"acknowledged_because": "this fixture accepts secret-dependent timing"});
    for collation in [Some("named:NOCASE"), Some("named:NOACCENT"), None] {
        let (opted, opted_searches) = answer(&policy, collation);
        assert_eq!(
            opted_searches,
            searches,
            "the redacted-only statement for {collation:?} at {}",
            W::NAME
        );
        assert_eq!(opted, baseline, "the same answer for {collation:?} at {}", W::NAME);
    }
    // A binary pin on this column is the author's error and the one case that proves the trap: the raw
    // condition is added and the row only the redacted value matches is lost.
    let (wrong, wrong_searches) = answer(&policy, Some("binary"));
    assert_eq!(
        wrong_searches,
        searches + 1,
        "a binary pin is what carries the raw condition at {}",
        W::NAME
    );
    assert_eq!(wrong.1.len(), 3, "the raw condition would drop `secret_other` at {}", W::NAME);
    warehouse.close_text_fixture(&path);
    std::fs::remove_dir_all(csv.parent().expect("a private directory")).expect("remove the generated CSV");
}

macro_rules! system {
    // Cube reads every raw row before any operator and never takes the prefilter: nothing to compare.
    (cube, $warehouse:ty) => {};
    (duckdb, $warehouse:ty) => {
        mod duckdb {
            #[test]
            fn raw_prefilter_matches_the_baseline_over_one_million_actual_native_rows() {
                super::native::<$warehouse>();
            }

            #[test]
            fn a_case_insensitive_column_never_takes_the_raw_condition() {
                super::collated::<$warehouse>();
            }
        }
    };
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
