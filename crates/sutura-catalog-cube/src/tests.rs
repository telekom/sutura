use sutura_domain::model::{ColumnName, ModelName, SourceName};
use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

use crate::document::Meta;
use crate::{CubeCatalog, CubeError, MetaReader, fixture};

/// A reader over one answer written in the test.
struct Answer(&'static str);

impl MetaReader for Answer {
    type Error = CubeError;

    fn read(&self) -> Result<Meta, Self::Error> {
        serde_json::from_str(self.0).map_err(|cause| CubeError::Read(Box::new(cause)))
    }
}

fn name(raw: &str) -> SourceName {
    SourceName::parse(raw).expect("a test source name is a name")
}

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test").expect("a test version is a version")
}

fn catalog(answer: &'static str) -> CubeCatalog<Answer> {
    CubeCatalog::new(name("cube_catalog"), version(), name("metrics"), Answer(answer))
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a name")
}

#[test]
fn every_recorded_cube_is_a_model_on_the_cube_source_with_its_dimensions_as_columns() {
    let pinned = fixture::over_fixture_source(name("metrics"), version())
        .load()
        .expect("the recorded answer loads");
    let models = pinned.definitions().models();
    let names: Vec<&str> = models.keys().map(ModelName::as_str).collect();
    assert_eq!(names, ["customers", "daily_usage", "products", "regions", "subscriptions"]);
    let subscriptions = &models[&ModelName::parse("subscriptions").expect("a name")];
    assert_eq!(subscriptions.source().as_str(), "metrics");
    assert_eq!(subscriptions.table_name().as_str(), "subscriptions");
    let columns: Vec<&str> = subscriptions.columns().map(|column| column.name().as_str()).collect();
    assert_eq!(
        columns,
        ["churned_in_month", "contract_term", "month", "status", "subscription_key"]
    );
    assert_eq!(
        *subscriptions.primary_key(),
        [column("month"), column("subscription_key")].into()
    );
}

#[test]
fn a_measure_is_reported_and_never_minted_as_a_metric() {
    let meta = fixture::FixtureReader.read().expect("the recorded answer decodes");
    let measures: Vec<&str> = meta
        .cubes()
        .iter()
        .flat_map(|cube| cube.measures().iter().map(crate::document::Measure::name))
        .collect();
    assert_eq!(
        measures,
        [
            "subscriptions.recurring_revenue",
            "subscriptions.active_subscriptions",
            "subscriptions.subscriptions_churned",
            "daily_usage.voice_minutes",
            "daily_usage.data_gb",
        ]
    );
    let pinned = fixture::over_fixture_source(name("metrics"), version())
        .load()
        .expect("the recorded answer loads");
    assert!(pinned.definitions().metrics().is_empty());
    assert!(pinned.definitions().relationships().is_empty());
}

#[test]
fn a_cube_without_a_description_is_refused_by_name() {
    let refused = catalog(
        r#"{"cubes":[{"name":"orders","type":"cube","measures":[],"segments":[],
            "dimensions":[{"name":"orders.id","type":"number","primaryKey":true}]}]}"#,
    )
    .load()
    .expect_err("a model needs a description");
    assert!(
        matches!(refused, CubeError::MissingDescription { ref on } if on == "orders"),
        "{refused:?}"
    );
}

#[test]
fn a_member_named_under_another_cube_is_refused() {
    let refused = catalog(
        r#"{"cubes":[{"name":"orders","type":"cube","description":"Orders.","measures":[],"segments":[],
            "dimensions":[{"name":"customers.id","type":"number","primaryKey":false}]}]}"#,
    )
    .load()
    .expect_err("a member outside its cube is not this cube's column");
    assert!(
        matches!(refused, CubeError::ForeignMember { ref cube, ref member } if cube == "orders" && member == "customers.id"),
        "{refused:?}"
    );
}

#[test]
fn the_recorded_answer_serves_every_cube_the_example_model_declares() {
    let model = include_str!("../../../examples/cube/model/single_player.yml");
    let mut declared: Vec<&str> = model
        .lines()
        .filter_map(|line| line.strip_prefix("  - name: "))
        .map(str::trim)
        .collect();
    let meta: Meta = serde_json::from_str(fixture::META).expect("the recorded answer decodes");
    let mut served: Vec<&str> = meta.cubes().iter().map(crate::document::Cube::name).collect();
    declared.sort_unstable();
    served.sort_unstable();
    assert_eq!(served, declared, "re-record src/fixture/meta.json from examples/cube/model");
}
