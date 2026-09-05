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
use sutura_runtime::Admission;

use crate::commands::{Composed, arg, catalog_prose, catalog_reader, render, report, started};
use crate::sources::{Opened, OpenedWith, configured, open_engine};
// The `BigQuery` arm's half of the same question, and reached only from that arm - so the import is
// gated for the reason `sources::bigquery`'s are: `dead_code` is `deny` here, and this crate's
// default build links no BigQuery adapter.
#[cfg(feature = "bigquery")]
use crate::sources::refuse_absent_tables;

/// What serving the surface amounts to: the service, how catalog descriptions are treated, and the
/// bound on how many questions may be executing at once.
///
/// Named because even with [`Composed`] aliased, the tuple stays over `clippy::type_complexity`
/// once the alias is expanded.
type Served<W> = (Composed<W>, CatalogProse, Admission);

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
    let (service, prose, admission) = mcp_service(catalog, opened, settings)?;
    // The limit printed beside the mode, the way `banner::announce_token_class` prints the token
    // class: a pipe has no header a token could arrive in, so this surface grants every
    // capability to whoever can reach the process. Stated at startup, not left as a default
    // nobody declared. Standard error, which is the log channel, so the MCP stream on stdout
    // stays a pure protocol.
    //
    // The admission bound is printed with it, and for the same reason: it is the number an operator
    // configured and the number a shed call is about, so it belongs where the posture is stated
    // rather than inside a semaphore nobody can see.
    eprintln!(
        "sutura: serving the agent surface over stdin/stdout - it grants every capability to \
         whoever can launch or reach this process, and answers at most {} questions at once",
        admission.bound()
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
            admission,
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
/// What is left here is the transport's own decisions, and there are two - how catalog descriptions
/// are treated, and how many questions may be executing at once. **It takes the whole `Settings`
/// rather than the values it needs**, so both are READ here and not handed in; `#266`'s `H1` is what
/// a caller-supplied setting costs.
fn mcp_service<W>(catalog: &LocalCatalog, opened: OpenedWith<W>, settings: &sutura_config::Settings) -> Result<Served<W>, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    // **Read here rather than taken as an argument**, and that is the whole of `#266`'s `H1` at this
    // layer: this line was the constant `CatalogProse::Quoted` under a comment calling it *the
    // default treatment*, which it was not - it was the only one. A caller-supplied setting would
    // move the defect one frame up and leave the same line uncovered, because a test can pass the
    // value it wants to see. The conversion is `crate::commands::catalog_prose`, shared with
    // `sutura prompt`, because two roots resolving one decision separately is how this survived.
    //
    // The admission bound is built HERE for the same reason and one more: this is the composition
    // root, so it is the one place that can decide there is exactly one bound for the process.
    // `#325`'s `F7` is what its absence cost - the agent surface spawned a question per request with
    // nothing counting them, while the HTTP surface took a slot from `runtime.max_concurrent_queries`
    // for every one of its own. `Admission::from_settings` is what stops the two keys being read
    // from different places.
    Ok((
        started(catalog, opened, settings.runtime())?,
        catalog_prose(settings.prompt().catalog_prose()),
        Admission::from_settings(settings.runtime()),
    ))
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
    /// The prose treatment and the admission bound are the caller's, because they are what this
    /// command resolves from the settings and a test that hardcoded either could not tell two
    /// settings documents apart.
    async fn connected<S>(
        service: std::sync::Arc<S>,
        prose: sutura_app::prompt::CatalogProse,
        admission: sutura_runtime::Admission,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()>
    where
        S: sutura_app::surface::Surface,
    {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let server = rmcp::serve_server(
            sutura_mcp::AgentSurface::new(service, sutura_app::Permitted::every_capability(), prose, admission),
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

    /// The documented example, opened over the in-process engine, under a settings DOCUMENT.
    ///
    /// The overlay occupies the position a deployment's own file does, which is the whole point: the
    /// setting has to arrive the way an operator writes it, or the read under test is the test's own
    /// argument. The same construction `sutura_http::harness::settings` uses, for the same reason.
    ///
    /// Extracted because two tests compose it and the composition is the thing under test in both:
    /// a second copy would be a second answer to *what does this command build*.
    fn example_composition(
        overlay: &str,
    ) -> (
        super::LocalCatalog,
        crate::sources::OpenedWith<sutura_exec_datafusion::DataFusionWarehouse>,
        sutura_config::Settings,
    ) {
        let catalog = crate::commands::catalog_reader(&example().join("catalog")).expect("the example catalog is readable");
        let pinned = sutura_domain::pinned::SemanticCatalog::load(&catalog).expect("the example catalog loads");
        let settings = sutura_config::Settings::load(
            &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
        )
        .expect("the test settings load");
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
        let (catalog, opened, settings) = example_composition("");
        let (service, prose, admission) = mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Quoted);
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(std::sync::Arc::clone(&service), prose, admission).await;

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
    /// The setting arrives as a settings DOCUMENT rather than as an argument, which is what makes
    /// this a test of the read and not of a parameter: a test that handed `mcp_service` the value it
    /// wanted to see would pass with the line back to a constant one frame up. Then over the wire
    /// through the SDK's own client, on both halves of the reply, because the defect was the WIRING -
    /// what each half renders for a setting it was given is pinned in `sutura-mcp`.
    ///
    /// The runtime and drop-order rationale above applies here unchanged. What this does NOT reach
    /// is `SUTURA_CONFIG_DIR` and `serve` - a settings tree on disk read by the spawned binary -
    /// which is `tests/mcp.rs`'s
    /// `a_deployments_prose_setting_reaches_both_halves_of_the_agent_surface`, on the split that
    /// suite's own header states: this one holds the composition, that one holds the process.
    #[test]
    fn the_mcp_composition_honours_the_prose_setting_it_was_configured_with() {
        let (catalog, opened, settings) = example_composition("prompt:\n  catalog_prose: omitted\n");
        let (service, prose, admission) = mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Omitted);
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(std::sync::Arc::clone(&service), prose, admission).await;
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
            // A description the example catalog really carries, on a dimension of every metric. What
            // each half renders is `sutura-mcp`'s own business and is pinned there; what this test
            // owns is that the setting reached them at all.
            assert!(!structured.contains("Where the customer is."), "{structured}");
            // No description FIELD either, which is `#266`'s `H1` in its own words, and the setting
            // echoed so a client reads *this deployment ships none* rather than inferring it.
            assert!(!structured.contains("description"), "{structured}");
            assert!(structured.contains(r#""catalog_prose":"omitted""#), "{structured}");
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

    /// `runtime.max_concurrent_queries` reaches the served agent surface from THIS command's root.
    ///
    /// **The composition half of `#325`'s `F7`, and the half no test in `sutura-mcp` can reach.**
    /// That crate's suite is handed an `Admission` and proves what one enforces; this root is what
    /// has to build one at all, and it built none - `serve_stdio` took no bound, so the agent
    /// surface answered every question a peer sent while the HTTP surface took a slot for each of
    /// its own from the same key.
    ///
    /// Two documents rather than one, because that is what tells a READ from a constant: the first
    /// is the settings tree as an operator inherits it and is compared against what those settings
    /// say, and the second names a number no default carries. A root that returned a constant passes
    /// the first and fails the second.
    ///
    /// No runtime here, deliberately: this asserts what the composition RESOLVES, and the same
    /// number's effect on a question in flight is `sutura_mcp::server`'s own suite, over a fake that
    /// can be held. Neither the engine nor a pipe is needed to read a setting.
    #[test]
    fn the_mcp_composition_bounds_execution_with_the_number_it_was_configured_with() {
        let (catalog, opened, settings) = example_composition("");
        let (service, _prose, admission) = mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(admission.bound(), settings.runtime().max_concurrent_queries().count());
        assert_eq!(admission.wait(), settings.runtime().admission_timeout().duration());
        drop(service);

        let (catalog, opened, settings) =
            example_composition("runtime:\n  max_concurrent_queries: 3\n  admission_timeout_seconds: 1\n");
        let (service, _prose, admission) = mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(admission.bound(), 3, "the configured bound did not reach the surface");
        assert_eq!(admission.wait(), std::time::Duration::from_secs(1));
        drop(service);
    }
}
