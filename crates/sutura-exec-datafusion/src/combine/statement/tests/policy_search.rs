//! What a part that reads a column under a text policy reports to its leg, and what a local part past
//! the deadline reports when it did.

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

/// Runs every leg at `engines` and notes what each was told about the policy.
struct Reading<'a> {
    engines: &'a mut Engines,
    told: Vec<PolicyRead>,
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
    ) -> Result<RawRows, Self::Stop> {
        self.told.push(policy);
        self.engines.run(source, statement)
    }
}

fn answer(
    sql: &str,
    table: StatementTable,
    deadline: Deadline,
    legs: &mut impl StatementLegs<Stop = SourceName>,
) -> StatementAnswer<SourceName> {
    let statement = RawStatement::parse(sql).expect("a test statement is a statement");
    let tables = [table];
    DataFusionCombiner::new()
        .expect("a combiner builds")
        .answer_statement(StatementQuestion::new(&statement, &tables, false, UNBOUNDED, deadline), legs)
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
            region_table(dialect, true),
            opened(Duration::ZERO, Duration::from_secs(60)),
            &mut legs,
        );
        assert!(matches!(answered, Some(Ok(_))), "{dialect:?}: {sql}: {answered:?}");
        assert_eq!(legs.told, [expected], "{dialect:?}: {sql}");
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
        region_table(TypeDialect::Cube, false),
        opened(Duration::ZERO, Duration::from_secs(60)),
        &mut legs,
    );
    assert!(matches!(answered, Some(Ok(_))), "{answered:?}");
    assert_eq!(legs.told, [PolicyRead::Unread]);
}

/// Twenty local self-joins of a three-row table, which Cube's joins keep local: a count no budget of
/// two seconds can reach the end of. Past the deadline the stop names the policy only where a policy
/// column was read.
#[test]
fn a_local_part_past_the_deadline_names_the_policy_only_when_it_read_one() {
    let aliases = || (0..20).map(|alias| format!("r{alias}"));
    let sql = format!(
        "select max({}) from {}",
        aliases()
            .map(|alias| format!("{alias}.sales_area"))
            .collect::<Vec<_>>()
            .join(" || "),
        aliases()
            .map(|alias| format!("dim_region {alias}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    for protected in [true, false] {
        let mut legs = Engines::new(None);
        let started = Instant::now();
        let answered = answer(
            &sql,
            region_table(TypeDialect::Cube, protected),
            opened(Duration::ZERO, Duration::from_secs(2)),
            &mut legs,
        );
        match (protected, &answered) {
            (true, Some(Err(StatementStopped::PolicySearchDeadlineExceeded)))
            | (false, Some(Err(StatementStopped::DeadlineExceeded))) => {}
            _ => panic!("protected={protected}: {answered:?}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "protected={protected} stopped long after the deadline: {:?}",
            started.elapsed()
        );
        assert!(!legs.ran.is_empty(), "the legs ran before the local part did");
    }
}
