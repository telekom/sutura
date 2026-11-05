//! What an ontology becomes, and every load refusal, over the recorded answers and edits of them.

use std::collections::BTreeMap;

use sutura_domain::capabilities::DefinitionKind;
use sutura_domain::catalog::ColumnType;
use sutura_domain::model::{ColumnName, JoinType, ModelName, RelationshipName, SourceName};
use sutura_domain::pinned::{CatalogKind, DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};

use crate::document::{LinkTypeSide, ObjectType, Ontology, Page};
use crate::{FoundryCatalog, FoundryError, OntologyReader, fixture};

/// A reader over an ontology a cell built.
struct Recorded(Ontology);

impl OntologyReader for Recorded {
    type Error = FoundryError;

    fn read(&self) -> Result<Ontology, Self::Error> {
        Ok(self.0.clone())
    }
}

fn catalog<R>(reader: R) -> FoundryCatalog<R>
where
    R: OntologyReader,
{
    let name = SourceName::parse("ontology").expect("a name");
    FoundryCatalog::new(
        name.clone(),
        DefinitionVersion::parse("test").expect("a version"),
        name,
        reader,
    )
}

/// The recorded answers as JSON a cell can edit: the object types, and each one's link sides.
struct Answers {
    types: Vec<serde_json::Value>,
    links: BTreeMap<String, serde_json::Value>,
}

impl Answers {
    fn recorded() -> Self {
        let page: serde_json::Value = serde_json::from_str(fixture::OBJECT_TYPES).expect("the recorded page is JSON");
        let types = page["data"].as_array().expect("a data array").clone();
        let links = fixture::OUTGOING
            .iter()
            .map(|(name, page)| {
                let page: serde_json::Value = serde_json::from_str(page).expect("the recorded page is JSON");
                (String::from(*name), page["data"].clone())
            })
            .collect();
        Self { types, links }
    }

    fn decoded(self) -> Ontology {
        let types: Page<ObjectType> =
            serde_json::from_value(serde_json::json!({ "data": self.types })).expect("an object type page");
        let outgoing = self
            .links
            .into_iter()
            .map(|(name, sides)| {
                let page: Page<LinkTypeSide> = serde_json::from_value(serde_json::json!({ "data": sides })).expect("a link page");
                (name, page.into_data())
            })
            .collect();
        Ontology::new(types.into_data(), outgoing)
    }
}

/// Loads the recorded ontology after `edit` changed it.
fn load_edited(edit: impl FnOnce(&mut Answers)) -> Result<PinnedDefinitions, FoundryError> {
    let mut answers = Answers::recorded();
    edit(&mut answers);
    catalog(Recorded(answers.decoded())).load()
}

fn object_type<'a>(types: &'a mut [serde_json::Value], api_name: &str) -> &'a mut serde_json::Value {
    types
        .iter_mut()
        .find(|object_type| object_type["apiName"] == api_name)
        .expect("a recorded object type")
}

fn name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a model name")
}

#[test]
fn every_object_type_is_a_model_with_its_properties_types_and_key() {
    let pinned = catalog(fixture::FixtureReader).load().expect("the recorded ontology loads");
    let models = pinned.definitions().models();
    assert_eq!(
        models.keys().map(ModelName::as_str).collect::<Vec<_>>(),
        ["Aircraft", "Airport", "Employee", "Flight"]
    );
    let flight = &models[&name("Flight")];
    assert_eq!(flight.table_name().as_str(), "Flight");
    assert_eq!(flight.description(), "One scheduled flight between two airports.");
    assert_eq!(
        flight.primary_key().iter().map(ColumnName::as_str).collect::<Vec<_>>(),
        ["flightId"]
    );
    let distance = flight
        .columns()
        .find(|column| column.name().as_str() == "distanceMiles")
        .expect("the property is a column");
    assert_eq!(distance.data_type().map(ColumnType::as_str), Some("double"));
    assert_eq!(distance.description(), "The great-circle distance, in miles.");
}

#[test]
fn a_missing_description_falls_back_to_the_display_name() {
    let pinned = catalog(fixture::FixtureReader).load().expect("the recorded ontology loads");
    let models = pinned.definitions().models();
    assert_eq!(models[&name("Employee")].description(), "Employee");
    let started = models[&name("Aircraft")]
        .columns()
        .find(|column| column.name().as_str() == "firstFlightDate")
        .expect("the property is a column");
    assert_eq!(started.description(), "First flight");

    let refused = load_edited(|answers| {
        let employee = object_type(&mut answers.types, "Employee");
        employee["displayName"] = serde_json::json!("  ");
    });
    assert!(matches!(refused, Err(FoundryError::MissingDescription { on }) if on == "Employee"));
}

#[test]
fn only_a_foreign_key_on_a_one_side_is_a_many_to_one_join() {
    let pinned = catalog(fixture::FixtureReader).load().expect("the recorded ontology loads");
    let relationships = pinned.definitions().relationships();
    assert_eq!(
        relationships.keys().map(RelationshipName::as_str).collect::<Vec<_>>(),
        ["Flight_aircraft", "Flight_arrivalAirport", "Flight_departureAirport"]
    );
    let departure = &relationships[&RelationshipName::parse("Flight_departureAirport").expect("a name")];
    assert_eq!(
        (departure.origin_model().as_str(), departure.target_model().as_str()),
        ("Flight", "Airport")
    );
    assert_eq!(departure.join_type(), JoinType::ManyToOne);
    let key = departure.keys().first();
    assert_eq!(
        (key.origin().as_str(), key.target().as_str()),
        ("departureAirportCode", "airportCode")
    );
}

#[test]
fn a_link_that_does_not_fit_is_refused_by_name() {
    let refused = load_edited(|answers| {
        answers.links.get_mut("Airport").expect("recorded")[0]["foreignKeyPropertyApiName"] = "airportCode".into();
    });
    assert!(
        matches!(refused, Err(FoundryError::ForeignKeyOnManySide { on, link }) if on == "Airport" && link == "arrivingFlights")
    );

    let refused = load_edited(|answers| {
        answers.links.get_mut("Flight").expect("recorded")[0]["cardinality"] = "SOME".into();
    });
    assert!(matches!(refused, Err(FoundryError::UnknownCardinality { cardinality, .. }) if cardinality == "SOME"));

    let refused = load_edited(|answers| answers.types.retain(|object_type| object_type["apiName"] != "Aircraft"));
    assert!(matches!(refused, Err(FoundryError::UnlistedTarget { target, .. }) if target == "Aircraft"));

    let refused = load_edited(|answers| {
        answers.links.get_mut("Flight").expect("recorded")[0]["foreignKeyPropertyApiName"] = "noSuchProperty".into();
    });
    assert!(matches!(refused, Err(FoundryError::Inconsistent { .. })), "{refused:?}");
}

#[test]
fn an_answer_name_that_is_no_name_is_refused_before_it_is_used() {
    let refused = load_edited(|answers| object_type(&mut answers.types, "Flight")["apiName"] = "Flight/../admin".into());
    assert!(
        matches!(refused, Err(FoundryError::Identifier { kind: "object type", .. })),
        "{refused:?}"
    );

    let refused = load_edited(|answers| object_type(&mut answers.types, "Airport")["primaryKey"] = "noSuchProperty".into());
    assert!(matches!(refused, Err(FoundryError::Inconsistent { .. })), "{refused:?}");

    let long = "x".repeat(500);
    let refused = load_edited(move |answers| object_type(&mut answers.types, "Airport")["apiName"] = long.into());
    let Err(FoundryError::Identifier { value, .. }) = refused else {
        panic!("an over-long name loaded: {refused:?}");
    };
    assert_eq!(value.len(), 80, "a refusal quoted more of the answer than it may");
}

#[test]
fn it_declares_what_object_type_metadata_carries_and_nothing_more() {
    type Catalog = FoundryCatalog<fixture::FixtureReader>;
    assert_eq!(Catalog::KIND, CatalogKind::Declaring);
    let declared = Catalog::capabilities();
    for kind in [
        DefinitionKind::Metrics,
        DefinitionKind::Grains,
        DefinitionKind::Anchors,
        DefinitionKind::RequiredFilters,
    ] {
        assert!(!declared.definitions().declares(kind), "{kind:?} is declared");
    }
}
