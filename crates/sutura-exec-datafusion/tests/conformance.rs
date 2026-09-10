//! This engine, held to the conformance packs, from its OWN crate.
//!
//! **The second binding of one pack.** `DuckDB` and this engine are different kinds of thing behind
//! one port - a plan is rendered into SQL and pushed down there, and executed as a logical plan over
//! Arrow here - so binding both is what shows the pack is a statement about the port rather than a
//! test that happens to fit one adapter.
//!
//! **The leg behaviour arrives here under the name that says it was EXECUTED**, and that is the one
//! line in this file worth a paragraph: this engine is the only adapter a release links, so until it
//! declared `Warehouse::EXECUTES_LEGS` no published artifact could answer a two-source question at
//! all. The two adapters no longer make opposite leg declarations, and what still shows the
//! declaration SELECTS is `sutura-exec-postgres`'s binding, which takes the default and gets
//! `a_leg_is_refused` - both directions of the pack are still bound, on different crates.
//!
//! It also exercises the run-time declination. This adapter takes the port's default `dry_run`,
//! because for an in-process engine checking costs what running costs - so the pre-flight behaviour
//! comes back `DECLINED` with a typed reason rather than green over nothing.
//!
//! **What this file does not establish:** the same three things the `DuckDB` binding's header lists,
//! plus one of its own - this engine is the side the differential suite in `sutura-app` compares
//! every data source AGAINST, so a bug shared between the pack's reference rows and this engine
//! would be invisible to both. The pack's reference is written by hand from the corpus rather than
//! recorded from a run, which is what keeps that from being circular.

// One `#[cfg(test)]` module, for the reason the sibling adapter's binding gives.
#[cfg(test)]
mod conformance {
    use sutura_conformance::{Fixture, corpus};
    use sutura_exec_datafusion::{DataFusionWarehouse, WorkingSet};

    /// The engine with the conformance corpus attached.
    ///
    /// The ceiling is a gibibyte, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` -
    /// written as a literal rather than read from that crate, because a conformance binding must not
    /// give this adapter a dependency on the settings tree to obtain one number. The corpus is seven
    /// rows, so no case in it comes near the bound; the bound's own assertions live in the adapter's
    /// `pool.rs`.
    /// **`Fixture::standing` unconditionally**, for the reason the sibling adapter's fixture gives:
    /// this engine is in-process, so there is no venue in which it cannot stand up.
    fn open() -> Fixture<DataFusionWarehouse> {
        let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
        let engine = DataFusionWarehouse::new(corpus::source(), corpus::posture(), WorkingSet::of_bytes(ceiling))
            .expect("an in-process engine starts");
        engine
            .attach_csv(&corpus::table(), &corpus::on_disk())
            .expect("the engine attaches the conformance corpus");
        Fixture::standing(engine)
    }

    // `executes_legs`, because this adapter declares `EXECUTES_LEGS`. The tag and the constant
    // cannot disagree: a binding tagged this way over an adapter declaring `false` is torn by a
    // `const` assertion in the pack, and one tagged `refuses_legs` over an adapter that ANSWERS a
    // leg fails as `Fault::ALegWasAnswered` - which is what makes this line the pack's own check on
    // the constant beside it rather than a label.
    //
    // What the cell asserts is the claim rather than a smoke test: the leg reads the same table over
    // the same range as one of the whole-plan cases and must land on that case's rows, content
    // compared. This adapter builds a logical plan for a leg (`src/leg.rs`) and NO SQL, so there is
    // no golden that can see this path - the conformance cell and `sutura-app`'s two-source
    // differential are its only evidence.
    sutura_conformance::execute_packs! {
        adapter: datafusion,
        warehouse: sutura_exec_datafusion::DataFusionWarehouse,
        open: crate::conformance::open,
        executes_legs,
    }
}
