//! The federated cases with more than one lookup leg, built here rather than read from a `.case`
//! file: a file names one lookup table, and these name the leg each lookup leg joins onto.
//!
//! Each runs for every combiner the federated pack binds, against the corpus's own rows.

use sutura_domain::federation::Federation;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, MetricName, TableName};
use sutura_domain::plan::leg::{LegPlan, LegTerm};
use sutura_domain::plan::{
    AnswerKey, FederatedPlan, InternalLabel, LookupLeg, LookupLegs, PlanBindings, PlanColumn, PlanKey, PlanTerm, ResultLabel,
    StatementTables, labels,
};
use sutura_domain::warehouse::{RowSet, Value};

use super::{FederatedCase, LOOKUP_TABLE, REGION_CODES_TABLE, bucket, lookup_source, range, range_bindings, source, table};

fn on(table_name: &str, column: &str) -> PlanColumn {
    PlanColumn::new(
        TableName::parse(table_name).expect("a corpus name is a name"),
        ColumnName::parse(column).expect("a corpus name is a name"),
    )
}

fn link(leg: usize, table_name: &str, column: &str) -> PlanKey {
    PlanKey::new(ResultLabel::internal(InternalLabel::link(leg)), on(table_name, column))
}

fn dimension(name: &str) -> ResultLabel {
    ResultLabel::dimension(&DimensionName::parse(name).expect("a corpus name is a name"))
}

fn lookup(source: sutura_domain::model::SourceName, table_name: &str, keys: Vec<PlanKey>) -> LegPlan {
    LegPlan::Lookup {
        source,
        metric: MetricName::parse("amount_total").expect("a corpus name is a name"),
        table: TableName::parse(table_name).expect("a corpus name is a name").into(),
        keys,
        bindings: PlanBindings::none(),
    }
}

/// One expected answer row: the key (empty for null), the day and the cents.
type Row<'a> = (&'a str, &'a str, i64);

/// `amount_total` by `key`, its fact leg projecting `links` over `region`.
fn case(name: &'static str, links: &[usize], lookups: Vec<LookupLeg>, key: AnswerKey, rows: &[Row<'_>]) -> FederatedCase {
    let metric = MetricName::parse("amount_total").expect("a corpus name is a name");
    let federation = Federation::of(&Measure::Simple(Term::Aggregate(AggregatedColumn::new(
        Aggregate::Sum,
        ColumnName::parse("amount_cents").expect("a corpus name is a name"),
    ))));
    let terms = labels(&federation)
        .into_iter()
        .map(|label| {
            LegTerm::new(
                PlanTerm::Aggregate {
                    aggregate: Aggregate::Sum,
                    column: on(super::TABLE, "amount_cents"),
                },
                ResultLabel::internal(label),
            )
        })
        .collect();
    let fact = LegPlan::Fact {
        source: source(),
        metric: metric.clone(),
        tables: StatementTables::only(table()),
        bucket: bucket(),
        keys: links.iter().map(|&leg| link(leg, super::TABLE, "region")).collect(),
        terms,
        bindings: range_bindings(),
        range: range(),
    };
    let measure_label = ResultLabel::measure(&metric);
    let plan = FederatedPlan::new(
        metric,
        measure_label,
        bucket(),
        fact,
        None,
        LookupLegs::parse(lookups).expect("a corpus case's lookup legs form a tree"),
        federation,
        vec![key],
    )
    .expect("a corpus fan-in case is a plan");
    let mut columns: Vec<String> = plan.keys().iter().map(|key| String::from(key.label())).collect();
    columns.push(String::from(plan.bucket_label()));
    columns.push(String::from(plan.measure_label()));
    let rows = rows
        .iter()
        .map(|&(key, day, cents)| {
            let key = if key.is_empty() {
                Value::Null
            } else {
                Value::Text(key.into())
            };
            vec![key, Value::Text(day.into()), Value::Integer(cents)]
        })
        .collect();
    FederatedCase {
        name,
        plan,
        expected: RowSet::new(columns, rows).expect("a corpus answer is well formed"),
    }
}

/// The two cases.
///
/// - Three lookup legs onto the fact, all on the lookup source: the second is INNER and drops the
///   event with no region, the other two are LEFT.
/// - A chain that crosses to the lookup source and comes back: `conformance_regions` on the lookup
///   source names each region, and `conformance_region_codes` on the fact's own source codes each
///   name. The second leg joins onto the first on `region_name`, which the fact leg does not hold.
pub(super) fn cases() -> Vec<FederatedCase> {
    let by_name = [
        ("Eastern", "2026-01-01", 350),
        ("Northern", "2026-01-01", 400),
        ("Eastern", "2026-01-02", 150),
        ("Northern", "2026-01-02", 1300),
    ];
    let three = case(
        "fan-in-three-lookup-legs-onto-the-fact",
        &[0, 1, 2],
        vec![
            LookupLeg::onto_fact(
                lookup(
                    lookup_source(),
                    LOOKUP_TABLE,
                    vec![
                        link(0, LOOKUP_TABLE, "region"),
                        PlanKey::new(dimension("region_name"), on(LOOKUP_TABLE, "region_name")),
                    ],
                ),
                true,
            ),
            LookupLeg::onto_fact(
                lookup(lookup_source(), LOOKUP_TABLE, vec![link(1, LOOKUP_TABLE, "region")]),
                false,
            ),
            LookupLeg::onto_fact(
                lookup(lookup_source(), LOOKUP_TABLE, vec![link(2, LOOKUP_TABLE, "region")]),
                true,
            ),
        ],
        AnswerKey::lookup(0, dimension("region_name")),
        &by_name,
    );
    let by_code = [
        ("E", "2026-01-01", 350),
        ("N", "2026-01-01", 400),
        ("", "2026-01-01", 50),
        ("E", "2026-01-02", 150),
        ("N", "2026-01-02", 1300),
    ];
    let back = case(
        "fan-in-a-chain-that-crosses-and-comes-back",
        &[0],
        vec![
            LookupLeg::onto_fact(
                lookup(
                    lookup_source(),
                    LOOKUP_TABLE,
                    vec![link(0, LOOKUP_TABLE, "region"), link(1, LOOKUP_TABLE, "region_name")],
                ),
                true,
            ),
            LookupLeg::onto_lookup(
                lookup(
                    source(),
                    REGION_CODES_TABLE,
                    vec![
                        link(1, REGION_CODES_TABLE, "region_name"),
                        PlanKey::new(dimension("region_code"), on(REGION_CODES_TABLE, "region_code")),
                    ],
                ),
                0,
                true,
            ),
        ],
        AnswerKey::lookup(1, dimension("region_code")),
        &by_code,
    );
    vec![three, back]
}
