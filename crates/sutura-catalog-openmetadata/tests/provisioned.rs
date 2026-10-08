#![forbid(unsafe_code)]
//! The provisioned `OpenMetadata` instance, asked what the recorded fixture and the fake server
//! cannot answer: whether `src/http.rs`'s reader decodes what a real deployment serves.
//!
//! `crates/sutura-catalog-openmetadata` was decided against the published entity schema
//! (`docs/what-openmetadata-can-carry.md`) and its reader only ever ran against a loopback fake.
//! `compose.services.yaml`'s `openmetadata` profile is the real instance, and this file is the
//! only thing here that talks to it. **Evidence only of the run that executed it**: every cell but
//! the first is `#[ignore]`d, behind `just openmetadata-acceptance` and CI's `ci-openmetadata-tier`
//! (`nix run .#openmetadata-acceptance`), and a green `just test` says nothing about them.
//!
//! * `every_not_carried_row_still_names_something_the_golden_states` - no venue, so it runs in
//!   `just test`: a [`NotCarried`] row whose subject left the golden fails rather than lingers.
//! * `the_provisioned_openmetadata_serves_the_surface_a_reader_would_call` - the version route
//!   answers, and the two list routes the reader pages answer a `2xx` to an authenticated caller.
//! * `a_bearerless_read_is_401_under_the_enforced_tier` - **the enforcement cell**: the tier keeps
//!   the image's `basic` JWT provider, so a bearer-less list is refused and the same list with a
//!   bearer is not. A silent rollback to an open instance turns it red.
//! * `the_golden_catalog_round_trips_through_a_live_openmetadata` - provisions
//!   `examples/single-player/catalog` through the REST API, reads it back through
//!   `HttpSnapshotReader`, and asserts the certified definitions equal the golden minus the named
//!   [`NotCarried`] rows - the golden-suite rule: the same catalog, the same certified answer, or a
//!   named exemption. The metrics are provisioned too, and asserted to be READ and never DEFINED.
//!
//! **Limits, next to the claims.** The bearer is the image's built-in administrator, logged in with
//! the upstream container's own default credential - a throwaway tier's, in no shipped or example
//! config. Authorization beyond "a token or none" is not exercised. What the wire mapping does not
//! write (a compound key, the nullability, a physical table name) is never provisioned, so if the
//! reader learns to carry one, nothing here reads that back.
//!
//! **Fail-closed where a tier was provisioned, loudly skipped where one was not**:
//! `sutura_dev::provisioned::here` panics under `SUTURA_DEV_REQUIRE_TIER` and prints a notice on a
//! developer machine; nothing here falls back to a default port.

#![cfg(all(test, feature = "http"))]

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use sutura_catalog_local::LocalCatalog;
    use sutura_catalog_openmetadata::document::Metric;
    use sutura_catalog_openmetadata::http::{
        DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpSnapshotReader, ReadBounds,
    };
    use sutura_catalog_openmetadata::{OpenMetadataCatalog, SnapshotReader as _};
    use sutura_domain::catalog::{Column, Definitions, Description, InconsistentDefinitions, JoinKey, Model, Relationship};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::{ColumnName, JoinType, MetricName, RelationshipName, SourceName, TableName};
    use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};

    /// How long one request may take: provisioning already gated on the server's health check.
    const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

    /// The service every entity this file writes sits under, and so the key its `sources` map is
    /// opened on.
    const SERVICE: &str = "golden";
    const DATABASE: &str = "default";
    const SCHEMA: &str = "sales";
    const SOURCE: &str = "local";
    const VERSION: &str = "golden-fixture-1";

    /// The container image's documented public default login, test-only; base64 because that is the
    /// login API's wire format (a plain password is refused with 400).
    const ADMIN_EMAIL: &str = "admin@open-metadata.org";
    const ADMIN_PASSWORD_BASE64: &str = "YWRtaW4=";

    /// Where this worktree's `openmetadata` is listening, or `None` on a machine that provisioned
    /// none (`sutura_dev::provisioned::here` has already printed the notice, or panicked where a
    /// job required the tier).
    fn endpoint() -> Option<String> {
        let inside = Path::new(env!("CARGO_MANIFEST_DIR"));
        sutura_dev::provisioned::here(inside, "openmetadata")
            .endpoint()
            .map(ToString::to_string)
    }

    /// `status_as_error` off, because the refusal cell asserts the status and every write asserts
    /// the server's own reason, which an `Err` would not carry.
    fn agent() -> ureq::Agent {
        sutura_http_client::agent(|config| config.timeout_global(Some(ANSWER_TIMEOUT)).http_status_as_error(false))
    }

    fn answer(mut response: ureq::http::Response<ureq::Body>) -> (u16, String) {
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().expect("the answer is text");
        (status, text)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the throwaway tier's JWT goes on the wire as the Authorization header of a test request"
    )]
    fn bearer(token: &Secret) -> String {
        format!("Bearer {}", token.expose_secret())
    }

    fn get(agent: &ureq::Agent, endpoint: &str, path: &str, token: Option<&Secret>) -> (u16, String) {
        let request = agent.get(format!("http://{endpoint}{path}"));
        let request = match token {
            Some(token) => request.header("Authorization", bearer(token)),
            None => request,
        };
        answer(
            request
                .call()
                .expect("the provisioned OpenMetadata answered, whatever it answered"),
        )
    }

    fn put(agent: &ureq::Agent, endpoint: &str, path: &str, token: &Secret, body: &serde_json::Value) -> (u16, String) {
        answer(
            agent
                .put(format!("http://{endpoint}{path}"))
                .header("Authorization", bearer(token))
                .header("Content-Type", "application/json")
                .send(serde_json::to_string(body).expect("a body serializes"))
                .expect("the provisioned OpenMetadata answered, whatever it answered"),
        )
    }

    /// The administrator's JWT, as the secret the reader takes.
    fn login(agent: &ureq::Agent, endpoint: &str) -> Secret {
        let body = serde_json::json!({ "email": ADMIN_EMAIL, "password": ADMIN_PASSWORD_BASE64 });
        let (status, text) = answer(
            agent
                .post(format!("http://{endpoint}/api/v1/users/login"))
                .header("Content-Type", "application/json")
                .send(body.to_string())
                .expect("the provisioned OpenMetadata answered the login"),
        );
        assert_eq!(status, 200, "the tier's default administrator logs in: {text}");
        let token = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|login| {
                login
                    .get("accessToken")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| panic!("the login answer carries an accessToken"));
        Secret::new(token)
    }

    #[test]
    #[ignore = "needs `just dev-up-openmetadata`; a docker service is only in the discovery file until \
                the next writer rewrites it - run `just openmetadata-acceptance`"]
    fn the_provisioned_openmetadata_serves_the_surface_a_reader_would_call() {
        let Some(endpoint) = endpoint() else { return };
        let agent = agent();
        let (status, version) = get(&agent, &endpoint, "/api/v1/system/version", None);
        assert_eq!(status, 200, "the version route answers: {version}");
        assert!(
            version.contains("\"version\""),
            "the version route names a version: {version}"
        );

        let token = login(&agent, &endpoint);
        for path in [
            "/api/v1/tables?limit=1&fields=columns,tableConstraints",
            "/api/v1/metrics?limit=1",
        ] {
            let (status, body) = get(&agent, &endpoint, path, Some(&token));
            assert_eq!(status, 200, "`{path}` answers a reader the pinned release serves: {body}");
        }
    }

    #[test]
    #[ignore = "needs `just dev-up-openmetadata`; a docker service is only in the discovery file until \
                the next writer rewrites it - run `just openmetadata-acceptance`"]
    fn a_bearerless_read_is_401_under_the_enforced_tier() {
        let Some(endpoint) = endpoint() else { return };
        let agent = agent();
        let path = "/api/v1/tables?limit=1";
        let (refused, body) = get(&agent, &endpoint, path, None);
        assert_eq!(refused, 401, "an unauthenticated list is refused: {body}");
        let (served, body) = get(&agent, &endpoint, path, Some(&login(&agent, &endpoint)));
        assert_eq!(
            served, 200,
            "the same list with a bearer is served, so the 401 is the auth and not the route: {body}"
        );
    }

    /// What the golden states that this adapter does not carry back, one row each, each with its
    /// reason.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum NotCarried {
        /// `convert_model` makes the physical table the entity's own name, so table == name.
        Table,
        /// The column `constraint` (`NULL`/`NOT_NULL`) is not read, so every column has no nullability.
        Nullable,
        /// A constraint has no name on the wire, so the reader synthesises `<origin>_<column>_fk`.
        RelationshipName,
        /// Prose: the server HTML-escapes a backtick (`&#96;`) and an apostrophe (`&#39;`) in a MODEL's
        /// description, and the reader returns the stored text. Measured on the pinned release: a
        /// column's description keeps its apostrophe, and no golden column prose holds a backtick;
        /// whether any other character is escaped is not measured.
        DescriptionEscaping,
        /// A compound primary key: the server refuses two columns each tagged `PRIMARY_KEY`, and a
        /// compound one is a table-level `PRIMARY_KEY` constraint, which the reader does not read.
        CompoundPrimaryKey,
        /// A compound-key relationship: `usage_subscription` has two keys, and a side declaring more
        /// than one column is refused by `one_referenced` rather than narrowed. The limit is this
        /// adapter's, not `OpenMetadata`'s - `columns`/`referredColumns` are arrays.
        Relationship(&'static str),
        /// Every metric: its binding is a free-text expression, reported and never minted - the
        /// declared half-a-definition (`docs/what-openmetadata-can-carry.md`).
        Metrics,
        /// The adapter declares no knowledge, so only definitions are compared.
        Knowledge,
    }

    const NOT_CARRIED: [NotCarried; 8] = [
        NotCarried::Table,
        NotCarried::Nullable,
        NotCarried::DescriptionEscaping,
        NotCarried::CompoundPrimaryKey,
        NotCarried::RelationshipName,
        NotCarried::Relationship("usage_subscription"),
        NotCarried::Metrics,
        NotCarried::Knowledge,
    ];

    /// What the reader names a foreign key it harvests: `harvest_relationship`'s own `format!`.
    fn synthesised(origin_model: &str, origin_column: &str) -> String {
        format!("{origin_model}_{origin_column}_fk")
    }

    fn needs_escaping(prose: &str) -> bool {
        prose.contains(['`', '\''])
    }

    /// The server's own spelling of `prose`, for the two characters measured.
    fn escaped(prose: &str) -> String {
        prose.replace('`', "&#96;").replace('\'', "&#39;")
    }

    fn one_key(relationship: &Relationship) -> Option<(&str, &str)> {
        match relationship.keys().iter().collect::<Vec<_>>().as_slice() {
            [JoinKey::Equal { origin, target }] => Some((origin.as_str(), target.as_str())),
            _ => None,
        }
    }

    impl NotCarried {
        /// Whether a row still names something this golden states, so a golden edit that drops the
        /// row's subject is caught by the not-ignored cell rather than silently carried.
        fn bites(self, golden: &PinnedDefinitions) -> bool {
            let definitions = golden.definitions();
            match self {
                Self::Table => definitions
                    .models()
                    .values()
                    .any(|model| model.table_name().as_str() != model.name().as_str()),
                Self::Nullable => definitions
                    .models()
                    .values()
                    .flat_map(Model::columns)
                    .any(|column| column.nullable().is_some()),
                Self::DescriptionEscaping => definitions.models().values().any(|model| needs_escaping(model.description())),
                Self::CompoundPrimaryKey => definitions.models().values().any(|model| model.primary_key().len() > 1),
                Self::RelationshipName => definitions.relationships().values().any(|relationship| {
                    one_key(relationship).is_some_and(|(origin, _)| {
                        relationship.name().as_str() != synthesised(relationship.origin_model().as_str(), origin)
                    })
                }),
                Self::Relationship(name) => definitions.relationships().keys().any(|key| key.as_str() == name),
                Self::Metrics => !definitions.metrics().is_empty(),
                Self::Knowledge => !golden.knowledge().declares().is_empty(),
            }
        }
    }

    fn golden() -> PinnedDefinitions {
        LocalCatalog::new(
            SourceName::parse(SOURCE).expect("a test source name is a name"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player/catalog"),
            DefinitionVersion::parse(VERSION).expect("a test version is a version"),
        )
        .load()
        .expect("the golden catalog loads")
    }

    /// The golden with `rows` applied - what the read-back must equal.
    fn carried(golden: &Definitions, rows: &[NotCarried]) -> Result<Definitions, InconsistentDefinitions> {
        let models = golden
            .models()
            .values()
            .map(|model| {
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
                    Description::parse(&if rows.contains(&NotCarried::DescriptionEscaping) {
                        escaped(model.description())
                    } else {
                        model.description().to_owned()
                    })
                    .expect("the golden model prose parses"),
                )
                .with_primary_key(
                    model
                        .primary_key()
                        .iter()
                        .filter(|_| !(rows.contains(&NotCarried::CompoundPrimaryKey) && model.primary_key().len() > 1))
                        .cloned(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let relationships = golden
            .relationships()
            .values()
            .filter(|relationship| {
                !rows
                    .iter()
                    .any(|row| matches!(row, NotCarried::Relationship(name) if *name == relationship.name().as_str()))
            })
            .map(|relationship| match one_key(relationship) {
                Some((origin, _)) if rows.contains(&NotCarried::RelationshipName) => Relationship::new(
                    RelationshipName::parse(synthesised(relationship.origin_model().as_str(), origin))
                        .expect("a synthesised name is a name"),
                    relationship.origin_model().clone(),
                    relationship.target_model().clone(),
                    relationship.join_type(),
                    relationship.keys().clone(),
                ),
                _ => relationship.clone(),
            })
            .collect();
        let metrics = if rows.contains(&NotCarried::Metrics) {
            Vec::new()
        } else {
            golden.metrics().values().cloned().collect()
        };
        Definitions::assemble(models, relationships, metrics)
    }

    /// What the wire mapping can write, decided from the golden alone and not from [`NOT_CARRIED`]:
    /// every model and every relationship with one plain key, so a stray row's subject is still
    /// provisioned and the final comparison refuses the row.
    fn writable(golden: &Definitions) -> Definitions {
        let relationships = golden
            .relationships()
            .values()
            .filter(|relationship| one_key(relationship).is_some())
            .cloned()
            .collect();
        Definitions::assemble(golden.models().values().cloned().collect(), relationships, Vec::new())
            .expect("what the wire can write holds together")
    }

    /// One `Column` write body. `VARCHAR` is the one type the server refuses without a length.
    fn column_body(model: &Model, column: &Column) -> serde_json::Value {
        let data_type = column.data_type().expect("the golden types every column").as_str();
        let mut body = serde_json::json!({ "name": column.name().as_str(), "dataType": data_type });
        if data_type == "VARCHAR" {
            body["dataLength"] = serde_json::json!(255);
        }
        if !column.description().is_empty() {
            body["description"] = serde_json::json!(column.description());
        }
        if model.primary_key().len() == 1 && model.primary_key().contains(column.name()) {
            body["constraint"] = serde_json::json!("PRIMARY_KEY");
        }
        body
    }

    /// Provisions `catalog` and the golden's metrics onto the tier: the service, database and
    /// schema, every table, then the foreign keys. The keys are a second write of each table because
    /// the server validates `referredColumns` against tables that must already exist.
    fn provision(agent: &ureq::Agent, endpoint: &str, token: &Secret, catalog: &Definitions, metrics: &Definitions) {
        let schema = format!("{SERVICE}.{DATABASE}.{SCHEMA}");
        let upserts = [
            (
                "/api/v1/services/databaseServices",
                serde_json::json!({ "name": SERVICE, "serviceType": "CustomDatabase", "connection": { "config": { "type": "CustomDatabase" } } }),
            ),
            (
                "/api/v1/databases",
                serde_json::json!({ "name": DATABASE, "service": SERVICE }),
            ),
            (
                "/api/v1/databaseSchemas",
                serde_json::json!({ "name": SCHEMA, "database": format!("{SERVICE}.{DATABASE}") }),
            ),
        ];
        for (path, body) in &upserts {
            let (status, text) = put(agent, endpoint, path, token, body);
            assert!((200..300).contains(&status), "the platform accepts `{path}`: {status} {text}");
        }
        let table = |model: &Model, with_keys: bool| {
            let key = (model.primary_key().len() > 1).then(|| {
                serde_json::json!({
                    "constraintType": "PRIMARY_KEY",
                    "columns": model.primary_key().iter().map(ColumnName::as_str).collect::<Vec<_>>(),
                })
            });
            let constraints = catalog
                .relationships()
                .values()
                .filter(|relationship| with_keys && relationship.origin_model() == model.name())
                .filter_map(|relationship| {
                    let (origin, target) = one_key(relationship)?;
                    Some(serde_json::json!({
                        "constraintType": "FOREIGN_KEY",
                        "columns": [origin],
                        "referredColumns": [format!("{schema}.{}.{target}", relationship.target_model().as_str())],
                        "relationshipType": match relationship.join_type() {
                            JoinType::ManyToOne => "MANY_TO_ONE",
                            JoinType::OneToMany => "ONE_TO_MANY",
                            JoinType::OneToOne => "ONE_TO_ONE",
                        },
                    }))
                })
                .chain(key)
                .collect::<Vec<_>>();
            let mut body = serde_json::json!({
                "name": model.name().as_str(),
                "databaseSchema": schema,
                "description": model.description(),
                "columns": model.columns().map(|column| column_body(model, column)).collect::<Vec<_>>(),
            });
            if !constraints.is_empty() {
                body["tableConstraints"] = serde_json::Value::Array(constraints);
            }
            body
        };
        for with_keys in [false, true] {
            for model in catalog.models().values() {
                let (status, text) = put(agent, endpoint, "/api/v1/tables", token, &table(model, with_keys));
                assert!(
                    (200..300).contains(&status),
                    "the platform accepts table `{}` (keys: {with_keys}): {status} {text}",
                    model.name().as_str()
                );
            }
        }
        for metric in metrics.metrics().values() {
            let body =
                serde_json::json!({ "name": metric.name().as_str(), "description": metric.description(), "metricType": "OTHER" });
            let (status, text) = put(agent, endpoint, "/api/v1/metrics", token, &body);
            assert!(
                (200..300).contains(&status),
                "the platform accepts metric `{}`: {status} {text}",
                metric.name().as_str()
            );
        }
    }

    /// Runs in `just test`: every [`NOT_CARRIED`] row must still name something the golden states.
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

    /// Provisions the golden onto the live tier and reads it back through the real reader.
    #[test]
    #[ignore = "needs `just dev-up-openmetadata`; a docker service is only in the discovery file until \
                the next writer rewrites it - run `just openmetadata-acceptance`"]
    fn the_golden_catalog_round_trips_through_a_live_openmetadata() {
        let Some(endpoint) = endpoint() else { return };
        let agent = agent();
        let token = login(&agent, &endpoint);
        let golden = golden();
        let expected = carried(golden.definitions(), &NOT_CARRIED).expect("the carried golden still holds together");
        provision(
            &agent,
            &endpoint,
            &token,
            &writable(golden.definitions()),
            golden.definitions(),
        );

        let reader = || {
            HttpSnapshotReader::new(
                Endpoint::parse(&format!("http://{endpoint}")).expect("the loopback endpoint parses"),
                token.clone(),
                ReadBounds::parse(DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES).expect("the default bounds are valid"),
                None,
            )
        };
        let mut sources = BTreeMap::new();
        drop(sources.insert(
            String::from(SERVICE),
            SourceName::parse(SOURCE).expect("a source name is a name"),
        ));
        let catalog = OpenMetadataCatalog::new(
            SourceName::parse(SOURCE).expect("a source name is a name"),
            DefinitionVersion::parse(VERSION).expect("a version is a version"),
            sources,
            reader(),
        );
        let read = catalog
            .load()
            .unwrap_or_else(|error| panic!("the provisioned golden did not load through HttpSnapshotReader: {error:?}"));
        assert_eq!(
            read.definitions(),
            &expected,
            "the certified catalog a live OpenMetadata served back is the golden minus NOT_CARRIED"
        );
        if !NOT_CARRIED.contains(&NotCarried::Knowledge) {
            assert_eq!(
                read.knowledge(),
                golden.knowledge(),
                "the knowledge a live OpenMetadata served back is the golden's"
            );
        }

        // Read, not defined: the metrics page decodes live and every golden metric is on it.
        let snapshot = reader().read().expect("the live snapshot reads");
        let mut served: Vec<&str> = snapshot.metrics().iter().map(Metric::name).collect();
        served.sort_unstable();
        let mut written: Vec<&str> = golden.definitions().metrics().keys().map(MetricName::as_str).collect();
        written.sort_unstable();
        assert_eq!(served, written, "every golden metric is served by the live metrics page");
    }
}
