//! Compare parsed caller and rewritten predicates, not source-code spellings.

use std::collections::HashMap;
use std::ops::ControlFlow;

use datafusion::sql::sqlparser::ast::{Expr, Query, SetExpr, Statement, TableFactor, Visit as _, Visitor};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;

use super::*;

fn table(dialect: TypeDialect, replacements: &[&str], bound: bool, lob: bool, enabled: bool) -> StatementTable {
    let mut policy = serde_json::json!({"redact": replacements.iter().map(|replacement|
        serde_json::json!({"pattern": "secret[^ ]*", "replace": replacement})).collect::<Vec<_>>()});
    if enabled {
        policy["raw_prefilter"] = serde_json::json!({"acknowledged_because": "accepted secret-dependent timing"});
    }
    if bound {
        policy["max_text_bytes"] = serde_json::json!(128);
    }
    let mut metadata = HashMap::from([(String::from("sutura:text_policy"), policy.to_string())]);
    let name_metadata = metadata.clone();
    if lob {
        metadata.insert(String::from("sutura:oracle_lob"), String::from("clob"));
    }
    StatementTable::new(
        named("data"),
        QualifiedTable::parse("notes").expect("a table"),
        dialect,
        Arc::new(Schema::new(vec![
            Field::new("BODY", DataType::Utf8, true).with_metadata(metadata),
            Field::new("id", DataType::Int64, true),
            Field::new("label", DataType::Utf8, true),
            Field::new("name", DataType::Utf8, true).with_metadata(name_metadata.clone()),
            Field::new("amount", DataType::Utf8, true).with_metadata(with(&name_metadata, "sutura:unbounded_number", "utf8")),
            Field::new("doc", DataType::Utf8, true).with_metadata(with(&name_metadata, "sutura:oracle_lob", "clob")),
            Field::new("big", DataType::UInt64, true),
        ])),
    )
}

fn with(base: &HashMap<String, String>, key: &str, value: &str) -> HashMap<String, String> {
    let mut metadata = base.clone();
    drop(metadata.insert(String::from(key), String::from(value)));
    metadata
}

fn parsed(sql: &str) -> Statement {
    Parser::parse_sql(&GenericDialect, sql)
        .expect("generated SQL parses")
        .pop()
        .expect("one statement")
}

#[derive(Default)]
struct RawPredicates(Vec<Option<Expr>>);
impl Visitor for RawPredicates {
    type Break = ();
    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if let SetExpr::Select(select) = query.body.as_ref()
            && let [from] = select.from.as_slice()
            && matches!(&from.relation, TableFactor::Table { name, .. } if name.0.last().and_then(|part| part.as_ident()).is_some_and(|part| part.value == "notes"))
        {
            self.0.push(select.selection.clone());
        }
        ControlFlow::Continue(())
    }
}

fn raw_predicates(sql: &str, table: StatementTable) -> Vec<Option<Expr>> {
    let fields = table
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let mut metadata = field.metadata().clone();
            if let Some(encoded) = metadata.get_mut("sutura:text_policy") {
                let mut policy: serde_json::Value = serde_json::from_str(encoded).expect("a fixture policy");
                drop(policy.as_object_mut().expect("a policy object").remove("raw_prefilter"));
                *encoded = policy.to_string();
            }
            field.as_ref().clone().with_metadata(metadata)
        })
        .collect::<Vec<_>>();
    let baseline = StatementTable::new(
        table.source().clone(),
        table.table().clone(),
        table.dialect(),
        Arc::new(Schema::new(fields)),
    );
    let baseline = super::super::text::apply(String::from(sql), &[baseline], table.dialect())
        .expect("the disabled policy must rewrite successfully");
    let dialect = table.dialect();
    let rewritten = super::super::text::apply(String::from(sql), &[table], dialect)
        .expect("an acknowledged policy must rewrite successfully");
    let original = parsed(&baseline);
    let rewritten = parsed(&rewritten);
    let outer = |statement: Statement| {
        let Statement::Query(query) = statement else {
            panic!("a query");
        };
        let SetExpr::Select(select) = *query.body else {
            panic!("a select");
        };
        select.selection
    };
    assert_eq!(
        outer(original),
        outer(rewritten.clone()),
        "the exact redacted predicate must remain decisive"
    );
    let mut raw = RawPredicates::default();
    assert!(rewritten.visit(&mut raw).is_continue());
    assert!(!raw.0.is_empty(), "the source table must still be read");
    raw.0
}

#[test]
fn opted_in_literal_contains_adds_only_a_raw_necessary_condition() {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let conditions = raw_predicates(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, true));
    let [Some(condition)] = conditions.as_slice() else {
        panic!("one raw necessary condition: {conditions:?}");
    };
    let expected = parsed("SELECT 1 FROM notes WHERE (\"notes\".\"BODY\" LIKE '%TERM%')");
    let Statement::Query(expected) = expected else {
        panic!("a query");
    };
    let SetExpr::Select(expected) = *expected.body else {
        panic!("a select");
    };
    assert_eq!(Some(condition), expected.selection.as_ref());
    let absent = raw_predicates(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, false));
    assert!(absent.iter().all(Option::is_none));
}

#[test]
fn raw_prefilter_falls_back_for_creation_bridges_marker_and_unsafe_scope() {
    // Establish that the same opted-in shape can optimize before asking which ones must not.
    let eligible = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    assert!(raw_predicates(eligible, table(TypeDialect::DataFusion, &["[masked]"], true, false, true))[0].is_some());
    for replacements in [
        vec![""],
        vec!["[TERM]"],
        vec!["T"],
        vec!["M"],
        vec!["[masked]", ""],
        vec!["[m\u{e4}sked]"],
    ] {
        assert!(
            raw_predicates(eligible, table(TypeDialect::DataFusion, &replacements, true, false, true))
                .iter()
                .all(Option::is_none),
            "{replacements:?}"
        );
    }
    for sql in [
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%truncated%'",
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%T_RM%'",
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" NOT LIKE '%TERM%'",
        "SELECT \"BODY\" FROM \"notes\" WHERE NOT (\"BODY\" LIKE '%TERM%')",
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%' OR \"BODY\" IS NULL",
        "SELECT \"BODY\" FROM \"notes\" WHERE UPPER(\"BODY\") LIKE '%TERM%'",
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" COLLATE binary LIKE '%TERM%'",
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%' ESCAPE '!'",
        "SELECT 'TERM' AS \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'",
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%' AND EXISTS (SELECT 1 FROM \"notes\")",
        "SELECT n.\"BODY\" FROM \"notes\" n JOIN \"notes\" m ON n.\"BODY\" = m.\"BODY\" WHERE n.\"BODY\" LIKE '%TERM%'",
    ] {
        assert!(
            raw_predicates(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, true))
                .iter()
                .all(Option::is_none),
            "{sql}"
        );
    }
}

#[test]
fn oracle_raw_lob_prefilter_uses_an_explicit_null_safe_binary_sort_guard() {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" n WHERE UPPER(n.\"BODY\") LIKE '%TERM%'";
    let conditions = raw_predicates(sql, table(TypeDialect::Oracle, &["[masked]"], true, true, true));
    let [Some(condition)] = conditions.as_slice() else {
        panic!("one guarded raw condition: {conditions:?}");
    };
    let expected = parsed(
        "SELECT 1 FROM notes WHERE CASE WHEN SYS_CONTEXT('USERENV', 'NLS_SORT') COLLATE BINARY = 'BINARY' THEN CASE WHEN UPPER(\"notes\".\"BODY\") LIKE '%TERM%' THEN 1 ELSE 0 END ELSE 1 END = 1",
    );
    let Statement::Query(expected) = expected else {
        panic!("a query");
    };
    let SetExpr::Select(expected) = *expected.body else {
        panic!("a select");
    };
    assert_eq!(Some(condition), expected.selection.as_ref());
    assert!(
        raw_predicates(sql, table(TypeDialect::Oracle, &["[term]"], true, true, true))
            .iter()
            .all(Option::is_none)
    );
    assert!(
        raw_predicates(sql, table(TypeDialect::Oracle, &["[masked]"], true, false, true))
            .iter()
            .all(Option::is_none)
    );
}

#[test]
fn oracle_raw_lob_prefilter_resolves_full_and_basename_qualifiers_without_an_alias() {
    let make_table = || {
        let source = table(TypeDialect::Oracle, &["[masked]"], true, true, true);
        StatementTable::new(
            named("data"),
            QualifiedTable::parse("owner.notes").expect("a qualified table"),
            TypeDialect::Oracle,
            Arc::clone(source.schema()),
        )
    };
    for qualifier in ["\"owner\".\"notes\"", "\"notes\""] {
        let sql = format!("SELECT count(\"BODY\") FROM \"owner\".\"notes\" WHERE UPPER({qualifier}.\"BODY\") LIKE '%TERM%'");
        assert!(raw_predicates(&sql, make_table())[0].is_some(), "{sql}");
    }
    for sql in [
        "SELECT count(\"BODY\") FROM \"owner\".\"notes\" n WHERE UPPER(\"notes\".\"BODY\") LIKE '%TERM%'",
        "SELECT count(\"BODY\") FROM \"owner\".\"notes\" WHERE UPPER(other.\"BODY\") LIKE '%TERM%'",
    ] {
        assert!(raw_predicates(sql, make_table()).iter().all(Option::is_none), "{sql}");
    }
}

#[test]
fn source_engines_without_comparison_evidence_keep_the_redacted_only_path() {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    assert!(raw_predicates(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, true))[0].is_some());
    for dialect in [
        TypeDialect::Postgres,
        TypeDialect::DuckDb,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
    ] {
        assert!(
            raw_predicates(sql, table(dialect, &["[masked]"], true, false, true))
                .iter()
                .all(Option::is_none),
            "{dialect:?}"
        );
    }
}

#[test]
fn a_sibling_conjunct_that_can_raise_keeps_the_redacted_only_path() {
    let sql = |sibling: &str| format!("SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%' AND {sibling}");
    let enabled = || table(TypeDialect::DataFusion, &["[masked]"], true, false, true);
    // `label` is a plain Utf8, which is also what an unlisted type or an unbounded NUMBER pins as,
    // so only a column carrying a text policy (`name`) is proven text for a string literal.
    for plain in [
        "id = 5",
        "id <> 5",
        "id >= -1",
        "5 < id",
        "id BETWEEN 1 AND 9",
        "id IN (1, 2, 3)",
        "name = 'x'",
        "name IN ('a', 'b')",
        "label IS NOT NULL",
        "amount IS NULL",
        "id IS NULL",
        "big = 5",
        "big >= +1",
        "(id = 5 AND name <> 'x')",
    ] {
        assert!(raw_predicates(&sql(plain), enabled())[0].is_some(), "{plain}");
    }
    for raising in [
        "1 / (id - 5) = 0",
        "CAST(label AS INT) = 1",
        "id = label",
        "label = 5",
        "label = 'x'",
        "label IN ('a', 'b')",
        "amount = 'abc'",
        "doc = 'x'",
        "amount = 5",
        "amount IN ('1', '2')",
        "big = -1",
        "big BETWEEN -1 AND 5",
        "name = 5",
        "id = 'x'",
        "id IN (1, label)",
        "id IN (1, 'x')",
        "id BETWEEN 1 AND label",
        "id BETWEEN label AND 9",
        "UPPER(label) = 'X'",
        "id + 1 = 2",
        "label LIKE '%x%'",
        "id = 1.5e0 / 0",
        "(id = 5 AND 1 / (id - 5) = 0)",
    ] {
        assert!(
            raw_predicates(&sql(raising), enabled()).iter().all(Option::is_none),
            "{raising}"
        );
    }
    let oracle = || table(TypeDialect::Oracle, &["[masked]"], true, true, true);
    let upper = |sibling: &str| format!("SELECT \"BODY\" FROM \"notes\" WHERE UPPER(\"BODY\") LIKE '%TERM%' AND {sibling}");
    assert!(raw_predicates(&upper("id = 5"), oracle())[0].is_some());
    assert!(
        raw_predicates(&upper("1 / (id - 5) = 0"), oracle())
            .iter()
            .all(Option::is_none)
    );
}

#[test]
fn the_cube_local_policy_never_adds_a_raw_condition() {
    let sql = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let acknowledged = || table(TypeDialect::Cube, &["[masked]"], true, false, true);
    assert!(
        raw_predicates(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, true))[0].is_some(),
        "the same shape does take a raw condition where the arm is eligible"
    );
    let local = super::super::text::apply_cube_local(String::from(sql), &[acknowledged()])
        .expect("the local policy rewrites an acknowledged Cube column");
    let mut raw = RawPredicates::default();
    assert!(parsed(&local).visit(&mut raw).is_continue());
    assert!(!raw.0.is_empty(), "the source table must still be read");
    assert!(raw.0.iter().all(Option::is_none), "{local}");
}

#[test]
fn a_backtick_dialect_renders_the_raw_condition_with_quoted_qualified_identifiers() {
    use super::super::text::raw_column;
    use super::super::text_prefilter::raw_condition;
    use sutura_domain::catalog::PrefilterCapability;

    for dialect in [TypeDialect::BigQuery, TypeDialect::ClickHouse] {
        let column = || raw_column(dialect, "notes", "body");
        assert_eq!(
            raw_condition(PrefilterCapability::DataFusion, column(), "TERM", false).to_string(),
            "(`notes`.`body` LIKE '%TERM%')",
            "{dialect:?}"
        );
        assert_eq!(
            raw_condition(PrefilterCapability::Oracle, column(), "TERM", true).to_string(),
            "CASE WHEN SYS_CONTEXT('USERENV', 'NLS_SORT') COLLATE BINARY = 'BINARY' THEN CASE WHEN UPPER(`notes`.`body`) LIKE '%TERM%' THEN 1 ELSE 0 END ELSE 1 END = 1",
            "{dialect:?}"
        );
    }
}

#[test]
fn the_redacted_projection_reads_the_qualified_stored_column_beside_its_alias() {
    let sql = "SELECT \"BODY\" FROM \"notes\"";
    for (dialect, quote, read) in [
        (TypeDialect::ClickHouse, '`', "replaceRegexpAll(`notes`.`BODY`,"),
        (TypeDialect::BigQuery, '`', "REGEXP_REPLACE(`notes`.`BODY`,"),
        (TypeDialect::DataFusion, '"', "REGEXP_REPLACE(\"notes\".\"BODY\","),
    ] {
        let rewritten = super::super::text::apply(
            String::from(sql),
            &[table(dialect, &["[masked]"], false, false, false)],
            dialect,
        )
        .expect("a protected table rewrites");
        assert!(rewritten.contains(read), "{dialect:?}: {rewritten}");
        assert!(
            rewritten.contains(&format!("AS {quote}BODY{quote}")),
            "{dialect:?}: {rewritten}"
        );
    }
}
