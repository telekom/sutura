//! The deployment a user actually gets: a settings file, a catalog directory, a listener, a bearer
//! token, one question, rows.
//!
//! **This is the only place in the repository where anything asks the SERVED deployment a
//! question.** `github.com/telekom/sutura#117` is the measurement that says why it had to exist:
//! `sutura-serve`'s twenty-one named tests are all startup refusals, `sutura_http::harness`
//! exercises the router in-crate over a fake service, and `crates/sutura-cli/tests/example.rs` runs
//! the example through a different binary with a different composition and no HTTP at all. So every
//! invariant that lives only on the composed path - the bearer gate, route governance, the generated
//! document as SERVED, the refusal statuses out of a real transport - had no end-to-end evidence on
//! any binary in any tier.
//!
//! # It spawns the binary, and that is not a shortcut
//!
//! `sutura-serve` is `[[bin]]` and nothing else: there is no library target, so an integration test
//! cannot call `run` and a test that assembled its own router would be proving the router rather
//! than the composition. `CARGO_BIN_EXE_sutura-serve` is the binary cargo just built from this
//! crate's own `main.rs`, so what is under test is the REAL composition root - the same environment
//! reading, the same layered settings, the same catalog load, the same anchor re-execution, the same
//! `ServiceState`, the same router, the same listener - reached the only way a deployment reaches it.
//!
//! # Why it can be a gate rather than a `nix run` app
//!
//! A files-backed source needs no network and no credential, so unlike `just bigquery-acceptance`
//! this runs inside `checks.nextest` - which is what makes it a real gate. The port comes from the
//! kernel (`server.port: 0`, read back off the socket and printed by `sutura_http::server::serve`),
//! because two checks share one sandbox and a fixed port is how that becomes a flake.
//!
//! # What the harness reads, and why the log is the channel
//!
//! `telemetry.format: bunyan` is written into the settings so the service's own log is one JSON
//! object per line, and the harness waits for the `listening` event and takes the address off its
//! `bound` field. Not a guess and not a scan for a free port: the kernel chose it, the process
//! reported it, and the two cannot disagree. The same stream is what the ordering assertion in
//! `the_liveness_probe_answers_only_once_the_catalog_has_loaded` reads.
//!
//! # `unix` only
//!
//! Stopping the process is `SIGTERM`, because that is the shutdown path a deployment takes and the
//! one `sutura_runtime::shutdown` installs a handler for. `unsafe_code` is `forbid` in this
//! workspace, so `libc::kill` is not available to a test, and `Child::kill` is `SIGKILL` - which
//! proves the process can be destroyed rather than that it drains. So the signal is sent through
//! `/bin/sh`, whose `kill` is a shell builtin present in every sandbox this suite runs in, and the
//! whole module is `cfg(unix)`.

// `cfg(test)` for the reason `crates/sutura-cli/tests/example.rs` gives: clippy honours
// `allow-expect-in-tests` only inside a `#[cfg(test)]` item, and without it every `expect` in this
// file and in `served/harness.rs` is a lint error.
// `cfg(unix)` and `cfg(test)` as TWO attributes rather than `cfg(all(test, unix))`, which is not a
// style choice: clippy looks for a literal `#[cfg(test)]` on an ancestor module to decide whether
// `allow-expect-in-tests` applies and whether `tests_outside_test_module` fires, and it does not
// see through an `all(..)`. Written the other way this module was thirty-five lint errors.
//
// The harness carries the same two attributes for the same two reasons - a `#[cfg(test)]` on the
// declaration is what puts a literal one on `harness`'s own ancestor chain - and it is declared here
// at the top level rather than inside `mod tests` because a `#[path]` inside an inline module
// resolves against that module's directory rather than this file's.
#[cfg(unix)]
#[cfg(test)]
#[path = "served/harness.rs"]
mod harness;

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    // The settings TYPE, in the process running the test, and it is here for one job: to render the
    // sentence a refusing deployment must print. See `refusal_of`.
    use sutura_config::{Environment, NotFitToServe, Settings, SettingsError, Sources};
    use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};

    // The harness, next door. It holds no assertion - see its own module documentation for why the
    // split moved this direction and not the other.
    use crate::harness::{
        LOCAL_SOURCE, LOOKUP_SOURCE, LOOPBACK, RECORD, RESOURCE, SINGLE_USER, TOKEN, VERSION, accepted_by, an_issuer, deployment,
        drained, example_root, position, question, recurring_revenue_by_region, recurring_revenue_june, refused_to_start,
        settings_declaring_inbound, settings_spanning_two_sources, start, start_configured, v1,
    };

    // ------------------------------------------------------------------- the harness itself ---

    #[test]
    fn a_finished_process_log_is_collected_after_its_readers_finish_and_not_before() {
        // `github.com/telekom/sutura#387` as an interleaving rather than as a rate, and it is the
        // HARNESS under test here, not the deployment. The defect: both handles were dropped at the spawn
        // and the channel was swept with `try_recv` once the child had been reaped. A reaped child
        // has exited; it has not thereby been READ, and the last thing a refusing deployment writes
        // is its refusal. So the sweep returned the log minus the one line the assertion was about
        // and the test failed as *the deployment did not refuse the declaration it cannot serve* -
        // a false report of leg 1 being unarmed, which is the worst wrong answer this suite has.
        //
        // The barrier removes the other order: the reader cannot have sent before the collection
        // begins, because it is released on the line above the call.
        //
        // **The limit, next to the claim.** What separates the two behaviours after the barrier is
        // an INJECTED DELAY, not an ordering a type holds: a collector that joins waits the 250ms
        // out, one that drains what is already there returns empty in microseconds. Five orders of
        // magnitude is evidence, not impossibility, and the honest way to red the old shape is the
        // mutation - put the `drop`-and-sweep back into `drained` and this test fails every run.
        let last = "the inbound identity declared by this deployment is not usable";
        let (sender, lines) = channel::<String>();
        let released = Arc::new(Barrier::new(2));
        let waiting = Arc::clone(&released);
        let reader = std::thread::spawn(move || {
            waiting.wait();
            std::thread::sleep(Duration::from_millis(250));
            drop(sender.send(String::from(last)));
        });

        released.wait();
        assert_eq!(
            drained(vec![reader], &lines),
            vec![String::from(last)],
            "the collector swept the channel before the reader had written the process's last line"
        );
    }

    #[test]
    fn the_liveness_probe_answers_only_once_the_catalog_has_loaded() {
        // The harness's own first test, kept trivial on purpose: a harness whose first assertion is
        // complicated is a harness whose failures mean nothing. If this is red, nothing below it is
        // worth reading.
        //
        // The ORDERING half is #117's open question - "the liveness probe answers before the catalog
        // is loaded, or does not, and the test says which" - and this is the answer: it does not.
        // The listener is opened at step 7 of `main::run` and the catalog is loaded at step 6, so
        // there is no window in which this process accepts a connection with an unvalidated bundle
        // behind it. That is also why `sutura_http::routes::health` has no readiness route: a
        // process that is listening has already reproduced every anchor its author certified.
        let served = start("liveness");
        let probe = served.get(sutura_http::constants::HEALTH_PATH, None);
        assert_eq!(probe.status, 200, "{}", probe.body);
        // Byte for byte, which is `routes::health`'s own rule: the body carries no version, no
        // build and no catalog, and this is the assertion that fails when somebody adds one.
        assert_eq!(probe.body, r#"{"status":"ok"}"#);

        let log = served.log();
        assert!(
            position(&log, "catalog loaded and every anchor reproduced its number") < position(&log, "listening"),
            "the listener opened before the bundle was validated:\n{}",
            log.join("\n")
        );
    }

    #[test]
    fn the_real_shutdown_path_stops_the_process_on_a_terminate_signal() {
        // The other half of the harness, and the one that makes every test above it honest: the
        // process is stopped the way an orchestrator stops it, and it exits successfully rather than
        // being destroyed. `Child::kill` would prove neither.
        //
        // `stopped` is the last line `main::stop` writes, after `Runtime::shutdown_timeout` has
        // given the blocking pool what was left of the grace period - so its presence is the
        // evidence that the configured drain ran rather than that the process vanished.
        let mut served = start("shutdown");
        assert_eq!(served.get(sutura_http::constants::HEALTH_PATH, None).status, 200);
        let status = served.terminate();
        assert!(
            status.success(),
            "a terminate signal did not produce a clean exit: {status:?}"
        );
        let log = served.log();
        assert!(
            position(&log, "stopped") > position(&log, "listening"),
            "the process did not report stopping:\n{}",
            log.join("\n")
        );
    }

    // ---------------------------------------------------------------------------- the cases ---

    #[test]
    fn a_question_with_no_bearer_token_is_refused_by_the_gate() {
        // The bug this prevents: a deployment that configured a token and serves its whole surface
        // to anybody, because the layer was mounted on a subtree the versioned routes are not
        // under. Nothing else in the repository can see that - `sutura_http`'s harness builds its
        // own router, so it proves the layer and not the composition.
        //
        // All three token-gated paths, because the gate is one layer and a hole in it is a hole per
        // path. The probe is deliberately NOT here: it is the one path with no token, asserted by
        // the harness test above.
        let served = start("no-token");
        for reply in [
            served.post(
                &v1(sutura_http::constants::base_paths::QUERY),
                None,
                &recurring_revenue_june(),
            ),
            served.get(&v1(sutura_http::constants::base_paths::CATALOG), None),
            served.get(sutura_http::constants::OPENAPI_JSON_PATH, None),
        ] {
            assert_eq!(reply.status, 401, "{}", reply.body);
            assert_eq!(reply.json()["code"], "unauthorized", "{}", reply.body);
        }
    }

    #[test]
    fn a_configured_deployment_answers_a_certified_question_over_http() {
        // The sentence roadmap #22 was waiting for, and the assertion
        // `crates/sutura-cli/tests/example.rs` already makes over the libraries - made here over
        // HTTP, on the composed binary, through the real router, the real service, the real static
        // broker and the real engine.
        //
        // The number is the example's own: `recurring-revenue-june__rows.snap` pins
        // `[["2026-06-01", "202121"]]`, and `recurring_revenue` declares an anchor, so that figure
        // was re-executed at startup before this process ever listened. Asserted by value rather
        // than snapshotted: it is one row of two cells, and a second copy of the CLI's snapshot
        // would be a file to keep in step rather than a claim.
        let served = start("answer");
        let reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &recurring_revenue_june(),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(body["columns"], serde_json::json!(["period", "recurring_revenue"]));
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "202121"]]));
        // Provenance travels with the answer, and the version is the one this deployment declared -
        // so a bundle served from somewhere else would be visible here rather than inferred.
        assert_eq!(body["provenance"]["definition_version"], VERSION);
        assert!(
            body["provenance"]["definition_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty()),
            "an answer arrived with no definition digest: {}",
            reply.body
        );
        // And which identity produced the one leg. `shared-service-user` is the truth for this
        // deployment: no source a shipped binary serves executes as the asking subject.
        assert_eq!(
            body["executed_as"],
            serde_json::json!([{ "source": "local", "posture": "shared-service-user" }])
        );
    }

    #[test]
    fn a_served_deployment_answers_a_question_spanning_two_sources() {
        // **The sentence `telekom/sutura#112` asks for.** Before `DataFusionWarehouse` declared
        // `Warehouse::EXECUTES_LEGS`, this deployment answered `409 federation_not_executable`:
        // `answer_federated` reads that constant at its first gate, every adapter a release links
        // took the port's default `false`, and the only adapter setting it was a development
        // dependency. So no published artefact could answer a two-source question whatever its
        // sources were, and this is the venue that says otherwise - the real binary, two adapters
        // opened by the real composition root, the real splitter, two real executions and the
        // combiner, over HTTP.
        //
        // **Why the numbers are the assertion and not just a `200`.** The grouped figures are the
        // example's own, pinned by `crates/sutura-cli/tests/snapshots/
        // recurring-revenue-by-region__rows.snap` where ONE data system answers them whole - so
        // asserting them here is a cross-topology claim rather than a recording of whatever came
        // back. And they sum to `202121`, which is the ungrouped June figure
        // `a_configured_deployment_answers_a_certified_question_over_http` above pins and which
        // `recurring_revenue`'s anchor re-executed at startup: a leg translation that dropped a
        // group, double-counted a join or lost the orphan row would still be a `200` and would not
        // reconcile.
        //
        // **The identity limit, next to the claim.** `executed_as` carries TWO legs and both are
        // `shared-service-user`. This is single-player federation: two sources are not two
        // identities. `DataFusionWarehouse::IMPERSONATION` is `NoPlaceForASubject`, and
        // `sutura_domain::source::deliverable_by` refuses `impersonation-at-source` against it in
        // both composition roots - so a `files` source declaring anything else does not start, and a
        // mixed-posture answer is unreachable on any published build. Leg 2 - a source executing AS
        // the asker - is not what this measures.
        //
        // **There is no golden for this path and there cannot be one** - the engine emits no SQL, so
        // `crates/sutura-app/tests/golden/legs.rs`, which pins a rendered leg per dialect, never sees
        // it. **What replaces it is three oracles none of which is the code under test**, and that is
        // the honest statement rather than the absence alone:
        //
        // 1. The anchor `value: 202121` is a literal DECLARED at
        //    `examples/single-player/catalog/metrics/recurring_revenue.md:42`. The six figures below
        //    sum to it, and this deployment re-executed that anchor at startup before it listened.
        // 2. `crates/sutura-cli/tests/snapshots/recurring-revenue-by-region__rows.snap` holds exactly
        //    these six rows, in this order, orphan included - produced by the SINGLE-SOURCE CLI
        //    binary, which has no splitter, no leg and no combiner on its path.
        // 3. `crates/sutura-conformance/src/corpus.rs`'s leg expectations are hand-written literals,
        //    not rows recorded from a run.
        //
        // So the leg path is compared against a declared figure, against a different binary's
        // committed answer, and against hand-written rows. That is a stronger position than a golden,
        // which would only have pinned the statement text this path never produces.
        let served = start_configured("two-sources", &settings_spanning_two_sources("two-sources"));
        let reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &recurring_revenue_by_region(),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(
            body["columns"],
            serde_json::json!(["region", "period", "recurring_revenue"]),
            "{}",
            reply.body
        );
        // Ordered, because the plan claims an order: ascending by the grouping columns with the
        // orphan customer's absent region LAST, which is what the combiner above the two legs has to
        // reproduce and what `telekom/sutura#325`'s F6 got backwards.
        //
        // The absent region is the STRING `"null"` and not JSON's `null`, which is this transport's
        // shape rather than this question's: every cell crosses as text, the same way the measure
        // does. Pinned as it is served, because that is what a client parses.
        assert_eq!(
            body["rows"],
            serde_json::json!([
                ["central", "2026-06-01", "51739"],
                ["east", "2026-06-01", "32598"],
                ["north", "2026-06-01", "42157"],
                ["south", "2026-06-01", "21203"],
                ["west", "2026-06-01", "49425"],
                ["null", "2026-06-01", "4999"],
            ]),
            "{}",
            reply.body
        );
        // The reconciliation, computed rather than restated: these six subgroups add up to the
        // ungrouped certified figure. Written as a sum so a copied row cannot satisfy it.
        let total: i64 = body["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|row| {
                let cell = row[2].as_str()?;
                cell.parse::<i64>().ok()
            })
            .sum();
        assert_eq!(
            total, 202_121,
            "the two-source subgroups do not add up to the certified June figure: {}",
            reply.body
        );
        // **BOTH legs, each under the posture its source was opened with.** One entry would mean the
        // question was answered whole by one adapter and this deployment is not the topology under
        // test. Source order is the record's own - `ExecutedAs` keeps its legs in a `BTreeMap`.
        assert_eq!(
            body["executed_as"],
            serde_json::json!([
                { "source": LOOKUP_SOURCE, "posture": "shared-service-user" },
                { "source": LOCAL_SOURCE, "posture": "shared-service-user" },
            ]),
            "{}",
            reply.body
        );
    }

    #[test]
    fn the_catalog_route_describes_the_bundle_this_deployment_serves() {
        // The second governed operation, over the same composition. It is here rather than folded
        // into the answer test because the two routes are two capabilities, and a route that stopped
        // being mounted is a different failure from one that stopped answering.
        let served = start("catalog");
        let reply = served.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["provenance"]["definition_version"], VERSION);
        let metrics = body["metrics"].as_array().cloned().unwrap_or_default();
        assert!(!metrics.is_empty(), "the served catalog describes no metric: {}", reply.body);
        assert!(
            metrics.iter().any(|metric| metric["name"] == "recurring_revenue"),
            "the metric this suite asks about is not in the served catalog: {}",
            reply.body
        );
    }

    #[test]
    fn a_refusal_reaches_the_caller_as_its_documented_status() {
        // The bug this prevents: a governance refusal arriving as a 500. The exhaustive match in
        // `sutura_http::wire::refusal` decides the status, and its own tests assert the mapping over
        // the enum - what none of them can say is that the composed transport still says it once a
        // real service, a real serializer and a real listener are in the way.
        //
        // Two refusals rather than one, and they are chosen to be different SHAPES: one the compiler
        // refuses because the catalog does not define the metric, one because the question's own
        // bounds are exceeded. Both are questions `examples/single-player/questions/` already holds
        // as `refused-` fixtures.
        let served = start("refusal");
        for (body, status, code) in [
            (
                question(
                    "refused-metric-unknown",
                    "customer_lifetime_value",
                    "2026-06-01",
                    "2026-07-01",
                ),
                404_u16,
                "metric_unknown",
            ),
            (
                question("refused-range-too-long", "recurring_revenue", "0001-01-01", "9999-12-31"),
                422_u16,
                "time_range_too_long",
            ),
        ] {
            let reply = served.post(&v1(sutura_http::constants::base_paths::QUERY), Some(TOKEN), &body);
            assert_eq!(reply.status, status, "{}", reply.body);
            let json = reply.json();
            assert_eq!(json["outcome"], "refusal", "{}", reply.body);
            assert_eq!(json["reason"]["code"], code, "{}", reply.body);
            // The body repeats the status, and a client that logged only the body still has it -
            // so the two have to agree or one of the two readers is being told something else.
            assert_eq!(json["reason"]["status"], status, "{}", reply.body);
            assert!(
                json["reason"]["detail"].as_str().is_some_and(|detail| !detail.is_empty()),
                "a refusal arrived with no sentence: {}",
                reply.body
            );
        }
    }

    #[test]
    fn the_served_document_names_every_governed_route() {
        // The generated document as SERVED. `sutura_http::openapi`'s own tests read `document()`
        // in-process, which proves the generator; this reads the bytes a caller gets from the
        // composed binary, which is what a client generator would consume.
        //
        // Derived from `sutura_http::governed()` rather than from a list written here, so a third
        // capability is covered by this test the day it is added - and a route that is mounted,
        // governed and MISSING from the document fails here rather than at whoever generated a
        // client from it.
        let served = start("document");
        let reply = served.get(sutura_http::constants::OPENAPI_JSON_PATH, Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let document = reply.json();
        for route in sutura_http::governed() {
            let item = &document["paths"][route.route()];
            assert!(
                !item.is_null(),
                "{} is governed and absent from the served document: {}",
                route.route(),
                document["paths"]
            );
            assert!(
                item.as_object().is_some_and(|methods| !methods.is_empty()),
                "{} is in the served document with no operation on it",
                route.route()
            );
        }
        // Liveness is outside the version prefix and still described, which is what an orchestrator
        // reads the document for.
        assert!(
            !document["paths"][sutura_http::constants::HEALTH_PATH].is_null(),
            "the served document does not describe the liveness probe: {}",
            document["paths"]
        );
    }

    // ------------------------------------------------------------------------------- leg 1 ---
    #[test]
    fn the_composed_binary_verifies_a_callers_own_token_and_refuses_every_forgery_alike() {
        // **Leg 1 on the composed binary, which nothing in this repository had.** The gate, the
        // validator, the key-set cache and the negatives all have their standing tests through
        // `sutura_http`'s assembled router, and issue #147 asks for the one thing an in-crate router
        // cannot say: that the REAL composition root read the key set its own settings file named,
        // built the gate, and mounted it over the governed routes. `inbound_gate` in `src/main.rs` had
        // no test at all - neither here nor in `src/tests.rs` - so a root that stopped arming leg 1
        // would have gone out green.
        //
        // The issuer is the same fixture `sutura_http` mints from, pointed at a spawned process
        // instead of at a `oneshot`: a generated key pair, a JWK set published to a real path, and a
        // deployment configured with the issuer's own `iss` and audience.
        let issuer = an_issuer();
        let published = PublishedKeySet::of(&issuer, "serve-leg-one").expect("the key set publishes");
        let served = start_configured(
            "leg-one",
            &settings_declaring_inbound(&example_root(), &issuer, published.path()),
        );
        // Armed at BOOT, before the listener opened - which is the difference between a process that
        // does not start and one that starts and authenticates nobody.
        assert!(
            served.startup.iter().any(|line| line.contains("leg 1 is armed")),
            "the composition root did not report arming leg 1:\n{}",
            served.startup.join("\n")
        );

        let token = issuer
            .mint(&accepted_by("user@example.com"))
            .expect("the issuer signs a token");
        let reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&token),
            &recurring_revenue_june(),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "202121"]]), "{}", reply.body);
        // **The limit, next to the claim.** Leg 1 named the caller; it did not make the source execute
        // as them. This deployment read its files under one identity, and the answer says so - so a
        // green run here is evidence about who was asking and about nothing else.
        assert_eq!(
            body["executed_as"],
            serde_json::json!([{ "source": "local", "posture": "shared-service-user" }])
        );

        // **And the caller the gate verified reached the RECORD, which none of the three assertions
        // above can see.** `executed_as` reports the source's posture, which is `shared-service-user`
        // whether a caller was established or not, and the rows are the example's own number either
        // way - so a root that verifies the token correctly and then answers as the deployment passes
        // everything up to here, and the forgeries below still `401`. This is leg 1's *names who is
        // asking* half, and it is asserted on what the deployment wrote down about the answer.
        //
        // The subject is asserted as well as the mode, because `verified` alone would hold for a
        // deployment that established somebody and recorded the wrong person.
        let record = served.awaiting(RECORD);
        let answered = record
            .iter()
            .rev()
            .find(|line| line.contains(RECORD))
            .expect("`awaiting` returns only once a line carries it");
        assert!(
            answered.contains(r#""subject_established":"verified""#),
            "the record for an answered question does not say a caller was verified:\n{answered}"
        );
        assert!(
            answered.contains(r#""subject":"user@example.com""#),
            "the record names a different subject from the one the token carried:\n{answered}"
        );

        // **The gate is on the governed routes and not on the merged ones**, asserted on this
        // deployment rather than on the token one. `sutura_http::router` layers leg 1 onto the
        // versioned router and merges liveness beside it, so a change that layered the merged router
        // instead would answer `401` to every orchestrator's probe - and the two existing probe tests
        // could not see it, because both start the deployment-token settings and never build this
        // middleware at all.
        assert_eq!(
            served.get(sutura_http::constants::HEALTH_PATH, None).status,
            200,
            "a deployment declaring inbound identity refuses its own liveness probe"
        );

        // Each of these is the accepted token with exactly one thing moved, so a check that stopped
        // running on this path fails by name. The second assertion is the one that matters at a
        // transport: the refusals are INDISTINGUISHABLE, because a caller who could tell "your
        // signature is wrong" from "your audience is wrong" has been told which half of a forgery to
        // fix.
        let good = accepted_by("user@example.com");
        let mut challenges: Vec<String> = Vec::new();
        for (case, minted) in [
            ("no token at all", Ok(String::new())),
            (
                "a wrong audience",
                issuer.mint(&good.clone().for_audience("https://someone-else.example.com")),
            ),
            (
                "a wrong issuer",
                issuer.mint(&good.clone().claiming_issuer("https://forger.example.com")),
            ),
            ("an expired token", issuer.mint(&good.clone().expired_since(60))),
            // The regression `docs/adr/0014` records by name: an OpenID Connect ID token from the same
            // issuer, for the same audience, correctly signed - and not an access token. It verified
            // once, and this is the first time the class check is asserted on a composed binary.
            (
                "an ID token where an access token is required",
                issuer.mint(&good.clone().classed(Token::ID_TOKEN)),
            ),
            // The oldest JWT defect there is, and a forgery that is right in every other respect.
            ("an unsigned token", issuer.mint_unsigned(&good)),
            ("a forged signature", issuer.mint_signed_by_a_stranger(&good)),
        ] {
            let token = minted.expect("the issuer mints every negative it is asked for");
            let presented = if token.is_empty() { None } else { Some(token.as_str()) };
            let reply = served.post(
                &v1(sutura_http::constants::base_paths::QUERY),
                presented,
                &recurring_revenue_june(),
            );
            assert_eq!(reply.status, 401, "{case} established a caller: {}", reply.body);
            assert_eq!(reply.json()["code"], "unauthorized", "{}", reply.body);
            challenges.push(reply.challenge.unwrap_or_default());
        }
        let distinct: std::collections::BTreeSet<&String> = challenges.iter().collect();
        assert_eq!(distinct.len(), 1, "the refusals are distinguishable: {distinct:?}");
        let challenge = challenges.first().expect("every case above pushed one");
        // It names this deployment's own resource identifier - the value the settings file declared -
        // and says nothing about which check failed.
        assert!(challenge.contains(RESOURCE), "{challenge}");
        assert!(!challenge.contains("error_description"), "{challenge}");
    }

    #[test]
    fn a_published_key_set_this_deployment_cannot_use_stops_the_process() {
        // The other half of arming leg 1, and it is only observable on a binary: `inbound_gate` reads
        // the key set with a `?`, so an unusable one has to be a process that DOES NOT START rather
        // than one that starts, logs that it establishes a caller identity, and answers `401` to
        // everybody with nothing in the log connecting the two.
        //
        // A symmetric key, because accepting one is how algorithm confusion works - the holder of a
        // *published* key could sign with it - and because the refusal has to happen while the gate is
        // being BUILT. `sutura_http`'s router test asserts the same document starts no gate; what it
        // cannot assert is what the process then does about it.
        let issuer = an_issuer();
        let published = PublishedKeySet::of(&issuer, "serve-unusable").expect("the key set publishes");
        published
            .rotate_to(&MockIssuer::key_set_of_symmetric_keys())
            .expect("the unusable set is published");
        let said = refused_to_start(
            Environment::Development,
            "unusable-keys",
            &settings_declaring_inbound(&example_root(), &issuer, published.path()),
        );
        let told = said.join("\n");
        // **Asserted on the refusal's own sentence, and the reason is that nothing else separates the
        // two ways this deployment can fail to start.** A root that read the key set fine and then
        // forgot to attach the gate also exits non-zero, also never logs `leg 1 is armed` and also
        // never listens - `sutura_http::router` refuses to assemble, which is the guard that exists so
        // this cannot be forgotten. Only what it SAYS tells an operator which of the two happened.
        //
        // **The limit of that, said plainly rather than dressed up:** `docs/serving.md` documents the
        // BEHAVIOUR - an unusable key set refuses rather than skips - and not the wording. The string
        // matched below is `sutura_http`'s own `Display`, so this assertion is held by recall across a
        // crate boundary and a reword there turns this test red with nothing pinning the pair. That is
        // tolerable for a startup refusal an operator reads once, and it is not a mechanism.
        //
        // The key set's PATH is deliberately not what is matched: the startup banner echoes the whole
        // resolved configuration, so a path assertion here passes for every refusal this deployment
        // can produce - measured, on a build that had been changed to swallow this very failure.
        // `contains("symmetric")` is a second, independent sentence - it comes from the key set
        // loader rather than from the wrapper above it - so it is not entailed by the first.
        assert!(
            told.contains("the inbound identity declared by this deployment is not usable"),
            "the deployment did not refuse the declaration it cannot serve:\n{told}"
        );
        assert!(
            told.contains("symmetric"),
            "the refusal did not say what was wrong with the key set:\n{told}"
        );
        assert!(
            !told.contains("leg 1 is armed"),
            "the deployment armed leg 1 over a key set it cannot use:\n{told}"
        );
        assert!(
            !told.contains("\"msg\":\"listening\""),
            "the deployment opened a listener on a key set it cannot use:\n{told}"
        );
    }

    // ------------------------------------------------------- the four startup refusals ---
    //
    // `github.com/telekom/sutura#302`. Each of the four is already asserted over `Settings::load` in
    // `crates/sutura-config/src/settings/tests.rs`, and that is a claim about the settings TYPE. What
    // an operator gets is a PROCESS, and *`load` returned `Err`* and *the binary exited without
    // binding a port* are two different statements - a service that starts and answers `401` to
    // everything is a different outcome from one that refuses to exist. Only the first had a test.
    //
    // The three assertions each case makes, and why each is separate:
    //
    // 1. **Exit `1`**, in `harness::refused_to_start`, and exactly 1 rather than non-zero - see its
    //    own note on why an abort is not a refusal.
    // 2. **Nothing bound**, in `stopped_before_binding`.
    // 3. **The operator's own sentence**, DERIVED from `sutura_config` rather than quoted here - see
    //    `refusal_of`. Nothing in this repository compared anything to the `not fit to serve`
    //    wording before, so the header an operator greps for was reachable by no test at all.

    /// Starts the composed binary on a deployment it must refuse, and takes what it said.
    ///
    /// **The "never binds" half, and it is the security-relevant one.** `sutura_http::server::serve`
    /// reads the port back off the socket and logs it, which is the same event the whole harness
    /// waits for - so its ABSENCE is this suite's own evidence that no listener was opened, on the
    /// same channel every other case trusts.
    ///
    /// **The limit, stated rather than implied:** this reads the process's own log. A root that bound
    /// a socket and did not report it would be invisible here, and what makes that narrow rather than
    /// hollow is that the event is emitted by the bind itself and nothing else in this file could
    /// pass without it.
    fn stopped_before_binding(environment: Environment, case: &str, settings: &str) -> Vec<String> {
        let told = refused_to_start(environment, case, settings);
        assert!(
            !told.iter().any(|line| line.contains(r#""msg":"listening""#)),
            "a deployment that must not start opened a listener:\n{}",
            told.join("\n")
        );
        told
    }

    /// What `sutura_config` refuses this same deployment for, asked in the process running the test.
    ///
    /// **The wording is DERIVED and the refusal is NAMED, and the split is the point.** Quoting the
    /// sentence here would put a copy of another crate's `Display` in this file - which is exactly
    /// the weakness `a_published_key_set_this_deployment_cannot_use_stops_the_process` states about
    /// its own two `contains`. Deriving it alone would be worse: a change to `Settings::refusals`
    /// moves both sides at once and the comparison stays green over a deployment that no longer
    /// refuses. So each case does both - it asserts the refusal SET by value, which is a claim about
    /// what this configuration means, and then asserts the binary printed the sentence that set
    /// renders to, which is a claim about what the process does with it.
    ///
    /// `with_overlay` is documented as "the same position a deployment's own file occupies", and
    /// `defaults` keeps an EMPTY variable map - so this reads the same layers as the child, which
    /// `harness::command` strips of every `SUTURA*` variable for the same reason.
    ///
    /// **Where the two do differ:** the child reads `base.yaml` off a directory and this reads a
    /// string, so the `config` crate's own origin naming can differ inside a deserialization error.
    /// That is why the misspelled-key case below compares the outer sentence and the key, and not
    /// the cause's text.
    fn refusal_of(environment: Environment, settings: &str) -> SettingsError {
        Settings::load(&Sources::defaults(environment).with_overlay(settings))
            .expect_err("this case's fixture is a deployment the settings type refuses")
    }

    /// Asserts the process wrote every line of `message`.
    ///
    /// **Line by line rather than as one string, and the channel is the reason.** `harness::forward`
    /// feeds standard output and standard error into ONE channel from two threads, so a banner line
    /// the process wrote before the refusal can be scheduled between two lines of it. Each stream is
    /// ordered against itself and the two are not ordered against each other, so a `contains` over
    /// the joined log would be a test that passes on a scheduling accident.
    ///
    /// `contains` per line rather than equality, because the first line of a refusal reaches
    /// standard error behind `main`'s own `sutura-serve: ` prefix.
    fn said_every_line_of(told: &[String], message: &str) {
        for expected in message.lines() {
            assert!(
                told.iter().any(|line| line.contains(expected)),
                "the deployment did not tell the operator `{expected}`:\n{}",
                told.join("\n")
            );
        }
    }

    #[test]
    fn a_non_loopback_bind_with_no_declared_terminator_stops_the_process() {
        // The single change that turns a local tool into a network service, refused on the binary.
        // The deployment is otherwise the one every serving case in this file starts: the same
        // catalog, the same source, the same deployment token - so the wildcard bind is the only
        // thing that can account for the refusal.
        let settings = deployment(
            &example_root(),
            "  host: \"0.0.0.0\"\n  port: 0\n",
            &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
        );
        let refused = refusal_of(Environment::Development, &settings);
        let SettingsError::NotFitToServe { ref refusals } = refused else {
            panic!("this fixture is meant to be a posture refusal and is {refused:?}");
        };
        // The WHOLE set, not a `contains`: a second refusal here would mean the process could be
        // stopping for something other than the undeclared terminator.
        assert_eq!(
            *refusals,
            vec![NotFitToServe::TlsTerminationUndeclared {
                bind: String::from("0.0.0.0:0")
            }]
        );
        let told = stopped_before_binding(Environment::Development, "undeclared-bind", &settings);
        said_every_line_of(&told, &refused.to_string());
    }

    #[test]
    fn a_production_deployment_with_no_credential_stops_the_process() {
        // The only case here that is about the ENVIRONMENT rather than about a key: this same file
        // starts on a laptop and refuses in production, which is why `harness::command` takes an
        // `Environment` at all.
        //
        // The port is the embedded default rather than `0`, and that is load-bearing: `server.port: 0`
        // in production is `EphemeralPortInProduction`, so a fixture that kept the harness's
        // kernel-chosen port would refuse for two reasons and this case could not say which one
        // stopped the process. Safe because the assertion below is that nothing was ever bound.
        let settings = deployment(&example_root(), "  host: \"127.0.0.1\"\n", SINGLE_USER);
        let refused = refusal_of(Environment::Production, &settings);
        let SettingsError::NotFitToServe { ref refusals } = refused else {
            panic!("this fixture is meant to be a posture refusal and is {refused:?}");
        };
        assert_eq!(
            *refusals,
            vec![NotFitToServe::AccessTokenRequired {
                because: "this is a production deployment with no inbound identity configured"
            }]
        );
        let told = stopped_before_binding(Environment::Production, "production-no-token", &settings);
        said_every_line_of(&told, &refused.to_string());
    }

    #[test]
    fn a_configured_source_with_no_declared_mode_stops_the_process() {
        // `security.identity` has no default and no derivation, and this is the refusal on the
        // binary that makes that true of the artefact rather than of the type. The fixture leaves
        // out exactly `SINGLE_USER` and keeps everything else, including the deployment token - so
        // the missing mode is the whole difference from a deployment that serves.
        let settings = deployment(&example_root(), LOOPBACK, &format!("  access_token: \"{TOKEN}\"\n"));
        let refused = refusal_of(Environment::Development, &settings);
        let SettingsError::NotFitToServe { ref refusals } = refused else {
            panic!("this fixture is meant to be a posture refusal and is {refused:?}");
        };
        // **This line restates `crates/sutura-config/src/settings/tests.rs`'s own assertion, and it is
        // here anyway.** Its job in this file is not coverage of the refusal - that test has it - but
        // to stop `refusal_of` and the binary moving TOGETHER: a `refusals()` that answered a
        // different refusal for this deployment would change the expected sentence and the printed
        // one alike, and the case would stay green over a process refused for the wrong reason.
        // Measured on the neighbouring case: with `refusals()` pushing `InProcessTlsWithoutMaterial`
        // for the non-loopback bind and its equivalent line deleted, `just serve-e2e` is `40 passed`
        // at exit 0.
        assert_eq!(*refusals, vec![NotFitToServe::DeploymentIdentityUndeclared { count: 1 }]);
        let told = stopped_before_binding(Environment::Development, "undeclared-mode", &settings);
        said_every_line_of(&told, &refused.to_string());
    }

    #[test]
    fn a_misspelled_key_stops_the_process_and_the_operator_is_told_which_key() {
        // The fourth refusal, and the only one that is not a posture: `deny_unknown_fields` makes an
        // unknown key a deserialization error, so this exercises the OTHER arm of the startup path -
        // an error with a `#[source]` chain under it.
        //
        // Which makes it the one case that holds `main::flatten`, and nothing did. The outer sentence
        // says only "the configuration sources could not be read"; the key an operator has to go and
        // fix is in the cause, and a root that printed `error.to_string()` alone would leave them
        // reading a deployment's whole settings tree looking for a typo. The three assertions are
        // therefore the outer sentence, the chain marker, and the key itself.
        let settings = deployment(
            &example_root(),
            &format!("{LOOPBACK}  prot: 9000\n"),
            &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
        );
        let refused = refusal_of(Environment::Development, &settings);
        assert!(
            matches!(refused, SettingsError::Source { .. }),
            "a misspelled key is meant to be a source error and is {refused:?}"
        );
        let told = stopped_before_binding(Environment::Development, "misspelled-key", &settings);
        said_every_line_of(&told, &refused.to_string());
        // **One line has to carry both, and that is a correction rather than a tightening for its own
        // sake.** Written as two independent `any`, `contains("prot")` was satisfied by the key
        // appearing ANYWHERE the process wrote - and `flatten` is exactly the code that decides
        // whether the cause reaches standard error at all, so a chain dropped while the key survived
        // elsewhere would have passed both. Anchoring the key to the line carrying `caused by:` makes
        // it one claim: the cause was walked, and the walked cause names the key.
        //
        // `caused by:` is `main::flatten`'s own wording, in this crate rather than across a
        // boundary, so this pins the walk in the file that performs it.
        assert!(
            told.iter().any(|line| line.contains("caused by:") && line.contains("prot")),
            "the operator was not shown a cause naming the key they misspelled:\n{}",
            told.join("\n")
        );
    }
}
