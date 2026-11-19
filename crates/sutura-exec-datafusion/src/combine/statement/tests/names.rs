//! A name that holds a backslash or a quote character is refused before any source is opened.

use std::collections::HashMap;

use super::literals::DIALECTS;
use super::*;

/// Every open and every statement a source would have been sent; no leg answers.
#[derive(Default)]
struct Recorded {
    opened: usize,
    statements: Vec<String>,
}

impl StatementLegs for Recorded {
    type Stop = SourceName;

    fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.opened += 1;
        Ok(())
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.statements.push(String::from(statement.as_str()));
        Err(source.clone())
    }
}

fn answered(statement: &str, tables: &[StatementTable], legs: &mut Recorded) -> StatementAnswer<SourceName> {
    DataFusionCombiner::new().expect("a combiner builds").answer_statement(
        StatementQuestion::new(
            &RawStatement::parse(statement).expect("a bounded fixture query"),
            tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        legs,
    )
}

fn regions_with(dialect: TypeDialect, fields: Vec<Field>) -> [StatementTable; 1] {
    [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("dim_region").expect("a fixture table"),
        dialect,
        Arc::new(Schema::new(fields)),
    )]
}

fn text(name: &str) -> Field {
    Field::new(name, DataType::Utf8, false)
}

#[test]
fn a_caller_alias_holding_a_backslash_or_a_quote_is_refused_in_every_dialect() {
    for dialect in DIALECTS {
        let tables = regions_with(dialect, vec![text("region"), text("sales_area")]);
        for statement in [
            r#"select region as "tail\" from dim_region"#,
            r#"select region as "it's" from dim_region"#,
            r#"select region as "a""b" from dim_region"#,
            r#"select region as "a`b" from dim_region"#,
        ] {
            let mut legs = Recorded::default();
            let answer = answered(statement, &tables, &mut legs);
            assert!(
                matches!(answer, Some(Err(StatementStopped::UnsupportedSyntax(SqlConstruct::Query)))),
                "{dialect:?}: {statement}: {answer:?}"
            );
            assert_eq!(legs.opened, 0, "{dialect:?}: {statement}");
        }
    }
}

#[test]
fn a_column_name_holding_a_quote_is_refused_where_it_would_be_written() {
    for dialect in DIALECTS {
        for (name, written) in [("its", true), ("it's", false), (r"tail\", false)] {
            let tables = regions_with(dialect, vec![text("region"), text(name)]);
            let mut legs = Recorded::default();
            let answer = answered(r#"select * from "dim_region""#, &tables, &mut legs);
            if written {
                assert!(
                    matches!(answer, Some(Err(StatementStopped::Leg(_)))),
                    "{dialect:?}: {name}: {answer:?}"
                );
                assert_eq!(legs.statements.len(), 1, "{dialect:?}: {name}");
            } else {
                assert!(
                    matches!(answer, Some(Err(StatementStopped::UnsupportedSyntax(SqlConstruct::Query)))),
                    "{dialect:?}: {name}: {answer:?}"
                );
                assert_eq!(legs.opened, 0, "{dialect:?}: {name}");
                assert!(legs.statements.is_empty(), "{dialect:?}: {name}");
            }
        }
    }
}

#[test]
fn a_native_oracle_statement_writing_a_column_name_holding_a_quote_is_refused() {
    let number = text("sales_area").with_metadata(HashMap::from([(
        String::from("sutura:unbounded_number"),
        String::from("utf8"),
    )]));
    for (statement, name, written) in [
        (r#"select * from "dim_region" where "sales_area" > 9"#, "its", true),
        (r#"select * from "dim_region" where "sales_area" > 9"#, "it's", false),
        (r#"select min("sales_area") from "dim_region""#, "it's", true),
    ] {
        let tables = regions_with(TypeDialect::Oracle, vec![text("region"), number.clone(), text(name)]);
        let mut legs = Recorded::default();
        let answer = answered(statement, &tables, &mut legs);
        if written {
            assert!(
                matches!(answer, Some(Err(StatementStopped::Leg(_)))),
                "{statement}: {name}: {answer:?}"
            );
            assert_eq!(legs.statements.len(), 1, "{statement}: {name}");
        } else {
            assert!(
                matches!(answer, Some(Err(StatementStopped::UnsupportedSyntax(SqlConstruct::Query)))),
                "{statement}: {name}: {answer:?}"
            );
            assert_eq!(legs.opened, 0, "{statement}: {name}");
        }
    }
}

#[test]
fn a_text_policy_view_over_an_unread_column_name_holding_a_quote_is_refused() {
    let protected = text("sales_area").with_metadata(HashMap::from([(
        String::from("sutura:text_policy"),
        String::from(r#"{"max_text_bytes":128}"#),
    )]));
    for dialect in DIALECTS {
        for (name, written) in [("its", true), ("it's", false)] {
            let tables = regions_with(dialect, vec![text("region"), protected.clone(), text(name)]);
            let mut legs = Recorded::default();
            let answer = answered(r#"select "sales_area" from "dim_region""#, &tables, &mut legs);
            if written {
                assert!(
                    matches!(answer, Some(Err(StatementStopped::Leg(_)))),
                    "{dialect:?}: {name}: {answer:?}"
                );
                assert!(!legs.statements.is_empty(), "{dialect:?}: {name}");
            } else {
                assert!(
                    matches!(answer, Some(Err(StatementStopped::UnsupportedSyntax(SqlConstruct::Query)))),
                    "{dialect:?}: {name}: {answer:?}"
                );
                assert_eq!(legs.opened, 0, "{dialect:?}: {name}");
            }
        }
    }
}

#[test]
fn a_native_oracle_text_policy_view_over_a_column_name_holding_a_quote_is_refused() {
    let number = text("sales_area").with_metadata(HashMap::from([(
        String::from("sutura:unbounded_number"),
        String::from("utf8"),
    )]));
    for (name, written) in [("its", true), ("it's", false)] {
        let protected = text(name).with_metadata(HashMap::from([(
            String::from("sutura:text_policy"),
            String::from(r#"{"max_text_bytes":128}"#),
        )]));
        let tables = regions_with(TypeDialect::Oracle, vec![text("region"), number.clone(), protected]);
        let mut legs = Recorded::default();
        let answer = answered(r#"select min("sales_area") from "dim_region""#, &tables, &mut legs);
        if written {
            assert!(matches!(answer, Some(Err(StatementStopped::Leg(_)))), "{name}: {answer:?}");
            assert_eq!(legs.statements.len(), 1, "{name}");
        } else {
            assert!(
                matches!(answer, Some(Err(StatementStopped::UnsupportedSyntax(SqlConstruct::Query)))),
                "{name}: {answer:?}"
            );
            assert_eq!(legs.opened, 0, "{name}");
        }
    }
}
