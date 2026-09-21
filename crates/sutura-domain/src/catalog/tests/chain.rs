//! The declared chain: a `via` of several relationships walked in order.
//!
//! A concept module rather than a split of [`super`], which was over
//! `cargo xtask max-lines`'s thousand-line cap. Everything a chain case needs is here - the
//! three-model fixture, its metric, and every cell that walks or refuses a chain - so the
//! boundary is the concept rather than a line count, and nothing here reaches outward.

use super::*;

/// `orders` → `customers` → `regions`, all on `local`: the fixture every multi-hop case walks.
///
/// `customers_regions` starts at `customers` - the target of hop 1 - so the two relationships join
/// up and the chain is a single path.
fn three_models() -> ModelsAndJoins {
    let (mut models, mut relationships) = two_models();
    models.push(model("regions", "local", &["code", "label"]));
    models[1] = model("customers", "local", &["id", "region_code"]);
    relationships.push(Relationship::new(
        relationship_name("customers_regions"),
        model_name("customers"),
        column("region_code"),
        model_name("regions"),
        column("code"),
        JoinType::ManyToOne,
    ));
    (models, relationships)
}

fn chain_metric() -> Metric {
    metric(
        "revenue",
        vec![dimension(
            "region",
            "code",
            Some(&["orders_customer", "customers_regions"]),
            Some(&["north"]),
        )],
    )
}

#[test]
fn a_dimension_through_a_chain_assembles_and_resolves_on_the_last_hops_model() {
    let (models, relationships) = three_models();
    let definitions = Definitions::assemble(models, relationships, vec![chain_metric()]).expect("a joined-up chain assembles");
    // The column is read on the LAST hop's target, not on `customers`: `code` is a column of
    // `regions`, and only `regions` has it.
    let dimension = definitions
        .metric(&metric_name("revenue"))
        .expect("the metric is there")
        .dimensions()
        .iter()
        .find(|d| d.1.name() == &dimension_name("region"))
        .expect("the dimension is there");
    assert_eq!(dimension.1.column(), &column("code"));
    assert_eq!(
        dimension.1.via(),
        Some(&[relationship_name("orders_customer"), relationship_name("customers_regions")][..])
    );
}

/// The resolution above is only observable through a refusal, so this is the arm that proves it.
///
/// `region_code` is a column of `customers` - hop 1's target - and NOT of `regions`, hop 2's
/// target. So a chain dimension naming it must be refused, and the refusal must name `regions`:
/// that is what distinguishes "resolved on the LAST hop's model" from "resolved on the first
/// hop's". The passing cell above asserts `column()` and `via()`, which are the values the
/// constructor was handed, so it would stay green if resolution moved to the wrong hop.
#[test]
fn a_chain_dimension_naming_an_earlier_hops_column_is_refused_naming_the_last_hops_model() {
    let (models, relationships) = three_models();
    let m = metric(
        "revenue",
        vec![dimension(
            "region",
            "region_code",
            Some(&["orders_customer", "customers_regions"]),
            None,
        )],
    );
    assert_eq!(
        Definitions::assemble(models, relationships, vec![m]).unwrap_err(),
        InconsistentDefinitions::UnknownDimensionColumn {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            model: model_name("regions"),
            column: column("region_code"),
        },
        "a chain resolves its column on the LAST hop's model, so an earlier hop's column is unknown"
    );
}

#[test]
fn a_chain_hop_that_could_duplicate_rows_is_refused_naming_the_hop() {
    let (models, mut relationships) = three_models();
    relationships[1] = Relationship::new(
        relationship_name("customers_regions"),
        model_name("customers"),
        column("region_code"),
        model_name("regions"),
        column("code"),
        JoinType::OneToMany,
    );
    let m = chain_metric();
    assert_eq!(
        Definitions::assemble(models, relationships, vec![m]).unwrap_err(),
        InconsistentDefinitions::JoinWouldDuplicateRows {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("customers_regions"),
            hop: 2,
        },
        "the second hop is what would multiply rows, so the report names 2"
    );
}

#[test]
fn a_chain_whose_hop_does_not_start_at_the_previous_target_is_refused() {
    // `customers_regions` starts at `regions` instead of at `customers`, so the chain is two
    // relationships rather than one path.
    let (models, mut relationships) = three_models();
    relationships[1] = Relationship::new(
        relationship_name("customers_regions"),
        model_name("regions"),
        column("code"),
        model_name("customers"),
        column("region_code"),
        JoinType::ManyToOne,
    );
    let m = chain_metric();
    assert_eq!(
        Definitions::assemble(models, relationships, vec![m]).unwrap_err(),
        InconsistentDefinitions::ChainDoesNotJoinUp {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            previous: relationship_name("orders_customer"),
            relationship: relationship_name("customers_regions"),
        }
    );
}

#[test]
fn a_chain_hop_onto_another_data_system_is_refused() {
    // `regions` sits on `elsewhere`, and hop 2 crossing there would put the chained join on another
    // data system's statement. Hop 1 is allowed to cross - that is the federated case - so the same
    // models with a single-hop dimension on `orders_customer` still assemble below.
    let (mut models, relationships) = three_models();
    models[2] = model("regions", "elsewhere", &["code", "label"]);
    let m = chain_metric();
    assert_eq!(
        Definitions::assemble(models.clone(), relationships.clone(), vec![m]).unwrap_err(),
        InconsistentDefinitions::HopCrossesSource {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("customers_regions"),
            own: SourceName::parse("local").expect("a test source is a source"),
            target_source: SourceName::parse("elsewhere").expect("a test source is a source"),
        }
    );

    let single = metric("revenue", vec![dimension("region", "id", Some(&["orders_customer"]), None)]);
    drop(
        Definitions::assemble(models, relationships, vec![single])
            .expect("hop 1 crossing is the federated case and still assembles"),
    );
}

#[test]
fn a_chain_that_crosses_a_data_system_and_comes_back_is_refused() {
    // The hole the target-only comparison left, and it was a WRONG ANSWER rather than an error:
    // `customers` sits on `elsewhere` and `regions` back on `local`, so hop 2's TARGET is local and
    // a check reading the target alone accepted the chain. `sutura_semantic` reads a chain's source
    // off its LAST hop, so this loaded as a purely local dimension and the whole-answer plan put
    // `elsewhere`'s table into one `local` statement - under a certified metric name, with no
    // refusal anywhere. Hop 2's ORIGIN is what gives it away, which is why both ends are compared.
    let (mut models, relationships) = three_models();
    models[1] = model("customers", "elsewhere", &["id", "region_code"]);
    assert_eq!(
        Definitions::assemble(models, relationships, vec![chain_metric()]).unwrap_err(),
        InconsistentDefinitions::HopCrossesSource {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("customers_regions"),
            own: SourceName::parse("local").expect("a test source is a source"),
            // `elsewhere` is where the hop STARTS here, not where it ends: the field names the
            // system that is not the metric's, at whichever end of the hop it turned up.
            target_source: SourceName::parse("elsewhere").expect("a test source is a source"),
        }
    );
}

#[test]
fn an_empty_chain_is_not_representable() {
    assert_eq!(
        ViaChain::of(vec![]).expect_err("an empty chain is refused"),
        InvalidViaChain::Empty
    );
}
