//! The provisioned `DataHub` instance, asked whether it is there and whether the surface a reader
//! would use answers.
//!
//! # What this test IS, stated before what it is not, because the distinction is the whole point
//!
//! `crates/sutura-catalog-datahub` is decided and tested against a recorded fixture, and
//! `docs/adr/0016` names ONE thing that fixture cannot answer: the read path against a provisioned
//! instance. `compose.services.yaml`'s `datahub` profile is that instance, and this is the first
//! thing in this repository that talks to it.
//!
//! **It proves the VENUE and the PLATFORM's half, and not the reader.** Two cells:
//!
//!   * `the_provisioned_datahub_serves_the_surface_a_reader_would_call` - a real `DataHub` GMS at
//!     the pinned version is reachable on the port this worktree's provisioning allocated, reports
//!     itself healthy, and serves its versioned `OpenAPI` v3 entity surface rather than `404`.
//!   * `the_deployment_defined_document_round_trips_through_a_provisioned_datahub` - a deployment
//!     CAN define the property `docs/adr/0016` describes, the platform's own validator accepts the
//!     corpus's own document as its scalar value, the document comes back byte for byte, and the
//!     `SINGLE` cardinality, the declared value type and the scalar's ceiling are enforced by the
//!     platform rather than only read off its schema. That is issue #202's feasibility question,
//!     answered against a running instance instead of a specification.
//!
//! What is still NOT here, stated because the gap is the useful part:
//!
//!   * **No aspect is DECODED.** There is no HTTP `AspectReader` - the only implementor is the
//!     recorded fixture - so nothing turns the served document into a `Snapshot`. The round trip is
//!     driven by this test's own requests, and the response shape it measures
//!     (`structuredProperties.properties[].values[].string`) is what that reader will have to map
//!     onto `document::MetricAspect`'s `sutura` field.
//!   * **No property NAME is the library's.** The urn appears in this file and nowhere in
//!     `src/`, which is the boundary `docs/adr/0016` draws: the grammar of the document is this
//!     repository's, the property that carries it is the deployment's.
//!   * **Nothing is authenticated.** The tier runs with `METADATA_SERVICE_AUTH_ENABLED: "false"`,
//!     as upstream's quickstart does, so the bearer half of the read path is untouched.
//!
//! Reading either cell as evidence of a working read path would be exactly the overstatement
//! `AGENTS.md` calls the defect itself. What they remove is the excuse: the next change writes a
//! reader, and both the venue and the platform's acceptance of the document are already measured.
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
//! the last run of that task reported.

// `cfg(test)` because clippy only honours `allow-expect-in-tests` and `allow-panic-in-tests` for
// code inside a `#[cfg(test)]` item, and `tests_outside_test_module` wants the `#[test]` function
// inside one - the same reason `crates/sutura-runtime/tests/blocking_span.rs` and this crate's own
// `tests/multi_player.rs` are shaped this way. An integration test target is only built for tests,
// so the attribute changes nothing about what compiles.
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sutura_catalog_datahub::AspectReader as _;

    /// How long GMS gets to answer. Generous: provisioning has already gated on its health check,
    /// so a request slower than this is a wedged JVM rather than a cold one, and a short timeout
    /// would turn that into a flake instead of a failure.
    const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

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

    /// The structured property this cell registers, and the one place in the repository where a
    /// property urn is written down.
    ///
    /// **It is written HERE and nowhere in the library, which is the whole distinction.** What a
    /// deployment defines is the property: its name, its urn, its value type, its cardinality and
    /// the entity types it binds to. The library names none of those - `document::MetricAspect`'s
    /// `sutura` field is the adapter's own canonical shape, and mapping a registered property onto
    /// it is the unbuilt HTTP `AspectReader`'s job. So this constant is **the test playing the
    /// deployment's part**, not the adapter reading a name it knows.
    const PROPERTY_URN: &str = "urn:li:structuredProperty:sutura";

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

    /// A metric entity carrying `values` under [`PROPERTY_URN`], upserted synchronously.
    fn write_property(agent: &ureq::Agent, endpoint: &str, id: &str, values: &serde_json::Value) -> (u16, String) {
        let urn = format!("urn:li:metric:(urn:li:dataPlatform:bigquery,orders,{id})");
        let body = serde_json::json!([{
            "urn": urn,
            "metricKey": { "value": { "platform": "urn:li:dataPlatform:bigquery", "path": "orders", "id": id } },
            "metricInfo": { "value": {
                "name": id,
                "expression": { "dialects": [{ "dialect": "ANSI_SQL", "expression": "SUM(amount_cents)" }] },
            } },
            "structuredProperties": { "value": { "properties": [{ "propertyUrn": PROPERTY_URN, "values": values }] } },
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

    /// The deployment-defined document, written into a real `DataHub` and read back out.
    ///
    /// # What this proves that the recorded fixture cannot
    ///
    /// The fixture proves what the adapter DECIDES about a document. Four things about the document
    /// itself were open until they were measured here, and all four are the platform's behaviour
    /// rather than this repository's:
    ///
    ///   * **The platform's own validator accepts the corpus document as a property value.** Not a
    ///     paraphrase of it: the document is read out of `fixture::FixtureReader` through the
    ///     crate's public port, so what is written is provably the string the unit half decodes,
    ///     and the two cannot drift.
    ///   * **It survives the round trip byte for byte**, which is what makes the scalar a transport
    ///     rather than a lossy field.
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
    /// **This is not the adapter's read path.** There is no HTTP `AspectReader`, so nothing here
    /// decodes the served document into a `Snapshot`; the mapping from the response shape
    /// (`structuredProperties.properties[].values[].string`) to `MetricAspect.sutura` is written by
    /// the next change, and this cell is the venue it will be measured in. Nothing here is
    /// authenticated either - the tier runs with `METADATA_SERVICE_AUTH_ENABLED: "false"`.
    #[test]
    #[ignore = "needs `just dev-up-datahub`; `just test` cannot see a docker service because the \
                postgres tier rewrites the discovery file - run `just datahub-acceptance`"]
    fn the_deployment_defined_document_round_trips_through_a_provisioned_datahub() {
        let Some(endpoint) = endpoint() else {
            return;
        };
        let agent = agent(false);

        // The deployment's half: register the property. Upserted, so a re-run over a tier that
        // already has it is the same request rather than a conflict.
        let definition = serde_json::json!([{
            "urn": PROPERTY_URN,
            "propertyDefinition": { "value": {
                "qualifiedName": "sutura",
                "displayName": "sutura",
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

        // The corpus's own document, through the crate's public port rather than restated here.
        let snapshot = sutura_catalog_datahub::fixture::FixtureReader
            .read()
            .expect("the recorded corpus reads");
        let document = snapshot
            .metrics()
            .iter()
            .find_map(|metric| metric.sutura())
            .expect("the corpus carries one certified metric")
            .string_value()
            .to_owned();

        let (status, body) = write_property(&agent, &endpoint, "revenue", &serde_json::json!([{ "string": document }]));
        assert_eq!(status, 200, "the platform accepts the corpus document as the scalar: {body}");

        // ONE request for the whole metric half of a bundle, aspects inline - which is the answer
        // to the read path's cost that `docs/adr/0016` and `docs/implementation-plan.md` both leave
        // open. A reader pages this surface; it does not fetch an entity per metric.
        let (status, body) = send(
            &agent,
            &format!("http://{endpoint}/openapi/v3/entity/metric?aspects=structuredProperties&aspects=metricInfo&count=100"),
            None,
        );
        assert_eq!(status, 200, "the metric surface answers a paged read: {body}");
        let served: serde_json::Value = serde_json::from_str(&body).expect("the answer is json");
        let entity = served["entities"]
            .as_array()
            .expect("a page is an array of entities")
            .iter()
            .find(|entity| entity["urn"].as_str().is_some_and(|urn| urn.ends_with(",revenue)")))
            .expect("the metric just written is on the page");
        assert_eq!(
            entity["structuredProperties"]["value"]["properties"][0]["propertyUrn"],
            serde_json::json!(PROPERTY_URN),
            "the page carries the property inline, not a reference to fetch"
        );
        assert_eq!(
            entity["metricInfo"]["value"]["expression"]["dialects"][0]["dialect"],
            serde_json::json!("ANSI_SQL"),
            "the promotion candidate's raw half rides the same response as the certified half"
        );
        assert_eq!(
            entity["structuredProperties"]["value"]["properties"][0]["values"][0]["string"],
            serde_json::json!(document),
            "the document read back is the document written, byte for byte"
        );

        // The ceiling. Deliberately far past it, so the assertion is about the refusal being
        // STATED rather than about where the boundary sits - which is a deployment's setting.
        let (status, body) = write_property(
            &agent,
            &endpoint,
            "over_the_ceiling",
            &serde_json::json!([{ "string": "x".repeat(1 << 17) }]),
        );
        assert_eq!(status, 400, "a scalar past the platform's ceiling is refused: {body}");
        assert!(
            body.contains("keywordMaxLength"),
            "the refusal names the setting that bounds it, so a deployment knows what to raise: {body}"
        );
        let maximum = stated_maximum(&body);
        assert!(
            document.len() * 10 < maximum,
            "the corpus document ({} bytes) has an order of magnitude of headroom under the platform's \
             stated maximum ({maximum} bytes) - if this fails, a metric's document has grown into the \
             index limit and the transport needs revisiting, not the test",
            document.len()
        );

        // The two constraints `docs/adr/0016` reads off the property's TYPE, asked of the platform.
        let (status, body) = write_property(
            &agent,
            &endpoint,
            "two_values",
            &serde_json::json!([{ "string": "{}" }, { "string": "{}" }]),
        );
        assert_eq!(status, 400, "a second value on a SINGLE property is refused: {body}");
        assert!(body.contains("cardinality"), "the refusal names the cardinality: {body}");

        let (status, body) = write_property(&agent, &endpoint, "wrong_type", &serde_json::json!([{ "double": 1.0 }]));
        assert_eq!(status, 400, "a number where the definition says string is refused: {body}");
        assert!(
            body.contains("should be a string"),
            "the refusal names the declared value type: {body}"
        );
    }
}
