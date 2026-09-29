//! The two-source fixture the cross-model plan cells in [`super`] share: harness only, so every
//! assertion stays in `plan/tests.rs`, which the causality gate holds because it adds the tests.

use super::*;

/// `facts` (local) and `customers` (remote) joined by `facts_customer`, plus a local `calendar`.
pub(super) struct TwoSources {
    facts: Model,
    customers: Model,
    calendar: Model,
    link: Relationship,
}

impl TwoSources {
    pub(super) fn new() -> Self {
        Self {
            facts: model("facts", "local", &["amount_cents", "customer_key", "day"]),
            customers: model("customers", "remote", &["customer_key", "region_code"]),
            calendar: model("calendar", "local", &["day"]),
            link: relationship("facts_customer", ("facts", "customer_key"), ("customers", "customer_key")),
        }
    }

    /// A metric on `facts` with a `region` dimension reached on `customers`.
    pub(super) fn metric(name: &str, measure: Measure) -> Metric {
        Metric::new(
            MetricName::parse(name).expect("a test metric is a metric"),
            ModelName::parse("facts").expect("a test model is a model"),
            measure,
            Vec::new(),
            column("day"),
            BTreeSet::from([Grain::Month]),
            vec![declared("region", "region_code", &["facts_customer"])],
            None,
            Description::default(),
            Audience::Open,
        )
        .expect("no dimensions to duplicate")
    }

    /// `metrics` asked by month over June, grouped by `region` when `by_region`. The second fact
    /// and the calendar are read off the first metric, as `resolve` reads them.
    pub(super) fn asking<'a>(&'a self, metrics: Vec<&'a Metric>, by_region: bool) -> Resolution<'a> {
        let first = metrics.first().copied().expect("a question names a metric");
        let keys = if by_region {
            vec![ResolvedDimension {
                dimension: first
                    .dimension(&dimension_name("region"))
                    .expect("the metric declares this dimension"),
                join: Some(vec![ResolvedJoin {
                    relationship: &self.link,
                    model: &self.customers,
                }]),
            }]
        } else {
            Vec::new()
        };
        Resolution {
            metric: first,
            metrics,
            model: &self.facts,
            grain: Grain::Month,
            range: TimeRange::new(
                Date::parse("2026-06-01").expect("a test date is a date"),
                Date::parse("2026-07-01").expect("a test date is a date"),
            )
            .expect("June is a range"),
            keys,
            filters: Vec::new(),
            top: None,
            second_fact_model: None,
            calendar_model: first.shared_calendar().map(|_| &self.calendar),
        }
    }
}
