//! The docker `DataHub` tier harness for the wave-one E2E: mint a PAT, provision the certified
//! metric under the deployment's structured property, and wait for the tier's search index to
//! catch up before the served binary boots.
//!
//! **A mechanical split for the same reason `harness.rs` records.** `tests/served/e2e.rs` reached
//! the 1000-line cap `cargo xtask max-lines` holds, and the direction is the same: the HARNESS
//! moves, never a `#[test]`. Everything here is helper code for the wave-one cell and carries no
//! assertion, so moving it out does not change what `just causality` can see. It is reached only
//! through `--datahub tier`, which `just e2e-datahub-bigquery` drives by name.

use std::path::Path;
use std::time::{Duration, Instant};

use sutura_catalog_datahub::AspectReader as _;
use sutura_catalog_datahub::fixture::FixtureReader;
use sutura_catalog_datahub::test_support::DEPLOYMENT_PROPERTY;
use sutura_dev::provisioned;

/// How long GMS's SEARCH-BACKED entity-list surface may lag a synchronous write it has already
/// accepted - the same eventually-consistent gap
/// `crates/sutura-catalog-datahub/tests/provisioned.rs`'s `INDEX_LAG_BUDGET` measured at ~2s on
/// this tier (2026-09-04, `just datahub-acceptance`); an order of magnitude of slack for a
/// loaded (hosted) runner rather than a threshold anything is read off.
const INDEX_LAG_BUDGET: Duration = Duration::from_secs(30);

/// How long [`DatahubTier::wait_until_indexed`] sleeps between polls of the same surface.
const INDEX_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The REAL docker `DataHub` tier, in `--datahub tier` mode: the live GMS the composed binary's
/// HTTP `AspectReader` will read from. `DataDir::prepared` has already written the catalog's
/// `token_file` placeholder; this replaces it with a PAT minted at run time (never committed)
/// and provisions the certified metric under the deployment's structured property - the same
/// two writes `crates/sutura-catalog-datahub/tests/provisioned.rs` makes, so the recorded
/// corpus and this live document cannot drift.
///
/// **Fail-not-skip, twice over.** The CI job (and `just e2e-datahub-bigquery --datahub tier`)
/// brings the tier up with `xtask dev-up --with datahub`; if the discovery file names no
/// `datahub` service this `expect`s by name instead of returning - a tier somebody asked for
/// and did not get is the exact overstated control this wave exists to refuse. And both writes
/// assert `200` from the platform, so a tier that rejects the document never reports green.
pub(super) struct DatahubTier {
    pub(super) endpoint: String,
    agent: ureq::Agent,
}

impl DatahubTier {
    /// The published loopback endpoint, or a panic naming the missing tier. `provisioned::here`
    /// has already panicked in the required direction inside the CI job; on a developer machine
    /// it returns `None`, and this turns that into the same fail-not-skip shape rather than a
    /// silent fallback to the fake.
    pub(super) fn required() -> Self {
        let inside = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let endpoint = provisioned::here(inside, "datahub")
            .endpoint()
            .expect("`--datahub tier` was asked but no datahub service is in the discovery file - run `just e2e-datahub-bigquery --datahub tier` (it calls `xtask dev-up --with datahub`) or `just dev-up-datahub` first")
            .to_string();
        Self {
            endpoint,
            agent: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .http_status_as_error(false)
                    .max_redirects(0)
                    .timeout_global(Some(Duration::from_secs(30)))
                    .proxy(ureq::Proxy::try_from_env())
                    .build(),
            ),
        }
    }

    fn send(&self, path: &str, bearer: Option<&str>, body: Option<&serde_json::Value>) -> (u16, String) {
        let url = format!("http://{}/{path}", self.endpoint);
        let request = bearer.map_or_else(
            || self.agent.post(&url),
            |bearer| self.agent.post(&url).header("Authorization", format!("Bearer {bearer}")),
        );
        let mut response = match body {
            Some(json) => request
                .header("Content-Type", "application/json")
                .send(serde_json::to_string(json).expect("a probe body serializes")),
            None => request.send(String::new()),
        }
        .expect("the live DataHub answered, whatever it answered");
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().expect("the answer is text");
        (status, text)
    }

    fn get(&self, path: &str, bearer: &str) -> (u16, String) {
        let url = format!("http://{}/{path}", self.endpoint);
        let mut response = self
            .agent
            .get(&url)
            .header("Authorization", format!("Bearer {bearer}"))
            .call()
            .expect("the live DataHub answered, whatever it answered");
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().expect("the answer is text");
        (status, text)
    }

    /// Blocks until every `wanted` urn appears on `entity`'s PAGED list - the SAME surface and
    /// query shape `sutura_catalog_datahub::http::HttpAspectReader::fetch` reads at boot, not a
    /// read-by-urn - or panics naming what the index never caught up on.
    ///
    /// **Why this exists at all.** [`Self::provision`]'s writes above are synchronous
    /// (`async=false`, every one asserted `200`), but the surface a served deployment reads to
    /// build its catalog is search-backed and therefore only eventually consistent with those
    /// writes - `crates/sutura-catalog-datahub/tests/provisioned.rs`'s
    /// `one_response_carries_both` measured the same gap on a by-urn read vs. a paged one. A
    /// served boot that starts the instant `provision` returns races that index: on a
    /// sufficiently fast machine the race is always won, which is why this was GREEN locally and
    /// refused at boot only on a slower hosted runner ("metrics declares kinds its own content
    /// does not supply" - `Structure`/`Relationships` declared unconditionally by
    /// `DataHubCatalog::capabilities`, absent because the paged reads that populate them raced
    /// ahead of the index). This closes that race with a real check on the property the served
    /// process depends on - never a sleep.
    fn wait_until_indexed(&self, pat: &str, entity: &str, aspects: &[&str], wanted: &[String]) {
        let query = aspects
            .iter()
            .map(|aspect| format!("aspects={aspect}"))
            .collect::<Vec<_>>()
            .join("&");
        let path = format!("openapi/v3/entity/{entity}?{query}&count=1000");
        let deadline = Instant::now() + INDEX_LAG_BUDGET;
        loop {
            let (status, body) = self.get(&path, pat);
            assert_eq!(status, 200, "the live DataHub answers the paged {entity} read: {body}");
            let page: serde_json::Value = serde_json::from_str(&body).expect("the answer is json");
            let seen: Vec<&str> = page["entities"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item["urn"].as_str())
                .collect();
            if wanted.iter().all(|want| seen.contains(&want.as_str())) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the tier's {entity} list never indexed {wanted:?} within {INDEX_LAG_BUDGET:?} of \
                 a synchronous write that already answered 200 - it had {seen:?}. Either the \
                 search index is further behind than the ~2s this tier was measured at, or the \
                 served binary's boot-time read races provisioning on this runner"
            );
            std::thread::sleep(INDEX_POLL_INTERVAL);
        }
    }

    /// A metric entity's urn, the same shape `provisioned.rs` writes raw (`(`, `)`, `,`, `:` are
    /// legal in a path and the surface answers `200` to both spellings).
    fn metric_urn(id: &str) -> String {
        format!("urn:li:metric:(urn:li:dataPlatform:bigquery,orders,{id})")
    }

    /// A `dataset` entity's urn - one function rather than the format string repeated in
    /// [`Self::dataset_bodies`] (what is written) and [`Self::wait_until_indexed`]'s caller
    /// (what is polled for), so the two cannot drift apart.
    fn dataset_urn(name: &str) -> String {
        format!("urn:li:dataset:(urn:li:dataPlatform:bigquery,{name},PROD)")
    }

    /// The one relationship [`Self::relationship_bodies`] writes and provisioning then waits
    /// for - named once for the same reason [`Self::dataset_urn`] is a function rather than a
    /// literal repeated at both call sites.
    const RELATIONSHIP_URN: &str = "urn:li:semanticModel:(urn:li:dataPlatform:bigquery,PROD,orders_to_customer)";

    /// The served binary presents the PAT via the token file this populates (the one
    /// `DataDir::prepared` left as a placeholder). The tier signs with its OWN
    /// `DATAHUB_TOKEN_SERVICE_SIGNING_KEY` - headless GMS exposes no `/auth/*` minting surface,
    /// so this is the honest offline mint (see `dev/src/mint.rs`). Nothing is committed; the
    /// PAT lives only in the generated token file and the env that exported it, and an absent
    /// source is a named failure, never a silent 200.
    fn adopt_minted_pat(token_file: &Path) -> String {
        let source = std::env::var("SUTURA_DATAHUB_TOKEN_FILE").unwrap_or_else(|_| panic!(
            "`--datahub tier` needs SUTURA_DATAHUB_TOKEN_FILE - the path of the PAT the just task / nix app minted with `sutura-dev mint-pat` and exported"
        ));
        let pat = std::fs::read_to_string(&source)
            .unwrap_or_else(|problem| panic!("the tier-minted PAT file {source} is unreadable: {problem}"));
        let pat = pat.trim().to_owned();
        std::fs::write(token_file, &pat).expect("the tier-minted PAT is writable into the token_file");
        println!(
            "e2e-datahub-bigquery: adopted the tier's self-minted PAT into {token_file}",
            token_file = token_file.display()
        );
        pat
    }

    /// The two models the certified fixture metric needs - `orders` (carrying every column the
    /// metric's measure, time column and required filter name) and `customers` (the dimension's
    /// column) - as `DataHub` `dataset` entities. The shapes were MEASURED against the pinned
    /// 1.7.0 tier's validator, not guessed: `schemaMetadata` demands `schemaName`, `platform`,
    /// `version`, `hash`, a `platformSchema` union and, per field, `nativeDataType` plus a
    /// fully-qualified `type`/type union key; `test_support::dataset_page` (which the fake
    /// serves) has neither, so this file states the full valid shape once.
    fn dataset_bodies() -> serde_json::Value {
        let field = |path: &str, native: &str| {
            serde_json::json!({
                "fieldPath": path,
                "nativeDataType": native,
                "type": { "type": { "com.linkedin.schema.StringType": {} } },
            })
        };
        let dataset = |name: &str, fields: Vec<serde_json::Value>| {
            serde_json::json!({
                "urn": Self::dataset_urn(name),
                "schemaMetadata": { "value": {
                    "schemaName": name,
                    "platform": "urn:li:dataPlatform:bigquery",
                    "version": 0,
                    "hash": format!("{name}-hash"),
                    "platformSchema": { "com.linkedin.schema.OtherSchema": { "rawSchema": "" } },
                    "fields": fields,
                } },
                "datasetProperties": { "value": { "description": format!("The {name} model.") } },
            })
        };
        serde_json::json!([
            dataset(
                "orders",
                vec![
                    field("order_id", "INT64"),
                    field("customer_id", "STRING"),
                    field("amount_cents", "INT64"),
                    field("order_date", "DATE"),
                    field("status", "STRING"),
                ]
            ),
            dataset("customers", vec![field("customer_id", "STRING"), field("segment", "STRING"),]),
        ])
    }

    /// The one relationship the certified fixture metric's `segment` dimension reaches
    /// `customers` through, as a `DataHub` `semanticModel` entity. Real `DataHub` carries a
    /// joined relationship INSIDE `semanticModelInfo.value.relationships[]` - a top-level
    /// `semanticModelRelationship` aspect is accepted then silently dropped by GMS, measured
    /// against the pinned 1.7.0 tier - so this entity states the validated shape: a
    /// `semanticModelKey` (`platform`, `PROD`, `id`), and `semanticModelInfo` with the
    /// relationship's `from`/`fromColumns`/`to`/`toColumns`/`cardinality`. The same fields
    /// `test_support::relationship_page` (which the fake serves) carries, so the two transports
    /// serve one content and the served binary's `harvest_relationship` walks both identically.
    fn relationship_bodies() -> serde_json::Value {
        serde_json::json!([{
            "urn": Self::RELATIONSHIP_URN,
            "semanticModelInfo": { "value": {
                "name": "orders_to_customer",
                "relationships": [{
                    "name": "orders_to_customer",
                    "from": Self::dataset_urn("orders"),
                    "fromColumns": ["customer_id"],
                    "to": Self::dataset_urn("customers"),
                    "toColumns": ["customer_id"],
                    "cardinality": "N_ONE",
                }],
            } },
        }])
    }

    /// Provision the recorded corpus's OWN content - the property definition, the two datasets
    /// the metric's model names, the relationship the metric's dimension reaches through, and
    /// the certified metric - so the tier serves the SAME corpus the fake serves: every
    /// document the recorded fixture holds, in dependency order (datasets before the
    /// relationship and metric that name them). The same writes `provisioned.rs`'s
    /// `a_document_served_by_a_real_datahub_decodes_into_a_certified_metric` makes (plus, here,
    /// the datasets and relationship the served binary's full catalog read needs). The
    /// platform's validator accepts the corpus document as the scalar; a `200` is asserted for
    /// every write so a rejecting tier never reports green.
    pub(super) fn provision(&self, token_file: &Path) {
        // Auth first: the writes below carry the tier-minted PAT as their bearer.
        let pat = Self::adopt_minted_pat(token_file);
        let property_urn = format!("urn:li:structuredProperty:{DEPLOYMENT_PROPERTY}");
        let definition = serde_json::json!([{
            "urn": property_urn,
            "propertyDefinition": { "value": {
                "qualifiedName": DEPLOYMENT_PROPERTY,
                "displayName": DEPLOYMENT_PROPERTY,
                "valueType": "urn:li:dataType:datahub.string",
                "cardinality": "SINGLE",
                "entityTypes": ["urn:li:entityType:datahub.metric"],
                "description": "The closed-vocabulary metric document a deployment defines.",
            } },
        }]);
        let (status, body) = self.send(
            "openapi/v3/entity/structuredproperty?async=false",
            Some(&pat),
            Some(&definition),
        );
        assert_eq!(status, 200, "the live DataHub rejects the property definition: {body}");

        // The models the certified metric names, so the served binary's catalog read holds
        // together ("metric revenue names model orders, which is not declared" is the failure
        // this prevents).
        let (status, body) = self.send(
            "openapi/v3/entity/dataset?async=false&createIfNotExists=false",
            Some(&pat),
            Some(&Self::dataset_bodies()),
        );
        assert_eq!(status, 200, "the live DataHub rejects the corpus's datasets: {body}");

        // The relationship the metric's `segment` dimension reaches `customers` through, so the
        // served binary's bundle assembles ("dimension segment of metric revenue is reached via
        // relationship orders_to_customer, which is not declared" is the failure this prevents).
        let (status, body) = self.send(
            "openapi/v3/entity/semanticModel?async=false&createIfNotExists=false",
            Some(&pat),
            Some(&Self::relationship_bodies()),
        );
        assert_eq!(status, 200, "the live DataHub rejects the corpus's relationship: {body}");

        let recorded = FixtureReader
            .read()
            .expect("the recorded corpus reads - it is the same fixture the fake serves");
        let certified = recorded
            .metrics()
            .iter()
            .find(|metric| metric.sutura().is_some())
            .expect("the corpus carries one certified metric");
        let property = certified
            .sutura()
            .expect("the metric just found is the one carrying the property");
        let id = certified.name();
        let scalar = serde_json::json!([{ "string": property.string_value() }]);
        let entity = serde_json::json!([{
            "urn": Self::metric_urn(id),
            "metricKey": { "value": { "platform": "urn:li:dataPlatform:bigquery", "path": "orders", "id": id } },
            "metricInfo": { "value": {
                "name": id,
                "expression": { "dialects": [{ "dialect": certified.dialect(), "expression": certified.expression() }] },
            } },
            "structuredProperties": { "value": { "properties": [{ "propertyUrn": property_urn, "values": scalar }] } },
        }]);
        let (status, body) = self.send(
            "openapi/v3/entity/metric?async=false&createIfNotExists=false",
            Some(&pat),
            Some(&entity),
        );
        assert_eq!(status, 200, "the live DataHub rejects the certified metric document: {body}");

        // Every write above answered `200` synchronously, but the served binary's boot-time
        // catalog read pages the search-backed list surface, not these urns directly - wait for
        // the SAME surface to catch up before handing control back to a caller about to boot
        // that binary, so a slower (hosted) runner's index lag is a wait rather than a refusal.
        self.wait_until_indexed(
            &pat,
            "dataset",
            &["schemaMetadata"],
            &[Self::dataset_urn("orders"), Self::dataset_urn("customers")],
        );
        self.wait_until_indexed(
            &pat,
            "semanticModel",
            &["semanticModelInfo"],
            &[Self::RELATIONSHIP_URN.to_owned()],
        );
        self.wait_until_indexed(&pat, "metric", &["metricInfo"], &[Self::metric_urn(id)]);

        println!(
            "e2e-datahub-bigquery: provisioned the certified metric `{id}` under {DEPLOYMENT_PROPERTY} on {}",
            self.endpoint
        );
    }
}
