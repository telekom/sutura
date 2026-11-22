//! Execute the boundary, including multibyte truncation and correlated uses.

use super::*;
use sutura_domain::warehouse::Value;

struct Captured(Vec<String>);

impl StatementLegs for Captured {
    type Stop = SourceName;

    fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        Ok(())
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.0.push(statement.as_str().to_owned());
        Err(source.clone())
    }
}

/// A CLOB, a NCLOB and a plain text column of one Oracle table, the CLOB and NCLOB under `policy`.
fn lob_table(policy: &str) -> [StatementTable; 1] {
    let lob = |name, kind| {
        Field::new(name, DataType::Utf8, true).with_metadata(std::collections::HashMap::from([
            (String::from("sutura:oracle_lob"), String::from(kind)),
            (String::from("sutura:text_policy"), String::from(policy)),
        ]))
    };
    [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("TEXT_EVENTS").unwrap(),
        TypeDialect::Oracle,
        Arc::new(Schema::new(vec![
            Field::new("LABEL", DataType::Utf8, true),
            lob("SCRIPT", "clob"),
            lob("NATIONAL", "nclob"),
        ])),
    )]
}

/// The final source statement for `sql` over `tables`, and each of its output expressions.
fn source_outputs(tables: &[StatementTable], sql: &str) -> (String, Vec<String>) {
    use datafusion::sql::sqlparser::ast::{SelectItem, SetExpr, Statement};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;

    let statement = RawStatement::parse(sql).unwrap();
    let mut legs = Captured(Vec::new());
    let answer = DataFusionCombiner::new().unwrap().answer_statement(
        StatementQuestion::new(
            &statement,
            tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(
        matches!(answer, Some(Err(StatementStopped::Leg(_)))),
        "{sql}: {answer:?}; {:?}",
        legs.0
    );
    let [reached] = legs.0.as_slice() else {
        panic!("one final statement: {:?}", legs.0)
    };
    let parsed = Parser::parse_sql(&GenericDialect, reached).unwrap();
    let [Statement::Query(query)] = parsed.as_slice() else {
        panic!("one query: {reached}")
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        panic!("one output projection: {reached}")
    };
    let outputs = select
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::ExprWithAlias { expr, .. } => expr.to_string(),
            other => panic!("explicit output names: {other}: {reached}"),
        })
        .collect();
    (reached.clone(), outputs)
}

#[test]
fn oracle_lob_top_n_keeps_the_qualified_output_in_scope() {
    use datafusion::sql::sqlparser::ast::{SetExpr, Statement, TableFactor};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;

    let [table] = lob_table(r#"{"max_text_bytes":128,"redact":[{"pattern":"secret","replace":"masked"}]}"#);
    let mut fields = table.schema().fields().to_vec();
    fields.push(Arc::new(Field::new("ID", DataType::Int64, false)));
    let tables = [StatementTable::new(
        table.source().clone(),
        table.table().clone(),
        table.dialect(),
        Arc::new(Schema::new(fields)),
    )];
    let (sql, _) = source_outputs(
        &tables,
        r#"SELECT r."LABEL", length(r."SCRIPT") AS n FROM "TEXT_EVENTS" r ORDER BY n DESC, r."ID" LIMIT 3"#,
    );
    let parsed = Parser::parse_sql(&GenericDialect, &sql).unwrap();
    let [Statement::Query(query)] = parsed.as_slice() else {
        panic!("expected one Oracle statement: {sql}");
    };
    let SetExpr::Select(output) = query.body.as_ref() else {
        panic!("expected the LOB result projection: {sql}");
    };
    let TableFactor::Derived { subquery, .. } = &output.from[0].relation else {
        panic!("expected the labelled result: {sql}");
    };
    let SetExpr::Select(labelled) = subquery.body.as_ref() else {
        panic!("expected the labelled result projection: {sql}");
    };
    let TableFactor::Derived {
        subquery,
        alias: Some(alias),
        ..
    } = &labelled.from[0].relation
    else {
        panic!("the qualified output must resolve through the derived table: {sql}");
    };
    assert_eq!(alias.name.value, "r", "{sql}");
    assert!(subquery.order_by.is_some() && subquery.fetch.is_some(), "{sql}");
}

/// Every derived table in `sql` carries an alias, and every qualified column reference resolves
/// against an alias or table name in scope where it is used: github.com/telekom/sutura#2041's
/// scoping rule, held generically rather than per rewrite. A missing alias, or a qualifier a
/// wrapper doesn't export, is exactly how a reference the planner built before the wrapper existed
/// goes out of scope one level up.
fn assert_every_derived_table_is_aliased(sql: &str) {
    use datafusion::sql::sqlparser::ast::{
        Expr, Query, Select, SetExpr, Statement, TableFactor, TableWithJoins, Visit as _, Visitor,
    };
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    use std::ops::ControlFlow;

    /// The name a `FROM` item or join relation makes available to the `SELECT` that holds it.
    fn relation_name(factor: &TableFactor, sql: &str) -> String {
        match factor {
            TableFactor::Table { name, alias, .. } => alias.as_ref().map_or_else(
                || name.0.last().expect("a table name has a part").to_string(),
                |a| a.name.value.clone(),
            ),
            TableFactor::Derived { alias, .. } => alias
                .as_ref()
                .unwrap_or_else(|| panic!("an unaliased derived table is out of scope above it: {sql}"))
                .name
                .value
                .clone(),
            TableFactor::NestedJoin { alias: Some(alias), .. } => alias.name.value.clone(),
            other => panic!("an unnamed FROM item is out of scope above it: {other}: {sql}"),
        }
    }

    fn relations_in(twj: &TableWithJoins, names: &mut Vec<String>, sql: &str) {
        names.push(relation_name(&twj.relation, sql));
        for join in &twj.joins {
            names.push(relation_name(&join.relation, sql));
        }
    }

    /// Checks every `CompoundIdentifier` built in `select`'s own clauses (not a nested query's)
    /// against `scope`, then recurses: into each `FROM` item's own derived table with its own
    /// scope, and into every expression subquery (`EXISTS`, `IN (SELECT ...)`, scalar subquery)
    /// with `scope` still visible, since a correlated subquery may reference an outer alias.
    fn check_select(select: &Select, scope: &[String], sql: &str) {
        struct ColumnsResolve<'a> {
            scope: &'a [String],
            sql: &'a str,
            subqueries: usize,
        }

        impl Visitor for ColumnsResolve<'_> {
            type Break = ();

            // A nested query resolves its own columns in its own scope (`check_query` recurses
            // into it separately, with its own scope); only count the depth here so expressions
            // still inside it are skipped below, without aborting the rest of this traversal.
            fn pre_visit_query(&mut self, _: &Query) -> ControlFlow<()> {
                self.subqueries += 1;
                ControlFlow::Continue(())
            }

            fn post_visit_query(&mut self, _: &Query) -> ControlFlow<()> {
                self.subqueries -= 1;
                ControlFlow::Continue(())
            }

            fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
                if self.subqueries == 0
                    && let Expr::CompoundIdentifier(parts) = expr
                    && parts.len() > 1
                {
                    let prefix = &parts[parts.len() - 2].value;
                    assert!(
                        self.scope.iter().any(|name| name == prefix),
                        "{prefix} is out of scope ({:?} available): {}: {}",
                        self.scope,
                        expr,
                        self.sql
                    );
                }
                ControlFlow::Continue(())
            }
        }

        let mut resolver = ColumnsResolve {
            scope,
            sql,
            subqueries: 0,
        };
        select.projection.visit(&mut resolver).is_break();
        select.selection.visit(&mut resolver).is_break();
        select.group_by.visit(&mut resolver).is_break();
        select.having.visit(&mut resolver).is_break();

        for item in &select.from {
            check_table_factor(&item.relation, scope, sql);
            for join in &item.joins {
                check_table_factor(&join.relation, scope, sql);
            }
        }
        for subquery in gather_subqueries(select) {
            check_query(&subquery, scope, sql);
        }
    }

    fn check_table_factor(factor: &TableFactor, outer_scope: &[String], sql: &str) {
        if let TableFactor::Derived { subquery, .. } = factor {
            let mut inner_scope = Vec::new();
            if let SetExpr::Select(inner) = subquery.body.as_ref() {
                for item in &inner.from {
                    relations_in(item, &mut inner_scope, sql);
                }
            }
            // a wrapper may itself reference an outer correlated alias, so keep both in scope.
            let mut scope = inner_scope;
            scope.extend(outer_scope.iter().cloned());
            check_query(subquery, &scope, sql);
        }
    }

    fn check_query(query: &Query, scope: &[String], sql: &str) {
        match query.body.as_ref() {
            SetExpr::Select(select) => check_select(select, scope, sql),
            SetExpr::SetOperation { left, right, .. } => {
                for side in [left.as_ref(), right.as_ref()] {
                    let nested = Query {
                        with: None,
                        body: Box::new((*side).clone()),
                        order_by: None,
                        limit_clause: None,
                        fetch: None,
                        locks: Vec::new(),
                        for_clause: None,
                        settings: None,
                        format_clause: None,
                        pipe_operators: Vec::new(),
                    };
                    check_query(&nested, scope, sql);
                }
            }
            SetExpr::Query(inner) => check_query(inner, scope, sql),
            _ => {}
        }
        if let Some(with) = &query.with {
            for cte in &with.cte_tables {
                check_query(&cte.query, scope, sql);
            }
        }
    }

    /// The outermost `EXISTS`/`IN (SELECT ...)`/scalar subquery reached from each of `select`'s
    /// own clauses; a subquery nested inside one of these is reached again when `check_query`
    /// recurses into it, so it is skipped here rather than collected a second time.
    fn gather_subqueries(select: &Select) -> Vec<Query> {
        struct Subqueries {
            found: Vec<Query>,
            depth: usize,
        }

        impl Visitor for Subqueries {
            type Break = ();

            fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
                if self.depth == 0 {
                    self.found.push(query.clone());
                }
                self.depth += 1;
                ControlFlow::Continue(())
            }

            fn post_visit_query(&mut self, _: &Query) -> ControlFlow<()> {
                self.depth -= 1;
                ControlFlow::Continue(())
            }
        }

        let mut found = Subqueries {
            found: Vec::new(),
            depth: 0,
        };
        select.projection.visit(&mut found).is_break();
        select.selection.visit(&mut found).is_break();
        select.group_by.visit(&mut found).is_break();
        select.having.visit(&mut found).is_break();
        found.found
    }

    let parsed = Parser::parse_sql(&GenericDialect, sql).unwrap_or_else(|cause| panic!("{sql}: {cause}"));
    let [Statement::Query(query)] = parsed.as_slice() else {
        panic!("one query: {sql}")
    };
    let mut scope = Vec::new();
    if let SetExpr::Select(select) = query.body.as_ref() {
        for item in &select.from {
            relations_in(item, &mut scope, sql);
        }
    }
    check_query(query, &scope, sql);
}

#[test]
fn oracle_lob_top_n_with_a_filter_keeps_the_qualified_output_in_scope() {
    // github.com/telekom/sutura#1972's own repro: a WHERE predicate below the length/substr/instr
    // projection, ordered and limited, over a redacted CLOB with no explicit table alias.
    let tables = lob_table(r#"{"max_text_bytes":4000,"redact":[{"pattern":"secret","replace":"masked"}]}"#);
    let (reached, outputs) = source_outputs(
        &tables,
        r#"SELECT "LABEL", length("SCRIPT") AS n, substr("SCRIPT", 4001, 120), instr(upper("SCRIPT"), 'END') AS p FROM "TEXT_EVENTS" WHERE "LABEL" <> 'missing' ORDER BY n DESC LIMIT 3"#,
    );
    assert_every_derived_table_is_aliased(&reached);
    assert_eq!(outputs.len(), 4, "{reached}");

    // `assert_every_derived_table_is_aliased` already walks every clause, `WHERE` included, and
    // panics on a `CompoundIdentifier` whose qualifier is out of scope - exactly #1972's
    // ORA-00904. The rendered statement also carries Oracle-only syntax (`DBMS_LOB."SUBSTR"`,
    // `REGEXP_LIKE`, `CHR()`) that DataFusion's own SQL frontend cannot plan, so round-tripping it
    // through the fake engine the way the sibling tests do would fail on that, not on scoping; a
    // live Oracle venue is the only place this statement is actually executable.
}

#[test]
fn oracle_lob_control_expressions_carry_their_lineage() {
    let tables = lob_table(r#"{"max_text_bytes":4000}"#);
    // A condition, a filter or an ordering reads its operands, so a LOB there reaches the output's
    // lineage; only a control over non-LOB columns leaves a text value unbounded.
    for (sql, label, body) in [
        (
            r#"SELECT CASE WHEN length("LABEL") > 0 THEN "LABEL" ELSE "LABEL" END AS label, CASE WHEN length("NATIONAL") > 0 THEN upper("SCRIPT") ELSE "LABEL" END AS body FROM "TEXT_EVENTS""#,
            None,
            "1986",
        ),
        (
            r#"SELECT min("LABEL") FILTER (WHERE length("LABEL") > 0) AS label, min(substr("SCRIPT", 4001, 120)) FILTER (WHERE length("LABEL") > 0) AS body FROM "TEXT_EVENTS""#,
            None,
            "3986",
        ),
        (
            r#"SELECT first_value("LABEL" ORDER BY length("NATIONAL")) AS label, first_value(substr("SCRIPT", 4001, 120) ORDER BY length("LABEL")) AS body FROM "TEXT_EVENTS""#,
            Some("1986"),
            "3986",
        ),
        (
            r#"SELECT first_value("LABEL") OVER (PARTITION BY length("LABEL") ORDER BY length("LABEL")) AS label, first_value(substr("SCRIPT", 4001, 120)) OVER (PARTITION BY length("SCRIPT") ORDER BY length("LABEL")) AS body FROM "TEXT_EVENTS""#,
            None,
            "3986",
        ),
    ] {
        let (reached, outputs) = source_outputs(&tables, sql);
        let [label_output, body_output] = outputs.as_slice() else {
            panic!("two explicitly named outputs: {reached}")
        };
        match label {
            None => assert_eq!(
                label_output, r#""sutura_lob_output"."c0""#,
                "a control over non-LOB columns cannot clip LABEL: {sql}"
            ),
            Some(ceiling) => assert!(
                label_output.contains("DBMS_LOB.\"SUBSTR\"") && label_output.contains(ceiling),
                "a LOB control reaches the output's lineage: {sql}"
            ),
        }
        assert!(
            body_output.contains("DBMS_LOB.\"SUBSTR\""),
            "a LOB value branch stays bounded: {sql}"
        );
        assert!(body_output.contains(body), "the value's own ceiling: {sql}");
    }
}

#[test]
fn oracle_lob_bounds_follow_text_outputs_through_expressions_and_union() {
    let tables = lob_table(r#"{"max_text_bytes":4000,"redact":[{"pattern":"secret","replace":"masked"}]}"#);
    for (sql, bounded, national) in [
        (
            r#"SELECT "LABEL", length("SCRIPT") AS n, substr("SCRIPT", 4001, 120), instr(upper("SCRIPT"), 'END') AS p FROM "TEXT_EVENTS" ORDER BY n DESC LIMIT 3"#,
            vec![2],
            false,
        ),
        (
            r#"WITH t AS (SELECT "LABEL", upper("SCRIPT") AS body FROM "TEXT_EVENTS") SELECT "LABEL", body, length(body) AS n FROM t ORDER BY n DESC LIMIT 3"#,
            vec![1],
            false,
        ),
        (
            r#"SELECT "LABEL" AS body FROM "TEXT_EVENTS" UNION ALL SELECT upper("SCRIPT") AS body FROM "TEXT_EVENTS""#,
            vec![0],
            false,
        ),
        (
            r#"SELECT "SCRIPT" AS body FROM "TEXT_EVENTS" UNION ALL SELECT upper("NATIONAL") AS body FROM "TEXT_EVENTS""#,
            vec![0],
            true,
        ),
        (
            r#"SELECT "LABEL", min(substr("SCRIPT", 4001, 120)) AS body FROM "TEXT_EVENTS" GROUP BY "LABEL""#,
            vec![1],
            false,
        ),
    ] {
        let (reached, outputs) = source_outputs(&tables, sql);
        for (index, expression) in outputs.iter().enumerate() {
            if bounded.contains(&index) {
                assert!(expression.contains("DBMS_LOB.\"SUBSTR\""), "protected text at {index}: {sql}");
                assert!(
                    expression.contains(if national { "1986" } else { "3986" }),
                    "the output's own character family decides its ceiling: {reached}"
                );
            } else {
                assert_eq!(
                    expression,
                    &format!("\"sutura_lob_output\".\"c{index}\""),
                    "unrelated text and scalar results pass unchanged: {sql}"
                );
            }
        }
    }
}

#[test]
fn oracle_lob_bounds_hold_where_the_output_lineage_cannot_be_traced() {
    let tables = lob_table(r#"{"max_text_bytes":4000}"#);
    // Each output after the first is a value a naive trace misses: it must stay bounded, under the
    // ceiling of the widest LOB it was derived from, or the national one where that is unknown.
    for (sql, bounded, ceiling) in [
        (
            r#"SELECT "LABEL", "LABEL" || (SELECT min("SCRIPT") FROM "TEXT_EVENTS") AS v FROM "TEXT_EVENTS""#,
            vec![1],
            "3986",
        ),
        (
            r#"SELECT initcap(substr("SCRIPT", 1, 3000)) AS v FROM "TEXT_EVENTS" GROUP BY ROLLUP(initcap(substr("SCRIPT", 1, 3000)))"#,
            vec![0],
            "3986",
        ),
        (
            r#"SELECT initcap(substr("SCRIPT", 1, 3000)) AS v FROM "TEXT_EVENTS" GROUP BY GROUPING SETS ((initcap(substr("SCRIPT", 1, 3000))), ())"#,
            vec![0],
            "3986",
        ),
        (
            r#"SELECT initcap(substr("SCRIPT", 1, 3000)) AS v FROM "TEXT_EVENTS" GROUP BY CUBE(initcap(substr("SCRIPT", 1, 3000)))"#,
            vec![0],
            "3986",
        ),
        (
            r#"SELECT "LABEL", concat(chr(ascii(substr("SCRIPT", 4001, 1))), chr(ascii(substr("SCRIPT", 4002, 1)))) AS v FROM "TEXT_EVENTS""#,
            vec![1],
            "3986",
        ),
        (
            r#"SELECT "LABEL", concat(CASE WHEN substr("SCRIPT", 4001, 1) = 'a' THEN 'a' ELSE 'b' END, CASE WHEN substr("SCRIPT", 4002, 1) = 'a' THEN 'a' ELSE 'b' END) AS v FROM "TEXT_EVENTS""#,
            vec![1],
            "3986",
        ),
        (
            r#"WITH t AS (SELECT "LABEL", ascii(substr("NATIONAL", 4001, 1)) AS code FROM "TEXT_EVENTS") SELECT "LABEL", chr(code) AS v FROM t"#,
            vec![1],
            "1986",
        ),
    ] {
        let (reached, outputs) = source_outputs(&tables, sql);
        for (index, expression) in outputs.iter().enumerate() {
            if bounded.contains(&index) {
                assert!(
                    expression.contains("DBMS_LOB.\"SUBSTR\""),
                    "an untraced text output stays bounded: {reached}"
                );
                assert!(
                    expression.contains(ceiling),
                    "the output's lineage decides its ceiling: {reached}"
                );
            } else {
                assert_eq!(
                    expression,
                    &format!("\"sutura_lob_output\".\"c{index}\""),
                    "a plain non-LOB column passes unchanged: {reached}"
                );
            }
        }
    }
}

#[test]
fn a_field_that_claims_an_unbounded_number_but_derives_from_a_lob_stays_bounded() {
    use super::super::text::output_ceiling;

    let field = |marks: &[(&str, &str)]| {
        Field::new("v", DataType::Utf8, true).with_metadata(
            marks
                .iter()
                .map(|(key, value)| (String::from(*key), String::from(*value)))
                .collect(),
        )
    };
    let number = ("sutura:unbounded_number", "utf8");
    assert_eq!(
        output_ceiling(&field(&[number, ("sutura:oracle_lob", "clob")]), 4000, false),
        Some(4000)
    );
    assert_eq!(
        output_ceiling(&field(&[number, ("sutura:oracle_lob", "nclob")]), 4000, false),
        Some(2000)
    );
    assert_eq!(
        output_ceiling(&field(&[number]), 4000, true),
        Some(2000),
        "unknown lineage is bounded"
    );
    assert_eq!(
        output_ceiling(&field(&[number, ("sutura:oracle_lob_free", "true")]), 4000, true),
        None,
        "a proven-free native number is not clipped"
    );
}

#[test]
fn oracle_lob_output_contains_no_statement_delimiter() {
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;

    struct Captured(Vec<String>);
    impl StatementLegs for Captured {
        type Stop = SourceName;

        fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
            Ok(())
        }

        fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
            if !explicit_source_projection(statement.as_str()) {
                return Err(source.clone());
            }
            self.0.push(String::from(statement.as_str()));
            Err(source.clone())
        }
    }

    let schema = Schema::new(vec![
        Field::new("ROW_ID", DataType::Int64, false),
        Field::new("SCRIPT", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([
            (String::from("sutura:oracle_lob"), String::from("clob")),
            (String::from("sutura:text_policy"), String::from(r#"{"max_text_bytes":4000}"#)),
        ])),
    ]);
    let tables = [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("TEXT_EVENTS").unwrap(),
        TypeDialect::Oracle,
        Arc::new(schema),
    )];
    let statement = RawStatement::parse(r#"SELECT "SCRIPT", "ROW_ID" FROM "TEXT_EVENTS" ORDER BY "ROW_ID""#).unwrap();
    let mut legs = Captured(Vec::new());
    let answer = DataFusionCombiner::new().unwrap().answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(matches!(answer, Some(Err(StatementStopped::Leg(_)))), "{answer:?}");
    let [sql] = legs.0.as_slice() else {
        panic!("one bounded LOB projection must reach the port: {:?}", legs.0);
    };
    assert!(
        sql.contains("DBMS_LOB.\"SUBSTR\"") && sql.contains("...[truncated]"),
        "the emitted LOB output must be bounded: {sql}"
    );
    assert!(
        sql.contains("CHR(59)"),
        "the generated ASCII pattern must retain its semicolon without a delimiter: {sql}"
    );
    assert!(
        !sql.contains(';'),
        "the Oracle raw reader refuses a delimiter even inside the generated pattern: {sql}"
    );
    assert_eq!(
        Parser::parse_sql(&GenericDialect, sql)
            .expect("the generated pattern remains a SQL expression")
            .len(),
        1
    );
}

#[test]
fn oracle_native_numeric_extrema_stay_exact_beside_bounded_lob_outputs() {
    use datafusion::sql::sqlparser::ast::{SelectItem, SetExpr, Statement};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;

    struct Captured(Vec<String>);
    impl StatementLegs for Captured {
        type Stop = SourceName;

        fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
            Ok(())
        }

        fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
            if !explicit_source_projection(statement.as_str()) {
                return Err(source.clone());
            }
            self.0.push(String::from(statement.as_str()));
            Err(source.clone())
        }
    }

    let metadata = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(key, value)| (String::from(*key), String::from(*value)))
            .collect()
    };
    let schema = Schema::new(vec![
        Field::new("NATIVE_NUMBER", DataType::Utf8, false).with_metadata(metadata(&[("sutura:unbounded_number", "utf8")])),
        Field::new("SCRIPT", DataType::Utf8, true).with_metadata(metadata(&[
            ("sutura:oracle_lob", "clob"),
            (
                "sutura:text_policy",
                r#"{"max_text_bytes":4000,"redact":[{"pattern":"secret","replace":"***"}]}"#,
            ),
        ])),
        Field::new("SCRIPT_N", DataType::Utf8, true).with_metadata(metadata(&[
            ("sutura:oracle_lob", "nclob"),
            (
                "sutura:text_policy",
                r#"{"max_text_bytes":4000,"redact":[{"pattern":"secret","replace":"***"}]}"#,
            ),
        ])),
    ]);
    let tables = [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("TEXT_EVENTS").unwrap(),
        TypeDialect::Oracle,
        Arc::new(schema),
    )];
    for common in [
        r#"WITH numbers AS (SELECT MIN("NATIVE_NUMBER") AS lo, MAX("NATIVE_NUMBER") AS hi FROM "TEXT_EVENTS")"#,
        r#"WITH first_values AS (SELECT MIN("NATIVE_NUMBER") AS lo, MAX("NATIVE_NUMBER") AS hi FROM "TEXT_EVENTS"), numbers AS (SELECT MAX(lo) AS lo, MIN(hi) AS hi FROM first_values)"#,
    ] {
        // Output aliases deliberately exchange numeric and LOB names.
        let statement = RawStatement::parse(format!(
            r#"{common} SELECT numbers.lo AS "SCRIPT", numbers.hi AS "SCRIPT_N", events."SCRIPT" AS "NATIVE_NUMBER", events."SCRIPT_N" AS "windowed" FROM "TEXT_EVENTS" events CROSS JOIN numbers ORDER BY numbers.lo"#
        ))
        .unwrap();
        let mut legs = Captured(Vec::new());
        let answer = DataFusionCombiner::new().unwrap().answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(matches!(answer, Some(Err(StatementStopped::Leg(_)))), "{answer:?}");
        let [sql] = legs.0.as_slice() else {
            panic!("one native statement must reach the port: {:?}", legs.0);
        };
        let parsed = Parser::parse_sql(&GenericDialect, sql).unwrap();
        let [Statement::Query(query)] = parsed.as_slice() else {
            panic!("one generated query: {sql}");
        };
        let SetExpr::Select(select) = query.body.as_ref() else {
            panic!("the output is a projection: {sql}");
        };
        assert_eq!(select.projection.len(), 4, "{sql}");
        for (index, item) in select.projection.iter().enumerate() {
            let SelectItem::ExprWithAlias { expr, .. } = item else {
                panic!("output labels remain explicit: {sql}");
            };
            let expression = expr.to_string();
            if index < 2 {
                assert_eq!(
                    expression,
                    format!("\"sutura_lob_output\".\"c{index}\""),
                    "native NUMBER extrema must not become LOB expressions: {sql}"
                );
            } else {
                assert!(
                    expression.contains("DBMS_LOB.\"SUBSTR\""),
                    "LOB output must remain bounded: {sql}"
                );
                assert!(expression.contains("...[truncated]"), "LOB output retains its marker: {sql}");
                let payload = if index == 2 { "3986" } else { "1986" };
                assert!(expression.contains(payload), "LOB output retains its byte ceiling: {sql}");
            }
        }
        assert!(
            sql.contains("REGEXP_REPLACE"),
            "source redaction must remain before operators: {sql}"
        );
        assert!(!sql.contains(';'), "the native statement cannot contain a delimiter: {sql}");
    }
}

#[test]
fn a_text_policy_replaces_every_use_before_source_operators_and_bounds_utf8() {
    for dialect in [TypeDialect::DataFusion, TypeDialect::Cube] {
        let wide = "\u{e9}".repeat(100);
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("region", DataType::Utf8, true),
                Field::new("sales_area", DataType::Utf8, true),
            ])),
            vec![
                Arc::new(StringArray::from(vec![Some("a"), Some("b"), Some("c"), Some("d")])),
                Arc::new(StringArray::from(vec![
                    Some("IDENTIFIED BY secret"),
                    Some("IDENTIFIED BY another"),
                    Some(wide.as_str()),
                    None,
                ])),
            ],
        )
        .expect("rectangular source rows");
        let schema = |bytes| {
            Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("sales_area", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([(
                String::from("sutura:text_policy"),
                format!(
                    r#"{{"max_text_bytes":{bytes},"redact":[{{"pattern":"IDENTIFIED BY [^ ]+","replace":"IDENTIFIED BY ***"}}]}}"#
                ),
            )])),
        ]))
        };
        let table = |bytes| {
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region").expect("a table"),
                dialect,
                schema(bytes),
            )
        };
        let run = |sql: &str, bytes| {
            let mut legs = Engines::new(None);
            drop(legs.at.insert(named("geo"), holding(&[("dim_region", batch.clone())])));
            let tables = [table(bytes)];
            let statement = RawStatement::parse(sql).expect("a fixture question");
            let answer = DataFusionCombiner::new().expect("a combiner").answer_statement(
                StatementQuestion::new(
                    &statement,
                    &tables,
                    true,
                    UNBOUNDED,
                    opened(Duration::ZERO, Duration::from_secs(60)),
                ),
                &mut legs,
            );
            let Some(Ok(rows)) = answer else {
                panic!("{sql}: {answer:?}");
            };
            rows.into_parts().1
        };
        let masked = vec![vec![Value::Text(String::from("IDENTIFIED BY ***"))]];
        for sql in [
            "select sales_area from dim_region where region = 'a'",
            "select min(sales_area) from dim_region where region in ('a', 'b')",
            "select r.sales_area from dim_region r where r.sales_area = 'IDENTIFIED BY ***' order by r.sales_area, r.region limit 1",
            "select r.sales_area from dim_region r where r.region = 'a' and exists (select 1 from dim_region s where s.sales_area = r.sales_area and s.region = 'b')",
        ] {
            assert_eq!(run(sql, 128), masked, "{sql}");
        }
        for prefix in ["s", "se", "sec", "secr", "secre", "secret"] {
            assert!(
                run(
                    &format!("select region from dim_region where sales_area like '%IDENTIFIED BY {prefix}%'"),
                    128
                )
                .is_empty(),
                "{prefix}"
            );
        }
        assert_eq!(
            run("select sales_area from dim_region where region = 'c'", 32),
            vec![vec![Value::Text(String::from("\u{e9}\u{e9}\u{e9}\u{e9}...[truncated]"))]]
        );
        assert_eq!(
            run("select sales_area from dim_region where region = 'd'", 32),
            vec![vec![Value::Null]]
        );
    }
}

#[test]
fn a_federation_cannot_search_an_oracle_lob_after_a_bounded_leaf() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("region", DataType::Utf8, false),
        Field::new("sales_area", DataType::Utf8, false).with_metadata(std::collections::HashMap::from([
            (String::from("sutura:text_policy"), String::from(r#"{"max_text_bytes":4000}"#)),
            (String::from("sutura:oracle_lob"), String::from("clob")),
        ])),
    ]));
    let tables = [
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").unwrap(),
            TypeDialect::Oracle,
            schema,
        ),
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").unwrap(),
            TypeDialect::DataFusion,
            customers().schema(),
        ),
    ];
    let mut legs = Engines::new(Some("geo"));
    let statement = RawStatement::parse(
        r#"select c.customer_id from dim_customer c join "dim_region" r on c.region = r."region" where r."sales_area" like '%late-note%'"#,
    )
    .unwrap();
    let answer = DataFusionCombiner::new().unwrap().answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            true,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(
        legs.opened.is_empty(),
        "a local operator must not consume a clipped LOB: {answer:?}"
    );
    assert!(matches!(answer, Some(Err(StatementStopped::NotPlanned))), "{answer:?}");
    assert!(legs.ran.is_empty(), "no source should run: {:?}", legs.ran);
}
