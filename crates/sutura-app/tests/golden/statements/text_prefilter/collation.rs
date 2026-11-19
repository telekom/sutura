//! A case-insensitively collated column must keep the redacted-only statement.
//!
//! Where the engine documents `LIKE` as consulting the column's collation, a raw test over such a
//! column is no longer the binary test the redacted predicate is proven against: `'[masked]'`
//! matches `%MASKED%` there although no binary reading finds it. The cell measures the premise
//! (what is stored, how `LIKE` and the redaction view answer over it) and then holds the refusal.

use sutura_domain::catalog::Collation;

use super::*;

/// What the engine answered, as far as a public log may say: rows, or the class of refusal.
type Reply = Result<OrderedRows, &'static str>;

fn reply(outcome: Result<Answered, FixtureQueryError>, term: &str) -> (Reply, usize) {
    match outcome {
        Ok((rows, planned)) => (Ok(sorted(rows)), searches_in(&planned, term)),
        Err(cause) => {
            let class = if cause.to_string().to_ascii_lowercase().contains("collat") {
                "collation"
            } else {
                "other"
            };
            (Err(class), 0)
        }
    }
}

fn shown(reply: &Reply) -> String {
    match reply {
        Ok((_columns, rows)) => format!("rows={}", rows.len()),
        Err(class) => format!("refused={class}"),
    }
}

/// The collation a column is stored with, as the engine's own catalog states it.
fn stored_collation<W>(warehouse: &W, table: &QualifiedTable) -> Value
where
    W: DataSystemUnderTest,
{
    let text = table.to_string();
    let [project, dataset, name] = text.split('.').collect::<Vec<_>>()[..] else {
        panic!("a provisioned BigQuery table is project.dataset.table");
    };
    let sql = RawStatement::parse(format!(
        "SELECT collation_name FROM `{project}.{dataset}`.INFORMATION_SCHEMA.COLUMNS WHERE table_name = '{name}' AND column_name = 'body'"
    ))
    .expect("a catalog read");
    let rows = warehouse
        .execute_planned_statement(&sql, &BTreeSet::from([table.clone()]), &presented(), deadline())
        .expect("BigQuery executes planned statements")
        .unwrap_or_else(|cause| panic!("the stored collation is readable: {cause:?}"));
    let [row] = rows.rows() else {
        panic!("exactly one stored column answers, saw {}", rows.rows().len());
    };
    row.first().cloned().expect("one collation cell")
}

fn matching<W>(warehouse: &W, table: &QualifiedTable, dialect: TypeDialect) -> i64
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    let sql = format!("SELECT count(*) AS n FROM {} WHERE \"body\" LIKE '%TERM%'", quoted(table));
    let rows = execute(warehouse, admitted(table, dialect, None), &sql).expect("a plain LIKE count");
    let [row] = rows.rows() else {
        panic!("one count row, saw {:?}", rows.rows());
    };
    let [Value::Integer(count)] = row.as_slice() else {
        panic!("one integer count, saw {row:?}");
    };
    *count
}

pub(super) fn trap<W>(warehouse: &W, binary: &QualifiedTable, collated: &QualifiedTable, dialect: TypeDialect)
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    assert_eq!(
        stored_collation(warehouse, binary),
        Value::Null,
        "the binary fixture must be stored with no collation at {}",
        W::NAME
    );
    assert_eq!(
        stored_collation(warehouse, collated),
        Value::Text(String::from("und:ci")),
        "the trap fixture must really be stored with case-insensitive collation at {}",
        W::NAME
    );
    let (exact, folded) = (matching(warehouse, binary, dialect), matching(warehouse, collated, dialect));
    assert_eq!(
        folded,
        exact + 1,
        "LIKE over the collated column must also find the one lower-case row at {}",
        W::NAME
    );
    println!(
        "raw-prefilter-collation: engine={} stored=und:ci like_binary_rows={exact} like_collated_rows={folded}",
        W::NAME
    );
    let redact = serde_json::json!([{"pattern":"secret_[^ ]+","replace":"[masked]"}]);
    let probes = [
        ("redact", "TERM", serde_json::json!({"redact": redact})),
        ("redact", "MASKED", serde_json::json!({"redact": redact})),
        (
            "redact-and-clip",
            "TERM",
            serde_json::json!({"max_text_bytes":128,"redact": redact}),
        ),
    ];
    // The binary table with an unrecorded collation must fall back too: Unknown is never binary.
    let pins = [
        ("collated", collated, Collation::named("und:ci")),
        ("unrecorded", binary, Collation::Unknown),
    ];
    for (pin, table, collation) in &pins {
        for (profile, term, disabled) in &probes {
            let (baseline, base_searches, opted, opted_searches) = both(warehouse, table, dialect, collation, (disabled, term));
            println!(
                "raw-prefilter-collation: engine={} pin={pin} profile={profile} term={term} baseline={} opted-in={}",
                W::NAME,
                shown(&baseline),
                shown(&opted)
            );
            assert_eq!(
                baseline,
                opted,
                "a column not pinned binary answers exactly as it does without the opt-in: {} {pin} {profile} {term}",
                W::NAME
            );
            assert_eq!(
                base_searches,
                opted_searches,
                "no raw condition may be added over a column not pinned binary: {} {pin} {profile} {term}",
                W::NAME
            );
        }
    }
    // What the guard stands between: the same collated column wrongly recorded as binary. Measured, not
    // asserted - the catalog's word is trusted as far as its author is.
    for (profile, term, disabled) in &probes {
        let (baseline, base_searches, opted, searches) = both(warehouse, collated, dialect, &Collation::Binary, (disabled, term));
        println!(
            "raw-prefilter-collation: engine={} pin=mislabelled-binary profile={profile} term={term} baseline={} opted-in={} raw_searches_added={}",
            W::NAME,
            shown(&baseline),
            shown(&opted),
            searches - base_searches
        );
    }
}

/// The baseline and the opted-in reply of one search over `table` pinned with `collation`.
fn both<W>(
    warehouse: &W,
    table: &QualifiedTable,
    dialect: TypeDialect,
    collation: &Collation,
    (disabled, term): (&serde_json::Value, &str),
) -> (Reply, usize, Reply, usize)
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    let mut enabled = disabled.clone();
    enabled["raw_prefilter"] = serde_json::json!({"acknowledged_because":"this fixture accepts secret-dependent timing"});
    let sql = format!("SELECT \"body\" FROM {} WHERE \"body\" LIKE '%{term}%'", quoted(table));
    let [(baseline, base_searches), (opted, opted_searches)] = [disabled, &enabled].map(|policy| {
        reply(
            execute_recorded(warehouse, admitted_as(table, dialect, Some(policy), collation), &sql),
            term,
        )
    });
    (baseline, base_searches, opted, opted_searches)
}
