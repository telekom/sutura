//! The planner's Oracle leg, executed by a fake source engine; actual Oracle belongs to its venue.

use super::*;

struct OracleLegs {
    engines: Engines,
    statements: Vec<String>,
}

impl StatementLegs for OracleLegs {
    type Stop = SourceName;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.engines.open(sources)
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        if *source == named("geo") {
            self.statements.push(String::from(statement.as_str()));
            // ponytail: translate only this fixture's limit; the live venue executes Oracle spelling.
            let text = statement
                .as_str()
                .replace("OFFSET 1 ROWS FETCH FIRST 1 ROWS ONLY", "LIMIT 1 OFFSET 1");
            let statement = RawStatement::parse(&text).map_err(|_cause| source.clone())?;
            return self.engines.run(source, &statement);
        }
        self.engines.run(source, statement)
    }
}

#[test]
fn an_oracle_part_preserves_quoted_aliases_and_fetch_limits_alone_and_in_a_federation() {
    let tables = vec![
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").expect("a fixture table"),
            TypeDialect::DuckDb,
            customers().schema(),
        ),
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").expect("a fixture table"),
            TypeDialect::Oracle,
            regions().schema(),
        ),
    ];
    for sql in ["select r.region from dim_region r order by r.region limit 1 offset 1", AREAS] {
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        let statement = RawStatement::parse(sql).expect("a bounded fixture query");
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
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
            panic!("the Oracle part must run: {answer:?}");
        };
        assert_eq!(rows, legs.engines.as_one(sql));
        assert_eq!(legs.statements.len(), 1);
        let text = &legs.statements[0];
        assert!(!text.contains("LIMIT"), "{text}");
        assert!(!text.contains("AS \"r\""), "a table alias has no AS: {text}");
        if sql.contains("offset") {
            assert!(text.contains("OFFSET 1 ROWS FETCH FIRST 1 ROWS ONLY"), "{text}");
        } else {
            assert!(
                text.contains("AS \"c0\""),
                "the local combine needs the quoted column label: {text}"
            );
        }
    }
}
