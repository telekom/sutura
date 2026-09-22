//! The engine's own collection, refused for crossing its byte budget.
//!
//! **Why this is not in `pool/ceiling_tests.rs` beside the working-set cells, measured rather than
//! assumed.** Those cells drive a whole question through the port, and the two bounds are sized from
//! one number (`WorkingSet::result_budget`), so a ceiling small enough to refuse the answer's
//! materialisation is one the grouped aggregate is refused by first. Probed at 64 KiB, 128 KiB,
//! 192 KiB, 256 KiB, 384 KiB, 512 KiB and 1 MiB against that file's thousand-group fixture: every
//! one came back `working_set_exhausted`, the operator reservation biting before a batch existed.
//!
//! So this file asks the narrower question the port cannot: a plan with **no** blocking operator -
//! a bare scan, whose reservation is negligible - collected under a budget smaller than its result.
//! That is the path round 7 of `telekom/sutura#929`'s review named, and it is reached directly
//! because nothing above the port can hold the two numbers apart.
//!
//! **What this file therefore does NOT establish:** that a *question* travelling the port is ever
//! refused by the budget rather than by the pool. On the engine, with both sized from one key, the
//! pool is the tighter of the two for any plan that aggregates, joins or sorts. The budget is what
//! covers the plan shapes it is not - and what covers a FOREIGN driver, where `MOST_RESULT_BYTES`
//! is the same guard with a number of its own.

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use sutura_domain::warehouse::{UnannouncedBatch, Warehouse as _};

use crate::{DataFusionError, DataFusionWarehouse, WorkingSet};

/// Wide rows, which is the shape a row ceiling cannot see and this budget can: two hundred rows
/// whose text column is long enough that the result costs far more than its row count suggests.
fn wide() -> RecordBatch {
    let rows = 200_i64;
    let columns: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(
            (0..rows).map(|row| format!("{row:0>512}")).collect::<Vec<String>>(),
        )),
        Arc::new(Int64Array::from((0..rows).collect::<Vec<i64>>())),
    ];
    let schema = Schema::new(vec![
        Field::new("region", DataType::Utf8, false),
        Field::new("amount_cents", DataType::Int64, false),
    ]);
    RecordBatch::try_new(Arc::new(schema), columns).expect("a test batch is rectangular")
}

/// An engine with the wide batch registered, at a configured ceiling of `bytes`.
///
/// **The ceiling is the adapter's own and the budget is derived from it**, which is what makes these
/// cells cover `WorkingSet::result_budget` and not just the guard: `collected` takes the
/// `WorkingSet`, so there is no budget a caller could pass instead.
fn engine(bytes: usize) -> DataFusionWarehouse {
    let adapter = DataFusionWarehouse::new(
        crate::source(),
        crate::test_posture(),
        WorkingSet::of_bytes(core::num::NonZeroUsize::new(bytes).expect("a test ceiling is positive")),
    )
    .expect("a bounded engine builds");
    drop(
        adapter
            .context
            .register_batch("wide", wide())
            .expect("an in-memory batch registers"),
    );
    adapter
}

#[test]
fn the_engines_own_collection_is_refused_for_crossing_its_byte_budget() {
    // A kibibyte-wide working set: a bare scan of a registered batch reserves nothing against the
    // pool, so the budget derived from that same number is what the result meets.
    let adapter = engine(1024);
    let refused = adapter
        .runtime()
        .expect("the adapter holds its runtime")
        .block_on(async {
            let frame = adapter.context.table("wide").await.expect("the registered table resolves");
            super::collected(frame, adapter.working_set()).await
        })
        .expect_err("a wide result does not fit a kibibyte");

    // **`Unannounced`, not `Execute`**: the engine ran, and the guard above it refused what came
    // back. An `Execute` here would mean the pool bit first and this cell was measuring that.
    let DataFusionError::Unannounced { ref cause } = refused else {
        panic!("the byte budget refused, not the engine: {refused:?}")
    };
    assert_eq!(*cause, UnannouncedBatch::OverBudget { most_bytes: 1024 });

    // And the port's own question, which is what makes this a refusal the caller can read rather
    // than a `503` inviting a retry that spends the same budget in the same place.
    assert!(adapter.result_did_not_fit(&refused), "{refused:?}");
    // Not the OTHER bound: no operator reservation was refused, so the caller is not told resources
    // were exhausted. The two numbers are the same value and the two refusals are not the same
    // refusal.
    assert_eq!(adapter.working_set_exhausted(&refused), None);
}

#[test]
fn the_same_scan_inside_its_byte_budget_is_collected() {
    // The control, and it is not decoration: without it the cell above passes just as well against
    // a `collected` that refuses everything, or against a fixture that never resolved a table.
    let adapter = engine(8 * 1024 * 1024);
    let collected = adapter
        .runtime()
        .expect("the adapter holds its runtime")
        .block_on(async {
            let frame = adapter.context.table("wide").await.expect("the registered table resolves");
            super::collected(frame, adapter.working_set()).await
        })
        .expect("two hundred wide rows fit eight mebibytes");
    assert_eq!(collected.rows(), 200);
    // Nothing is left reserved, so the budget refused nothing the pool then had to release.
    assert_eq!(adapter.context.runtime_env().memory_pool.reserved(), 0);
}
