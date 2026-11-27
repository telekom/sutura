//! Identifier resolution is observed through the source statements and their returned rows.

use datafusion::arrow::array::ArrayRef;
use datafusion::common::TableReference;
use datafusion::datasource::memory::MemorySchemaProvider;

use super::*;

type NumericColumn<'a> = (&'a str, Vec<i64>);

fn numeric_oracle(columns: &[NumericColumn<'_>]) -> (Vec<StatementTable>, Engines) {
    let batch = RecordBatch::try_new(
        Arc::new(Schema::new(
            columns
                .iter()
                .map(|(name, _)| Field::new(*name, DataType::Int64, false))
                .collect::<Vec<_>>(),
        )),
        columns
            .iter()
            .map(|(_, values)| Arc::new(Int64Array::from(values.clone())) as ArrayRef)
            .collect(),
    )
    .expect("the numeric fixture is rectangular");
    let context = SessionContext::new();
    drop(
        context
            .register_batch(TableReference::bare("T"), batch.clone())
            .expect("the exact Oracle table registers"),
    );
    let mut legs = Engines::new(None);
    drop(legs.at.insert(named("geo"), context));
    (
        vec![StatementTable::new(
            named("geo"),
            QualifiedTable::parse("T").expect("an exact Oracle path"),
            TypeDialect::Oracle,
            batch.schema(),
        )],
        legs,
    )
}

fn over_oracle(sql: &str, tables: &[StatementTable], legs: &mut Engines) -> StatementAnswer<SourceName> {
    let statement = RawStatement::parse(sql).expect("a bounded query");
    DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            tables,
            true,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        legs,
    )
}

#[test]
fn a_failed_oracle_fold_never_binds_a_quoted_lowercase_column_in_any_scope() {
    for sql in [
        "select id from t",
        "select a.id from t a",
        "with q as (select \"id\" from t) select id from q",
        "select q.id from (select \"id\" from t) q",
        "select * from t l join t r using(id)",
    ] {
        let (tables, mut legs) = numeric_oracle(&[("id", vec![7])]);
        let answer = over_oracle(sql, &tables, &mut legs);
        assert!(
            matches!(answer, Some(Err(StatementStopped::UnknownColumn))),
            "{sql}: {answer:?}"
        );
        assert!(legs.opened.is_empty(), "{sql}");
        assert!(legs.ran.is_empty(), "{sql}");
    }
    for sql in [
        "select \"id\" from t",
        "select a.\"id\" from t a",
        "with q as (select \"id\" from t) select \"id\" from q",
        "select \"id\" from t l join t r using(\"id\")",
    ] {
        let (tables, mut legs) = numeric_oracle(&[("id", vec![7])]);
        let answer = over_oracle(sql, &tables, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{sql}: {answer:?}");
        };
        assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(7)]], "{sql}");
    }
}

#[test]
fn coexisting_oracle_cases_and_using_keys_bind_their_own_values() {
    for (sql, expected) in [
        ("select id from t order by id", [1, 2]),
        ("select \"id\" from t order by \"id\"", [7, 8]),
        ("select id from t l join t r using(id) order by id", [1, 2]),
        ("select \"id\" from t l join t r using(\"id\") order by \"id\"", [7, 8]),
    ] {
        let (tables, mut legs) = numeric_oracle(&[("ID", vec![1, 2]), ("id", vec![7, 8])]);
        let answer = over_oracle(sql, &tables, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{sql}: {answer:?}");
        };
        let expected = expected.map(|value| vec![sutura_domain::warehouse::Value::Integer(value)]);
        assert_eq!(rows.rows(), expected, "{sql}");
    }
}

#[test]
fn oracle_inputs_take_precedence_over_output_aliases_in_where_and_join_on() {
    for sql in [
        "select a as b from t where b > 10",
        "select l.a as b from t l join u r on b = r.a",
    ] {
        let (mut tables, mut legs) = numeric_oracle(&[("A", vec![1, 2]), ("B", vec![20, 5])]);
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("A", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![20, 30]))],
        )
        .expect("the other input is rectangular");
        drop(
            legs.at
                .get(&named("geo"))
                .expect("the Oracle fake")
                .register_batch(TableReference::bare("U"), batch.clone())
                .expect("the join input registers"),
        );
        tables.push(StatementTable::new(
            named("geo"),
            QualifiedTable::parse("U").expect("a fixture table"),
            TypeDialect::Oracle,
            batch.schema(),
        ));
        let answer = over_oracle(sql, &tables, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{sql}: {answer:?}");
        };
        assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(1)]], "{sql}");
    }
}

#[test]
fn oracle_output_aliases_take_precedence_in_having_and_qualify() {
    for sql in [
        "select a as b from t group by a, b having b > 10",
        "select sum(a) as b from t group by b having b > 10",
        "select a as b from t qualify row_number() over (order by a) > 0 and b > 10",
    ] {
        let (tables, mut legs) = numeric_oracle(&[("A", vec![1, 20]), ("B", vec![20, 1])]);
        let answer = over_oracle(sql, &tables, &mut legs);
        let Some(Ok(rows)) = answer else {
            panic!("{sql}: {answer:?}");
        };
        assert_eq!(rows.rows(), [vec![sutura_domain::warehouse::Value::Integer(20)]], "{sql}");
    }
}

fn oracle_inputs() -> (Vec<StatementTable>, Engines) {
    let original = regions();
    let batch = RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Field::new("REGION", DataType::Utf8, false),
            Field::new("SalesArea", DataType::Utf8, false),
        ])),
        original.columns().to_vec(),
    )
    .expect("the Oracle metadata preserves the two columns");
    let context = SessionContext::new();
    drop(
        context
            .register_batch(TableReference::bare("DIM_REGION"), batch.clone())
            .expect("the exact table registers"),
    );
    let mut legs = Engines::new(None);
    drop(legs.at.insert(named("geo"), context));
    let tables = vec![
        tables().remove(0),
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("DIM_REGION").expect("an exact Oracle path"),
            TypeDialect::Oracle,
            batch.schema(),
        ),
    ];
    (tables, legs)
}

#[test]
fn oracle_names_fold_in_their_scope_and_quoted_columns_and_aliases_stay_exact() {
    for (sql, expected) in [
        (
            "select region from dim_region order by region",
            "select \"REGION\" from \"DIM_REGION\" order by \"REGION\"",
        ),
        (
            "select r.region as \"Label\" from dim_region r order by \"Label\"",
            "select \"REGION\" as \"Label\" from \"DIM_REGION\" order by \"Label\"",
        ),
        (
            "select r.\"SalesArea\" from dim_region r order by r.\"SalesArea\"",
            "select \"SalesArea\" from \"DIM_REGION\" order by \"SalesArea\"",
        ),
        (
            "with r as (select region as code from dim_region) select code from r order by code",
            "select \"REGION\" as code from \"DIM_REGION\" order by code",
        ),
        (
            "select d.region from (select region from dim_region) d order by d.region",
            "select \"REGION\" from \"DIM_REGION\" order by \"REGION\"",
        ),
    ] {
        let (tables, mut legs) = oracle_inputs();
        let statement = RawStatement::parse(sql).expect("a bounded query");
        let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
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
        assert_eq!(
            rows,
            rows_of(&legs.runtime, legs.at.get(&named("geo")).expect("the Oracle fake"), expected),
            "{sql}"
        );
        assert_eq!(legs.ran, [named("geo")]);
    }
}

/// A lowercase source prefix is compared to the source's own id, never folded by the source's own
/// dialect - only the schema/table that follows it is. `geo`'s id stays `geo` even though Oracle
/// folds everything after it to uppercase.
#[test]
fn a_lowercase_source_prefix_is_not_oracle_folded() {
    let (tables, mut legs) = oracle_inputs();
    let answer = over_oracle("select region from geo.dim_region order by region", &tables, &mut legs);
    let Some(Ok(rows)) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(
        rows,
        rows_of(
            &legs.runtime,
            legs.at.get(&named("geo")).expect("the Oracle fake"),
            "select \"REGION\" from \"DIM_REGION\" order by \"REGION\"",
        )
    );
    assert_eq!(legs.ran, [named("geo")]);
}

#[test]
fn oracle_and_lowercase_sources_resolve_independently_in_one_join() {
    let (tables, mut legs) = oracle_inputs();
    let statement = RawStatement::parse(
        "select r.\"SalesArea\", count(*) as customers from dim_customer c join dim_region r on c.region = r.region \
         group by r.\"SalesArea\" order by r.\"SalesArea\"",
    )
    .expect("a bounded mixed-source query");
    let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
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
        panic!("both source cases must resolve: {answer:?}");
    };
    assert_eq!(
        rows.rows(),
        [
            vec![
                sutura_domain::warehouse::Value::Text(String::from("apac")),
                sutura_domain::warehouse::Value::Integer(1)
            ],
            vec![
                sutura_domain::warehouse::Value::Text(String::from("emea")),
                sutura_domain::warehouse::Value::Integer(3)
            ],
        ]
    );
    assert_eq!(legs.opened, [BTreeSet::from([named("crm"), named("geo")])]);
}

#[test]
fn oracle_qualifiers_fold_without_changing_the_physical_path() {
    let (mut tables, mut legs) = oracle_inputs();
    let context = SessionContext::new();
    drop(
        context
            .catalog("datafusion")
            .expect("the fake's default catalog")
            .register_schema("REPORTING", Arc::new(MemorySchemaProvider::new()))
            .expect("the exact schema registers"),
    );
    let batch =
        RecordBatch::try_new(Arc::clone(tables[1].schema()), regions().columns().to_vec()).expect("the same physical columns");
    drop(
        context
            .register_batch(TableReference::partial("REPORTING", "DIM_REGION"), batch)
            .expect("the exact table registers"),
    );
    drop(legs.at.insert(named("geo"), context));
    tables[1] = StatementTable::new(
        named("geo"),
        QualifiedTable::parse("REPORTING.DIM_REGION").expect("a qualified physical path"),
        TypeDialect::Oracle,
        Arc::clone(tables[1].schema()),
    );
    let statement = RawStatement::parse("select region from reporting.dim_region order by region").expect("a bounded query");
    let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
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
        panic!("the exact physical path resolves: {answer:?}");
    };
    assert_eq!(
        rows,
        rows_of(
            &legs.runtime,
            legs.at.get(&named("geo")).expect("the qualified fake"),
            "select \"REGION\" from \"REPORTING\".\"DIM_REGION\" order by \"REGION\""
        )
    );
    assert_eq!(legs.ran, [named("geo")]);
}

#[test]
fn quoted_oracle_names_do_not_fold_and_colliding_source_paths_are_absent() {
    for sql in [
        "select region from \"dim_region\"",
        "select salesarea from dim_region",
        "select \"region\" from dim_region",
    ] {
        let (tables, mut legs) = oracle_inputs();
        let statement = RawStatement::parse(sql).expect("a bounded query");
        let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        if sql.contains("\"dim_region\"") {
            assert!(matches!(answer, Some(Err(StatementStopped::UnknownTable))), "{answer:?}");
        } else {
            assert!(matches!(answer, Some(Err(StatementStopped::UnknownColumn))), "{answer:?}");
        }
        assert_eq!(legs.opened.len(), 0);
    }
    let (mut tables, mut legs) = oracle_inputs();
    tables.push(StatementTable::new(
        named("other"),
        tables[1].table().clone(),
        TypeDialect::Oracle,
        Arc::clone(tables[1].schema()),
    ));
    let statement = RawStatement::parse("select region from dim_region").expect("a bounded query");
    let answer = DataFusionCombiner::new().expect("the combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            true,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(matches!(answer, Some(Err(StatementStopped::UnknownTable))), "{answer:?}");
    assert_eq!(legs.opened.len(), 0);
}

/// Captures the statement handed to each source and stops immediately, never running it - the
/// fake engine's own name normalization has nothing to do with whether the rendered SQL carries
/// the raw condition, so this avoids it entirely rather than chasing its quoting rules.
struct CapturedOnlyLegs {
    statements: Vec<String>,
}

impl StatementLegs for CapturedOnlyLegs {
    type Stop = SourceName;

    fn open(&mut self, _sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        Ok(())
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.statements.push(String::from(statement.as_str()));
        Err(source.clone())
    }
}

/// Walks every `Select` reached and collects each one whose `FROM` names `table` directly - the
/// redaction view's own base-table level(s), beneath whatever the unparser wrapped around it.
struct BaseTableSelectsOnly<'a> {
    table: &'a str,
    stack: Vec<datafusion::sql::sqlparser::ast::Select>,
    found: Vec<datafusion::sql::sqlparser::ast::Select>,
}

impl datafusion::sql::sqlparser::ast::Visitor for BaseTableSelectsOnly<'_> {
    type Break = ();

    fn pre_visit_select(&mut self, select: &datafusion::sql::sqlparser::ast::Select) -> std::ops::ControlFlow<Self::Break> {
        self.stack.push(select.clone());
        std::ops::ControlFlow::Continue(())
    }

    fn post_visit_select(&mut self, _select: &datafusion::sql::sqlparser::ast::Select) -> std::ops::ControlFlow<Self::Break> {
        self.stack.pop();
        std::ops::ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(
        &mut self,
        factor: &datafusion::sql::sqlparser::ast::TableFactor,
    ) -> std::ops::ControlFlow<Self::Break> {
        if let datafusion::sql::sqlparser::ast::TableFactor::Table { name, .. } = factor
            && name.to_string().trim_matches('"').eq_ignore_ascii_case(self.table)
            && let Some(enclosing) = self.stack.last()
        {
            self.found.push(enclosing.clone());
        }
        std::ops::ControlFlow::Continue(())
    }
}

/// #2038: a native dictionary CLOB with a bare raw-prefilter rule must still have its raw LIKE
/// condition pushed to the Oracle source, not only the redacted one, ahead of masking. Kept small
/// and in this pre-existing module (not `tests/oracle_prefilter.rs`, which base cannot compile) so
/// causality always has one cell of this fix that can run, and fail, against the unfixed tree.
#[test]
fn raw_prefilter_reaches_the_oracle_source_for_a_native_dictionary_clob() {
    use datafusion::sql::sqlparser::ast::{SelectItem, Visit as _};

    let policy = serde_json::json!({
        "max_text_bytes": 128,
        "redact": [{"pattern": "secret_[^ ]+", "replace": "[masked]"}],
        "raw_prefilter": {"acknowledged_because": "#2038 reproduction: a native dictionary CLOB, bare column rule"},
    });
    let metadata = std::collections::HashMap::from([
        (String::from("sutura:text_policy"), policy.to_string()),
        (String::from("sutura:oracle_lob"), String::from("clob")),
    ]);
    let schema = Arc::new(Schema::new(vec![
        Field::new("SCRIPT", DataType::Utf8, true).with_metadata(metadata),
        Field::new("OBJECT_NAME", DataType::Utf8, true),
    ]));
    let tables = vec![StatementTable::new(
        named("geo"),
        QualifiedTable::parse("DOCS").expect("a fixture table"),
        TypeDialect::Oracle,
        schema,
    )];
    let mut legs = CapturedOnlyLegs { statements: Vec::new() };
    let statement =
        RawStatement::parse("SELECT count(OBJECT_NAME) FROM DOCS WHERE SCRIPT LIKE '%MRC%'").expect("a bounded query");
    drop(DataFusionCombiner::new().expect("a combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    ));
    let reached = legs.statements.first().expect("the Oracle source was reached");
    let parsed =
        datafusion::sql::sqlparser::parser::Parser::parse_sql(&datafusion::sql::sqlparser::dialect::GenericDialect, reached)
            .expect("the reached statement reparses");
    let [parsed] = parsed.as_slice() else {
        panic!("expected one statement: {reached}");
    };
    let mut finder = BaseTableSelectsOnly {
        table: "DOCS",
        stack: Vec::new(),
        found: Vec::new(),
    };
    let _never_breaks = parsed.visit(&mut finder);
    let [base] = finder.found.as_slice() else {
        panic!(
            "expected exactly one SELECT naming DOCS directly: {reached} ({:?})",
            finder.found
        );
    };
    let selection = base
        .selection
        .as_ref()
        .unwrap_or_else(|| panic!("the base-table level must carry the raw condition: {reached}"));
    let raw = selection.to_string().to_uppercase();
    assert!(raw.contains("LIKE '%MRC%'"), "{reached} (base selection: {raw})");
    assert!(
        !raw.contains("REGEXP_REPLACE"),
        "the base-table condition must test the stored value, never the redacted one: {reached}"
    );
    let script = base
        .projection
        .iter()
        .find(|item| matches!(item, SelectItem::ExprWithAlias { alias, .. } if alias.value.eq_ignore_ascii_case("SCRIPT")))
        .unwrap_or_else(|| panic!("SCRIPT must still be projected, read or not: {reached}"));
    assert!(
        script.to_string().to_uppercase().contains("REGEXP_REPLACE"),
        "SCRIPT's own projection must still redact it: {reached}"
    );
}
