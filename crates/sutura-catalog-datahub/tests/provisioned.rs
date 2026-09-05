//! The provisioned `DataHub` instance, asked the one question the recorded fixture cannot answer.
//!
//! # What this test IS, stated before what it is not, because the distinction is the whole point
//!
//! `crates/sutura-catalog-datahub` is decided and tested against a recorded fixture, and
//! `docs/adr/0016` names ONE thing that fixture cannot answer: whether a real `DataHub` can carry
//! the deployment-defined document at all. `compose.services.yaml`'s `datahub` profile is that
//! instance, and this is the only thing in this repository that talks to it.
//!
//! **Two venue cells, plus one that needs no venue:**
//!
//!   * `the_deployment_names_its_property_and_the_adapter_names_its_field` - over two `&'static
//!     str`s, so it runs in `just test`. It is what makes the second cell a measurement of decision
//!     7 rather than a coincidence, and it would be held by recall if it lived inside an
//!     `#[ignore]`d cell.
//!   * `the_provisioned_datahub_serves_the_surface_a_reader_would_call` - a real `DataHub` GMS at
//!     the pinned version is reachable on the port this worktree's provisioning allocated, reports
//!     itself healthy, and serves its versioned `OpenAPI` v3 entity surface rather than `404`.
//!   * `a_document_served_by_a_real_datahub_decodes_into_a_certified_metric` - a deployment defines
//!     the property under a name of ITS OWN choosing, the platform's validator accepts the recorded
//!     corpus's own document as its scalar, and what the instance serves back decodes through this
//!     adapter into a certified `Metric` over the closed-vocabulary `Measure`. That is issue #202's
//!     feasibility question, answered against a running instance instead of a specification.
//!
//! # What is still NOT here, because the gap is the useful part
//!
//!   * **There is no HTTP `AspectReader`, so this is not a read PATH.** The only implementor
//!     outside a test is the recorded fixture; the other two are doubles - `src/tests.rs`'s
//!     `Stub` and this file's `Composed`. The request-shaping and the mapping from the response
//!     (`structuredProperties.properties[].values[].string`) onto `document::MetricAspect` are
//!     written HERE, in a test, and nowhere in `src/` - which is exactly what a real reader will
//!     have to own. What this cell removes is the excuse: the response shape and the platform's
//!     rules are measured, so writing that reader is engineering rather than research.
//!   * **Only the METRIC half is served.** The models and the relationship come from
//!     `fixture::FixtureReader`, so the snapshot this cell loads is half live and half recorded, and
//!     the certified metric therefore rests on a recorded model. Reading `dataset` and
//!     `semanticModel` aspects live is the rest of that reader's job.
//!   * **Nothing is authenticated.** The tier runs with `METADATA_SERVICE_AUTH_ENABLED: "false"`,
//!     as upstream's quickstart does, so the bearer half of a read path is untouched.
//!
//! Reading either cell as evidence of a working read path would be exactly the overstatement
//! `AGENTS.md` calls the defect itself.
//!
//! # Fail-closed where a tier was provisioned, loudly skipped where one was not
//!
//! `sutura_dev::provisioned::here` is the one decision, shared with every other harness: a job that
//! set `SUTURA_DEV_REQUIRE_TIER` gets a panic, and a developer machine gets a notice on stderr
//! naming what did not run. Nothing here skips silently, and nothing here falls back to a default
//! port - there is no constant to fall back to, which is the tier's design.
//!
//! # `#[ignore]`d, behind `just datahub-acceptance` - and the reason is a DEFECT IN THE SEAM
//!
//! This is the `just bigquery-acceptance` precedent and not a preference, and the fact that decided
//! it is worth writing down because it affects more than this test.
//!
//! **`.sutura-dev/endpoints.json` has two writers and the second erases the first.**
//! `xtask dev-up` writes every docker service it provisioned; `sutura-postgres-tier start` -
//! `nix/postgres-tier.nix`, which `just test` runs and which `checks.nextest` runs in the sandbox -
//! writes the file WHOLESALE with `postgres` as its only entry. So a docker-tier service is absent
//! from the discovery file for the whole of `just test`, whatever is actually running, and
//! `just test` sets `SUTURA_DEV_REQUIRE_TIER=1`. A fail-closed cell over a docker service is
//! therefore not merely inconvenient there - it is unconditionally red, because the tier it asks
//! about cannot be visible.
//!
//! That is a defect in the seam rather than in this test, and it is **not** fixed here: merging
//! rather than replacing raises a question this change has no business answering, namely what the
//! file's single `provisioner` field means once two provisioners contribute to it. It is recorded
//! rather than worked around silently, and the same clobbering already applies to `clickhouse` -
//! which nobody noticed only because nothing reads it yet.
//!
//! So the venue gets a named task, `just datahub-acceptance`, which brings the profile up and runs
//! this with the fail-closed direction set. **An `#[ignore]`d test is not evidence in the default
//! suite, and this file may not be cited as though it were** - what it is evidence of is whatever
//! the last run of that task reported. There is no CI venue at all: the nix sandbox has no docker
//! socket.

// `cfg(test)` because clippy only honours `allow-expect-in-tests` and `allow-panic-in-tests` for
// code inside a `#[cfg(test)]` item, and `tests_outside_test_module` wants the `#[test]` function
// inside one - the same reason `crates/sutura-runtime/tests/blocking_span.rs` and this crate's own
// `tests/multi_player.rs` are shaped this way. An integration test target is only built for tests,
// so the attribute changes nothing about what compiles.
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    use sutura_catalog_datahub::document::{MetricAspect, Snapshot};
    use sutura_catalog_datahub::{AspectReader, DataHubCatalog, DataHubError};
    use sutura_domain::model::{MetricName, SourceName};
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    /// How long GMS gets to answer. Generous: provisioning has already gated on its health check,
    /// so a request slower than this is a wedged JVM rather than a cold one, and a short timeout
    /// would turn that into a flake instead of a failure.
    const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

    /// How long the SEARCH INDEX gets to catch up with a write GMS has already accepted.
    ///
    /// A second constant rather than reusing [`ANSWER_TIMEOUT`], because the two are different
    /// quantities: that one is how long one request may take, this one is how far behind an
    /// eventually-consistent view may be. Measured at ~2 s on this tier
    /// (`just datahub-acceptance`, 2026-09-04, `DataHub` 1.7.0), so this is an order of magnitude of
    /// slack for a loaded machine rather than a threshold anything is read off.
    const INDEX_LAG_BUDGET: Duration = Duration::from_secs(30);

    /// The paths this asks for, and what each one being served means.
    ///
    /// `health` is GMS's own probe - the same one the compose health check runs, asked from OUTSIDE
    /// the container network this time, which is what makes it a statement about the published port
    /// rather than about the container.
    ///
    /// `openapi/v3/entity/dataset` is the surface `docs/adr/0016` names as the one a real reader
    /// will call. It is asked WITHOUT a query, so an empty instance answers an empty page rather
    /// than an error; what is being established is that the route exists at this pinned version,
    /// because a reader written against a surface the deployment does not serve is the failure this
    /// catches before anybody writes one.
    const PROBES: &[&str] = &["health", "openapi/v3/entity/dataset"];

    /// Where this worktree's `datahub` is listening, or `None` on a machine that provisioned none.
    ///
    /// `sutura_dev::provisioned::here` has already put the notice on stderr and, in the required
    /// direction, panicked rather than returning - so `None` here means "a developer machine with
    /// no tier", never "a job that asked for one and did not get it".
    fn endpoint() -> Option<String> {
        let inside = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        sutura_dev::provisioned::here(inside, "datahub")
            .endpoint()
            .map(ToString::to_string)
    }

    /// Built the way `sutura_exec_bigquery::wire::WireAgent::pinned` builds one, minus `https_only`:
    /// this is loopback plaintext by construction, because the tier publishes an ephemeral HTTP
    /// port.
    ///
    /// `status_as_error` is the one knob, and it is a knob because the two cells need opposite
    /// halves of `ureq` 3's default. The reachability probe wants a non-2xx to arrive as an `Err`
    /// naming the path (see its own comment). The round trip wants the platform's `400` BODY,
    /// because the whole point of those assertions is the reason `DataHub` states in it, and an
    /// `Err` carries the status without it.
    fn agent(status_as_error: bool) -> ureq::Agent {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(ANSWER_TIMEOUT))
                // A metadata platform on loopback has no reason to send this test anywhere else,
                // and a redirect is how a probe silently starts measuring a different server.
                .max_redirects(0)
                .http_status_as_error(status_as_error)
                .build(),
        )
    }

    #[test]
    #[ignore = "needs `just dev-up-datahub`; `just test` cannot see a docker service because the \
                postgres tier rewrites the discovery file - run `just datahub-acceptance`"]
    fn the_provisioned_datahub_serves_the_surface_a_reader_would_call() {
        let Some(endpoint) = endpoint() else {
            return;
        };
        let agent = agent(true);

        for path in PROBES {
            let url = format!("http://{endpoint}/{path}");
            let status = match agent.get(&url).call() {
                Ok(response) => response.status().as_u16(),
                Err(cause) => panic!(
                    "the provisioned DataHub did not answer `{path}` on {endpoint}: {cause}\n  \
                     provisioning gated on its health check, so this is the published port or the \
                     process, not a cold start. `just dev-down` then `just dev-up-datahub` rebuilds \
                     it."
                ),
            };
            // A `2xx` is the claim, and a `404` is the failure worth naming apart: it is what a
            // surface that MOVED between versions looks like - the pin in `compose.services.yaml`
            // and the path a reader is written against would then disagree, which is a defect in
            // this repository rather than in the deployment.
            //
            // **Which of the two arms a `404` arrives through was MEASURED rather than assumed, and
            // it is not this one.** `ureq` 3 treats a non-2xx as an `Err` by default, so a missing
            // route reaches the `panic!` above with the path in its message; verified by pointing
            // this at `openapi/v9/entity/nonesuch`, which failed there and not here. So this range
            // check is the belt-and-braces half - it catches a `3xx` that `max_redirects(0)` turned
            // into a returned response rather than a follow - and the diagnostic a reader will
            // actually see for a moved surface is the one above.
            assert!(
                (200..300).contains(&status),
                "the provisioned DataHub answered `{path}` with {status}, not a 2xx - if that is a \
                 404, the pinned version does not serve the surface `docs/adr/0016` names, and a \
                 reader written against it would fail the same way"
            );
        }
    }

    /// The name THIS deployment gives its structured property, and the one place in the repository
    /// where such a name is written down.
    ///
    /// **It deliberately contains no `sutura`, and that is the assertion rather than the style.**
    /// `docs/adr/0016` decision 7 says the property - its name, its urn, its value type, its
    /// cardinality, the entity types it binds to - is the DEPLOYMENT's, while the grammar of the
    /// document inside it is this repository's. A cell that registered the property under the same
    /// name as [`CANONICAL_FIELD`] could not tell those two apart: it would pass equally whether the
    /// name were the deployment's choice or a constant the library requires. Naming it something
    /// else is what makes the boundary a measurement, and the mapping below is where the deployment
    /// name meets the adapter's own field.
    const DEPLOYMENT_PROPERTY: &str = "deployment_metric_document";

    /// The field name on `document::MetricAspect`, which is this adapter's OWN canonical shape
    /// rather than `DataHub`'s envelope (`document.rs`'s header is explicit about that).
    ///
    /// Named here so the difference from [`DEPLOYMENT_PROPERTY`] is checked rather than described.
    const CANONICAL_FIELD: &str = "sutura";

    /// The second metric the paging claim needs, so the population is one where a page and a
    /// request-per-metric differ.
    const BESIDE_IT: &str = "revenue_beside_it";

    /// The two names held apart, checked where no venue is needed.
    ///
    /// **Its own cell, and NOT `#[ignore]`d, which is the point of splitting it out.** The
    /// independence of the deployment's property name from the adapter's field name is what makes
    /// the cell below a measurement rather than a coincidence - and a guard that only runs when
    /// somebody types `just datahub-acceptance` is held by recall for every other run. These are
    /// two `&'static str`s; `str::contains` is not const-callable, so a `#[test]` in the default
    /// suite is the closest available thing to a compile-time assertion.
    #[test]
    fn the_deployment_names_its_property_and_the_adapter_names_its_field() {
        assert!(
            !DEPLOYMENT_PROPERTY.contains(CANONICAL_FIELD),
            "`{DEPLOYMENT_PROPERTY}` must share nothing with `{CANONICAL_FIELD}`: a venue cell that \
             registered the property under the adapter's own field name would pass equally whether \
             the name were the deployment's choice or a constant this library requires, which is the \
             one thing `docs/adr/0016` decision 7 is about"
        );
    }

    /// One request, one status, one body - because the refusals asserted below are about the reason
    /// `DataHub` states, and a helper that discarded the body would discard the assertion.
    ///
    /// A body means an upsert and no body means a read; this surface needs no other verb, so the
    /// `Option` is the method rather than a second parameter that could disagree with it.
    fn send(agent: &ureq::Agent, url: &str, body: Option<&serde_json::Value>) -> (u16, String) {
        let mut response = body
            .map_or_else(
                || agent.get(url).call(),
                |json| {
                    agent
                        .post(url)
                        .header("Content-Type", "application/json")
                        .send(serde_json::to_string(json).expect("a probe body serializes"))
                },
            )
            .expect("the provisioned DataHub answered, whatever it answered - `just dev-up-datahub` rebuilds it");
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().expect("the answer is text");
        (status, text)
    }

    /// The urn of one metric entity. Written raw rather than percent-encoded: `(`, `)`, `,` and `:`
    /// are all legal in a path, and the surface answers `200` to both spellings - measured.
    fn metric_urn(id: &str) -> String {
        format!("urn:li:metric:(urn:li:dataPlatform:bigquery,orders,{id})")
    }

    /// The urn of the deployment's property. Derived from [`DEPLOYMENT_PROPERTY`] in one place, so
    /// the name the definition registers and the name a value is written under cannot disagree.
    fn property_urn() -> String {
        format!("urn:li:structuredProperty:{DEPLOYMENT_PROPERTY}")
    }

    /// A metric entity carrying `values` under the deployment's property, upserted synchronously.
    ///
    /// `raw` is where the promotion candidate's half comes from, and it is a parameter rather than
    /// two literals for a reason the equality assertion below depends on: that assertion compares
    /// the harvested aspect against the RECORDED one, so a `dialect` or an `expression` spelled here
    /// would turn the next fixture edit into a red with no cause in the edit.
    fn write_property(
        agent: &ureq::Agent,
        endpoint: &str,
        id: &str,
        raw: &MetricAspect,
        values: &serde_json::Value,
    ) -> (u16, String) {
        let body = serde_json::json!([{
            "urn": metric_urn(id),
            "metricKey": { "value": { "platform": "urn:li:dataPlatform:bigquery", "path": "orders", "id": id } },
            "metricInfo": { "value": {
                "name": id,
                "expression": { "dialects": [{ "dialect": raw.dialect(), "expression": raw.expression() }] },
            } },
            "structuredProperties": { "value": { "properties": [{ "propertyUrn": property_urn(), "values": values }] } },
        }]);
        send(
            agent,
            &format!("http://{endpoint}/openapi/v3/entity/metric?async=false&createIfNotExists=false"),
            Some(&body),
        )
    }

    /// The number `DataHub` names in a refusal as its own ceiling, so the headroom below is a
    /// measured ratio against the platform's stated limit rather than a constant this repository
    /// would have to keep in step with it.
    fn stated_maximum(refusal: &str) -> usize {
        let Some((_, after)) = refusal.split_once("maximum of ") else {
            panic!("the refusal states the maximum it enforced: {refusal}");
        };
        after
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("the stated maximum is a number")
    }

    /// The mapping a real `AspectReader` will own, written here because it is not written in `src/`.
    ///
    /// One served entity into this adapter's own [`MetricAspect`]: the promotion candidate's raw
    /// half out of `metricInfo`, and the certified half out of the value of the property the
    /// DEPLOYMENT named. `serde_json::from_value` then runs the library's own decode - the one
    /// carrying `deny_unknown_fields` - so the assertion below is about the adapter's shape and not
    /// about this function's.
    fn harvest(served: &serde_json::Value) -> MetricAspect {
        let info = &served["metricInfo"]["value"];
        let dialect = &info["expression"]["dialects"][0];
        let scalar = served["structuredProperties"]["value"]["properties"]
            .as_array()
            .expect("the served aspect carries a property list")
            .iter()
            .find(|property| {
                property["propertyUrn"]
                    .as_str()
                    .is_some_and(|urn| urn.ends_with(DEPLOYMENT_PROPERTY))
            })
            .expect("the entity carries the property this deployment defined")["values"][0]["string"]
            .clone();
        let mut aspect = serde_json::Map::new();
        drop(aspect.insert(String::from("name"), info["name"].clone()));
        drop(aspect.insert(String::from("dialect"), dialect["dialect"].clone()));
        drop(aspect.insert(String::from("expression"), dialect["expression"].clone()));
        // The one line where the deployment's property name meets the adapter's own field name.
        drop(aspect.insert(String::from(CANONICAL_FIELD), serde_json::json!({ "string_value": scalar })));
        serde_json::from_value(serde_json::Value::Object(aspect))
            .expect("the served document decodes into this adapter's canonical metric aspect")
    }

    /// A reader over a snapshot the caller composed, so `DataHubCatalog::load` can be driven over a
    /// snapshot whose metric half came off the wire and whose structural half did not.
    ///
    /// `src/tests.rs`'s `Stub` is the same shape and unreachable from here: an integration test is a
    /// separate crate and cannot see a `#[cfg(test)]` item.
    #[derive(Debug)]
    struct Composed(Snapshot);

    impl AspectReader for Composed {
        fn read(&self) -> Result<Snapshot, DataHubError> {
            Ok(self.0.clone())
        }
    }

    /// The source alias the corpus's one platform answers to. `fixture::over_fixture_source` makes
    /// the same mapping; this cell composes its own snapshot, so it makes it here.
    fn source_name() -> SourceName {
        SourceName::parse("local").expect("a test source name is a name")
    }

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test").expect("a test version is a version")
    }

    /// The deployment-defined document, written into a real `DataHub`, served back, and decoded by
    /// this adapter into a certified metric.
    ///
    /// # What this proves that the recorded fixture cannot
    ///
    /// The fixture proves what the adapter DECIDES about a document. Everything below is the
    /// platform's behaviour or the join between the two, and none of it was measured before:
    ///
    ///   * **A deployment can define the property under a name of its own**, and the platform
    ///     accepts the recorded corpus's own document as its scalar. Not a paraphrase of the
    ///     document: it is read out of `fixture::FixtureReader` through the crate's public port, so
    ///     what is written is provably the string the unit half decodes.
    ///   * **What the instance serves decodes into a certified `Metric`.** The served aspect is
    ///     mapped onto `document::MetricAspect`, and the decoded aspect is EQUAL to the one the
    ///     recorded fixture carries - which is the strongest available statement that the fixture is
    ///     faithful to the platform rather than to itself. `DataHubCatalog::load` then produces the
    ///     closed-vocabulary `Measure`, which is issue #202's question.
    ///   * **A read by urn is immediately consistent; the paged read is NOT.** Asserted in the shape
    ///     the difference has: the by-urn read is asked once, with no retry, straight after a
    ///     synchronous write, while the paged read is given a deadline. The paged surface is
    ///     search-backed - it answers `facets` and `totalCount` - so a reader that pages sees a
    ///     metric only after the index catches up. That is the caveat the read path's COST claim
    ///     needs: one page per entity type is the right cost, and it is not read-your-writes.
    ///   * **A page per entity type rather than a request per metric**, over a population where the
    ///     two differ. TWO metrics are written and ONE response has to carry both - which is the
    ///     assertion, because over a single metric a page and a per-metric fetch return the same
    ///     thing and the claim separates nothing.
    ///   * **The scalar has a ceiling, and it is the platform's `keywordMaxLength`** - the value is
    ///     indexed as an Elasticsearch keyword - so what bounds a metric's document is a
    ///     deployment's index configuration and not a constant in this crate. Asserted as a ratio
    ///     against the number the refusal names, because the number is a deployment's to change.
    ///   * **`SINGLE` cardinality and the declared value type are enforced server-side**, so the
    ///     `docs/adr/0016` transport note's "one string-valued property" is the platform's rule and
    ///     not merely this adapter's reading of it.
    ///
    /// # And what it still does not prove
    ///
    /// **This is not the adapter's read path**, and the module header says which pieces are absent:
    /// no `AspectReader` over HTTP, the request shaping and the mapping in this file rather than in
    /// `src/`, the structural half of the snapshot still recorded, and no authentication.
    #[test]
    #[ignore = "needs `just dev-up-datahub`; `just test` cannot see a docker service because the \
                postgres tier rewrites the discovery file - run `just datahub-acceptance`"]
    fn a_document_served_by_a_real_datahub_decodes_into_a_certified_metric() {
        let Some(endpoint) = endpoint() else {
            return;
        };
        let agent = agent(false);

        // The deployment's half: register the property, under the deployment's own name. Upserted,
        // so a re-run over a tier that already has it is the same request rather than a conflict.
        let definition = serde_json::json!([{
            "urn": property_urn(),
            "propertyDefinition": { "value": {
                "qualifiedName": DEPLOYMENT_PROPERTY,
                "displayName": DEPLOYMENT_PROPERTY,
                "valueType": "urn:li:dataType:datahub.string",
                "cardinality": "SINGLE",
                "entityTypes": ["urn:li:entityType:datahub.metric"],
                "description": "The closed-vocabulary metric document a deployment defines.",
            } },
        }]);
        let (status, body) = send(
            &agent,
            &format!("http://{endpoint}/openapi/v3/entity/structuredproperty?async=false"),
            Some(&definition),
        );
        assert_eq!(status, 200, "the platform accepts the property definition: {body}");

        // The corpus's own metric aspect, through the crate's public port rather than restated here.
        let recorded = sutura_catalog_datahub::fixture::FixtureReader
            .read()
            .expect("the recorded corpus reads");
        let certified = recorded
            .metrics()
            .iter()
            .find(|metric| metric.sutura().is_some())
            .expect("the corpus carries one certified metric");
        let property = certified
            .sutura()
            .expect("the metric just found is the one carrying the property");
        let urn = metric_urn(certified.name());

        let scalar = serde_json::json!([{ "string": property.string_value() }]);
        let (status, body) = write_property(&agent, &endpoint, certified.name(), certified, &scalar);
        assert_eq!(status, 200, "the platform accepts the corpus document as the scalar: {body}");

        // A SECOND metric, because the paging claim below is about a population where one page and
        // one request per metric differ. Same document under a different entity id: what is being
        // measured is the response, not the content.
        let (status, body) = write_property(&agent, &endpoint, BESIDE_IT, certified, &scalar);
        assert_eq!(
            status, 200,
            "the platform accepts a second metric carrying the property: {body}"
        );

        // ONE request by urn, asked ONCE. No retry is the assertion: this surface is
        // read-your-writes after a synchronous upsert, which is what makes it the one a reader can
        // trust immediately - and what makes the paged read below a different claim.
        let (status, body) = send(
            &agent,
            &format!("http://{endpoint}/openapi/v3/entity/metric/{urn}?aspects=structuredProperties&aspects=metricInfo"),
            None,
        );
        assert_eq!(
            status, 200,
            "a read by urn answers straight after a synchronous write: {body}"
        );
        let served: serde_json::Value = serde_json::from_str(&body).expect("the answer is json");

        // The harvest. The decoded aspect is compared against the RECORDED one, so this asserts the
        // round trip, the mapping and the fixture's fidelity to the platform in one comparison.
        let harvested = harvest(&served);
        assert_eq!(
            &harvested, certified,
            "the aspect decoded from what DataHub served is the aspect the recorded corpus carries"
        );

        // ...and through to the closed vocabulary, which is what issue #202 asked whether DataHub
        // could carry. The structural half is the corpus's: see the module header.
        let snapshot = Snapshot::new(
            recorded.datasets().to_vec(),
            recorded.relationships().to_vec(),
            vec![harvested],
        );
        let mut sources = BTreeMap::new();
        drop(sources.insert(String::from("bigquery"), source_name()));
        let bundle = DataHubCatalog::new(source_name(), version(), sources, Composed(snapshot))
            .load()
            .expect("the harvested metric assembles into a bundle");
        let name = MetricName::parse(certified.name()).expect("the corpus metric is named");
        let metric = bundle
            .definitions()
            .metric(&name)
            .expect("the harvested metric is certified rather than a promotion candidate");
        let content = property.assemble().expect("the recorded document decodes");
        assert_eq!(
            metric.measure(),
            content.measure(),
            "the certified measure is the one the served document carried, over the domain's closed \
             set - not the raw `{}` expression string beside it, which stays the promotion \
             candidate's half",
            certified.expression()
        );

        one_response_carries_both(&agent, &endpoint, certified);
        the_platforms_own_rules(&agent, &endpoint, certified, property.string_value().len());
    }

    /// The paging claim, over a population where paging and a fetch-per-metric differ.
    ///
    /// **Two metrics and ONE response**, which is the whole of why the second one is written: over a
    /// single entity a page and a request per metric return the same thing, so an assertion over one
    /// separates neither hypothesis. Given a DEADLINE rather than asked once, because this surface is
    /// search-backed and its index lags a synchronous write - the by-urn read in the caller answered
    /// at once and is asked exactly once for that contrast.
    fn one_response_carries_both(agent: &ureq::Agent, endpoint: &str, certified: &MetricAspect) {
        let wanted = [metric_urn(certified.name()), metric_urn(BESIDE_IT)];
        let deadline = Instant::now() + INDEX_LAG_BUDGET;
        let page = loop {
            let (status, body) = send(
                agent,
                &format!("http://{endpoint}/openapi/v3/entity/metric?aspects=structuredProperties&aspects=metricInfo&count=100"),
                None,
            );
            assert_eq!(status, 200, "the metric surface answers a paged read: {body}");
            let answer: serde_json::Value = serde_json::from_str(&body).expect("the answer is json");
            let entities = answer["entities"].as_array().expect("a page is an array of entities").clone();
            // Matched on the WHOLE urn rather than on a suffix: the page holds every metric a
            // previous run wrote, and a suffix is how one of those becomes the one asserted on.
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
                break found;
            }
            assert!(
                Instant::now() < deadline,
                "one response never carried both metrics that synchronous writes had already \
                 accepted, within {INDEX_LAG_BUDGET:?} - it had {}. Either the index is far further \
                 behind than the ~2 s this tier was measured at, or this surface does not page the \
                 way a reader would need it to",
                found.len()
            );
            std::thread::sleep(Duration::from_millis(250));
        };
        assert_eq!(
            page.len(),
            wanted.len(),
            "one response carried both metrics, so a bundle is a page per entity type rather than a \
             request per metric"
        );
        assert_eq!(
            page.first().map(harvest).as_ref(),
            Some(certified),
            "and each entity on that page carries the certified half and the promotion candidate's \
             raw half inline, so paging costs no second request per metric"
        );
    }

    /// The three rules `docs/adr/0016` reads off the property's TYPE, asked of the platform instead.
    ///
    /// Each is a `400` with its own reason in the body, which is why [`send`] keeps the body: the
    /// claim is what `DataHub` states, not merely that it refused.
    fn the_platforms_own_rules(agent: &ureq::Agent, endpoint: &str, certified: &MetricAspect, scalar: usize) {
        // The ceiling. Deliberately far past it, so the assertion is about the refusal being
        // STATED rather than about where the boundary sits - which is an index setting.
        let (status, body) = write_property(
            agent,
            endpoint,
            "over_the_ceiling",
            certified,
            &serde_json::json!([{ "string": "x".repeat(1 << 17) }]),
        );
        assert_eq!(status, 400, "a scalar past the platform's ceiling is refused: {body}");
        assert!(
            body.contains("keywordMaxLength"),
            "the refusal names the setting that bounds it, which is the whole of what is measured \
             here - nothing raises it and retries: {body}"
        );
        let maximum = stated_maximum(&body);
        assert!(
            scalar * 10 < maximum,
            "the corpus document ({scalar} bytes) has an order of magnitude of headroom under the \
             platform's stated maximum ({maximum} bytes) - if this fails, a metric's document has \
             grown into the index limit and the transport needs revisiting, not the test"
        );

        let (status, body) = write_property(
            agent,
            endpoint,
            "two_values",
            certified,
            &serde_json::json!([{ "string": "{}" }, { "string": "{}" }]),
        );
        assert_eq!(status, 400, "a second value on a SINGLE property is refused: {body}");
        assert!(body.contains("cardinality"), "the refusal names the cardinality: {body}");

        let (status, body) = write_property(
            agent,
            endpoint,
            "wrong_type",
            certified,
            &serde_json::json!([{ "double": 1.0 }]),
        );
        assert_eq!(status, 400, "a number where the definition says string is refused: {body}");
        assert!(
            body.contains("should be a string"),
            "the refusal names the declared value type: {body}"
        );
    }
}
