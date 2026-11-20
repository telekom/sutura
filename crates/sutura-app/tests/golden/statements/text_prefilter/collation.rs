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

/// A table column and the collation text its pin carries, absent for unknown.
type Pinned<'a> = (&'a str, Option<&'a str>);

fn table(
    path: &QualifiedTable,
    dialect: TypeDialect,
    columns: &[Pinned<'_>],
    (enabled, replacement): (bool, &str),
) -> StatementTable {
    let mut policy = serde_json::json!({"max_text_bytes":128,"redact":[{"pattern":"secret_[^ ]+","replace":replacement}]});
    if enabled {
        policy["raw_prefilter"] = serde_json::json!({"acknowledged_because":"this fixture accepts secret-dependent timing"});
    }
    let fields = columns
        .iter()
        .map(|(column, collation)| {
            pinned(
                Field::new(*column, DataType::Utf8, true)
                    .with_metadata(HashMap::from([(String::from("sutura:text_policy"), policy.to_string())])),
                *collation,
            )
        })
        .collect::<Vec<_>>();
    StatementTable::new(source(), path.clone(), dialect, Arc::new(Schema::new(fields)))
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

/// What the source received for one statement, with the policy off and then on.
fn both<W>(
    warehouse: &W,
    (path, dialect): (&QualifiedTable, TypeDialect),
    columns: &[Pinned<'_>],
    (replacement, sql): (&str, &str),
) -> [Outcome; 2]
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    [false, true].map(|enabled| execute_outcome(warehouse, table(path, dialect, columns, (enabled, replacement)), sql))
}

/// The statement's searches for `term` as sent with the policy off and on. A refusal before the
/// statement reached the source would read 0 and 0, so the first is required to be at least one.
fn searches((baseline, opted): (&Outcome, &Outcome), term: &str) -> (usize, usize) {
    let counts = (searches_in(&baseline.1, term), searches_in(&opted.1, term));
    assert!(counts.0 >= 1, "the statement must have reached the source: {counts:?}");
    counts
}

pub(super) fn cells<W>(warehouse: &W, path: &QualifiedTable, dialect: TypeDialect)
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    let from = quoted(path);
    let like = |column: &str, term: &str| format!("SELECT \"{column}\" FROM {from} WHERE \"{column}\" LIKE '%{term}%'");
    // A binary column takes the raw test, and the answer is the one the redacted test alone gives:
    // `comparison-creates` writes `term`, which a case-folding comparison would match and a binary one does not.
    for (column, expected) in [
        ("plain", &["visible TERM"][..]),
        ("byte_order", &["visible TERM"]),
        ("fixed", &["TERM", "xTERM y"]),
    ] {
        for replacement in ["[masked]", "term"] {
            let outcomes = both(
                warehouse,
                (path, dialect),
                &[(column, Some("binary"))],
                (replacement, &like(column, "TERM")),
            );
            let (before, after) = searches((&outcomes[0], &outcomes[1]), "TERM");
            let [(baseline, _), (opted, _)] = outcomes;
            let [baseline, opted] =
                [baseline, opted].map(|answer| answer.unwrap_or_else(|cause| panic!("{column} {replacement}: {cause}")));
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
    // A column with no pinned collation is not binary, whatever it really is.
    let outcomes = both(
        warehouse,
        (path, dialect),
        &[("plain", None)],
        ("[masked]", &like("plain", "TERM")),
    );
    let (before, after) = searches((&outcomes[0], &outcomes[1]), "TERM");
    let [(baseline, _), (opted, _)] = outcomes;
    assert_eq!(
        texts(&baseline.expect("an unknown collation still answers")),
        texts(&opted.expect("an unknown collation still answers"))
    );
    assert_eq!(before, after, "an unrecorded collation keeps the redacted-only statement");
    // A nondeterministic collation is looser than the proof, and `regexp_replace` refuses it: the
    // refusal must stand, so no raw search may empty the scan before it.
    let folded = ("folded", Some("named:folded_trap"));
    let outcomes = both(warehouse, (path, dialect), &[folded], ("[masked]", &like("folded", "QQQQ")));
    refused_alike(
        outcomes,
        "QQQQ",
        ("a named collation keeps the redacted-only statement", true),
    );
    // The same refusal reaches a sibling: a policy column compared with a literal is read through
    // the regex too, so a raw search on the other column, written after it, must not run first.
    let sibling = format!("SELECT \"plain\" FROM {from} WHERE \"folded\" = 'x' AND \"plain\" LIKE '%QQQQ%'");
    let outcomes = both(
        warehouse,
        (path, dialect),
        &[("plain", Some("binary")), folded],
        ("[masked]", &sibling),
    );
    refused_alike(
        outcomes,
        "QQQQ",
        ("a nondeterministic sibling keeps the redacted-only statement", false),
    );
    // A binary sibling is no obstacle, so the refusal above is the column's and not any policy column's.
    let binary = format!("SELECT \"plain\" FROM {from} WHERE \"byte_order\" = 'x' AND \"plain\" LIKE '%TERM%'");
    let columns = [("plain", Some("binary")), ("byte_order", Some("binary"))];
    let outcomes = both(warehouse, (path, dialect), &columns, ("[masked]", &binary));
    let (before, after) = searches((&outcomes[0], &outcomes[1]), "TERM");
    let [(baseline, _), (opted, _)] = outcomes;
    assert_eq!(texts(&baseline.expect("a binary sibling answers")), Vec::<String>::new());
    assert_eq!(texts(&opted.expect("a binary sibling answers")), Vec::<String>::new());
    assert_eq!(after - before, 1, "a binary sibling still takes the raw search");
    println!("raw-prefilter-collation: column=folded collation=nondeterministic applied=false outcome=refused");
}

/// The opted-in run added no raw search and ended as the baseline did, an error or rows alike.
///
/// With `refused` the baseline must be the engine's refusal of a nondeterministic collation. A
/// sibling's baseline may instead stop before reaching that column, because the source orders the
/// conjuncts, so there the equal ending and the unchanged statement are what is asserted.
fn refused_alike(outcomes: [Outcome; 2], term: &str, (kept, refused): (&str, bool)) {
    let (before, after) = searches((&outcomes[0], &outcomes[1]), term);
    assert_eq!(before, after, "{kept}");
    let [(baseline, _), (opted, _)] = outcomes;
    assert!(
        !refused || baseline.is_err(),
        "regexp_replace refuses a nondeterministic collation"
    );
    let ending = |answer: Result<RawRows, FixtureQueryError>| answer.map(|rows| texts(&rows)).map_err(|cause| cause.to_string());
    assert_eq!(
        ending(baseline),
        ending(opted),
        "an error and an empty answer must not tell the unredacted rows apart"
    );
}
