//! A link key is one of its metric's crossing hops, and a lookup leg is rooted where one of its
//! metric's chains crosses: read on a bundle with relationships no dimension crosses.
//!
//! `sales.orders` holds `revenue`, whose one dimension crosses into `crm.customers` and on to
//! `crm.regions`. `orders_rep` crosses into `hr.employees` and `customers_orders` crosses back into
//! `orders`, and no dimension uses either.

use super::*;

fn bundle() -> PinnedDefinitions {
    let models = vec![
        model(
            "orders",
            "sales",
            &["id", "amount_cents", "order_date", "status", "customer_id", "rep_id"],
        ),
        model("customers", "crm", &["id", "region_id"]),
        model("regions", "crm", &["id", "region_name"]),
        model("employees", "hr", &["id", "rep_name"]),
    ];
    let relationships = vec![
        relationship("orders_customer", "orders", "customers", "customer_id", "id"),
        relationship("customers_region", "customers", "regions", "region_id", "id"),
        relationship("orders_rep", "orders", "employees", "rep_id", "id"),
        Relationship::new(
            relationship_name("customers_orders"),
            model_name("customers"),
            model_name("orders"),
            JoinType::OneToMany,
            JoinKeys::of(vec![JoinKey::Equal {
                origin: column("id"),
                target: column("customer_id"),
            }])
            .expect("one key"),
        ),
    ];
    let region = Dimension::new(
        DimensionName::parse("region_name").expect("a test dimension is a dimension"),
        column("region_name"),
        Some(
            ViaChain::of(vec![
                relationship_name("orders_customer"),
                relationship_name("customers_region"),
            ])
            .expect("a chain has a hop"),
        ),
        Some(BTreeSet::from(
            ["EMEA", "APAC"].map(|value| DimensionValue::parse(value).expect("a test value is a value")),
        )),
        Description::default(),
    );
    let revenue = Metric::new(
        metric_name("revenue"),
        model_name("orders"),
        simple("amount_cents"),
        vec![RequiredFilter::Equals {
            column: column("status"),
            value: DimensionValue::parse("paid").expect("a test value is a value"),
        }],
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        vec![region],
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("one dimension cannot collide");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        Definitions::assemble(models, relationships, vec![revenue]).expect("the test bundle is consistent"),
        Knowledge::none(),
        ContributionManifest::single(source("sales"), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the test definitions hash")
}

fn link(at: usize, of_table: &str, of_column: &str) -> PlanKey {
    PlanKey::new(ResultLabel::internal(InternalLabel::link(at)), col(of_table, of_column))
}

/// `revenue`'s fact leg, its required filter and its range, with `keys`.
fn fact_leg(pinned: &PinnedDefinitions, keys: Vec<PlanKey>) -> (Value, Vec<ParamValue>) {
    let [from, until] = in_range(col("orders", "order_date"));
    let paid = PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::Equals {
            column: col("orders", "status"),
            param: 2,
        },
    );
    let mut params = dated();
    params.push(ParamValue::Text(String::from("paid")));
    let leg = LegPlan::Fact {
        source: source("sales"),
        metric: metric_name("revenue"),
        tables: StatementTables::parse(orders_path(), Vec::new()).expect("one table"),
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, col("orders", "order_date")),
        keys,
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: col("orders", "amount_cents"),
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::parse(vec![from, until, paid], params.clone()).expect("coherent"),
        range: range(),
    };
    (sent(&leg, pinned), params)
}

/// A lookup leg of `revenue` on `source_name`, rooted on `root`, with `keys` and no filter.
fn lookup_leg(pinned: &PinnedDefinitions, (source_name, root): (&str, &str), keys: Vec<PlanKey>) -> Value {
    let leg = LegPlan::Lookup {
        source: source(source_name),
        metric: metric_name("revenue"),
        table: StatementTables::parse(table(root), Vec::new()).expect("one table"),
        keys,
        bindings: PlanBindings::none(),
    };
    sent(&leg, pinned)
}

#[test]
fn the_planners_legs_are_accepted_beside_relationships_no_dimension_crosses() {
    let pinned = bundle();
    let (json, params) = fact_leg(&pinned, vec![link(0, "orders", "customer_id")]);
    let fact = parsed(json, &pinned, params);
    assert!(fact.is_ok(), "{fact:?}");
    let json = lookup_leg(&pinned, ("crm", "customers"), vec![link(0, "customers", "id")]);
    let lookup = parsed(json, &pinned, Vec::new());
    assert!(lookup.is_ok(), "{lookup:?}");
}

#[test]
fn a_fact_leg_links_only_on_a_crossing_one_of_its_metrics_chains_makes() {
    let pinned = bundle();
    let (json, params) = fact_leg(&pinned, vec![link(0, "orders", "rep_id")]);
    let refused = parsed(json, &pinned, params);
    assert!(
        matches!(refused, Err(LegNotCertified::LinkNotDeclared { ref column, .. }) if column.as_str() == "rep_id"),
        "{refused:?}"
    );
}

#[test]
fn a_lookup_leg_is_not_rooted_on_the_fact_table_a_relationship_crosses_back_into() {
    let pinned = bundle();
    let json = lookup_leg(&pinned, ("sales", "orders"), vec![link(0, "orders", "customer_id")]);
    let refused = parsed(json, &pinned, Vec::new());
    assert!(
        matches!(refused, Err(LegNotCertified::TableNotCertified { ref table, .. }) if table.to_string() == "orders"),
        "{refused:?}"
    );
}

#[test]
fn a_lookup_leg_on_the_fact_table_with_a_second_crossing_key_is_refused_at_its_root() {
    let pinned = bundle();
    let keys = vec![link(0, "orders", "customer_id"), link(1, "orders", "rep_id")];
    let json = lookup_leg(&pinned, ("sales", "orders"), keys);
    let refused = parsed(json, &pinned, Vec::new());
    assert!(
        matches!(refused, Err(LegNotCertified::TableNotCertified { ref table, .. }) if table.to_string() == "orders"),
        "{refused:?}"
    );
}

#[test]
fn a_lookup_leg_is_not_rooted_where_no_dimension_crosses() {
    let pinned = bundle();
    let json = lookup_leg(&pinned, ("hr", "employees"), vec![link(0, "employees", "id")]);
    let refused = parsed(json, &pinned, Vec::new());
    assert!(
        matches!(refused, Err(LegNotCertified::TableNotCertified { ref table, .. }) if table.to_string() == "employees"),
        "{refused:?}"
    );
}
