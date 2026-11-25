//! What a part that reads a column under a text policy reports to its leg.

use sutura_domain::plan::PolicyRead;

use super::*;

/// `dim_region` at `geo`, `sales_area` under a text policy when `protected`.
fn region_table(dialect: TypeDialect, protected: bool) -> StatementTable {
    let sales_area = Field::new("sales_area", DataType::Utf8, false);
    let sales_area = if protected {
        sales_area.with_metadata(std::collections::HashMap::from([(
            String::from("sutura:text_policy"),
            String::from(r#"{"max_text_bytes":128,"redact":[{"pattern":"emea","replace":"***"}]}"#),
        )]))
    } else {
        sales_area
    };
    StatementTable::new(
        named("geo"),
        QualifiedTable::parse("dim_region").expect("a table"),
        dialect,
        Arc::new(Schema::new(vec![Field::new("region", DataType::Utf8, false), sales_area])),
    )
}

/// Customers at `crm`, a `DuckDB` source, beside `dim_region` as `region_table` has it.
fn customers_and(region: StatementTable) -> Vec<StatementTable> {
    vec![
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").expect("a table"),
            TypeDialect::DuckDb,
            customers().schema(),
        ),
        region,
    ]
}

/// Runs every leg at `engines` and notes what each was told about the policy.
struct Reading<'a> {
    engines: &'a mut Engines,
    told: Vec<(SourceName, PolicyRead)>,
}

/// Notes what each leg was told, and stops at the first.
struct Noting {
    told: Vec<(SourceName, PolicyRead)>,
}

impl StatementLegs for Noting {
    type Stop = SourceName;

    fn open(&mut self, _sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        Ok(())
    }

    fn run(&mut self, source: &SourceName, _statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        Err(source.clone())
    }

    fn run_planned(
        &mut self,
        source: &SourceName,
        _statement: &RawStatement,
        _tables: &BTreeSet<QualifiedTable>,
        policy: PolicyRead,
        _ineligible: Option<sutura_domain::catalog::PrefilterIneligible>,
    ) -> Result<RawRows, Self::Stop> {
        self.told.push((source.clone(), policy));
        Err(source.clone())
    }
}

impl StatementLegs for Reading<'_> {
    type Stop = SourceName;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.engines.open(sources)
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.engines.run(source, statement)
    }

    fn run_planned(
        &mut self,
        source: &SourceName,
        statement: &RawStatement,
        _tables: &BTreeSet<QualifiedTable>,
        policy: PolicyRead,
        _ineligible: Option<sutura_domain::catalog::PrefilterIneligible>,
    ) -> Result<RawRows, Self::Stop> {
        self.told.push((source.clone(), policy));
        self.engines.run(source, statement)
    }
}

fn answer(
    sql: &str,
    tables: &[StatementTable],
    deadline: Deadline,
    legs: &mut impl StatementLegs<Stop = SourceName>,
) -> StatementAnswer<SourceName> {
    let statement = RawStatement::parse(sql).expect("a test statement is a statement");
    DataFusionCombiner::new()
        .expect("a combiner builds")
        .answer_statement(StatementQuestion::new(&statement, tables, true, UNBOUNDED, deadline), legs)
}

#[test]
fn a_leg_is_told_whether_it_reads_a_policy_column_and_where_the_policy_runs() {
    let search = "select region from dim_region where sales_area like '%apac%'";
    for (dialect, sql, expected) in [
        (TypeDialect::Cube, search, PolicyRead::Locally),
        (
            TypeDialect::Cube,
            "select region, sales_area from dim_region",
            PolicyRead::Locally,
        ),
        (TypeDialect::DataFusion, search, PolicyRead::AtSource),
        (TypeDialect::Postgres, search, PolicyRead::AtSource),
        (TypeDialect::Cube, "select region from dim_region", PolicyRead::Unread),
        (TypeDialect::DataFusion, "select region from dim_region", PolicyRead::Unread),
    ] {
        let mut engines = Engines::new(None);
        let mut legs = Reading {
            engines: &mut engines,
            told: Vec::new(),
        };
        let answered = answer(
            sql,
            &[region_table(dialect, true)],
            opened(Duration::ZERO, Duration::from_secs(60)),
            &mut legs,
        );
        assert!(matches!(answered, Some(Ok(_))), "{dialect:?}: {sql}: {answered:?}");
        assert_eq!(legs.told, [(named("geo"), expected)], "{dialect:?}: {sql}");
    }
}

#[test]
fn a_table_with_no_policy_column_tells_its_leg_nothing_was_read() {
    let mut engines = Engines::new(None);
    let mut legs = Reading {
        engines: &mut engines,
        told: Vec::new(),
    };
    let answered = answer(
        "select region from dim_region where sales_area like '%apac%'",
        &[region_table(TypeDialect::Cube, false)],
        opened(Duration::ZERO, Duration::from_secs(60)),
        &mut legs,
    );
    assert!(matches!(answered, Some(Ok(_))), "{answered:?}");
    assert_eq!(legs.told, [(named("geo"), PolicyRead::Unread)]);
}

/// Twenty local self-joins of a three-row table, which Cube's joins keep local: a count no budget of
/// two seconds can reach the end of.
const SELF_JOINS: usize = 20;

fn self_join() -> String {
    let aliases = || (0..SELF_JOINS).map(|alias| format!("r{alias}"));
    format!(
        "select max({}) from {}",
        aliases()
            .map(|alias| format!("{alias}.sales_area"))
            .collect::<Vec<_>>()
            .join(" || "),
        aliases()
            .map(|alias| format!("dim_region {alias}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Twelve pairs of a customer and a region table cross joined over two sources, as the existing
/// deadline cell has it: a local join no two seconds can finish.
fn federated_cross_join() -> String {
    let pairs = 0..12;
    format!(
        "select sum({}), max({}) from {}",
        pairs
            .clone()
            .map(|pair| format!("c{pair}.customer_id"))
            .collect::<Vec<_>>()
            .join(" + "),
        pairs
            .clone()
            .map(|pair| format!("r{pair}.sales_area"))
            .collect::<Vec<_>>()
            .join(" || "),
        pairs
            .map(|pair| format!("dim_customer c{pair}, dim_region r{pair}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn late(sql: &str, tables: &[StatementTable]) -> StatementAnswer<SourceName> {
    let mut legs = Engines::new(None);
    let started = Instant::now();
    let answered = answer(sql, tables, opened(Duration::ZERO, Duration::from_secs(2)), &mut legs);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "stopped long after the deadline: {:?}",
        started.elapsed()
    );
    assert!(!legs.ran.is_empty(), "the legs ran before the local part did");
    answered
}

/// No local-part stop names the policy: at Cube a filter on another column does not reach the source,
/// and where the policy ran at its source the local join is not what the policy made slow.
#[test]
fn a_local_part_past_the_deadline_stays_generic_whether_or_not_it_read_a_policy_column() {
    for (sql, tables) in [
        (self_join(), vec![region_table(TypeDialect::Cube, true)]),
        (self_join(), vec![region_table(TypeDialect::Cube, false)]),
        (federated_cross_join(), customers_and(region_table(TypeDialect::Cube, true))),
        (
            federated_cross_join(),
            customers_and(region_table(TypeDialect::Postgres, true)),
        ),
    ] {
        let answered = late(&sql, &tables);
        assert!(
            matches!(answered, Some(Err(StatementStopped::DeadlineExceeded))),
            "{answered:?}"
        );
    }
}

#[test]
fn each_leg_of_a_federation_is_told_about_its_own_part_only() {
    let mut engines = Engines::new(None);
    let mut legs = Reading {
        engines: &mut engines,
        told: Vec::new(),
    };
    let sql = "select r.sales_area, count(*) as customers from dim_customer c join dim_region r on c.region = r.region \
               group by r.sales_area";
    let answered = answer(
        sql,
        &customers_and(region_table(TypeDialect::Postgres, true)),
        opened(Duration::ZERO, Duration::from_secs(60)),
        &mut legs,
    );
    assert!(matches!(answered, Some(Ok(_))), "{answered:?}");
    legs.told.sort_by(|first, second| first.0.cmp(&second.0));
    assert_eq!(
        legs.told,
        [(named("crm"), PolicyRead::Unread), (named("geo"), PolicyRead::AtSource)]
    );
}

#[test]
fn the_native_oracle_number_path_tells_its_leg_whether_it_reads_a_policy_column() {
    let metadata = |key: &str, value: &str| std::collections::HashMap::from([(String::from(key), String::from(value))]);
    let schema = Arc::new(Schema::new(vec![
        Field::new("region", DataType::Utf8, false).with_metadata(metadata(
            "sutura:text_policy",
            r#"{"max_text_bytes":128,"redact":[{"pattern":"e","replace":"*"}]}"#,
        )),
        Field::new("sales_area", DataType::Utf8, false).with_metadata(metadata("sutura:unbounded_number", "utf8")),
    ]));
    let tables = [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("dim_region").expect("a table"),
        TypeDialect::Oracle,
        schema,
    )];
    for (sql, expected) in [
        (
            r#"select min("sales_area") from "dim_region" where "region" like 'e%'"#,
            PolicyRead::AtSource,
        ),
        (r#"select min("sales_area") from "dim_region""#, PolicyRead::Unread),
    ] {
        let mut legs = Noting { told: Vec::new() };
        let answered = answer(sql, &tables, opened(Duration::ZERO, Duration::from_secs(60)), &mut legs);
        assert!(matches!(answered, Some(Err(StatementStopped::Leg(_)))), "{sql}: {answered:?}");
        assert_eq!(legs.told, [(named("geo"), expected)], "{sql}");
    }
}
