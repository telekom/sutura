//! The wiring, against services that are actually running.
//!
//! Everything in `dev/src` can be tested over a discovery file somebody wrote by hand, and that is
//! where the shape of the mechanism is pinned. What a hand-written file cannot prove is the thing
//! the tier exists for: that a harness reaches **this worktree's** container, on the port docker
//! allocated for it, and that a test which had hardcoded the obvious constant would have failed.
//!
//! So this file asks docker. It is the one place in the workspace that does, and it declines rather
//! than fails where there is nothing to ask - see `sutura_dev::requirement` for which direction that
//! points on which machine class, and why the skip is written to stderr instead of being silent.
//!
//! **Bring the tier up with `just dev-up` before expecting these to assert anything.**

// `cfg(test)` because clippy only honours `allow-expect-in-tests` for code inside a `#[cfg(test)]`
// item, and an integration test target is compiled with `--test` so it is true here. Without it
// every `expect` below is a lint error.
#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{TcpStream, ToSocketAddrs as _};
    use std::path::Path;
    use std::time::Duration;

    use sutura_dev::discovery::Endpoint;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_dev::scope::{SERVICES, Scope, Service};

    /// How long a connection to something already reported healthy may take before it is a failure.
    ///
    /// Short on purpose. Provisioning has already gated on the health report, so a service that
    /// cannot be reached in a second is not slow - it is the wrong port, which is the case these
    /// tests exist to catch. A generous timeout here would turn that answer into a wait.
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

    /// The directory this test crate lives in - the one thing a test knows about where it is.
    fn inside() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    /// The declaration for one service, so a test names a service and never a port.
    fn declared(name: &str) -> &'static Service {
        SERVICES
            .iter()
            .find(|service| service.name() == name)
            .expect("the tier declares this service")
    }

    /// Ask for a service, or say what did not run and stop.
    ///
    /// Returns `None` on the skip path, where the notice has already reached stderr.
    fn provisioned(name: &str) -> Option<Endpoint> {
        match provisioned::here(inside(), name) {
            Provisioned::At(endpoint) => Some(endpoint),
            Provisioned::Skipped(_) => None,
        }
    }

    /// Whether `service` was provisioned by DOCKER here, and if not, why the skip.
    ///
    /// These tests validate the DOCKER provisioner - an ephemeral port, a worktree-derived
    /// project, a live socket. A nix-native tier (`nix/postgres-tier.nix`) merges into the same
    /// discovery file, so the question is asked **per entry**: one document-level answer stopped
    /// meaning anything the moment two provisioners contributed to one file, and where it said
    /// `nix` these cells skipped over a docker service that was up and published beside it
    /// (`github.com/telekom/sutura#317`). Read here (not via `provisioned::here`) because `here`
    /// honours `SUTURA_DEV_REQUIRE_TIER`, which the nix tier sets - asking for a service it did
    /// not provision would then fail rather than skip.
    ///
    /// The skip is written to stderr and names the tier, because this file's contract is that a
    /// green run nobody can see is a green run that tested nothing.
    fn docker_provisioned(service: &str) -> bool {
        let provisioner = sutura_dev::discovery::Endpoints::discover(&provisioned_scope())
            .ok()
            .and_then(|endpoints| endpoints.endpoint(service).ok().map(Endpoint::provisioner));
        if provisioner != Some(sutura_dev::discovery::Provisioner::Docker) {
            eprintln!(
                "SKIPPED: `{service}` is not docker-provisioned here ({}), so the docker wiring cells do not apply",
                provisioner.map_or_else(|| String::from("absent"), |found| found.to_string())
            );
            return false;
        }
        true
    }

    /// This worktree's scope, for reading the discovery file without going through `here`.
    fn provisioned_scope() -> Scope {
        let root = provisioned::worktree_root(inside()).expect("this test crate is inside a checkout");
        Scope::from_root(&root).expect("the worktree root resolves")
    }

    /// The claim, over every service a default provision starts.
    ///
    /// **This is the assertion the whole per-worktree design is for.** A test that had written the
    /// container's own port - 5432, 8123 - would pass on a machine with one instance listening
    /// there and fail against a worktree's ephemeral one, which is the bug class this tier removes.
    /// So the assertion is not "a port was returned": it is that the returned port is NOT the
    /// constant, and that something answers on it.
    #[test]
    fn a_harness_reaches_the_port_docker_allocated_and_not_the_one_in_the_declaration() {
        for service in SERVICES.iter().filter(|service| service.is_default()) {
            if !docker_provisioned(service.name()) {
                continue;
            }
            let Some(endpoint) = provisioned(service.name()) else {
                continue;
            };
            assert_ne!(
                endpoint.port(),
                service.container_port(),
                "{} came back on its container port, so this test could not tell discovery from a constant",
                service.name()
            );
            let stream = connect(&endpoint);
            assert!(
                stream.is_ok(),
                "{} was reported provisioned at {endpoint} and nothing is listening there: {:?}",
                service.name(),
                stream.err()
            );
        }
    }

    /// The endpoint belongs to THIS worktree's project, not to a neighbour's.
    ///
    /// Two worktrees of this repository run the tier at the same time as a matter of course, and the
    /// failure the design exists to prevent is a test that talked to the other one. The compose
    /// project name is derived from the worktree path, so comparing the discovery file's project
    /// against a scope derived here is what says which instance answered.
    #[test]
    fn the_endpoint_that_answered_belongs_to_this_worktree() {
        if !docker_provisioned("clickhouse") {
            return;
        }
        let scope = provisioned_scope();

        let Some(_endpoint) = provisioned("clickhouse") else {
            return;
        };
        let endpoints = sutura_dev::discovery::Endpoints::discover(&scope).expect("something answered, so the file is readable");
        assert_eq!(
            endpoints.project(),
            scope.project(),
            "the discovery file this harness read was written for another worktree"
        );
    }

    /// `ClickHouse` answers its own liveness path on the discovered port.
    ///
    /// A TCP connect proves a socket is bound; this proves the socket belongs to the service the
    /// discovery file named it as. `/ping` is `ClickHouse`'s own documented probe and it is what
    /// `compose.services.yaml` gates readiness on, so the two halves of the tier agree about what
    /// "up" means.
    ///
    /// Written with a `TcpStream` and no HTTP client: this crate's dependencies are `serde_json` and
    /// `sha2`, and adding a client to a dev harness for one GET is a supply-chain surface for a
    /// nine-line function.
    #[test]
    fn clickhouse_answers_on_the_discovered_port_and_not_on_its_container_port() {
        if !docker_provisioned("clickhouse") {
            return;
        }
        let Some(endpoint) = provisioned("clickhouse") else {
            return;
        };
        let body = get(&endpoint, "/ping").expect("clickhouse was reported healthy");
        assert!(body.contains("Ok."), "ClickHouse did not answer /ping: {body:?}");

        // And the negative half, which is what makes the positive one mean something: the constant
        // a test might have hardcoded is not bound at all on this host.
        let constant = declared("clickhouse").container_port();
        assert_ne!(
            endpoint.port(),
            constant,
            "the fixture cannot prove anything if they are equal"
        );
    }

    /// Open a connection to a discovered endpoint.
    fn connect(endpoint: &Endpoint) -> std::io::Result<TcpStream> {
        let address = format!("{}:{}", endpoint.host(), endpoint.port());
        let resolved = address
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::other(format!("{address} resolved to nothing")))?;
        let stream = TcpStream::connect_timeout(&resolved, CONNECT_TIMEOUT)?;
        stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
        stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;
        Ok(stream)
    }

    /// One HTTP/1.0 GET, by hand, returning whatever came back.
    fn get(endpoint: &Endpoint, path: &str) -> std::io::Result<String> {
        let mut stream = connect(endpoint)?;
        let request = format!("GET {path} HTTP/1.0\r\nHost: {}\r\n\r\n", endpoint.host());
        stream.write_all(request.as_bytes())?;
        stream.flush()?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        Ok(String::from_utf8_lossy(&response).into_owned())
    }

    #[test]
    fn the_skip_notice_names_the_profile_the_missing_service_declares() {
        // A DECLARATION test with no docker in it, reached through the library door: a harness told
        // that its service was not provisioned is pointed at the profile THAT SERVICE declares, so
        // the advice is one task rather than a list a reader has to choose from.
        //
        // Two wrong shapes came first, in order. `just dev-up-identity` unconditionally, which was
        // correct while `identity` was the only profile and became advice that starts the wrong
        // stack the day a second one arrived - a reader whose missing service was `datahub` was
        // being told to bring up Keycloak. Then a derived list of every profile, which is never
        // wrong and never says which one to type.
        //
        // This file used to be the only place the assertion could live, because the causality gate
        // reverts a changed file that added no test and `dev/src/provisioned.rs` held none. It holds
        // its own now - `every_command_this_remedy_names_is_a_just_task_that_exists` and
        // `a_service_whose_venue_is_a_nix_tier_is_not_told_to_enable_a_compose_profile` are beside
        // the code - so what is left here is the same claim through the door a harness uses.
        let profiled = SERVICES
            .iter()
            .find(|service| service.profile().is_some())
            .expect("the tier declares a profiled service");

        // The case has to be a worktree that HAS a discovery file and does not have this service:
        // that is the branch carrying the profile advice. Nothing provisioned at all is a different
        // diagnostic - measured, by writing this test the short way first and watching it fail on
        // the wrong branch. No docker in either, so this runs everywhere and this file's skip does
        // not reach it.
        let root = std::env::temp_dir().join(format!("sutura-advice-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp dirs are creatable");
        let scope = Scope::from_root(&root).expect("the directory exists");
        let started = SERVICES
            .iter()
            .find(|service| service.is_default())
            .expect("the tier declares a default service");
        sutura_dev::discovery::publish(&scope, &[(started.name(), String::from("127.0.0.1:47214"))])
            .expect("the discovery file is writable");

        let problem = provisioned::in_worktree(&root, profiled.name()).expect_err("it was not started");
        let message = problem.to_string();
        std::fs::remove_dir_all(&root).expect("cleanup");

        let profile = profiled.profile().expect("that is what it was found by");
        assert!(
            message.contains(&format!("`just dev-up-{profile}`")),
            "the notice names no task for `{profile}`, the profile the missing service declares: {message}"
        );
        for other in sutura_dev::scope::profiles() {
            assert!(
                other == profile || !message.contains(&format!("dev-up-{other}")),
                "`{other}` is another service's profile and the notice sends a reader to it: {message}"
            );
        }
    }
}
