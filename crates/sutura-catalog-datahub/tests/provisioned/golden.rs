//! The golden catalog round trip: `examples/single-player/catalog` provisioned into a live `DataHub`
//! and read back through this crate's real reader (`http::HttpAspectReader`) into a
//! [`DataHubCatalog::load`] that must equal the golden definitions minus the named [`NOT_CARRIED`]
//! rows. Evidence only of the run that executed it. The tier is shared with the sibling cells, so
//! this cell writes under its own platform (`postgres`) and its own structured property and keeps
//! only those entities (see [`Ours`] on both). Knowledge is not compared: the adapter declares none
//! ([`NotCarried::Knowledge`]), so only definitions are compared.
//!
//! It drives the real reader, so it needs the crate's `http` feature - both acceptance entry points
//! (`just datahub-acceptance` and `nix run .#datahub-acceptance`) pass `--features http`.
//!
//! Audience and shared calendar agree only because every golden metric states the adapter's defaults
//! (`audience: open`, no shared calendar): `DataHubCatalog::convert_metric` hard-codes
//! `Audience::Open` (src/lib.rs) and never calls `with_shared_calendar`. So this cell says nothing
//! about a restricted or shared-calendar metric sourced from `DataHub` - such a metric is not carried,
//! and a `NOT_CARRIED` row cannot name it (its `bites` would fail, the golden states none).
//!
//! A row's subject is never provisioned, so if the reader learns to carry it (nullability, a
//! compound key) nothing reads that back - the row stays until someone removes it, and then the
//! cell says whether it is still needed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sutura_catalog_datahub::document::{Snapshot, SuturaAnchor, SuturaContent, SuturaDimension};
use sutura_catalog_datahub::http::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpAspectReader, ReadBounds};
use sutura_catalog_datahub::{AspectReader, DataHubCatalog, DataHubError};
use sutura_catalog_local::LocalCatalog;
use sutura_domain::catalog::{
    AnchorValue, Column, Definitions, Description, InconsistentDefinitions, JoinKey, Metric, Model, ViaChain,
};
use sutura_domain::identity::Secret;
use sutura_domain::model::{JoinType, SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};

use super::tests::{INDEX_LAG_BUDGET, agent, endpoint, pat, send};

/// A platform no sibling cell writes, so the shared tier's other datasets are told apart by it.
const PLATFORM: &str = "postgres";

/// Its own structured property, so the sibling cells' metrics stay uncertified for this reader.
const PROPERTY: &str = "golden_metric_document";

const SOURCE: &str = "local";
const VERSION: &str = "golden-fixture-1";

/// The golden catalog this cell provisions and reads back.
fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player/catalog")
}

/// What the golden states that this adapter does not carry back, one row each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotCarried {
    /// `HttpAspectReader` fills `Model.table` from the dataset urn's name segment, so table == name.
    Table,
    /// `schemaMetadata` nullability is not read, so every column comes back with none.
    Nullable,
    /// A compound-key relationship: the golden's `usage_subscription` has two keys, the second
    /// month-truncated, but this adapter's `RelationshipAspect` (`http.rs`'s `one_column`) carries
    /// one plain column per side. The limit is this adapter's, not `DataHub`'s - its
    /// `fromColumns`/`toColumns` are arrays.
    Relationship(&'static str),
    /// A metric whose dimension is reached through an uncarried relationship.
    Metric(&'static str),
    /// The adapter declares no knowledge, so only definitions are compared.
    Knowledge,
}

/// Audience and shared calendar agree only because every golden metric states the adapter's
/// defaults (`audience: open`, no shared calendar): `DataHubCatalog::convert_metric` hard-codes
/// `Audience::Open` (src/lib.rs) and never calls `with_shared_calendar`. So this cell says nothing
/// about a restricted or shared-calendar metric sourced from `DataHub` - such a metric is not
/// carried, and a `NOT_CARRIED` row cannot name it (its `bites` would fail, the golden states
/// none).
const NOT_CARRIED: [NotCarried; 5] = [
    NotCarried::Table,
    NotCarried::Nullable,
    NotCarried::Relationship("usage_subscription"),
    NotCarried::Metric("voice_minutes"),
    NotCarried::Knowledge,
];

impl NotCarried {
    /// Whether a row still names something this golden actually states, so a golden edit that drops
    /// the row's subject is caught by the not-ignored cell rather than silently carried.
    fn bites(self, golden: &PinnedDefinitions) -> bool {
        match self {
            Self::Table => golden
                .definitions()
                .models()
                .values()
                .any(|model| model.table_name().as_str() != model.name().as_str()),
            Self::Nullable => golden
                .definitions()
                .models()
                .values()
                .flat_map(Model::columns)
                .any(|column| column.nullable().is_some()),
            Self::Relationship(name) => golden.definitions().relationships().keys().any(|key| key.as_str() == name),
            Self::Metric(name) => golden.definitions().metrics().keys().any(|key| key.as_str() == name),
            Self::Knowledge => !golden.knowledge().declares().is_empty(),
        }
    }
}

/// The golden catalog, as the markdown adapter loads it.
fn golden() -> PinnedDefinitions {
    LocalCatalog::new(
        SourceName::parse(SOURCE).expect("a test source name is a name"),
        golden_root(),
        DefinitionVersion::parse(VERSION).expect("a test version is a version"),
    )
    .load()
    .expect("the golden catalog loads")
}

/// Whether `name` is a carried relationship or metric this golden states.
fn not_carried(rows: &[NotCarried], name: &str) -> bool {
    rows.iter()
        .any(|row| matches!(row, NotCarried::Relationship(n) | NotCarried::Metric(n) if *n == name))
}

/// The golden with `rows` applied - what the read-back must equal; an `Err` means the golden
/// does not hold together without them.
fn carried(golden: &Definitions, rows: &[NotCarried]) -> Result<Definitions, InconsistentDefinitions> {
    let models = golden
        .models()
        .values()
        .map(|model| {
            // The `Nullable` row: every column rebuilt with nullability dropped (the read-back gives none).
            let columns = model.columns().map(|column| {
                Column::new(
                    column.name().clone(),
                    column.data_type().cloned(),
                    Description::parse(column.description()).expect("the golden column prose parses"),
                    if rows.contains(&NotCarried::Nullable) {
                        None
                    } else {
                        column.nullable()
                    },
                )
            });
            // The `Table` row: the read-back fills `Model.table` from the urn's name segment, so table == name.
            let table = if rows.contains(&NotCarried::Table) {
                TableName::parse(model.name().as_str())
                    .expect("a model name is a table name")
                    .into()
            } else {
                model.table().clone()
            };
            Model::new(
                model.name().clone(),
                model.source().clone(),
                table,
                columns,
                Description::parse(model.description()).expect("the golden model prose parses"),
            )
            .with_primary_key(model.primary_key().iter().cloned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let relationships = golden
        .relationships()
        .values()
        .filter(|relationship| !not_carried(rows, relationship.name().as_str()))
        .cloned()
        .collect();
    let metrics = golden
        .metrics()
        .values()
        .filter(|metric| !not_carried(rows, metric.name().as_str()))
        .cloned()
        .collect();
    Definitions::assemble(models, relationships, metrics)
}

/// A dataset urn on this cell's own platform.
fn dataset_urn(name: &str) -> String {
    format!("urn:li:dataset:(urn:li:dataPlatform:{PLATFORM},{name},PROD)")
}

/// The golden artifact of one metric, spelled back into the deployment-defined document.
fn sutura_content(metric: &Metric) -> SuturaContent {
    let dimensions = metric
        .dimensions()
        .values()
        .map(|dimension| {
            SuturaDimension::new(
                dimension.name().clone(),
                dimension.column().clone(),
                dimension
                    .via()
                    .map(|hops| ViaChain::of(hops.to_vec()).expect("the golden chain is non-empty")),
                dimension.allowed_values().cloned(),
                Description::parse(dimension.description()).expect("the golden dimension prose parses"),
            )
        })
        .collect();
    SuturaContent::full(
        metric.model().as_str().to_owned(),
        metric.measure().expect("the golden is closed-vocabulary").clone(),
        metric.time_column().as_str().to_owned(),
        metric.grains().iter().copied().collect(),
        String::from(metric.description()),
        metric.required_filters().to_vec(),
        dimensions,
        metric.anchor().map(|anchor| {
            SuturaAnchor::new(
                anchor.range(),
                AnchorValue::parse(anchor.value()).expect("the golden anchor value parses"),
            )
        }),
    )
}

/// Provisions the carried catalog onto the tier, in the measured wire shapes (`wire_pages.rs`'s
/// `dataset_bodies`/`relationship_bodies` and `write_property`'s property definition), only the
/// values changed to this cell's platform, property and entities.
fn provision(agent: &ureq::Agent, endpoint: &str, carried: &Definitions) {
    // 1. This cell's own structured property, under `PROPERTY`.
    let definition = serde_json::json!([{
        "urn": format!("urn:li:structuredProperty:{PROPERTY}"),
        "propertyDefinition": { "value": {
            "qualifiedName": PROPERTY,
            "displayName": PROPERTY,
            "valueType": "urn:li:dataType:datahub.string",
            "cardinality": "SINGLE",
            "entityTypes": ["urn:li:entityType:datahub.metric"],
            "description": "The certified catalog document this golden round trip defines.",
        } },
    }]);
    let (status, body) = send(
        agent,
        &format!("http://{endpoint}/openapi/v3/entity/structuredproperty?async=false"),
        Some(&definition),
    );
    assert_eq!(status, 200, "the platform accepts the golden document property: {body}");

    // 2. All datasets in ONE POST, a full `schemaMetadata` the pinned tier's validator accepts.
    let datasets = serde_json::json!(
        carried
            .models()
            .values()
            .map(|model| {
                let fields = model.columns().map(|column| {
                    let mut field = serde_json::json!({
                        "fieldPath": column.name().as_str(),
                        "nativeDataType": column.data_type().expect("the golden types every column").as_str(),
                        "type": { "type": { "com.linkedin.schema.StringType": {} } },
                    });
                    if !column.description().is_empty() {
                        field["description"] = serde_json::json!(column.description());
                    }
                    if model.primary_key().contains(column.name()) {
                        field["isPartOfKey"] = serde_json::json!(true);
                    }
                    field
                });
                serde_json::json!({
                    "urn": dataset_urn(model.name().as_str()),
                    "schemaMetadata": { "value": {
                        "schemaName": model.name().as_str(),
                        "platform": format!("urn:li:dataPlatform:{PLATFORM}"),
                        "version": 0,
                        "hash": format!("{}-hash", model.name().as_str()),
                        "platformSchema": { "com.linkedin.schema.OtherSchema": { "rawSchema": "" } },
                        "fields": fields.collect::<Vec<_>>(),
                    } },
                    "datasetProperties": { "value": { "description": model.description() } },
                })
            })
            .collect::<Vec<_>>()
    );
    let (status, body) = send(
        agent,
        &format!("http://{endpoint}/openapi/v3/entity/dataset?async=false&createIfNotExists=false"),
        Some(&datasets),
    );
    assert_eq!(status, 200, "the platform accepts the golden catalog's datasets: {body}");

    // 3. All carried relationships (the compound `usage_subscription` is NOT carried) in ONE POST.
    let relationships =
        serde_json::json!(carried.relationships().values().map(|relationship| {
        let keys: Vec<_> = relationship.keys().iter().collect();
        let [JoinKey::Equal { origin, target }] = keys.as_slice() else {
            panic!("a carried relationship has one plain key");
        };
        serde_json::json!({
            "urn": format!("urn:li:semanticModel:(urn:li:dataPlatform:{PLATFORM},PROD,{})", relationship.name().as_str()),
            "semanticModelInfo": { "value": {
                "name": relationship.name().as_str(),
                "relationships": [{
                    "name": relationship.name().as_str(),
                    "from": dataset_urn(relationship.origin_model().as_str()),
                    "fromColumns": [origin.as_str()],
                    "to": dataset_urn(relationship.target_model().as_str()),
                    "toColumns": [target.as_str()],
                    "cardinality": match relationship.join_type() {
                        JoinType::ManyToOne => "N_ONE",
                        JoinType::OneToMany => "ONE_N",
                        JoinType::OneToOne => "ONE_ONE",
                    },
                }],
            } },
        })
    }).collect::<Vec<_>>());
    let (status, body) = send(
        agent,
        &format!("http://{endpoint}/openapi/v3/entity/semanticModel?async=false&createIfNotExists=false"),
        Some(&relationships),
    );
    assert_eq!(status, 200, "the platform accepts the golden catalog's relationships: {body}");

    // 4. All carried metrics in ONE POST, each under this cell's property.
    let metrics = serde_json::json!(carried.metrics().values().map(|metric| {
        let content = sutura_content(metric);
        serde_json::json!({
            "urn": format!("urn:li:metric:(urn:li:dataPlatform:{PLATFORM},{},{})", metric.model().as_str(), metric.name().as_str()),
            "metricKey": { "value": { "platform": format!("urn:li:dataPlatform:{PLATFORM}"), "path": metric.model().as_str(), "id": metric.name().as_str() } },
            "metricInfo": { "value": {
                "name": metric.name().as_str(),
                "expression": { "dialects": [{ "dialect": "ANSI_SQL", "expression": metric.name().as_str() }] },
            } },
            "structuredProperties": { "value": { "properties": [{
                "propertyUrn": format!("urn:li:structuredProperty:{PROPERTY}"),
                "values": [{ "string": serde_json::to_string(&content).expect("the certified content serializes") }],
            }] } },
        })
    }).collect::<Vec<_>>());
    let (status, body) = send(
        agent,
        &format!("http://{endpoint}/openapi/v3/entity/metric?async=false&createIfNotExists=false"),
        Some(&metrics),
    );
    assert_eq!(status, 200, "the platform accepts the golden catalog's metrics: {body}");
}

/// `HttpAspectReader` reads the whole tier, and the sibling cells write to it concurrently, so this
/// keeps the entities this cell wrote: datasets on [`PLATFORM`], relationships it named. Metrics
/// need no filter - only `PROPERTY` certifies one for this reader, and an uncertified metric is
/// skipped by the load. That holds on a fresh tier (CI); on a reused local tier metrics an earlier
/// run wrote under `PROPERTY` stay certified and are read back, so a provisioning change can fail
/// closed until `just dev-down` resets the tier.
struct Ours {
    reader: HttpAspectReader,
    relationships: Vec<String>,
}

impl AspectReader for Ours {
    fn read(&self) -> Result<Snapshot, DataHubError> {
        let all = self.reader.read()?;
        Ok(Snapshot::new(
            all.datasets()
                .iter()
                .filter(|dataset| dataset.platform() == PLATFORM)
                .cloned()
                .collect(),
            all.relationships()
                .iter()
                .filter(|relationship| self.relationships.iter().any(|name| name.as_str() == relationship.name()))
                .cloned()
                .collect(),
            all.metrics().to_vec(),
        ))
    }
}

/// Runs in `just test`: every [`NOT_CARRIED`] row must still name something the golden states, so a
/// golden edit that neutralises a row's subject deletes it loudly here rather than silently shrinks
/// the round trip.
#[test]
fn every_not_carried_row_still_names_something_the_golden_states() {
    let golden = golden();
    for row in NOT_CARRIED {
        assert!(
            row.bites(&golden),
            "`{row:?}` exempts nothing the golden states - delete the row, it no longer earns its place"
        );
    }
}

/// Provisions [`golden_root`] onto the live tier and reads it back through the real reader.
#[test]
#[ignore = "needs `just dev-up-datahub`; a docker service is only in the discovery file until \
            the next writer rewrites it - run `just datahub-acceptance`"]
fn the_golden_catalog_round_trips_through_a_live_datahub() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let golden = golden();
    let expected = carried(golden.definitions(), &NOT_CARRIED).expect("the carried golden still holds together");
    provision(&agent(false), &endpoint, &expected);

    let reader = HttpAspectReader::new(
        Endpoint::parse(&format!("http://{endpoint}")).expect("the loopback endpoint parses"),
        String::from(PROPERTY),
        Secret::new(pat()),
        ReadBounds::parse(DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES).expect("the default bounds are valid"),
        None,
    );
    let mut sources = BTreeMap::new();
    drop(sources.insert(
        String::from(PLATFORM),
        SourceName::parse(SOURCE).expect("a source name is a name"),
    ));
    let catalog = DataHubCatalog::new(
        SourceName::parse(SOURCE).expect("a source name is a name"),
        DefinitionVersion::parse(VERSION).expect("a version is a version"),
        sources,
        Ours {
            reader,
            relationships: expected.relationships().keys().map(ToString::to_string).collect(),
        },
    );
    // The paged surface is search-backed and lags a synchronous write (~2 s measured), so the read
    // is retried until it matches or `INDEX_LAG_BUDGET` runs out; the assertion after the loop
    // reports a mismatch.
    let deadline = Instant::now() + INDEX_LAG_BUDGET;
    let read = loop {
        let read = catalog.load();
        if read.as_ref().is_ok_and(|pinned| pinned.definitions() == &expected) || Instant::now() >= deadline {
            break read;
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let read = read.unwrap_or_else(|error| {
        panic!("the provisioned golden did not load through HttpAspectReader within {INDEX_LAG_BUDGET:?}: {error:?}")
    });
    assert_eq!(
        read.definitions(),
        &expected,
        "the certified catalog a live DataHub served back is the golden minus NOT_CARRIED"
    );
    if !NOT_CARRIED.contains(&NotCarried::Knowledge) {
        assert_eq!(
            read.knowledge(),
            golden.knowledge(),
            "the knowledge a live DataHub served back is the golden's"
        );
    }
    // A row the read-back does not need is refused: without it the expectation must differ
    // from what came back (or not hold together), else it silently shrinks the round trip.
    for row in NOT_CARRIED {
        let others: Vec<NotCarried> = NOT_CARRIED.into_iter().filter(|other| *other != row).collect();
        let needed = match row {
            NotCarried::Knowledge => read.knowledge() != golden.knowledge(),
            _ => carried(golden.definitions(), &others).map_or(true, |without| &without != read.definitions()),
        };
        assert!(needed, "`{row:?}` exempts nothing the read-back drops - delete the row");
    }
}
