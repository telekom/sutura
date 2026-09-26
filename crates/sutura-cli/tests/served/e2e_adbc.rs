//! Hosted acceptance for a served `DataHub` metric executed through ADBC `BigQuery`.
//! The source uses one acknowledged shared identity; this cell makes no claim that `BigQuery` runs as
//! either verified caller. The fixture tables expire after a day if the test aborts before cleanup.

#[cfg(test)]
#[path = "datahub_tier.rs"]
mod datahub_tier;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_catalog_datahub::test_support::DEPLOYMENT_PROPERTY;
    use sutura_domain::model::{SourceName, TableName};
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_exec_bigquery::BigQueryWarehouse;
    use sutura_exec_bigquery::adbc::{BytesBilledCeiling, DriverLocation, Impersonation};
    use sutura_exec_bigquery::transport::{DatasetId, ProjectId};

    use super::datahub_tier::{DatahubTier, FixtureNames};
    use crate::harness::keycloak::KeycloakFixture;
    use crate::harness::{LOOPBACK, VERSION, config_path, derived_beside, keycloak_settings, start_configured_with_driver, v1};

    const CASE: &str = "e2e-datahub-adbc-bigquery";
    const CATALOG: &str = "metrics";

    fn required(name: &str) -> String {
        match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => value,
            _ => panic!("{name} is unset or empty; this hosted cell cannot run without it"),
        }
    }

    struct BigQueryFixture {
        project: String,
        dataset: String,
        credential_file: PathBuf,
        driver_location: String,
        warehouse: BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery>,
    }

    impl BigQueryFixture {
        fn required() -> Self {
            let credential_file = PathBuf::from(required("GOOGLE_APPLICATION_CREDENTIALS"));
            let key = std::fs::read(&credential_file).expect("the declared BigQuery credential is readable");
            let key: serde_json::Value = serde_json::from_slice(&key).expect("the declared BigQuery credential is JSON");
            let project = key["project_id"]
                .as_str()
                .filter(|value| !value.is_empty())
                .expect("the BigQuery credential names a project_id")
                .to_owned();
            let dataset = required("SUTURA_BQ_DATASET");
            let driver_location = required("SUTURA_BIGQUERY_ADBC_DRIVER");
            let driver = DriverLocation::parse(&driver_location).expect("the BigQuery ADBC driver path is absolute");
            let posture = SourcePosture::SharedServiceUser {
                declared: SharedIdentityDeclared::of(
                    AcknowledgementReason::parse("the hosted acceptance reads BigQuery under one shared CI identity")
                        .expect("the acknowledgement parses"),
                ),
            };
            let warehouse = BigQueryWarehouse::over_adbc(
                SourceName::parse(CATALOG).expect("the source alias parses"),
                posture,
                ProjectId::parse(&project).expect("the key's project_id parses"),
                DatasetId::parse(&dataset).expect("the dataset id parses"),
                driver,
                Impersonation::Disabled,
                BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a ceiling"),
            );
            Self {
                project,
                dataset,
                credential_file,
                driver_location,
                warehouse,
            }
        }
    }

    struct DataDir(PathBuf);

    impl DataDir {
        fn prepared() -> Self {
            let path = derived_beside(&config_path(CASE));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the fixture directory is creatable");
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
            std::fs::write(path.join("token"), "pat-under-test").expect("the DataHub token file is writable");
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

    struct LoadedFixture<'a> {
        warehouse: &'a BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery>,
        orders: TableName,
        customers: TableName,
    }

    impl<'a> LoadedFixture<'a> {
        fn loaded(
            warehouse: &'a BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery>,
            data: &DataDir,
            names: &FixtureNames,
        ) -> Self {
            let orders = TableName::parse(&names.orders).expect("the orders table name parses");
            let customers = TableName::parse(&names.customers).expect("the customers table name parses");
            assert_eq!(
                warehouse
                    .load_fixture(&orders, &data.orders_csv())
                    .expect("ADBC loaded orders"),
                5
            );
            assert_eq!(
                warehouse
                    .load_fixture(&customers, &data.customers_csv())
                    .expect("ADBC loaded customers"),
                2
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
            if let Err(cause) = self.warehouse.drop_table(&self.orders) {
                eprintln!("the orders fixture table did not drop: {cause:?}");
            }
            if let Err(cause) = self.warehouse.drop_table(&self.customers) {
                eprintln!("the customers fixture table did not drop: {cause:?}");
            }
        }
    }

    fn settings(
        issuer: &KeycloakFixture,
        endpoint: &str,
        token_file: &Path,
        bq: &BigQueryFixture,
        max_bytes_billed: u64,
    ) -> String {
        let key_set = derived_beside(&config_path(CASE)).join("keycloak-jwks.json");
        format!(
            "server:\n{LOOPBACK}security:\n  identity: \"single-user\"\n  \
             single_user_because: \"the hosted BigQuery acceptance uses one acknowledged shared CI identity\"\n  \
             inbound:\n    mode: \"direct\"\n    resource: \"{resource}\"\n    \
             authorization_server: \"{issuer_url}\"\n    key_set_file: \"{key_set}\"\n    \
             algorithms: [\"RS256\"]\n    token_type: \"any\"\n\
             telemetry:\n  format: \"bunyan\"\n\
             catalogs:\n  - name: \"{CATALOG}\"\n    kind: \"datahub\"\n    \
             dir: \"/unused-for-datahub\"\n    data_dir: \"/unused-for-datahub\"\n    \
             version: \"{VERSION}\"\n    endpoint: \"{endpoint}\"\n    \
             token_file: \"{token_file}\"\n    metric_property: \"{DEPLOYMENT_PROPERTY}\"\n\
             sources:\n  {CATALOG}:\n    kind: \"bigquery\"\n    \
             billing_project: \"{project}\"\n    dataset: \"{dataset}\"\n    \
             credential_file: \"{credential_file}\"\n    max_bytes_billed: {max_bytes_billed}\n    \
             posture: \"shared-service-user\"\n",
            resource = issuer.resource,
            issuer_url = issuer.issuer,
            key_set = key_set.display(),
            token_file = token_file.display(),
            project = bq.project,
            dataset = bq.dataset,
            credential_file = bq.credential_file.display(),
        )
    }

    #[test]
    #[ignore = "requires the hosted DataHub and Keycloak tiers, a BigQuery dataset, and the ADBC driver"]
    fn served_datahub_metric_executes_through_adbc_bigquery() {
        let names = FixtureNames::unique();
        let data = DataDir::prepared();
        let issuer = keycloak_settings(CASE);
        let bq = BigQueryFixture::required();
        let _loaded = LoadedFixture::loaded(&bq.warehouse, &data, &names);
        let tier = DatahubTier::required();
        tier.provision(&data.token_file(), &names);
        let endpoint = format!("http://{}", tier.endpoint);
        let question =
            serde_json::json!({"metrics": [names.metric()], "grain": "month", "range": {"start": "2026-06-01", "end": "2026-07-01"}})
                .to_string();
        let served = start_configured_with_driver(
            CASE,
            &settings(&issuer, &endpoint, &data.token_file(), &bq, 1024 * 1024 * 1024),
            Some(&bq.driver_location),
        );
        let answer = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&issuer.subject_a_token),
            &question,
        );
        assert_eq!(answer.status, 200, "{}", answer.body);
        let body = answer.json();
        assert_eq!(body["outcome"], "answer", "{}", answer.body);
        assert_eq!(body["columns"], serde_json::json!(["period", names.metric()]));
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "412345"]]));
        assert_eq!(body["provenance"]["definition_version"], VERSION);
        assert!(
            body["provenance"]["definition_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty()),
            "an answer arrived with no definition digest: {}",
            answer.body
        );
        assert_eq!(
            body["executed_as"],
            serde_json::json!([{"source": CATALOG, "posture": "shared-service-user"}])
        );

        let refused = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&issuer.subject_a_token),
            r#"{"metrics":["uncertified"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
        );
        assert_eq!(refused.status, 404, "{}", refused.body);
        let refused_body = refused.json();
        assert_eq!(refused_body["outcome"], "refusal");
        assert_eq!(refused_body["reason"]["code"], "metric_unknown");
        assert_eq!(refused_body["reason"]["status"], 404);

        let no_caller = served.post(&v1(sutura_http::constants::base_paths::QUERY), None, &question);
        assert_eq!(no_caller.status, 401, "{}", no_caller.body);
    }
}
