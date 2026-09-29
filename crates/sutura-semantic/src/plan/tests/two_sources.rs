//! The two-source fixture the cross-model plan cells in [`super`] share: harness only, so every
//! assertion stays in `plan/tests.rs`, which the causality gate holds because it adds the tests.

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::Definitions;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::Query;

use super::*;
use crate::compile;

/// `facts` (local) and `customers` (remote) joined by `facts_customer`, for a hand-built resolution.
pub(super) struct TwoSources {
    facts: Model,
    customers: Model,
    link: Relationship,
}

impl TwoSources {
    pub(super) fn new() -> Self {
        Self {
            facts: model("facts", "local", &["amount_cents", "customer_key", "day"]),
            customers: model("customers", "remote", &["customer_key", "region_code"]),
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

    /// `metrics` asked by month over June, grouped by `region` when `by_region`.
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
            cross: None,
        }
    }
}

/// Where each part of [`two_facts`]'s catalog lives, and whether the second fact links to the
/// lookup - the three things the cross-model cells vary.
pub(super) struct Placed {
    pub(super) visits_on: &'static str,
    pub(super) calendar_on: &'static str,
    pub(super) visits_link: bool,
}

/// The two-fact catalog, ASSEMBLED rather than hand-built, so the load check, `resolve` and `plan`
/// all read it: `revenue_per_visit` on `facts` divides `facts.amount_cents` by a count of
/// `visits.visit_id`, both facts reach `calendar` and link to `customers` on `remote`, and the
/// metric declares `calendar` as its shared calendar. `channel` is a dimension on `facts` alone.
pub(super) fn two_facts(placed: &Placed) -> PinnedDefinitions {
    let models = vec![
        model(
            "facts",
            "local",
            &["amount_cents", "customer_key", "day_key", "day", "channel"],
        ),
        model("visits", placed.visits_on, &["visit_id", "customer_key", "day_key"]),
        model("calendar", placed.calendar_on, &["day_key", "day"]),
        model("customers", "remote", &["customer_key", "region_code"]),
    ];
    let mut relationships = vec![
        relationship("facts_customer", ("facts", "customer_key"), ("customers", "customer_key")),
        relationship("facts_calendar", ("facts", "day_key"), ("calendar", "day_key")),
        relationship("visits_calendar", ("visits", "day_key"), ("calendar", "day_key")),
    ];
    if placed.visits_link {
        relationships.push(relationship(
            "visits_customer",
            ("visits", "customer_key"),
            ("customers", "customer_key"),
        ));
    }
    let metric = Metric::new(
        MetricName::parse("revenue_per_visit").expect("a test metric is a metric"),
        ModelName::parse("facts").expect("a test model is a model"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Count,
                column("visit_id"),
                ModelName::parse("visits").expect("a test model is a model"),
            )),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        },
        Vec::new(),
        column("day"),
        BTreeSet::from([Grain::Month]),
        vec![
            declared("region", "region_code", &["facts_customer"]),
            Dimension::new(
                dimension_name("channel"),
                column("channel"),
                None,
                None,
                Description::default(),
            ),
        ],
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("these dimensions are distinct")
    .with_shared_calendar(ModelName::parse("calendar").expect("a test model is a model"));
    let definitions =
        Definitions::assemble(models, relationships, vec![metric]).expect("both facts reach the calendar they share");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("two-facts").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

/// `revenue_per_visit` by month over June, grouped by `by`, through `compile` as a served question is.
pub(super) fn ask(pinned: &PinnedDefinitions, by: &[&str]) -> Result<Compiled, CompileFailure> {
    compile(&by_month_over_june(by), &ScopedView::everything(pinned), RowCeiling::DEFAULT)
}

fn by_month_over_june(by: &[&str]) -> Query {
    Query::single(
        MetricName::parse("revenue_per_visit").expect("a test metric is a metric"),
        Grain::Month,
        TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range"),
        by.iter().map(|name| dimension_name(name)).collect(),
        Vec::new(),
    )
}

/// Two metrics: `revenue`, a plain sum that declares a calendar, and `revenue_per_customer`, a
/// cross-model ratio that declares none. The calendar that decides the refusal is the ratio's own;
/// reading the FIRST metric's planned the ratio as one statement over `facts` (review probe P2).
pub(super) fn revenue_beside_a_ratio(ratio_calendar: bool) -> (Metric, Metric) {
    let calendar = || ModelName::parse("calendar").expect("a test model is a model");
    let revenue = TwoSources::metric(
        "revenue",
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
    );
    let ratio = TwoSources::metric(
        "revenue_per_customer",
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Count,
                column("customer_key"),
                ModelName::parse("customers").expect("a test model is a model"),
            )),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        },
    );
    if ratio_calendar {
        (revenue, ratio.with_shared_calendar(calendar()))
    } else {
        (revenue.with_shared_calendar(calendar()), ratio)
    }
}

/// Both facts and the calendar on `local`, the second fact linked to `customers`: the one shape a
/// cross-model ratio plans for.
pub(super) const ONE_SOURCE: Placed = Placed {
    visits_on: "local",
    calendar_on: "local",
    visits_link: true,
};

pub(super) fn federated(answer: Result<Compiled, CompileFailure>) -> Box<sutura_domain::plan::FederatedPlan> {
    match answer {
        Ok(Compiled::Federated { plan }) => plan,
        other => panic!("a two-fact ratio grouped by a shared dimension is a federated plan, got {other:?}"),
    }
}

pub(super) fn refusal(answer: Result<Compiled, CompileFailure>) -> RefusalReason {
    match answer {
        Ok(Compiled::Refused { reason }) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}
