//! A Cube source holding a protected table answers the policy-free part of a filter, ahead of the
//! local text policy, and nothing that reads a policy column. The fake behind the leg is an engine
//! holding the raw table, which refuses an answer past the row cap as Cube's SQL answer is refused.

use std::collections::HashMap;

use datafusion::arrow::array::{BooleanArray, StringArray};
use sutura_domain::plan::MAX_ROWS;

use super::*;

/// `MAX_ROWS` as a count of rows.
const CAP: usize = MAX_ROWS as usize;

const POLICY: &str = r#"{"redact":[{"pattern":"secret[^ ]*","replace":"[masked]"}]}"#;

fn cube_table() -> StatementTable {
    let policy = HashMap::from([(String::from("sutura:text_policy"), String::from(POLICY))]);
    StatementTable::new(
        named("metrics"),
        QualifiedTable::parse("notes").expect("a table"),
        TypeDialect::Cube,
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("body", DataType::Utf8, true).with_metadata(policy),
            Field::new("archived", DataType::Boolean, true),
        ])),
    )
}

fn notes(regions: &[&str], bodies: &[&str]) -> RecordBatch {
    let archived: Vec<_> = (0..regions.len()).map(|index| Some(index & 1 == 0)).collect();
    RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("body", DataType::Utf8, true),
            Field::new("archived", DataType::Boolean, true),
        ])),
        vec![
            Arc::new(StringArray::from(regions.to_vec())),
            Arc::new(StringArray::from(bodies.to_vec())),
            Arc::new(BooleanArray::from(archived)),
        ],
    )
    .expect("the notes are rectangular")
}

fn small() -> RecordBatch {
    notes(
        &["emea", "emea", "apac", "apac", "amer"],
        &["secret1 a", "b", "secret2 c", "[masked] d", "e"],
    )
}

/// The table as the caller must see it: the policy already applied, as the baseline answers.
fn masked() -> RecordBatch {
    notes(
        &["emea", "emea", "apac", "apac", "amer"],
        &["[masked] a", "b", "[masked] c", "[masked] d", "e"],
    )
}

/// Cube, as the combiner sees it: statements recorded, rows counted, and an answer past `MAX_ROWS` refused.
struct Cube {
    runtime: Runtime,
    context: SessionContext,
    sent: Vec<String>,
    answered: Vec<usize>,
    /// A Cube that answers every row, whatever it is sent to select: never fewer than the engine keeps.
    loose: bool,
}

impl Cube {
    fn holding(table: RecordBatch) -> Self {
        Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("a test runtime builds"),
            context: holding(&[("notes", table)]),
            sent: Vec::new(),
            answered: Vec::new(),
            loose: false,
        }
    }

    const fn loose(mut self) -> Self {
        self.loose = true;
        self
    }
}

impl StatementLegs for Cube {
    type Stop = SourceName;

    fn open(&mut self, _sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        Ok(())
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.sent.push(statement.as_str().to_owned());
        let text = if self.loose {
            unselected(statement.as_str())
        } else {
            statement.as_str().to_owned()
        };
        let rows = rows_of(&self.runtime, &self.context, &text);
        if rows.rows().len() > CAP {
            return Err(source.clone());
        }
        self.answered.push(rows.rows().len());
        Ok(rows)
    }
}

fn ask(sql: &str, legs: &mut Cube) -> StatementAnswer<SourceName> {
    let statement = RawStatement::parse(sql).expect("a test statement is a statement");
    let tables = [cube_table()];
    DataFusionCombiner::new().expect("a combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        legs,
    )
}

/// What one engine holding the masked table answers: the statement with the policy applied first.
fn baseline(sql: &str) -> RawRows {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a test runtime builds");
    rows_of(&runtime, &holding(&[("notes", masked())]), sql)
}

/// `sql` with its `WHERE` dropped.
fn unselected(sql: &str) -> String {
    use datafusion::sql::sqlparser::ast::{SetExpr, Statement};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    let mut parsed = Parser::parse_sql(&GenericDialect, sql).expect("a leg's text parses");
    let [Statement::Query(query)] = parsed.as_mut_slice() else {
        panic!("one query: {sql}")
    };
    let SetExpr::Select(select) = query.body.as_mut() else {
        panic!("one select: {sql}")
    };
    select.selection = None;
    query.to_string()
}

/// The one statement a leg was sent, and the text of its `WHERE`, or none.
fn selection(sent: &[String]) -> Option<String> {
    use datafusion::sql::sqlparser::ast::{SetExpr, Statement};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    let [sent] = sent else { panic!("one leg ran: {sent:?}") };
    let parsed = Parser::parse_sql(&GenericDialect, sent).expect("a leg's text parses");
    let [Statement::Query(query)] = parsed.as_slice() else {
        panic!("one query: {sent}")
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        panic!("one select: {sent}")
    };
    select.selection.as_ref().map(ToString::to_string)
}

#[test]
fn a_policy_free_conjunct_reaches_cube_and_the_answer_is_the_baseline() {
    let sql = "select region, body from notes where region = 'emea' and body like '%masked%' order by body";
    let mut legs = Cube::holding(small());
    let Some(Ok(rows)) = ask(sql, &mut legs) else {
        panic!("a narrowed search over a protected Cube table answers");
    };
    assert_eq!(rows, baseline(sql));
    assert_eq!(rows.rows().len(), 1, "the baseline is not empty");
    let selection = selection(&legs.sent).expect("the policy-free conjunct reached Cube");
    assert!(selection.contains("region"), "{selection}");
    assert!(!selection.contains("body"), "no policy conjunct reached Cube: {selection}");
    assert_eq!(legs.answered, [2], "Cube answered only the emea rows");
}

#[test]
fn a_conjunct_sent_to_cube_is_checked_again_locally_over_a_cube_that_answers_more() {
    let sql = "select region, body from notes where region = 'emea' order by body";
    let mut legs = Cube::holding(small()).loose();
    let Some(Ok(rows)) = ask(sql, &mut legs) else {
        panic!("a looser Cube is corrected");
    };
    assert_eq!(rows, baseline(sql));
    assert_eq!(legs.answered, [5], "Cube answered every row");
}

#[test]
fn a_conjunct_over_a_policy_column_stays_local_and_sees_the_masked_text() {
    for sql in [
        "select region from notes where body = '[masked] a' order by region",
        "select region from notes where body in ('[masked] c', 'b') order by region",
        "select region from notes where body is not null and body = '[masked] d'",
    ] {
        let mut legs = Cube::holding(small());
        let Some(Ok(rows)) = ask(sql, &mut legs) else {
            panic!("{sql}: a search over a protected Cube table answers");
        };
        assert_eq!(rows, baseline(sql), "{sql}");
        assert!(!rows.rows().is_empty(), "{sql}: the baseline is not empty");
        let selection = selection(&legs.sent);
        assert!(
            selection.as_deref().is_none_or(|text| !text.contains("body")),
            "{sql}: a policy column reached Cube: {selection:?}"
        );
    }
}

#[test]
fn only_a_comparison_cube_cannot_answer_more_strictly_than_the_engine_is_sent() {
    for (conjunct, sent) in [
        ("region = 'emea'", true),
        ("'emea' = region", true),
        ("region in ('emea', 'apac')", true),
        ("region is not null", true),
        ("body is not null", false),
        ("archived = true", false),
        ("archived <> true", false),
        ("region <> 'emea'", false),
        ("region not in ('emea')", false),
        ("region > 'b'", false),
        ("region like 'e%'", false),
        ("lower(region) = 'emea'", false),
        ("region = 'emea' or region = 'apac'", true),
        ("region in ('emea', 'apac', 'amer', 'none')", true),
        ("region = 'emea' or length(region) > 3", false),
        ("region = 'emea' or body = 'b'", false),
        ("not (region = 'emea')", false),
        ("region = body", false),
        ("length(region) > 3", false),
    ] {
        let sql = format!("select region, body from notes where {conjunct} order by region, body");
        let mut legs = Cube::holding(small());
        let Some(Ok(rows)) = ask(&sql, &mut legs) else {
            panic!("{conjunct}: answers");
        };
        assert_eq!(rows, baseline(&sql), "{conjunct}");
        assert_eq!(selection(&legs.sent).is_some(), sent, "{conjunct}: {:?}", legs.sent);
    }
}

#[test]
fn a_narrowed_search_over_a_table_past_the_row_cap_answers_and_a_scan_of_it_is_refused() {
    let count = CAP + 5;
    let regions: Vec<_> = (0..count).map(|index| if index < 3 { "rare" } else { "common" }).collect();
    let bodies = vec!["secret x"; count];
    let sql = "select region, body from notes where region = 'rare' and body like '%masked%'";
    let mut legs = Cube::holding(notes(&regions, &bodies));
    let Some(Ok(rows)) = ask(sql, &mut legs) else {
        panic!("a filter that narrows under the cap answers");
    };
    assert_eq!(rows.rows().len(), 3);
    assert_eq!(legs.answered, [3]);

    let mut legs = Cube::holding(notes(&regions, &bodies));
    let answer = ask("select region, body from notes where body like '%masked%'", &mut legs);
    assert!(
        matches!(answer, Some(Err(StatementStopped::Leg(_)))),
        "a filter over the policy column alone does not narrow Cube's answer: {answer:?}"
    );
}

/// A policy column reaches the cut as the policy's own expression, so no SQL here puts it bare in a
/// listed shape; the check is by field metadata and is what keeps a bare one out.
#[test]
fn a_bare_policy_column_is_never_listed_whatever_the_comparison() {
    use datafusion::common::DFSchema;
    use datafusion::prelude::{col, lit};

    let table = cube_table();
    let schema = DFSchema::try_from_qualified_schema("notes", table.schema()).expect("the table's own schema");
    for predicate in [
        col("body").eq(lit("x")),
        col("body").in_list(vec![lit("x"), lit("y")], false),
        col("body").is_null(),
        col("body").is_not_null(),
        col("body").eq(lit("x")).or(col("region").eq(lit("y"))),
    ] {
        assert!(
            super::super::pushdown::answered_by_cube(&predicate, &schema).is_none(),
            "{predicate}"
        );
    }
    let listed = col("region").eq(lit("x")).and(col("body").eq(lit("y")));
    assert_eq!(
        super::super::pushdown::answered_by_cube(&listed, &schema),
        Some(col("region").eq(lit("x")))
    );
}
