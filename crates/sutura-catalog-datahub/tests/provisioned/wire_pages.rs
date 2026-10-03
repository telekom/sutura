//! The `dataset` and `semanticModel` page cells (#1251 gap 2): each seeds the corpus's own entities
//! on the tier, reads the page back over the search-backed paged surface, and asserts the served
//! entity maps to the same aspect the seed does - every field the reader maps as of this copy,
//! kept equal by review, so a field the reader newly maps would not be caught here. They map the
//! response with this file's own copy of `src/http.rs`'s mapping, not through `HttpAspectReader`,
//! for the parent file's reason.

use std::time::{Duration, Instant};

use sutura_catalog_datahub::document::{DatasetAspect, RelationshipAspect};

use super::tests::{INDEX_LAG_BUDGET, agent, endpoint, send};

/// One `dataset` entity served by a real `DataHub` into this adapter's own [`DatasetAspect`].
///
/// This is the mapping `src/http.rs`'s `harvest_dataset` owns, re-written here because an
/// integration test is a separate crate and cannot reach it; `document::DatasetAspect`'s own
/// `deny_unknown_fields` decode runs on the assembled document, so the equality assertions below
/// are about the adapter's canonical shape and not about this function's keys.
fn harvest_dataset(served: &serde_json::Value) -> DatasetAspect {
    let urn = served["urn"].as_str().expect("a served dataset carries its urn");
    let inner = urn
        .strip_prefix("urn:li:dataset:(")
        .and_then(|rest| rest.strip_suffix(')'))
        .expect("a dataset urn is the parenthesised `(platform,name,ENV)` form");
    let mut parts = inner.splitn(3, ',');
    let platform = parts
        .next()
        .expect("a dataset urn names a platform")
        .strip_prefix("urn:li:dataPlatform:")
        .expect("the platform segment is a `urn:li:dataPlatform:` urn")
        .to_owned();
    let name = parts.next().expect("a dataset urn names the dataset").to_owned();
    let fields = served["schemaMetadata"]["value"]["fields"]
        .as_array()
        .expect("a served dataset carries its `schemaMetadata` fields");
    let columns: Vec<String> = fields
        .iter()
        .map(|field| {
            field["fieldPath"]
                .as_str()
                .expect("a schema field carries a `fieldPath`")
                .to_owned()
        })
        .collect();
    let mut column_metadata = serde_json::Map::new();
    let mut primary_key = Vec::new();
    for field in fields {
        let Some(path) = field.get("fieldPath").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let data_type = field.get("nativeDataType").and_then(serde_json::Value::as_str);
        let field_description = field.get("description").and_then(serde_json::Value::as_str);
        if data_type.is_some() || field_description.is_some() {
            let mut entry = serde_json::Map::new();
            if let Some(data_type) = data_type {
                drop(entry.insert(String::from("data_type"), serde_json::Value::String(data_type.to_owned())));
            }
            if let Some(field_description) = field_description {
                drop(entry.insert(
                    String::from("description"),
                    serde_json::Value::String(field_description.to_owned()),
                ));
            }
            drop(column_metadata.insert(path.to_owned(), serde_json::Value::Object(entry)));
        }
        if field.get("isPartOfKey").and_then(serde_json::Value::as_bool) == Some(true) {
            primary_key.push(serde_json::Value::String(path.to_owned()));
        }
    }
    let description = served["datasetProperties"]["value"]["description"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut document = serde_json::Map::new();
    drop(document.insert(String::from("name"), serde_json::Value::String(name.clone())));
    drop(document.insert(String::from("table"), serde_json::Value::String(name)));
    drop(document.insert(String::from("platform"), serde_json::Value::String(platform)));
    drop(document.insert(String::from("columns"), serde_json::Value::from(columns)));
    drop(document.insert(String::from("description"), serde_json::Value::String(description)));
    drop(document.insert(String::from("column_metadata"), serde_json::Value::Object(column_metadata)));
    drop(document.insert(String::from("primary_key"), serde_json::Value::Array(primary_key)));
    serde_json::from_value(serde_json::Value::Object(document))
        .expect("the served dataset decodes into this adapter's canonical dataset aspect")
}

/// An array field expected to hold exactly one string, the same refusal `src/http.rs`'s
/// `one_column` holds: `DataHub` is wider than this adapter's one column per relationship side.
fn one_column(value: &serde_json::Value, field: &str) -> String {
    value[field]
        .as_array()
        .filter(|columns| columns.len() == 1)
        .and_then(|columns| columns.first())
        .and_then(serde_json::Value::as_str)
        .expect("a relationship endpoint column is exactly one, by name")
        .to_owned()
}

/// A relationship endpoint (`from`/`to`) reduced to the model name it names, taking the name
/// segment out of the `dataset` urn - the same mapping `src/http.rs`'s `one_endpoint` holds.
fn one_endpoint(value: &serde_json::Value, field: &str) -> String {
    let urn = value[field].as_str().expect("a relationship endpoint is a dataset urn");
    let inner = urn
        .strip_prefix("urn:li:dataset:(")
        .and_then(|rest| rest.strip_suffix(')'))
        .expect("a dataset urn is the parenthesised `(platform,name,ENV)` form");
    inner.split(',').nth(1).expect("a dataset urn names the dataset").to_owned()
}

/// One relationship element out of a `semanticModel` entity's nested
/// `semanticModelInfo.value.relationships[]` array into this adapter's own [`RelationshipAspect`],
/// the mapping `src/http.rs`'s `harvest_relationship` owns.
fn harvest_relationship(served: &serde_json::Value) -> RelationshipAspect {
    let relationship = &served["semanticModelInfo"]["value"]["relationships"][0];
    let name = relationship["name"]
        .as_str()
        .expect("a relationship carries its name")
        .to_owned();
    let from_model = one_endpoint(relationship, "from");
    let from_column = one_column(relationship, "fromColumns");
    let to_model = one_endpoint(relationship, "to");
    let to_column = one_column(relationship, "toColumns");
    let cardinality = relationship
        .get("cardinality")
        .and_then(serde_json::Value::as_str)
        .map(|raw| match raw {
            "ONE_ONE" => "one_one",
            "ONE_N" => "one_n",
            "N_ONE" => "n_one",
            "N_N" => "n_n",
            _ => panic!("a relationship cardinality this adapter cannot map is refused by name: {raw}"),
        });
    let mut document = serde_json::Map::new();
    drop(document.insert(String::from("name"), serde_json::Value::String(name)));
    drop(document.insert(String::from("from_model"), serde_json::Value::String(from_model)));
    drop(document.insert(String::from("from_column"), serde_json::Value::String(from_column)));
    drop(document.insert(String::from("to_model"), serde_json::Value::String(to_model)));
    drop(document.insert(String::from("to_column"), serde_json::Value::String(to_column)));
    if let Some(cardinality) = cardinality {
        drop(document.insert(
            String::from("cardinality"),
            serde_json::Value::String(String::from(cardinality)),
        ));
    }
    serde_json::from_value(serde_json::Value::Object(document))
        .expect("the served relationship decodes into this adapter's canonical relationship aspect")
}

/// The search-backed list page for one entity type, polled until every wanted urn appears.
///
/// The paged surface is not read-your-writes (`docs/adr/0016`), so a synchronous `200` write is
/// followed by waiting out the index lag rather than reading once - the same deadline shape
/// `one_response_carries_both` holds and `sutura-cli`'s served suite's `wait_until_indexed`
/// does.
fn paged_until(agent: &ureq::Agent, endpoint: &str, path: &str, wanted: &[String]) -> Vec<serde_json::Value> {
    let deadline = Instant::now() + INDEX_LAG_BUDGET;
    loop {
        let (status, body) = send(agent, &format!("http://{endpoint}/{path}"), None);
        assert_eq!(status, 200, "the {path} surface answers a paged read: {body}");
        let answer: serde_json::Value = serde_json::from_str(&body).expect("the answer is json");
        let entities = answer["entities"].as_array().expect("a page is an array of entities");
        let found: Vec<serde_json::Value> = wanted
            .iter()
            .filter_map(|want| {
                entities
                    .iter()
                    .find(|entity| entity["urn"].as_str() == Some(want.as_str()))
                    .cloned()
            })
            .collect();
        if found.len() == wanted.len() {
            // A page that signals more results - a `scrollId`, or fewer entities than a stated
            // `total` - is what `src/http.rs`'s reader refuses as `HttpReaderError::MorePages`.
            // With the corpus at most two entities, a real last page must NOT signal more; this
            // is the live side of http.rs's #Paging open question.
            assert!(
                answer.get("scrollId").and_then(serde_json::Value::as_str).is_none(),
                "the {path} last page carries a scrollId, which the reader would refuse"
            );
            if let Some(total) = answer.get("total").and_then(serde_json::Value::as_u64) {
                assert!(
                    entities.len() as u64 >= total,
                    "the {path} page states {total} total but returned {} - the reader would refuse MorePages",
                    entities.len()
                );
            }
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "the {path} page never indexed {wanted:?} within {INDEX_LAG_BUDGET:?} of 200 writes - it had {}",
            found.len()
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// The corpus's two models as `dataset` entities, in the FULL shape the pinned 1.7.0 tier's
/// validator accepts - `schemaMetadata` demands `schemaName`, `platform`, `version`, `hash`, a
/// `platformSchema` union and a per-field `type` key, the same shape `sutura-cli`'s served
/// suite seeds - so this seed passes where a guessed minimal shape would `400`. The `order_id`
/// field also carries `description` and `isPartOfKey`, which are the two attributes
/// [`harvest_dataset`] maps into `column_metadata` and `primary_key`; measuring them live is
/// the point, so a tier that drops them makes the served aspect differ from the seed.
fn dataset_bodies() -> serde_json::Value {
    let field = |path: &str, native: &str, description: Option<&str>, part_of_key: bool| {
        let mut field = serde_json::json!({
            "fieldPath": path,
            "nativeDataType": native,
            "type": { "type": { "com.linkedin.schema.StringType": {} } },
        });
        if let Some(description) = description {
            field["description"] = serde_json::json!(description);
        }
        if part_of_key {
            field["isPartOfKey"] = serde_json::json!(true);
        }
        field
    };
    serde_json::json!([
        {
            "urn": "urn:li:dataset:(urn:li:dataPlatform:bigquery,orders,PROD)",
            "schemaMetadata": { "value": {
                "schemaName": "orders",
                "platform": "urn:li:dataPlatform:bigquery",
                "version": 0,
                "hash": "orders-hash",
                "platformSchema": { "com.linkedin.schema.OtherSchema": { "rawSchema": "" } },
                "fields": [
                    field("order_id", "INT64", Some("The order's own identifier."), true),
                    field("customer_id", "STRING", None, false),
                    field("amount_cents", "INT64", None, false),
                ],
            } },
            "datasetProperties": { "value": { "description": "Net revenue orders, in minor units." } },
        },
        {
            "urn": "urn:li:dataset:(urn:li:dataPlatform:bigquery,customers,PROD)",
            "schemaMetadata": { "value": {
                "schemaName": "customers",
                "platform": "urn:li:dataPlatform:bigquery",
                "version": 0,
                "hash": "customers-hash",
                "platformSchema": { "com.linkedin.schema.OtherSchema": { "rawSchema": "" } },
                "fields": [
                    field("customer_id", "STRING", None, false),
                    field("segment", "STRING", None, false),
                ],
            } },
            "datasetProperties": { "value": { "description": "The customer dimension." } },
        },
    ])
}

/// The corpus's one relationship as a `semanticModel` entity, in the nested shape a real
/// `DataHub` actually keeps - inside `semanticModelInfo.value.relationships[]`, not as a
/// top-level aspect (measured 2026-09-16) - the same shape `sutura-cli`'s served suite seeds.
fn relationship_bodies() -> serde_json::Value {
    serde_json::json!([{
        "urn": "urn:li:semanticModel:(urn:li:dataPlatform:bigquery,PROD,orders_to_customer)",
        "semanticModelInfo": { "value": {
            "name": "orders_to_customer",
            "relationships": [{
                "name": "orders_to_customer",
                "from": "urn:li:dataset:(urn:li:dataPlatform:bigquery,orders,PROD)",
                "fromColumns": ["customer_id"],
                "to": "urn:li:dataset:(urn:li:dataPlatform:bigquery,customers,PROD)",
                "toColumns": ["customer_id"],
                "cardinality": "N_ONE",
            }],
        } },
    }])
}

/// **A `dataset` page served by a real `DataHub` preserves its wire shape.** Issue #1251's gap 2
/// measures the dataset page live the same way the metric page already was: the corpus's models
/// are upserted and read back through the search-backed paged surface, and each served entity is
/// asserted to harvest into the SAME [`DatasetAspect`] the seed does - so a platform that drops
/// or reshapes any field this crate maps (the URN's platform, `fieldPath`, `nativeDataType`,
/// `isPartOfKey`, `datasetProperties.description`) goes red. **The `dataset` half of the reader's
/// wire mapping is the one that was still asserted only against `docs/adr/0016`'s schema rather
/// than a live instance; this cell is what that limit pointed at.**
#[test]
#[ignore = "needs `just dev-up-datahub`; a docker service is only in the discovery file until \
            the next writer rewrites it - run `just datahub-acceptance`"]
fn a_dataset_page_served_by_a_real_datahub_preserves_its_wire_shape() {
    const DATASET_PATH: &str = "openapi/v3/entity/dataset?aspects=schemaMetadata&aspects=datasetProperties&count=1000";
    let Some(endpoint) = endpoint() else {
        return;
    };
    let agent = agent(false);
    let bodies = dataset_bodies();
    let (status, body) = send(
        &agent,
        &format!("http://{endpoint}/openapi/v3/entity/dataset?async=false&createIfNotExists=false"),
        Some(&bodies),
    );
    assert_eq!(status, 200, "the platform accepts the corpus's datasets: {body}");

    let wanted: Vec<String> = bodies
        .as_array()
        .expect("dataset_bodies is an array")
        .iter()
        .map(|entity| entity["urn"].as_str().expect("a seeded dataset carries its urn").to_owned())
        .collect();
    let served = paged_until(&agent, &endpoint, DATASET_PATH, &wanted);
    for entity in &served {
        let urn = entity["urn"].as_str().expect("a served dataset carries its urn");
        let expected = bodies
            .as_array()
            .expect("dataset_bodies is an array")
            .iter()
            .find(|seed| seed["urn"].as_str() == Some(urn))
            .map(harvest_dataset)
            .expect("the served urn is one we seeded");
        let harvested = harvest_dataset(entity);
        assert_eq!(
            &harvested, &expected,
            "dataset {urn} round-trips its wire shape through the live tier"
        );
    }
}

/// **A `semanticModel` relationship page served by a real `DataHub` preserves its wire shape.**
/// The relationship is upserted in the nested shape a real `DataHub` actually keeps - inside
/// `semanticModelInfo.value.relationships[]`, not as a top-level aspect (measured 2026-09-16) -
/// and the served page is asserted to harvest into the SAME [`RelationshipAspect`] the seed
/// does, over the paged surface a reader calls.
#[test]
#[ignore = "needs `just dev-up-datahub`; a docker service is only in the discovery file until \
            the next writer rewrites it - run `just datahub-acceptance`"]
fn a_relationship_page_served_by_a_real_datahub_preserves_its_wire_shape() {
    const RELATIONSHIP_PATH: &str = "openapi/v3/entity/semanticModel?aspects=semanticModelInfo&count=1000";
    let Some(endpoint) = endpoint() else {
        return;
    };
    let agent = agent(false);
    let body = relationship_bodies();
    let (status, response_body) = send(
        &agent,
        &format!("http://{endpoint}/openapi/v3/entity/semanticModel?async=false&createIfNotExists=false"),
        Some(&body),
    );
    assert_eq!(status, 200, "the platform accepts the corpus's relationship: {response_body}");

    let urn = body[0]["urn"]
        .as_str()
        .expect("a seeded semanticModel carries its urn")
        .to_owned();
    let served = paged_until(&agent, &endpoint, RELATIONSHIP_PATH, &[urn]);
    let expected = harvest_relationship(&body[0]);
    let harvested = harvest_relationship(&served[0]);
    assert_eq!(
        &harvested, &expected,
        "the served relationship's wire shape equals the seed's"
    );
}
