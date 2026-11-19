//! A column's collation decides whether a raw `LIKE` may stand in for the redacted one.
//!
//! `PostgreSQL` binds a collation to each column. A deterministic collation makes `LIKE` compare
//! character for character, so the raw and the redacted test agree; a nondeterministic one
//! (case- or accent-insensitive) does not, and `regexp_replace` refuses it, so a raw test that
//! emptied the scan first would turn that refusal into an empty answer about unredacted text.

use super::*;

pub(super) const TABLE: &str = "text_prefilter_collated";

/// One row per way a term can hide: visible, redacted, differently cased, and padded by `char(16)`.
pub(super) const FIXTURE: &str = "\
CREATE COLLATION \"folded_trap\" (provider = icu, locale = 'und-u-ks-level2', deterministic = false);
DROP TABLE IF EXISTS \"text_prefilter_collated\";
CREATE TABLE \"text_prefilter_collated\" (
  \"plain\" text,
  \"byte_order\" text COLLATE \"C\",
  \"fixed\" char(16),
  \"folded\" text COLLATE \"folded_trap\"
);
INSERT INTO \"text_prefilter_collated\" VALUES
  ('visible TERM', 'visible TERM', 'TERM', 'visible TERM'),
  ('secret_other', 'secret_other', 'xTERM y', 'secret_other'),
  ('term', 'term', 'term', 'term'),
  ('secret_TERM', 'secret_TERM', 'secret_TERM', 'secret_TERM'),
  ('other', 'other', 'other', 'other')";

fn table(
    path: &QualifiedTable,
    dialect: TypeDialect,
    column: &str,
    collation: Option<&str>,
    enabled: bool,
    replacement: &str,
) -> StatementTable {
    let mut policy = serde_json::json!({"max_text_bytes":128,"redact":[{"pattern":"secret_[^ ]+","replace":replacement}]});
    if enabled {
        policy["raw_prefilter"] = serde_json::json!({"acknowledged_because":"this fixture accepts secret-dependent timing"});
    }
    let field = pinned(
        Field::new(column, DataType::Utf8, true)
            .with_metadata(HashMap::from([(String::from("sutura:text_policy"), policy.to_string())])),
        collation,
    );
    StatementTable::new(source(), path.clone(), dialect, Arc::new(Schema::new(vec![field])))
}

fn texts(rows: &RawRows) -> Vec<String> {
    let mut found: Vec<String> = rows
        .rows()
        .iter()
        .map(|row| match row.as_slice() {
            [Value::Text(text)] => text.clone(),
            other => panic!("one text cell per row: {other:?}"),
        })
        .collect();
    found.sort();
    found
}

/// What the source received for one query, with the policy off and then on.
fn both<W>(
    warehouse: &W,
    (path, dialect): (&QualifiedTable, TypeDialect),
    column: &str,
    collation: Option<&str>,
    (replacement, term): (&str, &str),
) -> [Outcome; 2]
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    let sql = format!("SELECT \"{column}\" FROM {} WHERE \"{column}\" LIKE '%{term}%'", quoted(path));
    [false, true].map(|enabled| execute_outcome(warehouse, table(path, dialect, column, collation, enabled, replacement), &sql))
}

pub(super) fn cells<W>(warehouse: &W, path: &QualifiedTable, dialect: TypeDialect)
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    // A binary column takes the raw test, and the answer is the one the redacted test alone gives:
    // `comparison-creates` writes `term`, which a case-folding comparison would match and a binary one does not.
    for (column, expected) in [
        ("plain", &["visible TERM"][..]),
        ("byte_order", &["visible TERM"]),
        ("fixed", &["TERM", "xTERM y"]),
    ] {
        for replacement in ["[masked]", "term"] {
            let [baseline, opted] = both(warehouse, (path, dialect), column, Some("binary"), (replacement, "TERM"));
            let [(baseline, before), (opted, after)] = [baseline, opted].map(|(answer, planned)| {
                (
                    answer.unwrap_or_else(|cause| panic!("{column} {replacement}: {cause}")),
                    searches_in(&planned, "TERM"),
                )
            });
            assert_eq!(texts(&baseline), expected, "{column} {replacement}: the redacted answer");
            assert_eq!(texts(&opted), expected, "{column} {replacement}: the opted-in answer");
            assert_eq!(
                after - before,
                1,
                "{column} {replacement}: a binary column takes exactly one raw search"
            );
            println!("raw-prefilter-collation: column={column} replacement={replacement:?} collation=binary applied=true");
        }
    }
    // A column the dictionary did not record is not binary, whatever it really is.
    let [(baseline, before), (opted, after)] =
        both(warehouse, (path, dialect), "plain", None, ("[masked]", "TERM")).map(|(answer, planned)| {
            (
                answer.expect("an unknown collation still answers"),
                searches_in(&planned, "TERM"),
            )
        });
    assert_eq!(texts(&baseline), texts(&opted));
    assert_eq!(before, after, "an unrecorded collation keeps the redacted-only statement");
    // A nondeterministic collation is looser than the proof, and `regexp_replace` refuses it: the
    // refusal must stand, so no raw search may empty the scan before it.
    let [(baseline, before), (opted, after)] = both(
        warehouse,
        (path, dialect),
        "folded",
        Some("named:folded_trap"),
        ("[masked]", "QQQQ"),
    )
    .map(|(answer, planned)| (answer, searches_in(&planned, "QQQQ")));
    let baseline = baseline.expect_err("regexp_replace refuses a nondeterministic collation");
    assert_eq!(before, after, "a named collation keeps the redacted-only statement");
    assert_eq!(
        baseline.to_string(),
        opted.expect_err("the opted-in query is refused the same way").to_string(),
        "an error and an empty answer must not tell the unredacted rows apart"
    );
    println!("raw-prefilter-collation: column=folded collation=nondeterministic applied=false outcome=refused");
}
