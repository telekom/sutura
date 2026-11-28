//! The numeric Oracle path's labelled exit over a table another source also holds - split out of
//! `oracle.rs` (which declares this as a submodule) so neither file crosses the line-count gate.

use super::super::*;

/// An Oracle leg answering the numeric path's labelled statement the way a live Oracle does: one
/// row under the `c0` label, so the output's names come from the planned schema, not from the leg.
struct Labelled(Vec<String>);

impl StatementLegs for Labelled {
    type Stop = SourceName;

    fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        Ok(())
    }

    fn run(&mut self, _: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        self.0.push(statement.as_str().to_owned());
        Ok(RawRows::of(
            vec![String::from("c0")],
            vec![vec![sutura_domain::warehouse::Value::Text(String::from("9"))]],
        ))
    }
}

/// The numeric path's labelled exit names its output from the first plan's schema. Over a table
/// whose physical path another source also holds, that plan reads the per-source registration, so
/// an unaliased computed column's name carries it until the answer's sweep restores the physical
/// path. `note` is an unread LOB column: it puts `geo`'s table on the labelled branch.
#[test]
fn the_numeric_labelled_exit_names_a_computed_column_over_a_colliding_table_physically() {
    let tables = [
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").expect("table"),
            TypeDialect::Oracle,
            Arc::new(Schema::new(vec![
                Field::new("sales_area", DataType::Utf8, false).with_metadata(std::collections::HashMap::from([(
                    String::from("sutura:unbounded_number"),
                    String::from("utf8"),
                )])),
                Field::new("note", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([(
                    String::from("sutura:oracle_lob"),
                    String::from("clob"),
                )])),
            ])),
        ),
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_region").expect("table"),
            TypeDialect::DuckDb,
            Arc::new(Schema::new(vec![Field::new("region", DataType::Utf8, false)])),
        ),
    ];
    let statement = RawStatement::parse(r#"select min("sales_area") from geo."dim_region""#).expect("a bounded query");
    let mut legs = Labelled(Vec::new());
    let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    let Some(Ok(rows)) = answer else {
        panic!("{answer:?}: {:?}", legs.0);
    };
    let [reached] = legs.0.as_slice() else {
        panic!("one native statement: {:?}", legs.0);
    };
    assert!(reached.contains("sutura_lob_values"), "the labelled branch: {reached}");
    let [column] = rows.columns() else {
        panic!("one output column: {:?}", rows.columns());
    };
    assert!(
        !column.contains('\u{1}'),
        "the internal registration reached the caller: {column}"
    );
    assert!(column.contains("dim_region"), "named from the physical path: {column}");
}
