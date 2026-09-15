//! Wave one of the identity-aware E2E (`just e2e-datahub-bigquery`): `DataHub` carries the certified
//! metric's definition, a REAL Keycloak issuer's token says who is asking, and the certified
//! question executes over HTTP `/v1/query`, on the composed `sutura-serve` binary, against a REAL
//! `BigQuery` project - the same `orders`/`customers` corpus `served/datahub.rs` certifies against,
//! loaded fresh into that project by this file rather than by a developer's own fixture.
//!
//! # The wave's claim, and the one thing this file does not claim
//!
//! Issue #134's whole path is **data metadata → a real issuer's token → a source that executes**.
//! This one runnable cell proves the first two and the source's EXECUTION under one shared
//! credential; it does not claim the source executes AS the asking subject - that per-subject leg
//! (`SESSION_USER()` naming the caller) is `crates/sutura-exec-bigquery/tests/exchanged_identity.rs`'s
//! own `#[ignore]`d cell, behind the maintainer's binding (issue #376 P2), and this task does not
//! invoke it. `docs/where-identity-is-proven.md` keeps that half `unrun`.
//!
//! **`the_wave_one_path_answers_a_verified_caller_under_the_shared_key`** boots a deployment once and asks it as
//! principal A, with an uncertified request, and as principal B - same binary, same settings file,
//! same catalog, three asks. It proves: (1) a `catalog.kind: datahub` deployment serves the
//! certified metric, harvested from the fake `DataHub`, over a real issuer's verified token AND
//! answered from a real `bigquery` source; (2) an uncertified question is a typed refusal, never
//! `200`; (3) two different provisioned subjects produce two different audit `subject`s, even
//! though the SOURCE executes as one shared identity for both. `DataHub` is the recorded fake
//! (`#202`'s `test_support`), the source is `bigquery` named identically to the catalog (the served
//! `datahub` arm's fixed `bigquery`→name mapping, exactly as `served/datahub.rs` proves green for
//! `files`), and the issuer is the provisioned Keycloak tier.
//!
//! # What is ONE cell rather than three
//!
//! The runnable wave is one `#[ignore]`d cell that boots the deployment once and asks three things.
//! Each `#[test]` would re-boot the deployment, and a boot RE-EXECUTES every anchor against the
//! engine (`served/harness.rs`'s `START_BUDGET` doc says so) plus reads the catalog's pages from
//! the fake another time - no additional evidence for the money. The two-subject property
//! especially needs one boot: the point is that one deployment answers two different callers as two
//! different subjects, which three boots would not show.
//!
//! # The dependency split, stated next to the claim
//!
//! This file compiles only inside `served.rs`'s `#[cfg(feature = "datahub")]` +
//! `#[cfg(feature = "bigquery")]` module declaration, and is `#[ignore]`d so none of it runs on
//! `just test`. Each dependency marks its requirement:
//!
//! - **`// requires #202`** (PR1 `#720` landed, PR2 `#750` is the base) - `catalog.kind: datahub`
//!   served, the settings keys `endpoint`/`token_file`/`metric_property`, and
//!   `sutura_catalog_datahub::test_support::{FakeServer, happy_path_answers, DEPLOYMENT_PROPERTY}`.
//! - **`// requires the provisioned Keycloak tier`** - `harness::keycloak_settings` (the
//!   `KeycloakFixture`) and the realm the tier writes at `start`.
//! - **A real `BigQuery` project** - `GOOGLE_APPLICATION_CREDENTIALS` and `SUTURA_BQ_DATASET` in the
//!   environment `just e2e-datahub-bigquery` runs under (direnv, on a developer's machine). Read the
//!   same way `crates/sutura-exec-bigquery/tests/support/support.rs`'s `Connection::required` reads
//!   them, and FAILING the same way when either is absent: this cell would otherwise report PASS
//!   over an engine that never touched `BigQuery`, which is the exact overstated control this wave
//!   exists to not repeat.
//!
//! # RED/GREEN
//!
//! **The cell is `#[ignore]`d, so `just test` does not reach it** - the task
//! (`just e2e-datahub-bigquery`, or the nix app `apps.e2e-datahub-bigquery`) brings the Keycloak
//! tier up and runs it by name. Mutations this cell guards: the served `datahub` arm rolled back to
//! its unconditional refusal (RED - the deployment never boots, the `refused_to_start` shape); the
//! Keycloak fixture or the realm reader removed (RED - does not compile / the provider is gone); the
//! audit `subject` collapsing to the deployment's own (RED - two provisioned subjects no longer
//! differ); the `bigquery` source rolled back to `kind: "files"` (RED - the rows and `executed_as`
//! this cell pins are read straight off the tables THIS file loads into `BigQuery`, so a `files`
//! source reading nothing this file wrote answers wrong or not at all); `GOOGLE_APPLICATION_CREDENTIALS`
//! or `SUTURA_BQ_DATASET` unset (FAILS by name, never silently skips). GREEN is this file as written.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use std::time::Duration;
    use sutura_catalog_datahub::AspectReader as _;
    use sutura_catalog_datahub::fixture::FixtureReader;
    use sutura_catalog_datahub::test_support::{DEPLOYMENT_PROPERTY, FakeServer, happy_path_answers};
    use sutura_dev::provisioned;
    use sutura_domain::model::{SourceName, TableName};
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_exec_bigquery::BigQueryWarehouse;
    use sutura_exec_bigquery::transport::{DatasetId as BqDatasetId, ProjectId as BqProjectId};
    use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
    use sutura_exec_bigquery::wire::{BigQueryWire, BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};

    // requires the provisioned Keycloak tier
    use crate::harness::keycloak::KeycloakFixture;
    use crate::harness::{
        LOOPBACK, RECORD, VERSION, config_path, derived_beside, keycloak_settings, keycloak_subject_of, start_configured, v1,
    };

    /// The `case` string this file's deployment owns - handed to [`keycloak_settings`] (which writes
    /// its fetched key set beside it) and to [`start_configured`] (which writes the settings file and
    /// clears only the config dir, a sibling of that key set - `served/harness.rs`'s
    /// `derived_beside`). The two must agree, because the `inbound` block this file writes names the
    /// key set at exactly the path the fixture wrote.
    const CASE: &str = "e2e-datahub-bigquery";

    /// The catalog's declared name, reused verbatim as the `sources:` entry's name - the served
    /// `datahub` arm fixes the `bigquery` dataPlatform→source mapping to the CATALOG's OWN name
    /// (`crates/sutura-serve/src/catalog.rs`), so a source under any other name is unreachable by
    /// the harvested models. The same identity `served/datahub.rs` uses.
    const CATALOG: &str = "metrics";

    /// The two `BigQuery` table names the fake's harvested schema names - `orders` (the measure, the
    /// time column, the required filter) and `customers` (the dimension) - reused verbatim because
    /// the served `datahub` arm resolves a harvested model to a PHYSICAL table of the same name in
    /// the declared source's own dataset.
    const ORDERS_TABLE: &str = "orders";
    const CUSTOMERS_TABLE: &str = "customers";

    /// One `BigQuery` environment variable, or a panic naming it.
    ///
    /// **Fail-not-skip, the same argument
    /// `crates/sutura-exec-bigquery/tests/support/support.rs`'s `named` makes for the adapter's own
    /// acceptance leg:** this cell is reached only by name (`#[ignore]`, run via
    /// `just e2e-datahub-bigquery`), so a developer who asked for the real `BigQuery` leg and got a
    /// green report over an engine that never touched it has been told the opposite of the truth.
    fn bq_named(key: &str, what: &str) -> String {
        match std::env::var(key) {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(_) => panic!(
                "{key} is not set - it names {what}. `the_wave_one_path_answers_a_verified_caller_under_the_shared_key` \
                 executes the certified metric over a real BigQuery project under one shared credential; \
                 see docs/showcase-datahub-bigquery.md."
            ),
        }
    }

    /// The bounds every job this file submits runs under - the same 30s/1GiB shape
    /// `crates/sutura-exec-bigquery/tests/support/support.rs`'s `bounds` uses, for the same reason:
    /// this is the one path in this test suite that spends real money.
    fn bounds() -> JobBounds {
        JobBounds::of(
            QueryDeadline::parse(30).expect("thirty seconds is a deadline"),
            BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a ceiling"),
        )
    }

    /// The adapter this file loads/drops fixture tables through and the deployment's own settings
    /// point the served binary at independently.
    type Wired = BigQueryWarehouse<BigQueryWire<Credential>>;

    /// The real `BigQuery` project this wave executes the certified metric against, read from the
    /// environment the same way `crates/sutura-exec-bigquery/tests/support/support.rs`'s
    /// `Connection::required` reads it: the credential's OWN project first, then
    /// `SUTURA_BQ_BILLING_PROJECT`, and a panic naming whichever is missing.
    struct BigQueryFixture {
        billing_project: String,
        dataset: String,
        credential_file: PathBuf,
        warehouse: Wired,
    }

    impl BigQueryFixture {
        fn required() -> Self {
            let credential_file = PathBuf::from(bq_named(
                "GOOGLE_APPLICATION_CREDENTIALS",
                "the service-account (or application-default) key this wave reads BigQuery under - \
                 one shared credential, never a per-subject one",
            ));
            let dataset = bq_named(
                "SUTURA_BQ_DATASET",
                "the dataset this wave loads its own `orders`/`customers` fixture tables into and \
                 answers the certified metric from",
            );
            let agent = WireAgent::pinned(bounds());
            let credentials = Credential::read(&CredentialFile::at(credential_file.clone()), agent.clone())
                .unwrap_or_else(|cause| panic!("GOOGLE_APPLICATION_CREDENTIALS names an unreadable credential: {cause}"));
            // Printed so a green run says WHICH identity produced it - the same reason
            // `support::Connection::required` prints it.
            println!("e2e-datahub-bigquery: BigQuery credential kind {}", credentials.kind());
            let billing_project = credentials.project().cloned().unwrap_or_else(|| {
                bq_named(
                    "SUTURA_BQ_BILLING_PROJECT",
                    "this credential names no project of its own, so the billing project has to be set",
                )
            });
            let posture = SourcePosture::SharedServiceUser {
                declared: SharedIdentityDeclared::of(
                    AcknowledgementReason::parse(
                        "the wave's one shared credential reaching the dataset for whoever the real issuer verified",
                    )
                    .expect("an acknowledgement is an acknowledgement"),
                ),
            };
            let warehouse = BigQueryWarehouse::new(
                SourceName::parse(CATALOG).expect("metrics is a usable source name"),
                posture,
                BqProjectId::parse(&billing_project).expect("a project id parses"),
                BqDatasetId::parse(&dataset).expect("a dataset id parses"),
                BigQueryWire::new(agent, credentials),
            );
            Self {
                billing_project,
                dataset,
                credential_file,
                warehouse,
            }
        }
    }

    /// The wave's OWN two `BigQuery` tables - `orders` and `customers` - loaded fresh from the same
    /// CSVs [`DataDir`] writes for the fake `DataHub`'s harvested schema, and removed when the test is
    /// done.
    ///
    /// **Not a developer-provided table.** This wave creates what it reads, the same shape
    /// `crates/sutura-exec-bigquery/tests/corpus.rs`'s `load_the_corpus`/`drop_the_corpus` use for
    /// its own fixture tables, so the certified metric answers from a REAL project rather than from
    /// a table nobody but a developer's own environment could point at.
    struct LoadedFixture<'a> {
        warehouse: &'a Wired,
        orders: TableName,
        customers: TableName,
    }

    impl<'a> LoadedFixture<'a> {
        fn loaded(warehouse: &'a Wired, data: &DataDir) -> Self {
            let orders = TableName::parse(ORDERS_TABLE).expect("orders is a usable table name");
            let customers = TableName::parse(CUSTOMERS_TABLE).expect("customers is a usable table name");
            let orders_rows = warehouse
                .load_fixture(&orders, &data.orders_csv())
                .unwrap_or_else(|cause| panic!("the orders fixture did not load into BigQuery: {cause:?}"));
            assert!(orders_rows > 0, "the orders fixture carried no rows");
            let customers_rows = warehouse
                .load_fixture(&customers, &data.customers_csv())
                .unwrap_or_else(|cause| panic!("the customers fixture did not load into BigQuery: {cause:?}"));
            assert!(customers_rows > 0, "the customers fixture carried no rows");
            println!(
                "e2e-datahub-bigquery: loaded {orders_rows} rows into `{ORDERS_TABLE}`, {customers_rows} into `{CUSTOMERS_TABLE}`"
            );
            Self {
                warehouse,
                orders,
                customers,
            }
        }
    }

    impl Drop for LoadedFixture<'_> {
        fn drop(&mut self) {
            // Best-effort: a drop failure is reported rather than a second panic over a value
            // already unwinding. The 24-hour expiration `sutura_exec_bigquery::importer` sets on
            // every `CREATE` is the backstop for whatever this cannot reach - the same guarantee
            // `tests/corpus.rs` states for its own per-run tables.
            if let Err(cause) = self.warehouse.drop_table(&self.orders) {
                eprintln!("e2e-datahub-bigquery: the `{ORDERS_TABLE}` fixture table did not drop: {cause:?}");
            }
            if let Err(cause) = self.warehouse.drop_table(&self.customers) {
                eprintln!("e2e-datahub-bigquery: the `{CUSTOMERS_TABLE}` fixture table did not drop: {cause:?}");
            }
        }
    }

    /// The data directory this case owns and removes on every path out, the same promise
    /// `served/datahub.rs`'s `DataDir` makes - it holds the CSVs this file loads into REAL `BigQuery`
    /// tables (named after the harvested models) plus the catalog's token file, a sibling of the
    /// settings directory so `written()` (via `start_configured`) cannot wipe it.
    struct DataDir(PathBuf);

    impl DataDir {
        fn prepared(case: &str) -> Self {
            let path = derived_beside(&config_path(case));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the data directory is creatable");
            // `orders`'s rows, byte for byte `served/datahub.rs`'s `ORDERS_CSV`: three active orders
            // inside the certified anchor's June window summing to its declared value, one cancelled
            // order (excluded by the metric's status filter) and one active May order (excluded by
            // the question's range) - so a filter or a range mistake moves the total rather than
            // leaving it right by accident.
            std::fs::write(
                path.join("orders.csv"),
                "order_id,customer_id,amount_cents,order_date,status\n\
                 O1,C1,200000,2026-06-01,active\n\
                 O2,C2,150000,2026-06-01,active\n\
                 O3,C1,62345,2026-06-01,active\n\
                 O4,C2,999999,2026-06-01,cancelled\n\
                 O5,C1,111111,2026-05-01,active\n",
            )
            .expect("orders.csv is writable");
            std::fs::write(path.join("customers.csv"), "customer_id,segment\nC1,retail\nC2,wholesale\n")
                .expect("customers.csv is writable");
            std::fs::write(path.join("token"), "pat-under-test").expect("the token file is writable");
            Self(path)
        }

        fn token_file(&self) -> PathBuf {
            self.0.join("token")
        }

        fn orders_csv(&self) -> PathBuf {
            self.0.join("orders.csv")
        }

        fn customers_csv(&self) -> PathBuf {
            self.0.join("customers.csv")
        }
    }

    impl Drop for DataDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

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
    struct DatahubTier {
        endpoint: String,
        agent: ureq::Agent,
    }

    impl DatahubTier {
        /// The published loopback endpoint, or a panic naming the missing tier. `provisioned::here`
        /// has already panicked in the required direction inside the CI job; on a developer machine
        /// it returns `None`, and this turns that into the same fail-not-skip shape rather than a
        /// silent fallback to the fake.
        fn required() -> Self {
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

        /// A metric entity's urn, the same shape `provisioned.rs` writes raw (`(`, `)`, `,`, `:` are
        /// legal in a path and the surface answers `200` to both spellings).
        fn metric_urn(id: &str) -> String {
            format!("urn:li:metric:(urn:li:dataPlatform:bigquery,orders,{id})")
        }

        /// Mint a personal-access token through the tier's token surface and write it to the token
        /// file the settings name (the one `DataDir::prepared` left as a placeholder). The seed
        /// admin credential the compose profile seeds is read from the environment the tier ran
        /// under; nothing is committed, and an absent PAT read is a named failure downstream.
        fn mint_pat(&self, data: &DataDir, admin_user: &str, admin_password: &str) -> String {
            let session = serde_json::json!({
                "username": admin_user, "password": admin_password,
            });
            let (status, body) = self.send("auth/authenticate", None, Some(&session));
            assert_eq!(status, 200, "the live DataHub refused the seeded admin credential: {body}");
            let session =
                serde_json::from_str::<serde_json::Value>(&body).expect("the authentication answer is JSON")["accessToken"]
                    .as_str()
                    .expect("the authentication answer carries an access token")
                    .to_owned();
            let mint = serde_json::json!({
                "actorUrn": format!("urn:li:corpuser:{admin_user}"),
                "type": "PERSONAL",
                "durationInMinutes": 60,
                "name": "wave-one-e2e",
            });
            let (status, body) = self.send("auth/accessTokens", Some(&session), Some(&mint));
            assert_eq!(status, 200, "the live DataHub refused to mint a PAT: {body}");
            let minted = serde_json::from_str::<serde_json::Value>(&body).expect("the minting answer is JSON");
            let pat = minted["accessToken"]
                .as_str()
                .expect("the minting answer carries an access token");
            std::fs::write(data.token_file(), pat).expect("the minted PAT is writable into the token_file");
            println!(
                "e2e-datahub-bigquery: minted a PAT into {token_file}",
                token_file = data.token_file().display()
            );
            pat.to_owned()
        }

        /// Provision the recorded corpus's OWN certified metric under the deployment's structured
        /// property - the property definition plus the metric entity, the same two writes
        /// `provisioned.rs`'s `a_document_served_by_a_real_datahub_decodes_into_a_certified_metric`
        /// makes. The platform's validator accepts the corpus document as the scalar; a `200` is
        /// asserted for both writes so a rejecting tier never reports green.
        fn provision(&self, admin_user: &str, admin_password: &str, data: &DataDir) {
            // Auth first: the writes below carry the minted PAT as their bearer.
            let pat = self.mint_pat(data, admin_user, admin_password);
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
            println!(
                "e2e-datahub-bigquery: provisioned the certified metric `{id}` under {DEPLOYMENT_PROPERTY} on {}",
                self.endpoint
            );
        }
    }
    fn settings(fixture: &KeycloakFixture, endpoint: &str, token_file: &Path, bq: &BigQueryFixture) -> String {
        let key_set = derived_beside(&config_path(CASE)).join("keycloak-jwks.json");
        format!(
            "server:\n\
             {LOOPBACK}\
             security:\n\
             {SECURITY_HEAD}\
             {inbound}\
             telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"{CATALOG}\"\n    \
                 kind: \"datahub\"\n    \
                 dir: \"/unused-for-datahub\"\n    \
                 data_dir: \"/unused-for-datahub\"\n    \
                 version: \"{VERSION}\"\n    \
                 endpoint: \"{endpoint}\"\n    \
                 token_file: \"{token_file}\"\n    \
                 metric_property: \"{DEPLOYMENT_PROPERTY}\"\n\
             sources:\n  \
               {CATALOG}:\n    \
                 kind: \"bigquery\"\n    \
                 billing_project: \"{billing_project}\"\n    \
                 dataset: \"{dataset}\"\n    \
                 credential_file: \"{credential_file}\"\n    \
                 max_bytes_billed: 1073741824\n    \
                 posture: \"shared-service-user\"\n",
            inbound = inbound_block(fixture, &key_set),
            endpoint = endpoint,
            token_file = token_file.display(),
            billing_project = bq.billing_project,
            dataset = bq.dataset,
            credential_file = bq.credential_file.display(),
        )
    }

    /// The `security.inbound` block. The audience is the fixture's own `resource` (read off the
    /// minted token), the issuer comes from the realm document the fixture fetched, and
    /// `algorithms` pins `RS256` because that is what the tier's generated keys verify with -
    /// every fact from the fixture, never copied. `token_type: "any"` for the same measured reason
    /// `harness/keycloak.rs`'s `settings_naming` carries it: the tier mints `typ: JWT`, not the
    /// `at+jwt` the default would refuse.
    fn inbound_block(fixture: &KeycloakFixture, key_set: &Path) -> String {
        format!(
            "  inbound:\n    mode: \"direct\"\n    resource: \"{resource}\"\n    \
             authorization_server: \"{issuer}\"\n    key_set_file: \"{key_set}\"\n    \
             algorithms: [\"RS256\"]\n    token_type: \"any\"\n",
            resource = fixture.resource,
            issuer = fixture.issuer,
            key_set = key_set.display(),
        )
    }

    /// The head of the `security:` block this deployment makes: `single-user` identity - the same
    /// head every other proving-green served cell in this suite uses (`served/datahub.rs`,
    /// `harness/keycloak.rs`'s `settings_naming`) - so the source executes as this process under one
    /// shared credential, and who is asking is still established per request through `inbound` and
    /// recorded in the audit `subject`. **The per-subject EXECUTION is not claimed here**: that is
    /// `crates/sutura-exec-bigquery/tests/exchanged_identity.rs`'s own cell, behind #376 P2, and
    /// `docs/where-identity-is-proven.md` keeps it `unrun`. `single-user` refuses a missing
    /// `single_user_because`, so one is written.
    const SECURITY_HEAD: &str = "  identity: \"single-user\"\n  single_user_because: \"wave one's served \
                                fixture reads a real BigQuery project as one shared credential, whoever \
                                asks - the per-subject exchange is #376 P2\"\n";

    /// The one question this path asks as both principals, over exactly the certified metric's
    /// anchor range - the same shape `served/datahub.rs`'s `QUESTION` holds, so the answer this
    /// file pins is the same value that file already proves.
    const QUESTION: &str = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

    /// A question this catalog does NOT certify - the wave's refusal leg, presented with a VALID
    /// principal token so what is refused is the question, not the caller's signature. The metric
    /// name is deliberately not one the recorded corpus defines.
    const UNCERTIFIED: &str =
        r#"{"metric":"definitely_not_certified","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

    /// The refusal `examples/wave-one/refusal.json` pins - compared by FIELD, not by the mere
    /// presence of a `code`/`detail` key, so a status or code drift on either side is caught.
    const REFUSAL_STATUS: u16 = 404;
    const REFUSAL_CODE: &str = "metric_unknown";

    /// The `subject` field of one bunyan record line - the same read `served.rs`'s keycloak cell
    /// makes: two real Keycloak subjects mint two different `sub` claims, and this is how the audit
    /// assertion tells whether the record carries the ASKER'S subject.
    fn subject_field(line: &str) -> &str {
        let after = line
            .split_once(r#""subject":""#)
            .map_or_else(|| panic!("no `subject` field in the record: {line}"), |(_, rest)| rest);
        after
            .split_once('"')
            .map_or_else(|| panic!("the `subject` field is not terminated: {line}"), |(value, _)| value)
    }

    #[test]
    #[ignore = "needs the Keycloak tier and a real BigQuery project; run via `just e2e-datahub-bigquery`, which brings the tier up first"]
    fn the_wave_one_path_answers_a_verified_caller_under_the_shared_key() {
        // One boot, three asks. The Keycloak fixture mints both subjects and writes the key set the
        // `inbound` block names; the datahub fake serves the recorded corpus twice - once for the
        // engine-open load and once inside `LocalService::start_composed`, which loads the catalog it
        // serves rather than trusting the bundle it was handed (`crates/sutura-serve/src/main.rs`
        // says so), the same six connections `served/datahub.rs`'s cell accounts for.
        // The keycloak fixture writes its fetched key set into `derived_beside(&config_path(CASE))`
        // - the SAME directory this file's `DataDir` owns - so the data directory is prepared FIRST
        // (removing any stale one) and the fixture's `create_dir_all`/write that follows is what
        // leaves the key set intact for the deployment the `inbound` block names. The reverse order
        // would wipe the just-fetched key set and the deployment would refuse a missing file.
        let data = DataDir::prepared(CASE);
        let fixture = keycloak_settings(CASE);
        // The real BigQuery leg: read the environment (fail-not-skip), load THIS file's own
        // `orders`/`customers` rows into it, and keep the loader alive so its `Drop` removes them
        // once every ask below is done - on the happy path AND on a panic partway through.
        let bq = BigQueryFixture::required();
        let _loaded = LoadedFixture::loaded(&bq.warehouse, &data);
        // Which DataHub the served binary reads from: the loopback fake (the recorded corpus served
        // over HTTP, `--datahub fake`, the default locally - no docker) or the REAL docker tier
        // (`--datahub tier`, the hosted job's, which provisions the certified metric and mints a
        // PAT). One flag, one settings builder, no second test; both go through the binary's own
        // HTTP AspectReader over an endpoint the settings name. `SUTURA_E2E_DATAHUB_MODE` is set by
        // the `just` task / nix app from the `--datahub` parameter; anything else refuses loudly.
        let (endpoint, token_file, fake) = match std::env::var("SUTURA_E2E_DATAHUB_MODE").as_deref() {
            Ok("fake") | Err(_) => {
                let mut answers = happy_path_answers();
                answers.extend(happy_path_answers());
                let server = FakeServer::start(answers);
                let endpoint = server.endpoint();
                (endpoint, data.token_file(), Some(server))
            }
            Ok("tier") => {
                // Admin credential the composed datahub profile seeds (see `compose.services.yaml`);
                // read from the environment the tier ran under, never committed here.
                let admin_user = std::env::var("SUTURA_DATAHUB_ADMIN_USER").unwrap_or_else(|_| "datahub".into());
                let admin_password = std::env::var("SUTURA_DATAHUB_ADMIN_PASSWORD").unwrap_or_else(|_| {
                    panic!(
                        "`--datahub tier` needs SUTURA_DATAHUB_ADMIN_PASSWORD - the seed credential the compose profile expects"
                    )
                });
                let tier = DatahubTier::required();
                let endpoint = tier.endpoint.clone();
                tier.provision(&admin_user, &admin_password, &data);
                (endpoint, data.token_file(), None)
            }
            other => {
                panic!("unknown SUTURA_E2E_DATAHUB_MODE {other:?} - set it from the `--datahub fake|tier` switch, never by hand")
            }
        };
        let deployment = start_configured(CASE, &settings(&fixture, &endpoint, &token_file, &bq));

        // Ask 1: principal A's Keycloak-minted token asks the certified DataHub-harvested metric,
        // answered from the REAL BigQuery table this test just loaded.
        let reply = deployment.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&fixture.subject_a_token),
            QUESTION,
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "412345"]]), "{}", reply.body);
        assert_eq!(
            body["executed_as"],
            serde_json::json!([{ "source": CATALOG, "posture": "shared-service-user" }]),
            "{}",
            reply.body
        );
        // The token file actually reached the wire (fake mode) or the tier accepted the minted PAT
        // (tier mode). Only the FAKE can capture the outbound authorizations - the served binary
        // cannot read the live tier's log - so in tier mode the PAT's acceptance is proven by the
        // tier answering the read at all and by `mint_pat`'s own `200`s above.
        // `finish` joins the fake thread; the audit record is written by the deployment's own
        // blocking pool, so this reads it after the fake is reaped with a bounded sweep of the
        // deployment's log rather than the fixed `awaiting` deadline.
        if let Some(server) = fake {
            let authorizations = server.finish();
            assert!(
                !authorizations.is_empty(),
                "the served binary sent no DataHub page request at all"
            );
            assert!(
                authorizations
                    .iter()
                    .all(|seen| seen.as_deref() == Some("Bearer pat-under-test")),
                "every DataHub page request must carry the token_file's bearer, got: {authorizations:?}"
            );
        }
        // The audit record arrives with the answer, read straight from the deployment's log - the
        // sweep and the `awaiting` deadline both fight the channel's behavior after the fake is
        // reaped (fake mode), so this reads once, after `finish` has joined the fake's thread.
        // Tier mode has no fake to reap - the served binary reads the REAL tier, and the audit
        // record is read straight off the deployment's log.
        let lines_a = deployment.log();
        assert!(
            lines_a
                .iter()
                .rev()
                .any(|line| line.contains(r#""subject_established":"verified""#)),
            "ask A's record does not say a caller was verified:\n{lines_a:?}"
        );
        // The audit record carries the MASKED subject (`mask_principal_into` in `sutura-domain`'s
        // `principal` masks a `sub` to its first character plus `***`), so two UUIDs sharing a
        // first hex character collide 1 in 16 - a bare `assert_ne!` on the records would pass on
        // that prefix alone. The property rests instead on the full `sub` each token's OWN payload
        // mints (the harness decoded it), and each record is tied to ITS OWN token's mask - the
        // same pattern `served/keycloak_test.rs` proves green. Uniqueness is `sub_a != sub_b`;
        // attribution to the record is the `assert_eq!` through the same one-hex mask.
        let sub_a = keycloak_subject_of(&fixture.subject_a_token);
        let sub_b = keycloak_subject_of(&fixture.subject_b_token);
        assert_ne!(sub_a, sub_b, "two provisioned subjects minted the same `sub` claim");
        let mask = |sub: &str| {
            sutura_domain::identity::SubjectId::parse(sub)
                .expect("a Keycloak UUID parses as a subject")
                .to_string()
        };
        let subject_a = lines_a
            .iter()
            .rev()
            .find(|line| line.contains(RECORD))
            .map(|line| subject_field(line))
            .expect("ask A produced an audit record carrying its subject");
        assert_eq!(
            subject_a,
            mask(&sub_a),
            "A's record does not carry the mask of A's own token's subject"
        );

        // Ask 2a: NO credential - the bearer gate refuses before the question is ever looked at.
        let unauth = deployment.post(&v1(sutura_http::constants::base_paths::QUERY), None, QUESTION);
        assert_eq!(
            unauth.status, 401,
            "a request with no credential was answered: {}",
            unauth.body
        );
        assert_eq!(unauth.json()["code"], "unauthorized", "{}", unauth.body);

        // Ask 2b: a VALID principal token, but a question this catalog does not certify - a typed
        // refusal with its reason, never `200`. What is refused is the question, not the caller.
        // Pinned against `examples/wave-one/refusal.json`'s own fields, not merely `!= 200` and not
        // merely the presence of a `code`/`detail` key - a status or code drift on either side of
        // that pairing is what this comparison exists to catch.
        let refused = deployment.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&fixture.subject_a_token),
            UNCERTIFIED,
        );
        assert_eq!(
            refused.status, REFUSAL_STATUS,
            "an uncertified question answered with the wrong status: {}",
            refused.body
        );
        let refused_body = refused.json();
        assert_eq!(refused_body["outcome"], "refusal", "{}", refused.body);
        assert_eq!(
            refused_body["reason"]["status"],
            serde_json::json!(REFUSAL_STATUS),
            "{}",
            refused.body
        );
        assert_eq!(refused_body["reason"]["code"], REFUSAL_CODE, "{}", refused.body);
        assert!(refused_body["reason"]["detail"].is_string(), "{}", refused.body);

        // Ask 3: principal B, the same certified question - a different subject, so the audit
        // record it produces has to name a different subject too (the two-subject property
        // `nix/keycloak-tier.nix`'s `subjects` list provisions for).
        let reply_b = deployment.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&fixture.subject_b_token),
            QUESTION,
        );
        assert_eq!(reply_b.status, 200, "{}", reply_b.body);
        // `log()` drains the deployment's channel: this read returns the lines since the last read,
        // and the last read was after `finish` (ask A) - so this associates reply B with its own
        // record. Records arrive with their answers (never only at teardown), so a post-reply read
        // is enough.
        let lines_b = deployment.log();
        let subject_b = lines_b
            .iter()
            .rev()
            .find(|line| line.contains(RECORD))
            .map(|line| subject_field(line))
            .expect("ask B produced an audit record carrying its subject");
        assert_eq!(
            subject_b,
            mask(&sub_b),
            "B's record does not carry the mask of B's own token's subject"
        );
        // Uniqueness is `sub_a != sub_b` above (full `sub`s, not their one-hex masks, which the
        // audit's masking lets collide 1 in 16): each of the two distinct provisioned subjects'
        // tokens produced a record carrying that subject's OWN mask, so no record was attributed
        // to the wrong principal.
    }
}
