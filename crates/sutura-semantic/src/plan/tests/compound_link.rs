//! A crossing relationship with more than one key links its legs on every one of them
//! (`telekom/sutura#1490`): one link part per key, the type check per part, before either leg runs.
//!
//! Built by hand, as every federated-splitter cell here is: the fixture is one crossing whose two keys
//! are a plain pair and a day truncated to a month.

use sutura_domain::catalog::{Column, ColumnType};
use sutura_domain::plan::{KeyValue, PlanColumn, PlanKey};

use super::*;

struct Compound {
    facts: Model,
    customers: Model,
    crossing: Relationship,
    metric: Metric,
}

fn typed(name: &str, declared: &str) -> Column {
    Column::new(
        column(name),
        Some(ColumnType::parse(declared).expect("a test column type is a column type")),
        Description::default(),
        None,
    )
}

impl Compound {
    /// `facts.customer_key` against `customers.customer_key` as a plain key, then `facts.day`
    /// truncated to a month against `customers.month`, the two declared as given.
    fn new(customer_key: (&str, &str), day: &str, month: &str) -> Self {
        let on = |name: &str, source: &str, columns: Vec<Column>| {
            Model::new(
                ModelName::parse(name).expect("a test model is a model"),
                SourceName::parse(source).expect("a test source is a source"),
                TableName::parse(format!("dim_{name}")).expect("a test table is a table"),
                columns,
                Description::default(),
            )
        };
        let crossing = Relationship::new(
            RelationshipName::parse("facts_customer").expect("a test relationship is a relationship"),
            ModelName::parse("facts").expect("a test model is a model"),
            ModelName::parse("customers").expect("a test model is a model"),
            JoinType::ManyToOne,
            JoinKeys::of(vec![
                JoinKey::Equal {
                    origin: column("customer_key"),
                    target: column("customer_key"),
                },
                JoinKey::TruncatedEqual {
                    origin: column("day"),
                    grain: Grain::Month,
                    target: column("month"),
                },
            ])
            .expect("two keys is a non-empty set"),
        );
        let metric = Metric::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            ModelName::parse("facts").expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("day"),
            BTreeSet::from([Grain::Month]),
            vec![declared("region", "region_code", &["facts_customer"])],
            None,
            Description::default(),
            Audience::Open,
        )
        .expect("one dimension is distinct");
        Self {
            facts: on(
                "facts",
                "local",
                vec![
                    Column::from(column("amount_cents")),
                    typed("customer_key", customer_key.0),
                    typed("day", day),
                ],
            ),
            customers: on(
                "customers",
                "elsewhere",
                vec![
                    typed("customer_key", customer_key.1),
                    typed("month", month),
                    Column::from(column("region_code")),
                ],
            ),
            crossing,
            metric,
        }
    }

    fn asking(&self) -> Resolution<'_> {
        Resolution {
            metric: &self.metric,
            metrics: vec![&self.metric],
            model: &self.facts,
            grain: Grain::Month,
            range: TimeRange::new(
                Date::parse("2026-06-01").expect("a test date is a date"),
                Date::parse("2026-07-01").expect("a test date is a date"),
            )
            .expect("June is a range"),
            keys: vec![ResolvedDimension {
                dimension: self
                    .metric
                    .dimension(&dimension_name("region"))
                    .expect("the metric declares this dimension"),
                join: Some(vec![ResolvedJoin {
                    relationship: &self.crossing,
                    model: &self.customers,
                }]),
            }],
            filters: Vec::new(),
            top: None,
            cross: None,
        }
    }
}

fn link(part: usize) -> String {
    InternalLabel::Link { leg: 0, part }.label()
}

/// **Every key is one link column on both legs.** The fact leg groups by the plain column and by the
/// day truncated to its month, under the part's own label; the lookup leg projects each target as
/// stored under the same two labels.
#[test]
fn a_compound_crossing_relationship_links_every_key_part() {
    let compound = Compound::new(("BIGINT", "BIGINT"), "DATE", "DATE");
    let Ok(Plan::Federated(planned)) = plan(&compound.asking()) else {
        panic!("a compound crossing federates");
    };
    let fact_keys = planned.fact().keys();
    assert_eq!(
        fact_keys.iter().map(PlanKey::label).collect::<Vec<_>>(),
        [link(0), link(1)],
        "the fact leg carries one link per key"
    );
    assert_eq!(
        read_from(fact_keys.first()),
        pair("dim_facts", "customer_key"),
        "part 0 is the plain column"
    );
    let day = PlanColumn::new(TableName::parse("dim_facts").expect("a test table is a table"), column("day"));
    assert_eq!(
        fact_keys.get(1).map(PlanKey::value),
        Some(KeyValue::Truncated {
            column: &day,
            grain: Grain::Month
        }),
        "part 1 is the day truncated to the grain"
    );
    let lookup = planned.lookups().legs().first().expect("one lookup leg").leg();
    assert_eq!(
        lookup.keys().iter().map(PlanKey::label).take(2).collect::<Vec<_>>(),
        [link(0), link(1)],
        "the lookup leg projects the same labels"
    );
    assert_eq!(read_from(lookup.keys().first()), pair("dim_customers", "customer_key"));
    assert_eq!(read_from(lookup.keys().get(1)), pair("dim_customers", "month"));
}

/// **A part of a kind that cannot join is refused whichever part it is**, before either leg runs: the
/// check reads every pair, not the first. The control declares both parts as they are stored.
#[test]
fn every_part_of_a_compound_crossing_key_is_type_checked() {
    let refused = |compound: &Compound| match plan(&compound.asking()) {
        Err(PlanError::Refused(reason)) => Some(reason.code()),
        Ok(_) => None,
        Err(other) => panic!("expected a refusal or a plan, got {other:?}"),
    };
    assert_eq!(refused(&Compound::new(("BIGINT", "BIGINT"), "DATE", "DATE")), None);
    assert_eq!(
        refused(&Compound::new(("BIGINT", "VARCHAR"), "DATE", "DATE")),
        Some("federation_link_type_mismatch"),
        "the first part is an integer against text"
    );
    assert_eq!(
        refused(&Compound::new(("BIGINT", "BIGINT"), "DATE", "VARCHAR")),
        Some("federation_link_type_mismatch"),
        "the second part is a truncated date against text"
    );
}
