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
//! **It proves the VENUE and not the reader.** What it asserts is that a real `DataHub` GMS at the
//! pinned version is reachable on the port this worktree's provisioning allocated, that it reports
//! itself healthy, and that its versioned `OpenAPI` v3 entity surface - the surface an
//! `AspectReader` over HTTP will call - is served rather than `404`. That is the measurement which
//! turns "needs a provisioned instance" from an obstacle into a fact, and it is deliberately ALL it
//! claims:
//!
//!   * **No aspect is read and none is decoded here.** There is no HTTP `AspectReader` yet - the
//!     only implementor is the recorded fixture - so there is nothing to point at this endpoint.
//!   * **Nothing is ingested.** The instance is empty, so this cannot and does not say that a
//!     `sutura` structured property survives a round trip through the platform.
//!   * **Nothing is authenticated.** The tier runs with `METADATA_SERVICE_AUTH_ENABLED: "false"`,
//!     as upstream's quickstart does, so the bearer half of the read path is untouched.
//!
//! Reading it as evidence of a working read path would be exactly the overstatement `AGENTS.md`
//! calls the defect itself. What it removes is the excuse: the next change writes a reader, and the
//! venue it is measured against already exists and is already gated.
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

    #[test]
    #[ignore = "needs `just dev-up-datahub`; `just test` cannot see a docker service because the \
                postgres tier rewrites the discovery file - run `just datahub-acceptance`"]
    fn the_provisioned_datahub_serves_the_surface_a_reader_would_call() {
        let inside = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let provisioned = sutura_dev::provisioned::here(inside, "datahub");
        let Some(endpoint) = provisioned.endpoint() else {
            // The notice is already on stderr, and in the required direction `here` panicked rather
            // than reaching this line.
            return;
        };

        // Built the way `sutura_exec_bigquery::wire::WireAgent::pinned` builds one, minus
        // `https_only`: this is loopback plaintext by construction, because the tier publishes an
        // ephemeral HTTP port.
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(ANSWER_TIMEOUT))
                // A metadata platform on loopback has no reason to send this test anywhere else,
                // and a redirect is how a probe silently starts measuring a different server.
                .max_redirects(0)
                .build(),
        );

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
}
