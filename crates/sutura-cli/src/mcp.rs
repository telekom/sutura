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
use sutura_app::surface::{LocalService, Surface as _};
use sutura_catalog_local::LocalCatalog;
use sutura_domain::pinned::SemanticCatalog as _;
use sutura_runtime::TracingAuditSink;

use crate::commands::{arg, catalog_prose, catalog_reader, render, report};
use crate::sources::{Opened, OpenedWith, configured, open_engine, refuse_unattached, served_tables};
// The `BigQuery` arm's half of the same question, and reached only from that arm - so the import is
// gated for the reason `sources::bigquery`'s are: `dead_code` is `deny` here, and this crate's
// default build links no BigQuery adapter.
#[cfg(feature = "bigquery")]
use crate::sources::refuse_absent_tables;

/// The service this command serves, over one of the adapters this binary links.
///
/// Named because the concrete type is over `clippy::type_complexity`: the warehouse, the audit sink
/// and the broker are the three collaborators every command in this binary composes. Generic in the
/// warehouse since the `bigquery` feature landed; the other two are this binary's own choice and
/// never vary.
type McpSurface<W> = LocalService<W, TracingAuditSink, sutura_config::StaticCredentialBroker>;

/// What serving the surface amounts to: the service, and how catalog descriptions are treated.
///
/// Named because even with `McpSurface` aliased, `(McpSurface<W>, CatalogProse)` stays over
/// `clippy::type_complexity` once the alias is expanded.
type Served<W> = (McpSurface<W>, CatalogProse);

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
            Opened::Files(opened) => serve(&catalog, opened, &settings),
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
                serve(&catalog, opened, &settings)
            }
        }
    })())
}

/// Starts the service over one adapter and serves the agent surface on this process's pipes.
///
/// Generic in the adapter, so the two arms above share every line after them - including the runtime,
/// the startup notice and the bounded teardown, none of which is a per-adapter decision.
fn serve<W>(catalog: &LocalCatalog, opened: OpenedWith<W>, settings: &sutura_config::Settings) -> Result<(), String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    let (service, prose) = mcp_service(catalog, opened, settings.runtime(), settings.prompt().catalog_prose())?;
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
/// The same answer path `query` exercises, wrapped in the service an agent client speaks to. The
/// one difference worth stating is the audit sink: [`LocalService::start`] requires one (a service
/// with no sink does not exist), and `TracingAuditSink` is it. A locally launched process installs
/// no subscriber, so those records go nowhere for the whole session - a different weight than
/// `query`, where one person's own terminal loses nothing, but the same honest default: nothing
/// claims a record was kept when none was. The sink is only the writer, so any composition that
/// does install a subscriber must send it to standard error - on this transport standard output is
/// the protocol channel, which is why the startup notice is an `eprintln!`.
fn mcp_service<W>(
    catalog: &LocalCatalog,
    opened: OpenedWith<W>,
    runtime: sutura_config::RuntimeSettings,
    prose: sutura_config::CatalogProse,
) -> Result<Served<W>, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    let working_set = runtime.working_set().bytes().get() as u64;
    // `LocalService::start` loads the catalog again and re-runs every anchor - that is its contract,
    // the constructor that returns a service only if the bundle is fit to serve. `catalog` is handed
    // over rather than the `pinned` rebuilt, so the two loads cannot disagree about the version or
    // the source name - and `refuse_unattached` closes the one gap that remains: a model added to the
    // catalog directory between `load()` above and the load inside `start` would otherwise be served
    // with no table registered behind it, failing its first question at query time. The same check
    // `sutura-serve` runs at boot, so a served surface refuses to start in the same cases.
    let service = LocalService::start(catalog, opened.engines, TracingAuditSink::new(), opened.broker, working_set)
        .map_err(|cause| format!("{}\nthis bundle is not fit to serve", render(&cause)))?;
    // Skipped for a data system nothing was attached to, which is the narrowing `Opened::attached`
    // documents: the check compares the tables the served bundle names against the tables the engine
    // HOLDS, and it holds them because the attach step put them there. Nothing to compare is not the
    // same as nothing missing.
    if let Some(attached) = opened.attached {
        refuse_unattached(&served_tables(service.definitions()), &attached)?;
    }
    // **The operator's setting, not a constant.** This line read `CatalogProse::Quoted` and called
    // it *the default treatment*, which it was not: it was the ONLY treatment, so a deployment that
    // had set `prompt.catalog_prose: omitted` - and had it honoured by the prompt and by the HTTP
    // catalog body - still shipped every description over the agent surface, which is the reader the
    // setting exists for. `#266`'s `H1`. The conversion is `crate::commands::catalog_prose` because
    // this is the second root reading one decision, and a second inline `if` is how they drift.
    Ok((service, catalog_prose(prose)))
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
    ///
    /// The prose treatment is the caller's, because it is what this command resolves from the
    /// settings and a test that hardcoded it could not tell the two settings apart.
    async fn connected<S>(
        service: std::sync::Arc<S>,
        prose: sutura_app::prompt::CatalogProse,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()>
    where
        S: sutura_app::surface::Surface,
    {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let server = rmcp::serve_server(
            sutura_mcp::AgentSurface::new(service, sutura_app::Permitted::every_capability(), prose),
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
    /// The documented example, opened over the in-process engine, with the settings a bare process
    /// resolves.
    ///
    /// Extracted because two tests compose it and the composition is the thing under test in both:
    /// a second copy would be a second answer to *what does this command build*.
    fn example_composition() -> (
        super::LocalCatalog,
        crate::sources::OpenedWith<sutura_exec_datafusion::DataFusionWarehouse>,
        sutura_config::Settings,
    ) {
        let catalog = crate::commands::catalog_reader(&example().join("catalog")).expect("the example catalog is readable");
        let pinned = sutura_domain::pinned::SemanticCatalog::load(&catalog).expect("the example catalog loads");
        let settings = crate::sources::configured().expect("the embedded defaults load");
        // An exhaustive match into an `Option` rather than a refutable `let`: `clippy::unreachable`
        // is denied here, and a match is also what makes a third linked adapter a compile error in
        // this test the way it is in the command itself.
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
        (catalog, opened, settings)
    }

    #[test]
    fn the_mcp_composition_serves_every_tool_the_surface_declares() {
        let (catalog, opened, settings) = example_composition();
        let (service, prose) = mcp_service(&catalog, opened, settings.runtime(), settings.prompt().catalog_prose())
            .expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Quoted);
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(std::sync::Arc::clone(&service), prose).await;

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

    /// `prompt.catalog_prose: omitted` reaches the served agent surface from THIS command's root.
    ///
    /// The last link of `#266`'s `H1`, and the one no test in `sutura-mcp` can reach: that crate's
    /// suite is handed a setting, while this root is what reads one. It read none - `mcp_service`
    /// returned `CatalogProse::Quoted` as a constant - so a deployment whose prompt and whose HTTP
    /// catalog body both withheld the catalog's prose still served every description to an agent
    /// over stdio, which is the surface with no token and no scope narrowing at all.
    ///
    /// Asserted over the wire through the SDK's own client, on both halves of the reply and over the
    /// real example catalog, because the defect was the WIRING: each half was already covered by a
    /// test in `sutura-mcp` against a setting that arrived as an argument.
    #[test]
    fn the_mcp_composition_honours_the_prose_setting_it_was_configured_with() {
        let (catalog, opened, settings) = example_composition();
        let (service, prose) = mcp_service(&catalog, opened, settings.runtime(), sutura_config::CatalogProse::Omitted)
            .expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Omitted);
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(std::sync::Arc::clone(&service), prose).await;
            let result = client
                .call_tool(rmcp::model::CallToolRequestParams::new(
                    sutura_app::Capability::DescribeCatalog.id(),
                ))
                .await
                .expect("the catalog tool answers");
            let structured = result
                .structured_content
                .as_ref()
                .expect("the catalog carries structured content")
                .to_string();
            // A description the example catalog really carries, on a dimension of every metric.
            assert!(!structured.contains("Where the customer is."), "{structured}");
            assert!(!structured.contains("description"), "{structured}");
            let text = result
                .content
                .first()
                .and_then(rmcp::model::ContentBlock::as_text)
                .map(|block| block.text.clone())
                .expect("the catalog carries a text block");
            assert!(!text.contains("Where the customer is."), "{text}");
            assert!(text.contains("NOT included"), "{text}");
            drop(client.cancel().await);
        });
    }
}
