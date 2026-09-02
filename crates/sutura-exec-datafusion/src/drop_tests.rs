//! The adapter is safe to drop where its nested runtime alone is not.
//!
//! Tokio's `Runtime::drop` panics when called in a context where blocking is not allowed - which is
//! a worker thread of a caller's runtime, exactly where the agent surface is torn down - and this
//! workspace builds with `panic = "abort"`, so that panic is process death. The `Drop` on
//! [`super::DataFusionWarehouse`] shuts the runtime down via `shutdown_background` instead, which is
//! tokio's documented way to drop a runtime from inside an async context.
//!
//! These two tests were red against the previous shape (a bare `tokio::runtime::Runtime` field) and
//! are green against the `Drop`. They pin the mechanism rather than the reasoning: the first drops
//! the adapter inside the same thread's async context, the second uses the engine's runtime on the
//! main thread and releases the adapter on a worker thread - the shape the `mcp` command's shutdown
//! actually takes.

use sutura_domain::model::SourceName;

fn adapter() -> crate::DataFusionWarehouse {
    crate::DataFusionWarehouse::new(
        SourceName::parse("local").expect("a test source is a source"),
        crate::test_posture(),
        crate::WorkingSet::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test ceiling is positive")),
    )
    .expect("a current-thread runtime builds")
}

#[test]
fn dropping_a_warehouse_inside_an_async_context_does_not_abort() {
    let adapter = adapter();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("an outer runtime builds");
    rt.block_on(async move {
        drop(adapter);
    });
}

#[test]
fn dropping_a_warehouse_on_a_worker_thread_after_the_engine_was_used_on_the_main_thread_does_not_abort() {
    let adapter = adapter();
    // Use the engine's own runtime from the main thread first, as `attach` does during the CLI's
    // `open_engine`, so the core has lived on one thread before the adapter is released elsewhere.
    adapter.runtime().expect("a test runtime is present").block_on(async {});
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .expect("an outer runtime builds");
    rt.block_on(async move {
        let handle = tokio::spawn(async move {
            drop(adapter);
        });
        handle.await.expect("the worker task finished without panicking");
    });
}
