//! Opus round 7 on github.com/telekom/sutura#2047 (item 2): #1943's own shape is a genuine
//! two-table JOIN - two DIFFERENT relations, not one relation self-joined under two aliases -
//! whose join key is projected from BOTH sides, each under its own distinct alias (`a.region`,
//! `r.region AS r_region`), with a `LIMIT` and no `ORDER BY` forcing the join itself into a
//! derived table. Unlike the self-join cells in `text_self_join.rs`, the wrapped plan's own
//! schema here carries no bare-name collision at all - the original query already gave the two
//! occurrences of the join key distinct output names, so `plan.schema()`'s field names differ and
//! `derive_with_dialect_alias`'s collision guard never fires. This is exactly the answer main
//! gives and the self-join refusal must never remove it.
//!
//! One representative shape, run across every dialect this branch's unparser targets. `DuckDb`,
//! `DataFusion` and Oracle (via `OracleLegs`) are checked against the fake engine for real; the
//! fake engine's own SQL frontend is `DataFusion`'s, which does not accept `Postgres`/`ClickHouse`/
//! `BigQuery`/Cube's own quoting, so those four are checked with a permissive leg that always
//! answers `Ok` regardless of the text it is handed - proving only that nothing refused the
//! statement before it reached a leg, not that a live engine of that dialect would accept the
//! rendered text. That gap is #1909's own scope, not this branch's.

use super::oracle::OracleLegs;
use super::*;

/// The join-key-from-both-sides shape: two distinct tables, the shared key aliased differently on
/// each side, `LIMIT` and no `ORDER BY` forcing a derived table.
const JOIN_KEY_BOTH_SIDES: &str = "select c.region, r.region as r_region, r.sales_area \
                                    from dim_customer c join dim_region r on c.region = r.region \
                                    limit 10";

/// A leg that never actually runs `statement`: it always answers an empty result. Used only for
/// dialects the fake engine's `DataFusion` SQL frontend cannot parse back (`Postgres`,
/// `ClickHouse`, `BigQuery`, Cube's `Postgres`-flavoured text) - it proves the statement reached a
/// leg at all (not refused), nothing about whether a live engine of that dialect would run it.
struct AlwaysAnswers;

impl StatementLegs for AlwaysAnswers {
    type Stop = SourceName;

    fn open(&mut self, _sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        Ok(())
    }

    fn run(&mut self, _source: &SourceName, _statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        Ok(RawRows::of(Vec::new(), Vec::new()))
    }
}

#[test]
fn a_join_key_projected_from_both_sides_still_answers_in_every_dialect() {
    let statement = RawStatement::parse(JOIN_KEY_BOTH_SIDES).expect("a fixture question");

    // DuckDB (crm) joined with Postgres (geo), federated - `tables()`'s own split.
    let mut legs = Engines::new(None);
    let answer = DataFusionCombiner::new().expect("a combiner").answer_statement(
        StatementQuestion::new(
            &statement,
            &tables(),
            true,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(
        matches!(answer, Some(Ok(_))),
        "a non-self two-table join with the key aliased distinctly on each side must still \
         answer (federated DuckDb+Postgres): {answer:?}"
    );

    // A single DataFusion source holding both tables.
    let single_source = |dialect: TypeDialect| {
        vec![
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_customer").expect("a test table is a table"),
                dialect,
                customers().schema(),
            ),
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region").expect("a test table is a table"),
                dialect,
                regions().schema(),
            ),
        ]
    };
    let mut legs = Engines::new(None);
    drop(legs.at.insert(
        named("geo"),
        holding(&[("dim_customer", customers()), ("dim_region", regions())]),
    ));
    let answer = DataFusionCombiner::new().expect("a combiner").answer_statement(
        StatementQuestion::new(
            &statement,
            &single_source(TypeDialect::DataFusion),
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(
        matches!(answer, Some(Ok(_))),
        "a non-self two-table join with the key aliased distinctly on each side must still \
         answer (DataFusion): {answer:?}"
    );

    // Oracle: executed for real through `oracle_fetch_to_limit`, not just captured as text.
    let mut legs = OracleLegs {
        engines: Engines::new(None),
        statements: Vec::new(),
    };
    drop(legs.engines.at.insert(
        named("geo"),
        holding(&[("dim_customer", customers()), ("dim_region", regions())]),
    ));
    let answer = DataFusionCombiner::new().expect("a combiner").answer_statement(
        StatementQuestion::new(
            &statement,
            &single_source(TypeDialect::Oracle),
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(
        matches!(answer, Some(Ok(_))),
        "a non-self two-table join with the key aliased distinctly on each side must still \
         answer (Oracle, no ORA-00904/ORA-00918): {answer:?}; source SQL: {:?}",
        legs.statements
    );
    assert!(
        !legs.statements.is_empty(),
        "an answered statement must reach the Oracle leg: {:?}",
        legs.statements
    );

    // Postgres, ClickHouse, BigQuery and Cube: not refused before reaching a leg (#1909 tracks
    // actually running these dialects' own rendered text against a real or fake engine of that
    // dialect).
    for dialect in [
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Cube,
    ] {
        let mut legs = AlwaysAnswers;
        let answer = DataFusionCombiner::new().expect("a combiner").answer_statement(
            StatementQuestion::new(
                &statement,
                &single_source(dialect),
                false,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(
            matches!(answer, Some(Ok(_))),
            "{dialect:?}: a non-self two-table join with the key aliased distinctly on each side \
             must not be refused: {answer:?}"
        );
    }
}
