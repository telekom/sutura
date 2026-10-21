//! `examples/federated` served: one metric from `postgres`, grouped by one dimension from
//! `clickhouse`, with the models and the certified metric read from a `DataHub` fake. The example's
//! own settings run against both provisioned tiers, and the answer is compared with the one-source
//! golden over the same corpus.
//!
//! Both sources are `shared-service-user` under `single-user`, so each leg runs as the deployment's
//! own user, and the answer names that identity per leg.
//!
//! The settings are the example's conf with its placeholders replaced by the provisioned tiers, each
//! loaded with the corpus.
//!
//! This cell skips, green and silently, where either tier is not provisioned (`just test` provisions
//! both and sets `SUTURA_DEV_REQUIRE_TIER`, which turns a skip into a failure).

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use sutura_catalog_datahub::test_support::{FakeServer, Scripted};

    use sutura_domain::model::{SourceName, TableName};
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_exec_clickhouse::ClickHouseWarehouse;
    use sutura_exec_clickhouse::fixture::{FixtureVariable, credential_from_env};
    use sutura_exec_clickhouse::transport::Endpoint;

    use crate::harness::{config_path, derived_beside, example_root, load_into_tier, start_configured, v1};

    /// The fact table on Postgres and the dimension table on `ClickHouse`, named as `examples/federated`
    /// loads them.
    const FACT: &str = "fct_subscription_monthly";
    const DIMENSION: &str = "dim_customer";

    /// `text` with the one declaration `from` replaced, refusing a conf that no longer declares it.
    fn put(text: &str, from: &str, to: &str) -> String {
        assert!(
            text.contains(from),
            "examples/federated/conf/base.yaml no longer declares {from}"
        );
        text.replace(from, to)
    }

    /// Loads `dim_customer` into the `ClickHouse` tier, as the published user.
    fn load_dimension(host: &str, port: u16) {
        let reason = AcknowledgementReason::parse("the served federated example uses one fixture user")
            .expect("the fixture reason is an acknowledgement");
        let loader = ClickHouseWarehouse::connect_in_database(
            SourceName::parse("fixture").expect("a fixture source is a source"),
            SourcePosture::SharedServiceUser {
                declared: SharedIdentityDeclared::of(reason),
            },
            Endpoint::plaintext(host, port),
            credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}")),
            "default",
            sutura_domain::warehouse::ResultBudget::of_bytes(
                core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive"),
            ),
        )
        .unwrap_or_else(|cause| panic!("the clickhouse tier did not open: {cause}"));
        let table = TableName::parse(DIMENSION).expect("a fixture table is a table");
        let path = example_root().join("data").join(format!("{DIMENSION}.csv"));
        loader
            .load_federated_csv(&table, &path)
            .unwrap_or_else(|cause| panic!("{} did not load: {cause}", path.display()));
    }

    /// The example's settings pointed at both tiers and at `catalog`, the `DataHub` fake's endpoint, with
    /// the guard that holds the Postgres load for as long as the case asks questions. `None` is either
    /// tier's ordinary no-tier outcome.
    fn federated_settings(case: &str, catalog: &str) -> Option<(String, impl Sized + use<>)> {
        let clickhouse = sutura_dev::provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse");
        let clickhouse = clickhouse.endpoint()?;
        let postgres = load_into_tier(case, FACT)?;
        load_dimension(clickhouse.host(), clickhouse.port());

        let scratch = derived_beside(&config_path(case));
        let file = |name: &str, contents: &str| {
            let path = scratch.join(name);
            std::fs::write(&path, contents).expect("an example file is writable");
            path.display().to_string()
        };
        let published =
            |variable: FixtureVariable| std::env::var(variable.name()).unwrap_or_else(|_| panic!("{variable} is not published"));
        let conf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/federated/conf/base.yaml");
        let declared = std::fs::read_to_string(&conf).expect("the example's settings are readable");
        let replaced = [
            ("port: 8080", String::from("port: 0")),
            ("\"http://127.0.0.1:8081\"", format!("\"{catalog}\"")),
            (
                "\"/run/sutura/datahub-token\"",
                format!("\"{}\"", file("datahub-token", "datahub-token-under-test\n")),
            ),
            ("port: 5432", format!("port: {}", postgres.port)),
            ("database: \"federated\"", format!("database: \"{}\"", postgres.database)),
            ("user: \"sutura\"", format!("user: \"{}\"", postgres.user)),
            (
                "\"/run/sutura/postgres-password\"",
                format!("\"{}\"", postgres.password_file.display()),
            ),
            ("\"/run/sutura/postgres-ca.pem\"", format!("\"{}\"", postgres.anchor)),
            ("port: 8123", format!("port: {}", clickhouse.port())),
            ("user: \"default\"", format!("user: \"{}\"", published(FixtureVariable::User))),
            (
                "\"/run/sutura/clickhouse-password\"",
                format!("\"{}\"", file("clickhouse-password", &published(FixtureVariable::Password))),
            ),
        ];
        let settings = replaced
            .iter()
            .fold(declared, |settings, (from, to)| put(&settings, from, to));
        Some((format!("{settings}telemetry:\n  format: \"bunyan\"\n"), postgres.guard))
    }

    const QUESTION: &str = r#"{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["segment"]}"#;

    /// The one-source golden for [`QUESTION`] on Postgres: the answer this deployment must equal.
    const ORACLE: &str = "../sutura-app/tests/snapshots/recurring-revenue-by-segment__rows@postgres.snap";

    /// A `dataset` entity over `table` on `platform`, read as model `<platform>__<table>`.
    fn dataset(platform: &str, table: &str, columns: &[(&str, &str)]) -> serde_json::Value {
        serde_json::json!({
            "urn": format!("urn:li:dataset:(urn:li:dataPlatform:{platform},{table},PROD)"),
            "schemaMetadata": { "value": { "fields": columns
                .iter()
                .map(|(name, native)| serde_json::json!({ "fieldPath": name, "nativeDataType": native }))
                .collect::<Vec<_>>() } },
            "datasetProperties": { "value": { "description": format!("{table} on {platform}.") } },
        })
    }

    /// The three pages one catalog read makes: the two datasets, their relationship and the
    /// certified `recurring_revenue`, anchored at the one-source figure.
    fn pages() -> Vec<Scripted> {
        let fact = |platform| format!("urn:li:dataset:(urn:li:dataPlatform:{platform},fct_subscription_monthly,PROD)");
        let dimension = |platform| format!("urn:li:dataset:(urn:li:dataPlatform:{platform},dim_customer,PROD)");
        let datasets = serde_json::json!({ "entities": [
            dataset("postgres", "fct_subscription_monthly", &[
                ("month", "DATE"),
                ("subscription_key", "BIGINT"),
                ("customer_key", "BIGINT"),
                ("product_key", "BIGINT"),
                ("status", "VARCHAR"),
                ("mrr_cents", "NUMERIC"),
                ("churned_in_month", "BOOLEAN"),
                ("contract_term", "VARCHAR"),
            ]),
            dataset("clickhouse", "dim_customer", &[
                ("customer_key", "BIGINT"),
                ("customer_id", "VARCHAR"),
                ("segment", "VARCHAR"),
                ("region", "VARCHAR"),
            ]),
        ] });
        let relationships = serde_json::json!({ "entities": [{
            "urn": "urn:li:semanticModel:(urn:li:dataPlatform:postgres,PROD,subscription_customer)",
            "semanticModelInfo": { "value": {
                "name": "subscription_customer",
                "relationships": [{
                    "name": "subscription_customer",
                    "from": fact("postgres"),
                    "fromColumns": ["customer_key"],
                    "to": dimension("clickhouse"),
                    "toColumns": ["customer_key"],
                    "cardinality": "N_ONE",
                }],
            } },
        }] });
        let document = serde_json::json!({
            "model": "postgres__fct_subscription_monthly",
            "measure": { "simple": { "aggregate": "sum", "column": "mrr_cents" } },
            "time_column": "month",
            "grains": ["month"],
            "required_filters": [{ "equals": { "column": "status", "value": "active" } }],
            "dimensions": [{
                "name": "segment",
                "column": "segment",
                "via": "subscription_customer",
                "allowed_values": ["business", "consumer", "wholesale"],
            }],
            "anchor": { "range": { "start": "2026-06-01", "end": "2026-07-01" }, "value": "202121" },
        });
        let metrics = serde_json::json!({ "entities": [{
            "urn": "urn:li:metric:(urn:li:dataPlatform:postgres,fct_subscription_monthly,recurring_revenue)",
            "metricInfo": { "value": {
                "name": "recurring_revenue",
                "expression": { "dialects": [{ "dialect": "ANSI_SQL", "expression": "SUM(mrr_cents)" }] },
            } },
            "structuredProperties": { "value": { "properties": [{
                "propertyUrn": "urn:li:structuredProperty:sutura",
                "values": [{ "string": document.to_string() }],
            }] } },
        }] });
        [datasets, relationships, metrics].iter().map(Scripted::ok).collect()
    }

    /// The golden's rows as the wire carries them: every cell a string, a null spelled `null`.
    fn oracle() -> Vec<serde_json::Value> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(ORACLE);
        let golden = std::fs::read_to_string(&path).unwrap_or_else(|cause| panic!("{} is unreadable: {cause}", path.display()));
        golden
            .lines()
            .skip_while(|line| *line != "---")
            .skip(1)
            .skip_while(|line| *line != "---")
            .skip(2)
            .map(|row| {
                row.split('\t')
                    .map(|cell| match cell {
                        "Null" => serde_json::Value::String(String::from("null")),
                        cell => {
                            let inner = cell
                                .split_once('(')
                                .and_then(|(_, rest)| rest.strip_suffix(')'))
                                .unwrap_or_else(|| panic!("{cell} is not a golden cell"));
                            serde_json::Value::String(String::from(inner))
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_metric_on_postgres_grouped_by_a_dimension_on_clickhouse_equals_the_one_source_golden() {
        let case = "federated-demo";
        // The catalog is read twice at boot, three pages each.
        let mut answers = pages();
        answers.extend(pages());
        let catalog = FakeServer::start(answers);
        let Some((settings, _loaded)) = federated_settings(case, &catalog.endpoint()) else {
            return;
        };
        let deployment = start_configured(case, &settings);

        let reply = deployment.post(&v1(sutura_http::constants::base_paths::QUERY), None, QUESTION);
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(body["rows"], serde_json::Value::Array(oracle()), "{}", reply.body);
        let legs: BTreeSet<(String, String)> = body["executed_as"]
            .as_array()
            .unwrap_or_else(|| panic!("no executed_as: {}", reply.body))
            .iter()
            .map(|leg| (leg["source"].to_string(), leg["posture"].to_string()))
            .collect();
        let shared = |source: &str| (format!("\"{source}\""), String::from("\"shared-service-user\""));
        assert_eq!(legs, BTreeSet::from([shared("billing"), shared("crm")]), "{}", reply.body);
    }
}
