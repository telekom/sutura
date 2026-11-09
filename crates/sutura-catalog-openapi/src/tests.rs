use std::collections::BTreeMap;

use serde_json::{Value, json};
use sutura_domain::model::{ColumnName, ModelName, RelationshipName, SourceName};
use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

use crate::fixture::{self, DOCUMENT, FixtureReader, MAPPING, SAMPLE_DOCUMENT, SAMPLE_MAPPING};
use crate::{Contract, ContractError, OpenApiCatalog, OpenApiError};

fn name(raw: &str) -> SourceName {
    SourceName::parse(raw).expect("a test source name is a name")
}

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test").expect("a test version is a version")
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a name")
}

fn model(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a name")
}

/// `text` with `old` replaced, refusing a replacement that would change nothing.
fn replaced(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the fixture no longer holds {old:?}");
    text.replace(old, new)
}

fn refused_contract(document: &str, mapping: &str) -> ContractError {
    Contract::parse(document, mapping).expect_err("the contract is refused")
}

fn recorded() -> BTreeMap<String, Value> {
    serde_json::from_str(fixture::ANSWERS).expect("the recorded answers are JSON")
}

fn load(contract: Contract, answers: BTreeMap<String, Value>) -> Result<sutura_domain::pinned::PinnedDefinitions, OpenApiError> {
    OpenApiCatalog::new(
        name("registry"),
        version(),
        name("warehouse"),
        contract,
        FixtureReader::new(answers),
    )
    .load()
}

fn example() -> Contract {
    Contract::parse(DOCUMENT, MAPPING).expect("the example contract checks")
}

const FIRST_PAGE: &str = "/v1/workspaces/demo/datasets?pageSize=2";

fn dataset(raw_name: &str) -> Value {
    json!({ "name": raw_name, "summary": "A dataset.", "key": ["id"], "fields": [{ "name": "id" }] })
}

#[test]
fn every_listed_dataset_is_a_model_with_its_columns_key_and_joins() {
    let pinned = fixture::over_fixture_source(name("warehouse"), version())
        .expect("the fixture opens")
        .load()
        .expect("the recorded answers load");
    let definitions = pinned.definitions();
    let models: Vec<&str> = definitions.models().keys().map(ModelName::as_str).collect();
    assert_eq!(models, ["customer", "region", "subscription"]);

    let customer = &definitions.models()[&model("customer")];
    assert_eq!(customer.source().as_str(), "warehouse");
    assert_eq!(customer.table_name().as_str(), "customer");
    assert_eq!(
        customer.description(),
        "One row per customer.",
        "an empty summary falls back to the title"
    );
    let columns: Vec<&str> = customer.columns().map(|column| column.name().as_str()).collect();
    assert_eq!(columns, ["customer_id", "region_id", "segment"]);
    assert_eq!(*customer.primary_key(), [column("customer_id")].into());

    let subscription = &definitions.models()[&model("subscription")];
    assert_eq!(
        *subscription.primary_key(),
        [column("month"), column("subscription_id")].into()
    );
    let month = subscription.column(&column("month")).expect("month is a column");
    assert_eq!(month.description(), "The first day of the billed month.");

    let joins: Vec<[&str; 3]> = definitions
        .relationships()
        .values()
        .map(|join| {
            [
                join.name().as_str(),
                join.origin_model().as_str(),
                join.target_model().as_str(),
            ]
        })
        .collect();
    assert_eq!(
        joins,
        [
            ["customer_region", "customer", "region"],
            ["subscription_customer", "subscription", "customer"],
        ],
        "a link with no foreign key on this side is not carried"
    );
    let key = definitions.relationships()[&RelationshipName::parse("customer_region").expect("a name")]
        .keys()
        .first();
    assert_eq!((key.origin(), key.target()), (&column("region_id"), &column("region_id")));
}

#[test]
fn the_vendor_sample_reads_keyed_properties_and_a_to_one_link() {
    let contract = Contract::parse(SAMPLE_DOCUMENT, SAMPLE_MAPPING).expect("the sample mapping checks against its document");
    let base = "/api/v2/ontologies/ri.ontology.main.ontology.00000000-0000-0000-0000-000000000000/objectTypes";
    let answers = BTreeMap::from([
        (
            format!("{base}?pageSize=500"),
            json!({ "data": [
                { "apiName": "Airport", "displayName": "Airport", "primaryKey": "airportId",
                  "properties": { "airportId": { "dataType": { "type": "string" }, "displayName": "Airport id" },
                                  "airportName": { "dataType": { "type": "string" } } } },
                { "apiName": "Flight", "displayName": "Flight", "description": "One scheduled flight.",
                  "primaryKey": "flightId",
                  "properties": { "flightId": { "dataType": { "type": "string" } },
                                  "originAirportId": { "dataType": { "type": "string" },
                                                       "description": "The airport the flight departs from." } } }
            ] }),
        ),
        (
            format!("{base}/Airport/outgoingLinkTypes"),
            json!({ "data": [{ "apiName": "departingFlights", "objectTypeApiName": "Flight", "cardinality": "MANY" }] }),
        ),
        (
            format!("{base}/Flight/outgoingLinkTypes"),
            json!({ "data": [{ "apiName": "originAirport", "objectTypeApiName": "Airport", "cardinality": "ONE",
                               "foreignKeyPropertyApiName": "originAirportId" }] }),
        ),
    ]);
    let pinned = load(contract, answers).expect("the sample answers load");
    let definitions = pinned.definitions();
    let airport = &definitions.models()[&model("Airport")];
    assert_eq!(airport.description(), "Airport");
    let columns: Vec<&str> = airport.columns().map(|column| column.name().as_str()).collect();
    assert_eq!(columns, ["airportId", "airportName"]);
    let identifier = airport.column(&column("airportId")).expect("a column");
    assert_eq!(identifier.description(), "Airport id");
    let joins: Vec<&str> = definitions.relationships().keys().map(RelationshipName::as_str).collect();
    assert_eq!(joins, ["originAirport"]);
}

#[test]
fn a_document_that_is_not_openapi_3_0_or_3_1_is_refused_by_its_version() {
    for (document, found) in [
        (r#"{"swagger": "2.0", "paths": {}}"#, Some("swagger 2.0")),
        (r#"{"openapi": "4.0.0", "paths": {}}"#, Some("openapi 4.0.0")),
        (r#"{"paths": {}}"#, None),
    ] {
        let refused = refused_contract(document, MAPPING);
        assert!(
            matches!(&refused, ContractError::UnsupportedVersion { found: seen } if seen.as_deref() == found),
            "{refused:?}"
        );
    }
}

#[test]
fn a_reference_outside_the_document_is_refused_before_anything_is_read() {
    let document = replaced(
        DOCUMENT,
        r##"{ "$ref": "#/components/parameters/PageSize" }"##,
        r#"{ "$ref": "https://other.example.com/parameters.json#/PageSize" }"#,
    );
    let refused = refused_contract(&document, MAPPING);
    assert!(
        matches!(&refused, ContractError::ExternalReference { reference } if reference.starts_with("https://other.example.com/")),
        "{refused:?}"
    );
}

#[test]
fn an_operation_the_document_does_not_have_is_refused() {
    let mapping = replaced(MAPPING, "operation: listDatasets\n", "operation: listTables\n");
    let refused = refused_contract(DOCUMENT, &mapping);
    assert!(
        matches!(&refused, ContractError::UnknownOperation { operation } if operation.as_str() == "listTables"),
        "{refused:?}"
    );
}

#[test]
fn a_mapped_operation_that_is_not_a_get_is_refused() {
    let mapping = replaced(MAPPING, "operation: listDatasets\n", "operation: createDataset\n");
    let refused = refused_contract(DOCUMENT, &mapping);
    assert!(
        matches!(&refused, ContractError::NotAGet { operation } if operation.as_str() == "createDataset"),
        "{refused:?}"
    );
}

#[test]
fn an_operation_that_needs_a_credential_other_than_a_bearer_is_refused() {
    let document = replaced(
        DOCUMENT,
        r#""bearer": { "type": "http", "scheme": "bearer" }"#,
        r#""bearer": { "type": "apiKey", "in": "query", "name": "key" }"#,
    );
    let refused = refused_contract(&document, MAPPING);
    assert!(
        matches!(&refused, ContractError::UnsupportedSecurity { operation } if operation.as_str() == "listDatasets"),
        "{refused:?}"
    );
}

#[test]
fn a_path_that_could_leave_the_declared_endpoint_is_refused() {
    for path in [
        "/v1/@other.example.com/{workspace}/datasets",
        "//other.example.com/{workspace}/datasets",
        "https://other.example.com/{workspace}/datasets",
        "/v1/../{workspace}/datasets",
        "/v1/%2e%2e/{workspace}/datasets",
    ] {
        let document = replaced(
            DOCUMENT,
            r#""/v1/workspaces/{workspace}/datasets": {"#,
            &format!(r#""{path}": {{"#),
        );
        let refused = refused_contract(&document, MAPPING);
        assert!(
            matches!(&refused, ContractError::OffOriginPath { operation } if operation.as_str() == "listDatasets"),
            "{path}: {refused:?}"
        );
    }
}

#[test]
fn a_path_parameter_the_mapping_does_not_bind_is_refused() {
    let mapping = replaced(MAPPING, "parameters:\n  workspace: demo\n", "");
    let refused = refused_contract(DOCUMENT, &mapping);
    assert!(
        matches!(&refused, ContractError::UnboundParameter { operation, parameter }
            if operation.as_str() == "listDatasets" && parameter == "workspace"),
        "{refused:?}"
    );
}

#[test]
fn a_fixed_parameter_that_is_not_one_plain_path_segment_is_refused() {
    for value in ["../admin", "a/b", "..", "a%2Fb", "a?b"] {
        let mapping = replaced(MAPPING, "  workspace: demo\n", &format!("  workspace: \"{value}\"\n"));
        let refused = refused_contract(DOCUMENT, &mapping);
        assert!(matches!(refused, ContractError::Mapping { .. }), "{value}: {refused:?}");
    }
}

#[test]
fn a_fixed_parameter_no_path_uses_is_refused() {
    let mapping = replaced(MAPPING, "  workspace: demo\n", "  workspace: demo\n  region: north\n");
    let refused = refused_contract(DOCUMENT, &mapping);
    assert!(
        matches!(&refused, ContractError::UnusedParameter { parameter } if parameter.as_str() == "region"),
        "{refused:?}"
    );
}

#[test]
fn a_paging_parameter_the_operation_does_not_declare_is_refused() {
    let mapping = replaced(
        MAPPING,
        "    cursor: pageToken\n    next: /next\n    size:",
        "    cursor: page\n    next: /next\n    size:",
    );
    let refused = refused_contract(DOCUMENT, &mapping);
    assert!(
        matches!(&refused, ContractError::UndeclaredParameter { operation, parameter, location: "query" }
            if operation.as_str() == "listDatasets" && parameter.as_str() == "page"),
        "{refused:?}"
    );
}

#[test]
fn a_required_header_with_the_paging_query_name_is_refused() {
    let document = replaced(
        DOCUMENT,
        r##"{ "$ref": "#/components/parameters/PageToken" }"##,
        r##"{ "$ref": "#/components/parameters/PageToken" },
          { "name": "pageToken", "in": "header", "required": true, "schema": { "type": "string" } }"##,
    );
    let refused = refused_contract(&document, MAPPING);
    assert!(
        matches!(&refused, ContractError::UnboundParameter { operation, parameter }
            if operation.as_str() == "listDatasets" && parameter.as_str() == "pageToken"),
        "{refused:?}"
    );
}

#[test]
fn a_pointer_the_answer_schema_does_not_declare_is_refused() {
    let mapping = replaced(MAPPING, "  name: /name\n  description:", "  name: /label\n  description:");
    let refused = refused_contract(DOCUMENT, &mapping);
    assert!(
        matches!(&refused, ContractError::NotInSchema { operation, pointer }
            if operation.as_str() == "listDatasets" && pointer == "/label"),
        "{refused:?}"
    );
}

#[test]
fn a_mapping_word_outside_the_vocabulary_is_refused() {
    let mapping = replaced(
        MAPPING,
        "  name: /name\n  description:",
        "  name: /name\n  expression: /x\n  description:",
    );
    let refused = refused_contract(DOCUMENT, &mapping);
    assert!(matches!(refused, ContractError::Mapping { .. }), "{refused:?}");
}

#[test]
fn a_listed_name_that_is_not_an_identifier_is_refused_by_name() {
    let answers = BTreeMap::from([(FIRST_PAGE.to_owned(), json!({ "items": [dataset("../secrets")] }))]);
    let refused = load(example(), answers).expect_err("a model name is an identifier");
    assert!(
        matches!(&refused, OpenApiError::Identifier { kind: "model", value, .. } if value == "../secrets"),
        "{refused:?}"
    );
}

#[test]
fn an_answer_that_holds_nothing_where_the_mapping_points_is_refused() {
    let answers = BTreeMap::from([(
        FIRST_PAGE.to_owned(),
        json!({ "items": [{ "summary": "A dataset with no name.", "fields": [] }] }),
    )]);
    let refused = load(example(), answers).expect_err("a model needs its name");
    assert!(
        matches!(&refused, OpenApiError::Missing { operation, pointer, .. }
            if operation.as_str() == "listDatasets" && pointer == "/name"),
        "{refused:?}"
    );
}

#[test]
fn a_dataset_without_any_description_is_refused_by_name() {
    let answers = BTreeMap::from([(
        FIRST_PAGE.to_owned(),
        json!({ "items": [{ "name": "orders", "summary": " ", "fields": [{ "name": "id" }] }] }),
    )]);
    let refused = load(example(), answers).expect_err("a model needs a description");
    assert!(
        matches!(&refused, OpenApiError::MissingDescription { on } if on == "orders"),
        "{refused:?}"
    );
}

#[test]
fn a_repeated_cursor_is_refused_rather_than_followed() {
    let answers = BTreeMap::from([
        (FIRST_PAGE.to_owned(), json!({ "items": [dataset("first")], "next": "again" })),
        (
            format!("{FIRST_PAGE}&pageToken=again"),
            json!({ "items": [dataset("second")], "next": "again" }),
        ),
    ]);
    let refused = load(example(), answers).expect_err("a repeated cursor never ends");
    assert!(matches!(refused, OpenApiError::RepeatedCursor { .. }), "{refused:?}");
}

#[test]
fn an_empty_page_that_names_a_next_page_is_refused() {
    let answers = BTreeMap::from([(FIRST_PAGE.to_owned(), json!({ "items": [], "next": "more" }))]);
    let refused = load(example(), answers).expect_err("an empty page that names more never ends");
    assert!(matches!(refused, OpenApiError::NoProgress { .. }), "{refused:?}");
}

#[test]
fn a_cursor_goes_back_as_one_percent_encoded_query_value() {
    let mut answers = recorded();
    let mut first = answers.remove(FIRST_PAGE).expect("the first page is recorded");
    first["next"] = json!("a b/c&d=e");
    answers.insert(FIRST_PAGE.to_owned(), first);
    let second = answers
        .remove(&format!("{FIRST_PAGE}&pageToken=page-2"))
        .expect("the second page is recorded");
    answers.insert(format!("{FIRST_PAGE}&pageToken=a%20b%2Fc%26d%3De"), second);
    let pinned = load(example(), answers).expect("the encoded cursor reaches the second page");
    assert_eq!(pinned.definitions().models().len(), 3);
}

#[test]
fn a_link_onto_a_model_without_a_single_column_key_is_refused() {
    let mut answers = recorded();
    answers.insert(
        "/v1/workspaces/demo/datasets/customer/links".to_owned(),
        json!({ "items": [{ "name": "customer_subscription", "target": "subscription", "column": "customer_id" }] }),
    );
    let refused = load(example(), answers).expect_err("a composite key cannot be the far side of one column");
    assert!(
        matches!(&refused, OpenApiError::TargetKeyUnknown { relationship, target }
            if relationship.as_str() == "customer_subscription" && target.as_str() == "subscription"),
        "{refused:?}"
    );
}
