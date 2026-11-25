//! Compare parsed caller and rewritten predicates, not source-code spellings.

use std::collections::HashMap;
use std::ops::ControlFlow;

use datafusion::sql::sqlparser::ast::{Expr, Query, SetExpr, Statement, TableFactor, Visit as _, Visitor};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;

use sutura_domain::catalog::COLLATION_METADATA_KEY;

use sutura_domain::catalog::Collation;
use sutura_domain::catalog::PrefilterIneligible;

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

/// `source` with its `BODY` column pinned under `collation`, as the pin writes it; `None` pins nothing.
fn duckdb_collated(source: &StatementTable, collation: Option<&str>) -> StatementTable {
    let fields: Vec<Field> = source
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let mut metadata = field.metadata().clone();
            if field.name() == "BODY" {
                drop(metadata.remove("sutura:collation"));
                if let Some(collation) = collation {
                    drop(metadata.insert(String::from("sutura:collation"), String::from(collation)));
                }
            }
            field.as_ref().clone().with_metadata(metadata)
        })
        .collect();
    StatementTable::new(
        source.source().clone(),
        source.table().clone(),
        source.dialect(),
        Arc::new(Schema::new(fields)),
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
fn an_underscore_needle_is_admitted_only_where_every_replacement_alignment_still_mismatches() {
    let sql = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%T_RM%'";
    // `[masked]` shares none of the needle's literal bytes (T, R, M), so every alignment that
    // overlaps it - full or partial - still carries a forced mismatch; the lone `_` never has to.
    assert!(raw_predicates(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, true))[0].is_some());
    // `TaRM` is the same length as the needle and agrees at every literal position (only the `_`
    // position differs), so the replacement's own fixed bytes satisfy "T_RM" outright: a row whose
    // raw text never contained the needle would still redact to something that matches it. Barred.
    assert!(
        raw_predicates(sql, table(TypeDialect::DataFusion, &["TaRM"], true, false, true))
            .iter()
            .all(Option::is_none)
    );
}

#[test]
fn a_pure_literal_needle_sharing_a_boundary_byte_is_admitted_when_every_alignment_still_mismatches() {
    let sql = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%MXY%'";
    // The pre-`_` rule refused any needle containing the replacement's first or last byte; `MXY`
    // contains `M`, `MASKED`'s first byte, and would have been refused by it. `preserves` admits
    // it instead, because every alignment that overlaps `MASKED` still carries a disagreeing byte
    // (`X` or `Y` against `MASKED`'s own bytes at every shift) - this needle has no `_` in it at
    // all, so admitting it is strictly wider than the old rule, not a consequence of `_` alone.
    assert!(raw_predicates(sql, table(TypeDialect::DataFusion, &["MASKED"], true, false, true))[0].is_some());
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
        TypeDialect::DuckDb,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
    ] {
        let source = table(dialect, &["[masked]"], true, false, true);
        let source = if dialect == TypeDialect::DuckDb {
            duckdb_collated(&source, Some("binary"))
        } else {
            source
        };
        let predicates = raw_predicates(sql, source);
        if dialect == TypeDialect::DuckDb {
            assert!(predicates.iter().any(Option::is_some), "{dialect:?}");
            continue;
        }
        assert!(predicates.iter().all(Option::is_none), "{dialect:?}");
    }
}

#[test]
fn a_duckdb_column_takes_the_raw_condition_only_when_pinned_binary() {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let enabled = table(TypeDialect::DuckDb, &["[masked]"], true, false, true);
    assert!(raw_predicates(sql, duckdb_collated(&enabled, Some("binary")))[0].is_some());
    // `NOCASE` and `NOACCENT` are consulted by `LIKE` and survive `REGEXP_REPLACE`; an unreadable
    // fact and a missing one are no evidence either.
    for pinned in [
        Some("named:NOCASE"),
        Some("named:NOACCENT"),
        Some("named:de"),
        Some("BINARY"),
        Some("unreadable"),
        None,
    ] {
        assert!(
            raw_predicates(sql, duckdb_collated(&enabled, pinned))
                .iter()
                .all(Option::is_none),
            "{pinned:?}"
        );
    }
    for dialect in [TypeDialect::DataFusion, TypeDialect::Oracle] {
        let enabled = table(dialect, &["[masked]"], true, dialect == TypeDialect::Oracle, true);
        let sql = if dialect == TypeDialect::Oracle {
            "SELECT count(\"BODY\") FROM \"notes\" WHERE UPPER(\"BODY\") LIKE '%TERM%'"
        } else {
            sql
        };
        assert!(
            raw_predicates(sql, duckdb_collated(&enabled, None))[0].is_some(),
            "{dialect:?} reads no collation fact"
        );
    }
    let clickhouse = table(TypeDialect::ClickHouse, &["[masked]"], true, false, true);
    assert!(
        raw_predicates(sql, duckdb_collated(&clickhouse, Some("binary")))
            .iter()
            .any(Option::is_some),
        "ClickHouse takes the new binary-column arm"
    );
}

/// `source` with the named columns pinned as a read leaves them: each is the pinned collation text.
fn postgres_collated(source: &StatementTable, pins: &[(&str, &str)]) -> StatementTable {
    let fields = source
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let mut metadata = field.metadata().clone();
            if let Some((_, collation)) = pins.iter().find(|(name, _)| field.name() == name) {
                metadata.insert(String::from(COLLATION_METADATA_KEY), String::from(*collation));
            }
            field.as_ref().clone().with_metadata(metadata)
        })
        .collect::<Vec<_>>();
    StatementTable::new(
        source.source().clone(),
        source.table().clone(),
        source.dialect(),
        Arc::new(Schema::new(fields)),
    )
}

#[test]
fn a_postgres_column_takes_the_raw_condition_only_where_it_is_pinned_binary() {
    let sql = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let source = || table(TypeDialect::Postgres, &["[masked]"], true, false, true);
    let conditions = raw_predicates(sql, postgres_collated(&source(), &[("BODY", "binary")]));
    let [Some(condition)] = conditions.as_slice() else {
        panic!("one raw necessary condition: {conditions:?}");
    };
    assert_eq!(condition.to_string(), "(\"notes\".\"BODY\" LIKE '%TERM%')");
    for collation in [
        None,
        Some("named:public.case_insensitive"),
        Some("named:pg_catalog.und-x-icu"),
        Some("other"),
    ] {
        let pins: Vec<_> = collation.map(|text| ("BODY", text)).into_iter().collect();
        assert!(
            raw_predicates(sql, postgres_collated(&source(), &pins))
                .iter()
                .all(Option::is_none),
            "{collation:?}"
        );
    }
    // An engine that fixes its comparison for every column needs no record of it.
    assert!(raw_predicates(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, true))[0].is_some());
}

#[test]
fn a_binary_postgres_column_keeps_every_barrier_and_has_no_upper_arm() {
    let eligible = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let binary = |source: &StatementTable| postgres_collated(source, &[("BODY", "binary")]);
    // The arm exists, so every refusal below is the barrier's and not an absent arm's.
    let source = table(TypeDialect::Postgres, &["[masked]"], true, false, true);
    assert!(raw_predicates(eligible, binary(&source))[0].is_some());
    for replacements in [vec!["[TERM]"], vec!["T"], vec!["M"], vec!["[masked]", ""]] {
        let source = table(TypeDialect::Postgres, &replacements, true, false, true);
        assert!(
            raw_predicates(eligible, binary(&source)).iter().all(Option::is_none),
            "{replacements:?}"
        );
    }
    for sql in [
        "SELECT \"BODY\" FROM \"notes\" WHERE UPPER(\"BODY\") LIKE '%TERM%'",
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" COLLATE \"C\" LIKE '%TERM%'",
        "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" ILIKE '%TERM%'",
    ] {
        let source = table(TypeDialect::Postgres, &["[masked]"], true, false, true);
        assert!(raw_predicates(sql, binary(&source)).iter().all(Option::is_none), "{sql}");
    }
}

#[test]
fn a_postgres_sibling_on_another_policy_column_must_be_pinned_binary_too() {
    // `name` carries a text policy, so the view reads it through the regex a nondeterministic
    // collation refuses: beside the raw search it can raise on one row unless it is binary as well.
    let sql = |sibling: &str| format!("SELECT \"BODY\" FROM \"notes\" WHERE {sibling} AND \"BODY\" LIKE '%TERM%'");
    let source = || table(TypeDialect::Postgres, &["[masked]"], true, false, true);
    for sibling in [
        "\"name\" = 'x'",
        "\"name\" IS NULL",
        "\"name\" IN ('x', 'y')",
        "\"name\" BETWEEN 'a' AND 'b'",
    ] {
        for (pins, applies) in [
            (vec![("BODY", "binary"), ("name", "binary")], true),
            (vec![("BODY", "binary")], false),
            (vec![("BODY", "binary"), ("name", "named:public.folded")], false),
        ] {
            let applied = raw_predicates(&sql(sibling), postgres_collated(&source(), &pins))
                .iter()
                .any(Option::is_some);
            assert_eq!(applied, applies, "{sibling} {pins:?}");
        }
    }
    // A sibling on a column with no policy is read through no regex.
    assert!(
        raw_predicates(&sql("\"id\" = 5"), postgres_collated(&source(), &[("BODY", "binary")]))
            .iter()
            .any(Option::is_some)
    );
}

#[test]
fn a_replacement_that_differs_only_in_case_never_passes_the_proof_where_a_collation_may_apply() {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let predicates = |replacement| {
        raw_predicates(
            sql,
            duckdb_collated(&table(TypeDialect::DuckDb, &[replacement], true, false, true), Some("binary")),
        )
    };
    assert!(predicates("[masked]")[0].is_some());
    for replacement in ["term", "[tErM]"] {
        assert!(predicates(replacement).iter().all(Option::is_none), "{replacement}");
    }
    assert!(raw_predicates(sql, table(TypeDialect::DataFusion, &["term"], true, false, true))[0].is_some());
}

#[test]
fn a_non_ascii_replacement_never_passes_the_proof() {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%K%'";
    let duckdb = |replacement| duckdb_collated(&table(TypeDialect::DuckDb, &[replacement], true, false, true), Some("binary"));
    assert!(raw_predicates(sql, duckdb("[redacted]"))[0].is_some());
    let kelvin = "[\u{212A}]";
    for source in [duckdb(kelvin), table(TypeDialect::DataFusion, &[kelvin], true, false, true)] {
        assert!(raw_predicates(sql, source).iter().all(Option::is_none));
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
fn declared_scalar_text_siblings_allow_the_raw_search_without_weakening_unknown_types() {
    for dialect in [
        TypeDialect::DataFusion,
        TypeDialect::DuckDb,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let source = || {
            let original = table(dialect, &["[masked]"], true, dialect == TypeDialect::Oracle, true);
            let mut fields = original.schema().fields().to_vec();
            fields.extend(["env", "is_deleted"].map(|name| {
                Arc::new(
                    Field::new(name, DataType::Utf8, true)
                        .with_metadata(HashMap::from([(String::from("sutura:scalar_text"), String::from("true"))])),
                )
            }));
            collated(
                &StatementTable::new(
                    original.source().clone(),
                    original.table().clone(),
                    dialect,
                    Arc::new(Schema::new(fields)),
                ),
                Some("binary"),
            )
        };
        let search = if dialect == TypeDialect::Oracle {
            "UPPER(\"BODY\")"
        } else {
            "\"BODY\""
        };
        let sql = |sibling: &str| format!("SELECT \"BODY\" FROM \"notes\" WHERE {sibling} AND {search} LIKE '%TERM%'");
        for sibling in [
            "env = 'E' AND is_deleted = 'N'",
            "env IN ('E', 'W') AND is_deleted <> 'Y'",
            "env BETWEEN 'A' AND 'Z' AND is_deleted IS NOT NULL",
        ] {
            assert!(raw_predicates(&sql(sibling), source())[0].is_some(), "{dialect:?} {sibling}");
        }
        for sibling in ["label = 'E'", "amount = '1'", "doc = 'E'", "CAST(env AS INT) = 1"] {
            assert!(
                raw_predicates(&sql(sibling), source()).iter().all(Option::is_none),
                "{dialect:?} {sibling}"
            );
        }
        // Even a known collation is not declared scalar-text provenance.
        let forged = postgres_collated(&source(), &[("label", "binary")]);
        assert!(raw_predicates(&sql("label = 'E'"), forged).iter().all(Option::is_none));
        for marker in [None, Some("false"), Some("TRUE"), Some("")] {
            let original = source();
            let fields = original
                .schema()
                .fields()
                .iter()
                .map(|field| {
                    let mut field = field.as_ref().clone();
                    if field.name() == "env" {
                        field.metadata_mut().remove("sutura:scalar_text");
                        if let Some(marker) = marker {
                            field
                                .metadata_mut()
                                .insert(String::from("sutura:scalar_text"), String::from(marker));
                        }
                    }
                    field
                })
                .collect::<Vec<_>>();
            let unproven = StatementTable::new(
                original.source().clone(),
                original.table().clone(),
                dialect,
                Arc::new(Schema::new(fields)),
            );
            assert!(
                raw_predicates(&sql("env = 'E'"), unproven).iter().all(Option::is_none),
                "{dialect:?} {marker:?}"
            );
        }
    }
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

fn bigquery_conditions(collation: Option<&Collation>) -> Vec<Option<Expr>> {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let source = table(TypeDialect::BigQuery, &["[masked]"], true, false, true);
    let fields = source
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let mut field = field.as_ref().clone();
            if let Some(collation) = collation.filter(|_| field.name() == "BODY") {
                collation.pin_onto(&mut field);
            }
            field
        })
        .collect::<Vec<_>>();
    let pinned = StatementTable::new(
        source.source().clone(),
        source.table().clone(),
        TypeDialect::BigQuery,
        Arc::new(Schema::new(fields)),
    );
    raw_predicates(sql, pinned)
}

#[test]
fn bigquery_takes_the_raw_condition_only_over_a_column_pinned_binary() {
    let binary = bigquery_conditions(Some(&Collation::Binary));
    let [Some(condition)] = binary.as_slice() else {
        panic!("one raw necessary condition over a binary pin");
    };
    let Statement::Query(expected) = parsed("SELECT 1 FROM notes WHERE (`notes`.`BODY` LIKE '%TERM%')") else {
        panic!("a query");
    };
    let SetExpr::Select(expected) = *expected.body else {
        panic!("a select");
    };
    assert_eq!(
        Some(condition),
        expected.selection.as_ref(),
        "backticks, qualified by the table"
    );
    for collation in [None, Some(Collation::Unknown), Some(Collation::named("und:ci"))] {
        let conditions = bigquery_conditions(collation.as_ref());
        assert!(conditions.iter().all(Option::is_none), "{collation:?}: {conditions:?}");
    }
}

/// `table` with its text column pinned under `collation`, as pinning would have written it.
fn collated(table: &StatementTable, collation: Option<&str>) -> StatementTable {
    let fields = table
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let mut metadata = field.metadata().clone();
            if field.name() == "BODY"
                && let Some(collation) = collation
            {
                drop(metadata.insert(String::from(COLLATION_METADATA_KEY), String::from(collation)));
            }
            field.as_ref().clone().with_metadata(metadata)
        })
        .collect::<Vec<_>>();
    StatementTable::new(
        table.source().clone(),
        table.table().clone(),
        table.dialect(),
        Arc::new(Schema::new(fields)),
    )
}

#[test]
fn clickhouse_prefilters_a_binary_column_with_the_stored_column_named_through_its_table() {
    let sql = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let base = table(TypeDialect::ClickHouse, &["[masked]"], true, false, true);
    let conditions = raw_predicates(sql, collated(&base, Some("binary")));
    let [Some(condition)] = conditions.as_slice() else {
        panic!("one raw necessary condition: {conditions:?}");
    };
    assert_eq!(condition.to_string(), "(`notes`.`BODY` LIKE '%TERM%')");
    let rewritten = super::super::text::apply(String::from(sql), &[collated(&base, Some("binary"))], TypeDialect::ClickHouse)
        .expect("a protected table rewrites");
    assert!(
        rewritten.contains("FROM \"notes\" WHERE (`notes`.`BODY` LIKE '%TERM%')) AS `notes`"),
        "the condition filters the stored table inside the view: {rewritten}"
    );
    assert!(rewritten.contains("replaceRegexpAll(`notes`.`BODY`,"), "{rewritten}");
}

/// `text::apply_to` itself - the real production entry point, not the test-only [`apply`]
/// wrapper that discards [`PrefilterIneligible`] - over one parsed statement.
fn ineligibility(sql: &str, table: StatementTable) -> Option<PrefilterIneligible> {
    let mut statements = Parser::parse_sql(&GenericDialect, sql).expect("generated SQL parses");
    let [statement] = statements.as_mut_slice() else {
        panic!("one statement");
    };
    let dialect = table.dialect();
    super::super::text::apply_to(statement, &[table], dialect).expect("an acknowledged policy must rewrite successfully")
}

#[test]
fn apply_to_names_a_needle_outside_the_alphanumeric_or_underscore_alphabet() {
    let sql = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%ab-c%'";
    assert_eq!(
        ineligibility(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, true)),
        Some(PrefilterIneligible::NeedleShape)
    );
}

#[test]
fn apply_to_names_the_replacement_a_needle_agrees_with() {
    let sql = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    assert_eq!(
        ineligibility(sql, table(TypeDialect::DataFusion, &["[TERM]"], true, false, true)),
        Some(PrefilterIneligible::AgreesWithReplacement { replacement_index: 0 })
    );
}

#[test]
fn apply_to_admits_an_eligible_needle() {
    let sql = "SELECT \"BODY\" FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    assert_eq!(
        ineligibility(sql, table(TypeDialect::DataFusion, &["[masked]"], true, false, true)),
        None
    );
}

/// The same three outcomes through Oracle's native `UPPER(column)` comparison, which `condition`
/// only recognizes as evidence at all when the column is a LOB - a separate branch from the
/// bare-column comparison the tests above exercise.
#[test]
fn apply_to_covers_the_oracle_native_upper_path() {
    let eligible_sql = "SELECT count(\"BODY\") FROM \"notes\" n WHERE UPPER(n.\"BODY\") LIKE '%TERM%'";
    assert_eq!(
        ineligibility(eligible_sql, table(TypeDialect::Oracle, &["[masked]"], true, true, true)),
        None
    );
    let shape_sql = "SELECT count(\"BODY\") FROM \"notes\" n WHERE UPPER(n.\"BODY\") LIKE '%ab-c%'";
    assert_eq!(
        ineligibility(shape_sql, table(TypeDialect::Oracle, &["[masked]"], true, true, true)),
        Some(PrefilterIneligible::NeedleShape)
    );
    assert_eq!(
        ineligibility(eligible_sql, table(TypeDialect::Oracle, &["[TERM]"], true, true, true)),
        Some(PrefilterIneligible::AgreesWithReplacement { replacement_index: 0 })
    );
}
