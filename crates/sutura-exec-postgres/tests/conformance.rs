//! This adapter, held to the conformance packs, from its OWN crate.
//!
//! **The third binding, and the first one whose data system might not be there.** `DuckDB` and the
//! engine stand up in-process; this one needs a server listening, and that is exactly why it was
//! registered in the golden matrix and carried
//! `cargo xtask check-conformance-bindings`' one declared exemption until now
//! (`telekom/sutura#348`). The harness had no way to say *no tier is up here* that was not a pass,
//! so a binding written before it would have panicked in its fixture on any machine with no server.
//!
//! # Where this actually runs, and what it does when the tier is absent
//!
//! **Every venue that runs the suite provisions the tier**, so these cells RUN rather than skip:
//! `just test`, `just gates`, `just causality` and `nix/run-gate.sh tests` (which both hooks reach)
//! all source `nix/with-tier.sh`, and the nix sandbox's `checks.nextest` provisions the same script
//! through `preCheck`. That script exports `SUTURA_DEV_REQUIRE_TIER` **only where a tier is
//! actually up**, so "the server is there" and "these cells are required" cannot come apart.
//!
//! What happens where it is not up is therefore not this file's decision, and deliberately so:
//! `sutura_dev::provisioned::here` reads that requirement once for every harness in this repository
//! and **panics** in the required direction - so a venue that provisioned a tier and then lost it
//! FAILS here rather than skipping. Where nothing required one (a host without the tier binary),
//! it writes its `SKIPPED` notice to stderr naming the worktree, the file it read and the task that
//! starts the tier, and this fixture answers [`Fixture::Absent`]: every cell prints `NOT RUN` with
//! that diagnostic, and the census prints no coverage line at all.
//!
//! **The limit, next to the claim:** the harness cannot tell an absence this fixture DISCOVERED
//! from one it merely declared - `xtask/src/boundaries/harness.rs` holds `sutura-conformance` to
//! `sutura-domain` alone, and its own remedy assigns reaching a provisioned tier to an adapter's
//! fixture. What that trust is worth is the paragraph above: in every venue that counts, the
//! provisioner has already failed the run before an absence could be returned.
//!
//! # The fixture path is shared wider than this crate, and `telekom/sutura#405` is that
//!
//! This is the first fixture that loads `corpus::on_disk()` into a **server**, and it reads that
//! file seven times per binding - once per behaviour plus the census. The path is
//! `<temp_dir>/sutura-conformance/<table>.csv`, which carries no worktree and no digest, so
//! another checkout of this repository running `just test` is a second writer of it. If one lands
//! between this fixture's `on_disk()` and Postgres reading it, the cell fails as a content fault
//! naming the case and this adapter while the run that caused it is green. **Not fixed here** -
//! `crates/sutura-conformance/src/corpus.rs` is a fixture every binding shares and this file is
//! about one adapter - and #405 is where the per-worktree path lands.
//!
//! # What this file does not establish
//!
//! The three things the `DuckDB` binding's header lists - nothing about impersonation (this adapter
//! declares it cannot carry a per-subject credential), nothing about the rendered SQL beyond the
//! endpoint accepting it, and nothing about the three hard cases `docs/adr/0012` names - plus one
//! of its own: the corpus here is a single table, while the golden matrix's postgres cells read the
//! four example tables and compare against the engine. This is the port's contract; that is the
//! differential.

// One `#[cfg(test)]` module holding the fixture and the binding, which is this workspace's shape for
// an integration test target: the strict lints exempt test code, and a fixture at file scope is not
// test code as far as clippy is concerned.
#[cfg(test)]
mod conformance {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sutura_conformance::{Fixture, Missing, corpus};
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_exec_postgres::PostgresWarehouse;
    use sutura_exec_postgres::fixture::FixtureCredential;

    /// The service the provisioner is asked for.
    ///
    /// The same name `nix/postgres-tier.nix` publishes and `just postgres-tier start` brings up, so
    /// the remedy a skip prints is derived from the venue that answers for it rather than written
    /// here.
    const SERVICE: &str = "postgres";

    /// The tier's Postgres with the conformance corpus loaded into a private schema.
    ///
    /// Called once per emitted test, so no state crosses between them - and the isolation is per
    /// CELL rather than per binding: each open creates its own schema, so cells running in parallel
    /// against one server cannot clobber one another's table. That is the same arrangement the
    /// golden matrix's postgres cells use, for the same reason.
    fn open() -> Fixture<PostgresWarehouse> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            // The notice has already reached stderr, and the required direction has already
            // panicked inside `here`. So this arm is only ever the developer-machine one.
            Provisioned::Skipped(absent) => return Fixture::Absent(Missing::tier(SERVICE, &absent)),
        };
        // The tier publishes the credential beside the endpoint, and this fixture defaults
        // nothing: an endpoint with no credential is a half-run provisioner, and the refusal names
        // the variable. See `sutura_exec_postgres::fixture`.
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let config = PostgresWarehouse::local_config(endpoint.host(), endpoint.port(), &credential);
        let schema = format!("conformance_{}_{}", std::process::id(), schema_counter());
        let warehouse = PostgresWarehouse::connect_in_schema(corpus::source(), corpus::posture(), &config, &schema)
            .unwrap_or_else(|e| panic!("postgres did not open at {endpoint}: {e}"));
        warehouse
            .load_csv(&corpus::table(), &corpus::on_disk())
            .unwrap_or_else(|e| panic!("postgres could not load the conformance corpus: {e}"));
        Fixture::standing(warehouse)
    }

    /// A per-process counter, so each `open` in one test process gets a distinct schema name.
    ///
    /// Kept even though nextest gives each test its own process: `cargo test` does not, and a
    /// fixture whose isolation depends on which runner invoked it is not isolated.
    fn schema_counter() -> usize {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    // `refuses_legs`, because this adapter leaves `EXECUTES_LEGS` at its default: it renders a whole
    // plan and pushes it down, and a leg arriving here needs a combiner above it that nothing builds
    // yet - `PostgresError::LegWithoutCombiner` is that guard, and this pack is the only thing that
    // exercises it. The tag and the constant are torn apart by a `const` assertion inside the
    // expansion, so tagging it the other way does not build.
    //
    // The emitted names are `conformance::postgres::<behaviour>`, which is what makes this adapter's
    // tier selectable on its own.
    sutura_conformance::execute_packs! {
        adapter: postgres,
        warehouse: sutura_exec_postgres::PostgresWarehouse,
        open: crate::conformance::open,
        refuses_legs,
    }
}
