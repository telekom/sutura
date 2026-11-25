//! What an ontology becomes, and every load refusal, over the synthetic answers and edits of them.

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

/// The synthetic answers as JSON a cell can edit: the object types, and each one's link sides.
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

/// Loads the synthetic ontology after `edit` changed it.
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
fn an_answer_name_that_is_no_name_is_refused_naming_its_object_type() {
    let refused = load_edited(|answers| {
        let properties = object_type(&mut answers.types, "Airport")["properties"]
            .as_object_mut()
            .expect("a property map");
        let property = properties.remove("city").expect("a recorded property");
        properties.insert(String::from("city/../admin"), property);
    });
    assert!(
        matches!(&refused, Err(FoundryError::Identifier { kind: "property", on, link: None, .. }) if on == "Airport"),
        "{refused:?}"
    );
    let rendered = refused.expect_err("refused").to_string();
    assert!(
        rendered.contains("\"Airport\"") && rendered.contains("city/../admin"),
        "{rendered}"
    );

    let refused = load_edited(|answers| {
        answers.links.get_mut("Flight").expect("recorded")[0]["foreignKeyPropertyApiName"] = "tail number".into();
    });
    assert!(
        matches!(
            &refused,
            Err(FoundryError::Identifier { kind: "foreign key", on, link: Some(link), .. }) if on == "Flight" && link == "aircraft"
        ),
        "{refused:?}"
    );
    let rendered = refused.expect_err("refused").to_string();
    assert!(
        rendered.contains("\"Flight\"") && rendered.contains("\"aircraft\""),
        "{rendered}"
    );

    let refused = load_edited(|answers| object_type(&mut answers.types, "Airport")["primaryKey"] = "noSuchProperty".into());
    assert!(matches!(refused, Err(FoundryError::Inconsistent { .. })), "{refused:?}");

    let long = "x".repeat(500);
    let refused = load_edited(move |answers| {
        let properties = object_type(&mut answers.types, "Airport")["properties"]
            .as_object_mut()
            .expect("a property map");
        let property = properties.remove("city").expect("a recorded property");
        properties.insert(long, property);
    });
    let Err(FoundryError::Identifier { value, .. }) = refused else {
        panic!("an over-long name loaded: {refused:?}");
    };
    assert_eq!(value.len(), 80, "a refusal quoted more of the answer than it may");
}

/// The recorded object types with the two platform-provided ones spliced in first, as a tenant's
/// list serves them, and a `ONE` link from `Flight` to one of them.
fn with_platform_types(answers: &mut Answers) {
    let page: serde_json::Value = serde_json::from_str(fixture::PLATFORM_OBJECT_TYPES).expect("the platform page is JSON");
    let platform = page["data"].as_array().expect("a data array").clone();
    answers.types.splice(0..0, platform);
    answers
        .links
        .get_mut("Flight")
        .expect("recorded")
        .as_array_mut()
        .expect("a link list")
        .push(serde_json::json!({
            "apiName": "evolution",
            "displayName": "Evolution",
            "status": "ACTIVE",
            "objectTypeApiName": "com.palantir.system.Evolution",
            "cardinality": "ONE",
            "foreignKeyPropertyApiName": "tailNumber",
            "linkTypeRid": "ri.ontology.main.link-type.00000000-0000-0000-0000-000000000095"
        }));
}

const PLATFORM: [&str; 2] = ["com.palantir.system.Evolution", "com.palantir.system.EvolutionProposal"];

#[test]
fn a_dotted_object_type_is_a_model_under_its_mapped_name_and_keeps_its_dotted_object() {
    let pinned = load_edited(with_platform_types).expect("every object type loads, dotted or not");
    let definitions = pinned.definitions();
    assert_eq!(
        definitions.models().keys().map(ModelName::as_str).collect::<Vec<_>>(),
        [
            "Aircraft",
            "Airport",
            "Employee",
            "Flight",
            "com_palantir_system_Evolution",
            "com_palantir_system_EvolutionProposal"
        ]
    );
    let evolution = &definitions.models()[&name("com_palantir_system_Evolution")];
    assert_eq!(evolution.table().name().as_str(), "com_palantir_system_Evolution");
    assert_eq!(
        evolution
            .table()
            .object_path()
            .map(sutura_domain::model::ObjectPath::dotted)
            .as_deref(),
        Some(PLATFORM[0]),
        "the dotted API name stays the object a statement names"
    );
    assert_eq!(evolution.table().to_string(), format!("`{}`", PLATFORM[0]));
    let link = &definitions.relationships()[&RelationshipName::parse("Flight_evolution").expect("a relationship name")];
    assert_eq!(link.target_model().as_str(), "com_palantir_system_Evolution");
    assert_eq!(link.keys().first().target().as_str(), "evolutionId");
}

#[test]
fn two_api_names_that_map_to_one_model_name_are_refused_naming_both() {
    let refused = load_edited(|answers| {
        with_platform_types(answers);
        let mut shadow = answers.types[0].clone();
        shadow["apiName"] = "com_palantir_system_Evolution".into();
        answers.types.push(shadow);
    });
    let Err(FoundryError::ModelNameCollision { first, second, model }) = &refused else {
        panic!("a shadowing name loaded: {refused:?}");
    };
    assert_eq!(
        (first.as_str(), second.as_str()),
        (PLATFORM[0], "com_palantir_system_Evolution")
    );
    assert_eq!(model.as_str(), "com_palantir_system_Evolution");
    let rendered = refused.expect_err("refused").to_string();
    assert!(
        rendered.contains(PLATFORM[0]) && rendered.contains("\"com_palantir_system_Evolution\""),
        "{rendered}"
    );
}

#[test]
fn an_api_name_that_cannot_be_mapped_is_refused_naming_it() {
    for unmappable in [
        "com..system.Evolution",
        "com.palantir.system.",
        "com.palantir/system.Evolution",
        "com.9lives.Evolution",
    ] {
        let refused = load_edited(|answers| {
            with_platform_types(answers);
            answers.types[0]["apiName"] = unmappable.into();
        });
        assert!(
            matches!(&refused, Err(FoundryError::ObjectTypeName { on, .. }) if on == unmappable),
            "{unmappable}: {refused:?}"
        );
        let rendered = refused.expect_err("refused").to_string();
        assert!(rendered.contains(unmappable), "{rendered}");
    }
}

#[test]
fn a_link_to_an_object_type_the_read_never_listed_is_still_refused() {
    let refused = load_edited(|answers| {
        with_platform_types(answers);
        answers.types.retain(|object_type| object_type["apiName"] != PLATFORM[0]);
    });
    assert!(
        matches!(&refused, Err(FoundryError::UnlistedTarget { on, link, target }) if on == "Flight" && link == "evolution" && target == PLATFORM[0]),
        "{refused:?}"
    );
}

#[test]
fn a_composed_relationship_name_too_long_for_a_name_is_shortened_stably() {
    let link = "aircraftAssignedForTheScheduledDepartureUnderTheLongTermOperationalPlanningAgreement";
    let composed = format!("Flight_{link}");
    assert!(composed.len() > 63, "{}", composed.len());
    let load = || {
        load_edited(|answers| {
            answers.links.get_mut("Flight").expect("recorded")[0]["apiName"] = link.into();
        })
        .expect("a long link name loads")
    };
    let names = |pinned: &PinnedDefinitions| {
        pinned
            .definitions()
            .relationships()
            .keys()
            .map(|name| String::from(name.as_str()))
            .collect::<Vec<_>>()
    };
    let first = names(&load());
    assert_eq!(first, names(&load()), "the mapping is not stable across loads");
    let long = first
        .iter()
        .find(|name| name.starts_with("Flight_aircraftAssigned"))
        .expect("the long link is carried");
    assert_eq!(long.len(), 63, "{long}");
    assert_eq!(
        *long,
        sutura_domain::model::fitted(composed, &format!("Flight\u{1f}{link}")),
        "not the shortening a model name takes"
    );
    assert!(
        first.iter().any(|name| name == "Flight_arrivalAirport"),
        "a short composition was changed: {first:?}"
    );
}

#[test]
fn two_links_whose_composed_names_meet_are_refused_naming_both() {
    let refused = load_edited(|answers| {
        let mut shadow = object_type(&mut answers.types, "Flight").clone();
        shadow["apiName"] = "Flight_x".into();
        answers.types.push(shadow);
        let flight = answers.links.get_mut("Flight").expect("recorded");
        flight[0]["apiName"] = "x_y".into();
        let mut shadow_links = flight.clone();
        shadow_links[0]["apiName"] = "y".into();
        answers.links.insert(String::from("Flight_x"), shadow_links);
    });
    let Err(FoundryError::RelationshipNameCollision {
        first_on,
        first_link,
        second_on,
        second_link,
        name,
    }) = &refused
    else {
        panic!("a shadowing relationship loaded: {refused:?}");
    };
    assert_eq!(
        (
            first_on.as_str(),
            first_link.as_str(),
            second_on.as_str(),
            second_link.as_str()
        ),
        ("Flight", "x_y", "Flight_x", "y")
    );
    assert_eq!(name.as_str(), "Flight_x_y");
}

#[test]
fn the_platform_types_are_left_out_by_default_and_a_link_into_them_is_omitted() {
    let ontology = || {
        let mut answers = Answers::recorded();
        with_platform_types(&mut answers);
        answers.decoded()
    };
    let pinned = catalog(Recorded(ontology()))
        .excluding(sutura_domain::model::ObjectTypeExclusions::default())
        .load()
        .expect("the platform types are left out, not refused");
    let definitions = pinned.definitions();
    assert_eq!(
        definitions.models().keys().map(ModelName::as_str).collect::<Vec<_>>(),
        ["Aircraft", "Airport", "Employee", "Flight"]
    );
    assert!(
        !definitions
            .relationships()
            .keys()
            .any(|name| name.as_str() == "Flight_evolution"),
        "a link into an excluded type was carried"
    );

    let kept = catalog(Recorded(ontology()))
        .excluding(sutura_domain::model::ObjectTypeExclusions::none())
        .load()
        .expect("an empty list keeps them");
    assert_eq!(kept.definitions().models().len(), 6);

    let custom = sutura_domain::model::ObjectTypeExclusions::parse(["Employee"]).expect("an exact name");
    let pinned = catalog(Recorded(ontology())).excluding(custom).load().expect("loads");
    assert!(!pinned.definitions().models().contains_key(&name("Employee")));
    assert!(
        pinned
            .definitions()
            .models()
            .contains_key(&name("com_palantir_system_Evolution"))
    );
}

#[test]
fn a_long_dotted_name_kept_by_the_exclusions_is_shortened_and_one_left_out_is_not_read() {
    use sutura_domain::model::ObjectTypeExclusions;
    let load = |api_name: &str, exclusions: ObjectTypeExclusions| {
        let mut answers = Answers::recorded();
        with_platform_types(&mut answers);
        answers.types[1]["apiName"] = api_name.into();
        catalog(Recorded(answers.decoded())).excluding(exclusions).load()
    };
    for (api_name, exclusions, prefix) in [
        (
            "com.example.system.EvolutionModelStudioUnslothFineTuningExperiment",
            ObjectTypeExclusions::default(),
            "com_example_system_",
        ),
        (
            "com.palantir.system.EvolutionModelStudioUnslothFineTuningExperiment",
            ObjectTypeExclusions::none(),
            "com_palantir_system_EvolutionModel",
        ),
    ] {
        let pinned = load(api_name, exclusions).expect("a long dotted name is shortened, not refused");
        let model = pinned
            .definitions()
            .models()
            .keys()
            .find(|model| model.as_str().starts_with(prefix))
            .expect("the long-named type is kept");
        assert_eq!(model.as_str().len(), 63, "{model}");
    }
    let pinned = load(
        "com.palantir.system.EvolutionModelStudioUnslothFineTuningExperiment",
        ObjectTypeExclusions::default(),
    )
    .expect("the default leaves it out");
    assert_eq!(pinned.definitions().models().len(), 4);
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
