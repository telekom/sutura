//! This adapter, held to the conformance packs, from its OWN crate.
//!
//! **The whole file is a fixture and a declaration.** Every assertion lives in
//! `sutura-conformance`, written once against the execution port; what is here is how to open a
//! `DuckDB` over the pack's corpus and which leg declaration this adapter makes. That is the
//! property `docs/adr/0012` asks for: adding a data system is a registration rather than a test
//! edit, and an adapter in its own crate can prove itself without a suite of its own.
//!
//! Before this, an adapter that wanted the same coverage hand-wrote it -
//! `crates/sutura-exec-bigquery/tests/corpus.rs` is that, and it is the assertion-in-two-places
//! failure the record opens with.
//!
//! **What this file does not establish**, because the pack's own headers say so and this is the
//! place a reader arrives first: nothing about impersonation (this adapter declares it cannot carry
//! a per-subject credential, and `docs/adr/0012` decides that absence gets no pack), nothing about
//! the rendered SQL - the goldens in `sutura-app` pin that per dialect - and nothing about the three
//! hard cases the record names, none of which is in the pack's corpus yet.

// One `#[cfg(test)]` module holding the fixture and the binding, which is this workspace's shape for
// an integration test target: the strict lints exempt test code, and a fixture at file scope is not
// test code as far as clippy is concerned.
#[cfg(test)]
mod conformance {
    use sutura_conformance::corpus;
    use sutura_exec_duckdb::DuckDbWarehouse;

    /// An in-memory `DuckDB` with the conformance corpus attached.
    ///
    /// Called once per emitted test, so no state crosses between them: an in-memory database is
    /// opened, the corpus is attached as a view over its CSV, and it is dropped with the test. A
    /// corpus read every run cannot drift from the bytes the pack states, which is the same argument
    /// the golden suite's fixture makes for the example corpus.
    fn open() -> DuckDbWarehouse {
        let warehouse = DuckDbWarehouse::in_memory(corpus::source(), corpus::posture()).expect("an in-memory database opens");
        warehouse
            .attach_csv(&corpus::table(), &corpus::on_disk())
            .expect("duckdb attaches the conformance corpus");
        warehouse
    }

    // `executes_legs`, because this adapter declares `EXECUTES_LEGS`. The tag and the constant are
    // torn apart by a `const` assertion inside the expansion, so tagging it the other way does not
    // build - and the leg behaviour arrives under the name that says which direction ran.
    //
    // The emitted names are `conformance::duckdb::<behaviour>`, which is what makes one adapter's
    // tier selectable on its own.
    sutura_conformance::execute_packs! {
        adapter: duckdb,
        warehouse: sutura_exec_duckdb::DuckDbWarehouse,
        open: crate::conformance::open,
        executes_legs,
    }
}
