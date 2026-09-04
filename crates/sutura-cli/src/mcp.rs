//! The `mcp` command: the agent surface, composed over the standard input and output of this
//! process.
//!
//! The composition root for [`sutura_mcp::serve_stdio`], and the point of issue #110: the agent
//! surface used to be reachable only from its own crate's tests. An agent client launches this
//! process and speaks the Model Context Protocol on its pipes.
//!
//! It shares the `query` command's composition, in [`crate::sources`]: one catalog, one data system
//! opened from whichever of the two declarations names it, and the identity that declaration
//! carries. The difference is the driving port - the service answers questions for a peer on the
//! other end of a pipe instead of one taken from a path on the command line.
//!
//! Kept in its own module rather than inlined into `commands.rs` because it is a composition of its
//! own - the driving port over a pipe, with an async runtime the other commands do not want - and
//! because inlining it would push `commands.rs` over the 1000-line gate. `main.rs`'s command table
//! names it directly.

use std::path::Path;
use std::process::ExitCode;

use sutura_app::prompt::CatalogProse;
use sutura_catalog_local::LocalCatalog;
use sutura_domain::pinned::SemanticCatalog as _;

use crate::commands::{Composed, arg, catalog_reader, render, report, started};
use crate::sources::{Opened, OpenedWith, configured, open_engine};
// The `BigQuery` arm's half of the same question, and reached only from that arm - so the import is
// gated for the reason `sources::bigquery`'s are: `dead_code` is `deny` here, and this crate's
// default build links no BigQuery adapter.
#[cfg(feature = "bigquery")]
use crate::sources::refuse_absent_tables;

/// What serving the surface amounts to: the service, and how catalog descriptions are treated.
///
/// Named because even with [`Composed`] aliased, `(Composed<W>, CatalogProse)` stays over
/// `clippy::type_complexity` once the alias is expanded.
type Served<W> = (Composed<W>, CatalogProse);

/// `mcp <catalog-dir> [data-dir]`: serve the agent surface over standard input and output.
///
/// The data directory is optional for the reason [`crate::commands::query`] states: a deployment that
/// declares its data system in the `sources:` tree has already said where the data is.
pub(crate) fn mcp(args: &[String]) -> ExitCode {
    report((|| {
        let usage = "mcp <catalog-dir> [data-dir]";
        let root = arg(args, 0, "catalog-dir", usage)?;
        let data = args.get(1).map(std::path::PathBuf::from);
        let catalog = catalog_reader(Path::new(&root))?;
        let pinned = catalog.load().map_err(|e| render(&e))?;
        let settings = configured()?;
        // The exhaustive match is here for the reason `crate::commands::query`'s is: the service is
        // monomorphised per adapter, so a third linked adapter is a compile error at this line.
        match open_engine(
            &pinned,
            settings.sources(),
            settings.runtime(),
            settings.server().request_timeout(),
            data.as_deref(),
        )? {
            Opened::Files(opened) => serve(&catalog, opened, settings.runtime()),
            #[cfg(feature = "bigquery")]
            Opened::BigQuery(opened) => {
                // **The pre-flight, and this line is where its ORDER is decided** - the same order
                // `sutura-serve`'s root keeps, for the same reason. It runs after `open_engine`,
                // which read the credential for the declared source, so an operator whose credential
                // file is wrong is told about the credential file and not about a table they would
                // then go and not fix. And it runs before `serve`, which announces the surface and
                // hands the pipe to a peer: a refusal after that point is a refusal an agent client
                // has already been told it can ask questions through.
                //
                // In the `Files` arm this would be a call whose only possible answer is the port's
                // default, because the engine is GIVEN its tables - `refuse_unattached`, inside
                // `mcp_service`, is that arm's version of this check and compares the two sets it has.
                //
                // **The parity is one-sided, and saying so is the point.** `refuse_unattached` closes
                // the window between this root's TWO loads: `catalog.load()` above is the first, this
                // pre-flight reads that bundle, and `LocalService::start` inside `mcp_service` loads a
                // second time. So a model added to the catalog directory between the two is caught on
                // a `files` source and caught by nothing on a `bigquery` one - the same gap
                // `sutura-serve`'s root states at its own call site, open here for the same reason.
                // Closing it is an architecture decision rather than a call-site move:
                // `LocalService` exposes no accessor for the engines it was handed, so there is
                // nothing to re-ask once the second load has happened.
                refuse_absent_tables(&pinned, &opened.engines)?;
                serve(&catalog, opened, settings.runtime())
            }
        }
    })())
}

/// Starts the service over one adapter and serves the agent surface on this process's pipes.
///
/// Generic in the adapter, so the two arms above share every line after them - including the runtime,
/// the startup notice and the bounded teardown, none of which is a per-adapter decision.
fn serve<W>(catalog: &LocalCatalog, opened: OpenedWith<W>, runtime: sutura_config::RuntimeSettings) -> Result<(), String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    let (service, prose) = mcp_service(catalog, opened, runtime)?;
    // The limit printed beside the mode, the way `banner::announce_token_class` prints the token
    // class: a pipe has no header a token could arrive in, so this surface grants every
    // capability to whoever can reach the process. Stated at startup, not left as a default
    // nobody declared. Standard error, which is the log channel, so the MCP stream on stdout
    // stays a pure protocol.
    eprintln!(
        "sutura: serving the agent surface over stdin/stdout - it grants every capability to \
         whoever can launch or reach this process"
    );
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("no async runtime: {e}"))?;
    // The service is shared rather than moved in, and the reason is the one `serve_stdio`
    // documents: the engine's own `Drop` makes releasing it safe anywhere once a question is not
    // in flight, but a peer can disconnect while one IS answering on a pool thread - and
    // releasing the engine then would abort this process. The outer handle below releases it on
    // the main thread, once `shutdown_timeout` has let that in-flight answer finish.
    let service = std::sync::Arc::new(service);
    let served = runtime
        .block_on(sutura_mcp::serve_stdio(
            std::sync::Arc::clone(&service),
            sutura_app::Permitted::every_capability(),
            prose,
        ))
        .map_err(|e| render(&e));
    // Bound the teardown the way `sutura-serve`'s `stop` does: dropping a runtime with a
    // question still answering would wait for it however long it takes, and nothing here can
    // cancel one. This gives the pool a moment to finish and then exits on our own terms.
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    drop(service);
    served
}

/// The agent surface this command serves: the `query` composition behind the driving port.
///
/// **Literally the same composition, not a parallel one.** [`started`] is the single place this
/// binary builds a service - the constructor that takes an audit sink and re-runs every anchor, plus
/// the unattached-table check that closes the gap its second catalog load leaves. That used to be
/// the one difference between the two commands: `query` called the answer function directly and
/// wrote no record at all, which is issue #266's A1, and it now goes through here too.
///
/// A locally launched process installs no subscriber either way, so those records go nowhere for the
/// whole session - the honest default rather than a claim that a record was kept when none was, and
/// the limit the invariants row states. The sink is only the writer, so any composition that does
/// install a subscriber must send it to standard error: on this transport standard output is the
/// protocol channel, which is why the startup notice is an `eprintln!`.
///
/// What is left here is the transport's own decision, and there is one - how catalog descriptions
/// are treated.
fn mcp_service<W>(
    catalog: &LocalCatalog,
    opened: OpenedWith<W>,
    runtime: sutura_config::RuntimeSettings,
) -> Result<Served<W>, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    // The default treatment of catalog descriptions: quoted in, as the other commands render them.
    Ok((started(catalog, opened, runtime)?, CatalogProse::Quoted))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::mcp_service;

    fn example() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
    }

    /// A client and a server joined by an in-memory pipe, with the peer permitted everything.
    ///
    /// The client and the server are the MCP SDK's own, so the bytes on that pipe are the protocol:
    /// nothing in this crate sits between the assertion and the wire. This is the same shape
    /// `sutura_mcp::server::tests` uses; the point here is that the service is THIS command's, over
    /// the real engine and the documented example, rather than a fake warehouse.
    ///
    /// Takes an `Arc<S>` rather than an owned `S`, and the caller keeps another handle to the same
    /// value out of `block_on`. The engine's `Drop` makes releasing the service safe anywhere once
    /// it is idle, so the async server task only ever decrements an `Arc`; the caller's own handle
    /// releases it after its runtime is gone, which keeps the one remaining edge - an engine
    /// released mid-answer - from being the path this suite exercises as it shuts down.
    async fn connected<S>(service: std::sync::Arc<S>) -> rmcp::service::RunningService<rmcp::RoleClient, ()>
    where
        S: sutura_app::surface::Surface,
    {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let server = rmcp::serve_server(
            sutura_mcp::AgentSurface::new(
                service,
                sutura_app::Permitted::every_capability(),
                sutura_app::prompt::CatalogProse::Quoted,
            ),
            server_side,
        );
        let client = rmcp::serve_client((), client_side);
        let (server, client) = tokio::join!(server, client);
        let running: rmcp::service::RunningService<rmcp::RoleServer, _> = server.expect("the server initializes");
        drop(tokio::spawn(async move {
            drop(running.waiting().await);
        }));
        client.expect("the client initializes")
    }

    /// THE claim of issue #110, end to end: the `mcp` command's own composition serves the two tools
    /// over the protocol. The schema snapshots in `sutura-mcp` already pin the tool inputs; what was
    /// missing is that a composition root links the surface and something answers.
    ///
    /// A plain `test` rather than `#[tokio::test]`, deliberately. `mcp_service` opens the engine and
    /// re-runs its anchors, and the engine drives its OWN runtime with `block_on` - which panics when
    /// called from within a runtime, exactly as `sutura-serve`'s `run` documents. So the service is
    /// built before any runtime exists, and the runtime is entered only for the handshake.
    ///
    /// The service is kept in this scope, NOT created inside the async block, so the caller's own
    /// handle releases it after its runtime is gone - the same shape the `mcp` command's shutdown
    /// takes, where a main-thread handle frees the engine once blocking work has settled.
    #[test]
    fn the_mcp_composition_serves_every_tool_the_surface_declares() {
        let catalog = crate::commands::catalog_reader(&example().join("catalog")).expect("the example catalog is readable");
        let pinned = sutura_domain::pinned::SemanticCatalog::load(&catalog).expect("the example catalog loads");
        // An exhaustive match into an `Option` rather than a refutable `let`: `clippy::unreachable`
        // is denied here, and a match is also what makes a third linked adapter a compile error in
        // this test the way it is in the command itself.
        let settings = crate::sources::configured().expect("the embedded defaults load");
        let opened = match crate::sources::open_engine(
            &pinned,
            &sutura_config::SourceRegistry::default(),
            settings.runtime(),
            settings.server().request_timeout(),
            Some(&example().join("data")),
        )
        .expect("the example catalog opens with nothing declared")
        {
            crate::sources::Opened::Files(opened) => Some(opened),
            #[cfg(feature = "bigquery")]
            crate::sources::Opened::BigQuery(_) => None,
        }
        .expect("the example declares a files source");
        let (service, prose) = mcp_service(&catalog, opened, settings.runtime()).expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Quoted);
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(std::sync::Arc::clone(&service)).await;

            // Both tools, in the declared order, over the wire.
            let tools = client.list_all_tools().await.expect("tools/list answers");
            let advertised: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
            let expected: Vec<&str> = sutura_app::Capability::every().map(sutura_app::Capability::id).collect();
            assert_eq!(advertised, expected, "{advertised:?}");

            // One of them answers a question the example certifies, so "something answers" is not
            // just a listing.
            let result = client
                .call_tool(
                    rmcp::model::CallToolRequestParams::new(sutura_app::Capability::AskMetric.id()).with_arguments(
                        serde_json::json!({
                            "metric": "recurring_revenue",
                            "grain": "month",
                            "range": { "start": "2026-01-01", "end": "2026-07-01" },
                        })
                        .as_object()
                        .cloned()
                        .expect("a fixture is an object"),
                    ),
                )
                .await
                .expect("a certified question is not a protocol error");
            assert_ne!(result.is_error, Some(true), "{result:?}");
            let structured = result
                .structured_content
                .as_ref()
                .expect("an answer carries structured content");
            assert_eq!(structured.get("outcome").and_then(serde_json::Value::as_str), Some("answer"));
            drop(client.cancel().await);
        });
    }
}
