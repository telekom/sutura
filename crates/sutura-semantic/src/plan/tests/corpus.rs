//! The three-hop corpus the chain cells in [`super`] share: harness only, so every assertion stays in
//! `plan/tests.rs`, which the causality gate holds because it adds the tests.

use super::*;

/// `facts -> customers -> regions` beside `facts -> products -> brands`, with the three models the
/// chains end on placed on the sources given.
///
/// Two chains that share NO hop, which is what the order cell needs: two chains sharing hop 1
/// dedup to the same list in either order, so a corpus built that way passes with the sort
/// removed - measured, and it is why `family` is reached through a relationship of its own.
///
/// `region_code` is hop 2's origin column and sits on `customers` only: `dim_facts` does not
/// declare it, which is what turns the hop-qualification defect into a binder error rather than
/// a silent regrouping in this venue.
pub(super) struct Corpus {
    facts: Model,
    /// Every declared hop beside the model on its far side, which is what a resolution holds.
    pub(super) reached: Vec<(Relationship, Model)>,
    held: Metric,
}

impl Corpus {
    pub(super) fn with_customers_on(customers: &str) -> Self {
        Self::placed(customers, "local", "local")
    }

    pub(super) fn placed(customers: &str, regions: &str, brands: &str) -> Self {
        Self {
            facts: model("facts", "local", &["amount_cents", "day", "customer_key", "product_key"]),
            reached: vec![
                (
                    relationship("facts_customer", ("facts", "customer_key"), ("customers", "customer_key")),
                    model("customers", customers, &["customer_key", "region_code"]),
                ),
                (
                    relationship("customers_region", ("customers", "region_code"), ("regions", "code")),
                    model("regions", regions, &["code", "label"]),
                ),
                (
                    relationship("facts_product", ("facts", "product_key"), ("products", "product_key")),
                    model("products", "local", &["product_key", "family", "brand_key"]),
                ),
                (
                    relationship("product_brand", ("products", "brand_key"), ("brands", "brand_key")),
                    model("brands", brands, &["brand_key", "name"]),
                ),
            ],
            held: Metric::new(
                MetricName::parse("revenue").expect("a test metric is a metric"),
                ModelName::parse("facts").expect("a test model is a model"),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
                Vec::new(),
                column("day"),
                BTreeSet::from([Grain::Month]),
                vec![
                    declared("region", "label", &["facts_customer", "customers_region"]),
                    declared("family", "family", &["facts_product"]),
                    declared("brand", "name", &["facts_product", "product_brand"]),
                ],
                None,
                Description::default(),
                Audience::Open,
            )
            .expect("these dimensions are distinct"),
        }
    }

    /// The dimension named, with each hop of its declared chain resolved by name.
    pub(super) fn key(&self, name: &str) -> ResolvedDimension<'_> {
        let held = self
            .held
            .dimension(&dimension_name(name))
            .expect("the metric declares this dimension");
        ResolvedDimension {
            dimension: held,
            join: Some(
                held.via()
                    .unwrap_or_default()
                    .iter()
                    .map(|hop| {
                        let (relationship, model) = self
                            .reached
                            .iter()
                            .find(|(declared_hop, _)| declared_hop.name() == hop)
                            .expect("the corpus declares this hop");
                        ResolvedJoin { relationship, model }
                    })
                    .collect(),
            ),
        }
    }

    pub(super) fn asking<'a>(&'a self, keys: Vec<ResolvedDimension<'a>>) -> Resolution<'a> {
        Resolution {
            metric: &self.held,
            metrics: vec![&self.held],
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

/// The federated plan of a question grouped by `by`, over a corpus that puts a chain on a second system.
pub(super) fn federated_by(corpus: &Corpus, by: &str) -> Box<sutura_domain::plan::FederatedPlan> {
    match plan(&corpus.asking(vec![corpus.key(by)])).expect("this resolution plans") {
        Plan::Federated(planned) => planned,
        Plan::Mono(_) => panic!("a dimension on another data system is a federated plan"),
    }
}

/// Where a key reads from: its table and its column.
pub(super) fn read_from(key: Option<&sutura_domain::plan::PlanKey>) -> (String, String) {
    let column = key.expect("the leg carries this key").column();
    (column.table().to_string(), column.column().to_string())
}

/// Each join's `ON` as its first key's origin table and target table.
pub(super) fn join_tables(leg: &LegPlan) -> Vec<(String, String)> {
    leg.tables()
        .joins()
        .iter()
        .map(|join| {
            let first = join.keys().first();
            (first.origin().table().to_string(), first.target().table().to_string())
        })
        .collect()
}

pub(super) fn pair(table: &str, column: &str) -> (String, String) {
    (String::from(table), String::from(column))
}
