//! Wave one of the identity-aware E2E (`just e2e-datahub-bigquery`): `DataHub` carries the certified
//! metric's definition, a REAL Keycloak issuer's token says who is asking, and the certified
//! question executes over HTTP `/v1/query`, on the composed `sutura serve` binary, against a REAL
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
//! `files`), and the issuer is the provisioned Keycloak tier. Over the `agent` feature (which `just
//! e2e-datahub-bigquery`'s `--all-features` compiles) the same deployment additionally answers the
//! certified question over `/mcp`, folded into the SAME cell so the byte-for-byte join - same rows,
//! same masked `subject` - spans one boot, never two.
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

// The docker DataHub tier harness, split out by the same `cargo xtask max-lines` 1000-line cap
// that `harness.rs` and `served.rs`'s other `#[path] mod` children record: it carries no `#[test]`,
// so moving it does not change what `just causality` can see.
#[cfg(test)]
#[path = "datahub_tier.rs"]
mod datahub_tier;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::harness::Served;
    use sutura_catalog_datahub::test_support::{DEPLOYMENT_PROPERTY, FakeServer, happy_path_answers};
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
    /// (`crates/sutura-cli/src/serve/catalog.rs`), so a source under any other name is unreachable by
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
    /// The `server:` head: the wave-one loopback bind, and - under the `agent` feature, which
    /// `just e2e-datahub-bigquery` compiles via `--all-features` - the `agent_surface` switch that
    /// mounts `/mcp`. Emitted ONLY under that feature so a no-`agent` compile of this module neither
    /// mounts the transport nor trips `serve`'s `agent_refused_if_enabled` boot refusal: the MCP
    /// asks are gated on the same feature, so the two can never disagree.
    fn server_head() -> String {
        let mut head = String::from(LOOPBACK);
        #[cfg(feature = "agent")]
        head.push_str("  agent_surface:\n    enabled: true\n");
        head
    }

    use super::datahub_tier::DatahubTier;

    fn settings(fixture: &KeycloakFixture, endpoint: &str, token_file: &Path, bq: &BigQueryFixture) -> String {
        let key_set = derived_beside(&config_path(CASE)).join("keycloak-jwks.json");
        format!(
            "server:\n\
             {server_head}\
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
            server_head = server_head(),
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

    // ------------------------------------------------------------------ the agent surface ----
    // The MCP half of the wave-one join, gated on the `agent` feature exactly like `server_head`
    // gates the `agent_surface` switch above: `just e2e-datahub-bigquery` compiles `--all-features`
    // so both are present where this cell runs, and a no-`agent` compile of this module has neither
    // the mount nor the asks that would answer it.

    /// One MCP `initialize` body, the same shape `served/agent.rs` drives to prime the handshake
    /// (the transport's stateless mode serves it one-shot under `legacy_session_mode: false`).
    #[cfg(feature = "agent")]
    fn mcp_initialize(id: i64) -> String {
        serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": "initialize",
            "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                       "clientInfo": {"name": "sutura-serve-e2e", "version": "0.0.0"}}
        }))
        .expect("the initialize body serializes")
    }

    /// One MCP `tools/list` body.
    #[cfg(feature = "agent")]
    fn mcp_tools_list(id: i64) -> String {
        serde_json::to_string(&serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {}}))
            .expect("the tools/list body serializes")
    }

    /// One MCP `tools/call` body. The `arguments` are taken VERBATIM as the wave-one `QUESTION`
    /// string, so the byte-for-byte question text is the same over `/v1/query` and `/mcp`.
    #[cfg(feature = "agent")]
    fn mcp_tools_call(id: i64, name: &str, arguments_json: &str) -> String {
        let arguments: serde_json::Value =
            serde_json::from_str(arguments_json).unwrap_or_else(|cause| panic!("the question is a JSON object: {cause}"));
        serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }))
        .expect("the tools/call body serializes")
    }

    /// The tool names a `tools/list` reply advertised, for the `ask_metric` assertions.
    #[cfg(feature = "agent")]
    fn mcp_tool_names(reply: &crate::harness::Reply) -> Vec<String> {
        reply
            .json()
            .get("result")
            .and_then(|result| result.get("tools"))
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("a tools/list result carries a `tools` array: {}", reply.body))
            .iter()
            .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str).map(String::from))
            .collect()
    }

    /// Boot ONE deployment over the loopback fake (the recorded corpus, the default locally) or
    /// the REAL docker `DataHub` tier (the hosted job's), and return the pair every ask shares - the
    /// fake to reap (fake mode, for the outbound-bearer sweep) and the served deployment.
    ///
    /// One flag, one settings builder, no second test; both read `DataHub` through the binary's own
    /// HTTP `AspectReader` over an endpoint the settings name. `SUTURA_E2E_DATAHUB_MODE` is set by the
    /// `just` task / nix app from the `--datahub` parameter; anything else refuses loudly.
    fn boot_wave_one(fixture: &KeycloakFixture, data: &DataDir, bq: &BigQueryFixture) -> (Option<FakeServer>, Served) {
        let (endpoint, token_file, fake) = match std::env::var("SUTURA_E2E_DATAHUB_MODE").as_deref() {
            Ok("fake") | Err(_) => {
                let mut answers = happy_path_answers();
                answers.extend(happy_path_answers());
                let server = FakeServer::start(answers);
                let endpoint = server.endpoint();
                (endpoint, data.token_file(), Some(server))
            }
            Ok("tier") => {
                // The docker tier minted its own PAT before this test ran (see `dev/src/mint.rs` /
                // the just task and nix app); the served binary presents it via the token_file
                // `adopt_minted_pat` populated, and provision() provisions the certified metric with
                // it. No admin credential is read - the headless GMS has no login surface to use one.
                let tier = DatahubTier::required();
                tier.provision(&data.token_file());
                // `DatahubTier` talks to the tier by bare host:port (it builds the scheme itself),
                // but the served binary's settings key needs a scheme - the same shape the FAKE's
                // `endpoint()` returns, so one settings builder stays valid for both modes.
                let endpoint = format!("http://{}", tier.endpoint);
                (endpoint, data.token_file(), None)
            }
            other => {
                panic!("unknown SUTURA_E2E_DATAHUB_MODE {other:?} - set it from the `--datahub fake|tier` switch, never by hand")
            }
        };
        let deployment = start_configured(CASE, &settings(fixture, &endpoint, &token_file, bq));
        (fake, deployment)
    }

    #[test]
    #[ignore = "needs the Keycloak tier and a real BigQuery project; run via `just e2e-datahub-bigquery`, which brings the tier up first"]
    fn the_wave_one_path_answers_a_verified_caller_under_the_shared_key() {
        // One boot, three asks. The Keycloak fixture mints both subjects and writes the key set the
        // `inbound` block names; the datahub fake serves the recorded corpus twice - once for the
        // engine-open load and once inside `LocalService::start_composed`, which loads the catalog it
        // serves rather than trusting the bundle it was handed (`crates/sutura-cli/src/serve.rs`
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
        let (fake, deployment) = boot_wave_one(&fixture, &data, &bq);

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
        // tier answering the read at all and by `provision`'s own `200`s above.
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

        // The agent-surface half of the wave-one join: the SAME wave-one question asked over
        // `/mcp`, behind the SAME leg-1 gate and the SAME serving `Surface` the HTTP asks above
        // used. This is the byte-for-byte join over the composed binary that #758's served cells
        // can only hold at the router level - one deployment, one verified caller, the same rows
        // and the same masked subject over both transports. The asks mirror the HTTP asks in order:
        // (i) `tools/list` as A, (ii) `tools/call` `ask_metric` as A, (iii) no bearer, (iv) the
        // uncertified question as A.
        #[cfg(feature = "agent")]
        {
            let init_reply = deployment.mcp(Some(&fixture.subject_a_token), &mcp_initialize(10));
            assert_eq!(init_reply.status, 200, "{}", init_reply.body);
            let tools = mcp_tool_names(&deployment.mcp(Some(&fixture.subject_a_token), &mcp_tools_list(11)));
            assert!(
                tools.iter().any(|tool| tool == "ask_metric"),
                "a verified caller must be advertised `ask_metric` over /mcp, got: {tools:?}"
            );

            // (ii) The certified question as A, over /mcp. The answer's `structuredContent` is the
            // same `OutcomeContent` the HTTP surface serializes, so the SAME one row and SAME
            // `executed_as` leg are asserted here as the REST ask asserted on `body`.
            let mcp_reply = deployment.mcp(Some(&fixture.subject_a_token), &mcp_tools_call(12, "ask_metric", QUESTION));
            let mcp_json = mcp_reply.json();
            let mcp_content = mcp_json
                .get("result")
                .and_then(|result| result.get("structuredContent"))
                .unwrap_or_else(|| panic!("the tools/call result carried no `structuredContent`: {}", mcp_reply.body));
            assert_eq!(mcp_content["outcome"], "answer", "{}", mcp_reply.body);
            assert_eq!(
                mcp_content["rows"],
                serde_json::json!([["2026-06-01", "412345"]]),
                "{}",
                mcp_reply.body
            );
            assert_eq!(
                mcp_content["executed_as"],
                serde_json::json!([{ "source": CATALOG, "posture": "shared-service-user" }]),
                "{}",
                mcp_reply.body
            );
            // The record lags the response (written from the blocking pool, not ordered against
            // it) - so this is a BLOCKING `awaiting`, the same read `served/keycloak_test.rs`
            // makes, not a `log()` sweep that races a record arriving a millisecond later. Blocking
            // on `"route":"/mcp"` alone returns too early: every `/mcp` call also writes a
            // `[REQUEST - START]`/`finished processing request` line carrying that same field
            // (measured: `initialize`'s own START line satisfies it before this ask even runs). The
            // audit sink's `target` fires only for a real audit record, so it stays the block
            // needle (also why `RECORD`, written inside the router's `[REQUEST - EVENT]` span,
            // would never see `/mcp`'s own `[SERVE_INNER - EVENT]` one) - but `target` alone
            // matches every record (HTTP `answered`, the REST refusal's `refused`, and the MCP one
            // alike), so the FOUND line must also carry `"route":"/mcp"` and `answered`: a stale
            // REST record (verified, A's own mask, left over from ask 2's refusal) must fail this
            // join loudly, not pass it under the right subject for the wrong reason. The join:
            // whoever asked over HTTP as A and over MCP as A is the SAME masked subject established
            // from the SAME Keycloak token.
            let lines_mcp = deployment.awaiting(r#""target":"sutura_runtime::audit""#);
            let mcp_subject_a = lines_mcp
                .iter()
                .rev()
                .find(|line| {
                    line.contains(r#""subject_established":"verified""#)
                        && line.contains(r#""route":"/mcp""#)
                        && line.contains("answered")
                })
                .map(|line| subject_field(line))
                .expect("ask A over /mcp produced an audit record carrying its subject");
            assert_eq!(
                mcp_subject_a, subject_a,
                "A's MCP record must carry the SAME masked subject as A's REST record - one caller, one subject, two transports"
            );

            // (iii) No bearer over /mcp: leg 1 refuses it BEFORE the transport - a `401`, never a
            // `200` and never a JSON-RPC answer. Same challenge every forgery gets
            // (`sutura_http::inbound::tests::router::the_agent_route_refuses_an_unverified_caller…`).
            let unauth_mcp = deployment.mcp(None, &mcp_tools_list(13));
            assert_eq!(
                unauth_mcp.status, 401,
                "a request with no credential was answered over /mcp: {}",
                unauth_mcp.body
            );

            // (iv) The uncertified question over /mcp: a typed GOVERNANCE refusal - an `Ok` tool
            // result (`isError` absent, `outcome: "refusal"`), per `sutura_mcp::server`'s pinned
            // contract, never a transport error. `RefusalContent` carries `code`+`detail` and no
            // `status` (there is no HTTP status in a tool result) - so the join on the REST refusal
            // is `reason.code`, which is the SAME `metric_unknown` the REST `body` pinned.
            let refused_mcp = deployment.mcp(Some(&fixture.subject_a_token), &mcp_tools_call(14, "ask_metric", UNCERTIFIED));
            let refused_json = refused_mcp.json();
            let refused_content = refused_json
                .get("result")
                .and_then(|result| result.get("structuredContent"))
                .unwrap_or_else(|| panic!("the refused tools/call carried no `structuredContent`: {}", refused_mcp.body));
            assert_eq!(refused_content["outcome"], "refusal", "{}", refused_mcp.body);
            assert_eq!(refused_content["reason"]["code"], REFUSAL_CODE, "{}", refused_mcp.body);
            assert!(refused_content["reason"]["detail"].is_string(), "{}", refused_mcp.body);
        }
    }
}
