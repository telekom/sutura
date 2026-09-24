//! The `mcp` command: the agent surface, composed over the standard input and output of this
//! process.
//!
//! The composition root for [`sutura_mcp::serve_stdio`], and the point of issue #110: the agent
//! surface used to be reachable only from its own crate's tests. An agent client launches this
//! process and speaks the Model Context Protocol on its pipes.
//!
//! **Since issue #970, this root opens `catalogs:` the same way `crate::serve` does** -
//! [`crate::serve::catalog::open_catalog`] and [`crate::serve::catalog::load`], never a directory
//! argument of this command's own - which is what makes a `catalog.kind: datahub` or `okf`
//! deployment reachable from the stdio surface rather than only from `sutura serve`. The DATA
//! side is unchanged and still `crate::sources`: one data system, opened from whichever of the
//! two declarations names it, the same composition `query` shares.
//!
//! Kept in its own module rather than inlined into `commands.rs` because it is a composition of its
//! own - the driving port over a pipe, with an async runtime the other commands do not want - and
//! because inlining it would push `commands.rs` over the 1000-line gate. `main.rs`'s command table
//! names it directly.

use std::process::ExitCode;
use std::sync::Arc;

use sutura_app::surface::{Surface, SurfaceFailure};
use sutura_config::RequestTimeout;
use sutura_domain::identity::RequestContext;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::raw::{RawOutcome, RawStatement};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_runtime::Admission;

use crate::commands::{agent_instructions, catalog_prose, render, report};
use crate::sources::{Opened, OpenedWith, configured, open_engine};
// The `BigQuery` arm's half of the same question, and reached only from that arm - so the import is
// gated for the reason `sources::bigquery`'s are: `dead_code` is `deny` here, and this crate's
// default build links no BigQuery adapter.
#[cfg(feature = "bigquery")]
use crate::sources::refuse_absent_tables;

/// What serving the surface amounts to: the erased service, how catalog descriptions are treated,
/// the two bounds - how many questions may be executing at once, and how long a peer waits for one
/// - and the rendered document a peer's `initialize` result carries.
///
/// **Erased to `Arc<dyn Surface>` rather than generic in the adapter**, unlike the old shape this
/// replaced: once a catalog KIND is also a choice this root makes (issue #970), the composition is
/// two independent matches - which adapter, which catalog - and erasing right after
/// [`compose`] is what stops the rest of this file needing a second type parameter for a
/// distinction nothing past that point cares about. `crate::serve`'s own `main.rs` erases at the
/// same point for the same reason.
type Served = (
    Arc<dyn Surface>,
    sutura_app::prompt::CatalogProse,
    Admission,
    RequestTimeout,
    Arc<str>,
);

/// `mcp [data-dir]`: serve the agent surface over standard input and output.
///
/// The data directory is optional for the reason [`crate::commands::query`] states: a deployment that
/// declares its data system in the `sources:` tree has already said where the data is.
pub(crate) fn mcp(args: &[String]) -> ExitCode {
    report((|| {
        let data = args.first().map(std::path::PathBuf::from);
        let settings = configured()?;
        // The ONE boot-time `outbound` value flows to BOTH the catalog adapter and the source
        // wire, mirroring `crate::serve::run` - a deployment declaring `security.outbound` now
        // verifies its `datahub`/`okf` catalog reader against the same CA set a `bigquery` source
        // wire is verified against, on this composition root too.
        let outbound = crate::sources::resolve_outbound_anchors(&settings)?;
        let catalogs = crate::serve::catalog::open_catalog(settings.catalogs(), outbound.as_ref())?;
        let pinned = crate::serve::catalog::load(&catalogs)?;
        match open_engine(
            &pinned,
            settings.sources(),
            settings.runtime(),
            settings.server().request_timeout(),
            data.as_deref(),
            outbound.as_ref(),
        )? {
            Opened::Files(opened) => serve(compose(&catalogs, opened, &settings)?, &settings),
            #[cfg(feature = "bigquery")]
            Opened::BigQuery(opened) => {
                // **The pre-flight, and this line is where its ORDER is decided** - the same order
                // `crate::serve`'s root keeps, for the same reason: after `open_engine`, which read
                // the credential for the declared source, and before `serve`, which hands the pipe
                // to a peer.
                refuse_absent_tables(&pinned, &opened.engines)?;
                serve(compose(&catalogs, opened, &settings)?, &settings)
            }
            #[cfg(feature = "postgres")]
            Opened::Postgres(opened) => {
                // A `postgres` source attaches nothing and reports no table inventory, so
                // `compose`'s own unattached check has nothing to add - the same reasoning
                // `crate::serve`'s own arm carries. A mistyped `table:` is caught on the first
                // question against it.
                serve(compose(&catalogs, opened, &settings)?, &settings)
            }
            #[cfg(feature = "clickhouse")]
            Opened::ClickHouse(opened) => {
                // The `postgres` arm's reason exactly - `ClickHouseWarehouse` takes the port's
                // default `preflight`.
                serve(compose(&catalogs, opened, &settings)?, &settings)
            }
            #[cfg(feature = "oracle")]
            Opened::Oracle(opened) => {
                // The `clickhouse` arm's reason: `OracleWarehouse` takes the port's default
                // `preflight`.
                serve(compose(&catalogs, opened, &settings)?, &settings)
            }
        }
    })())
}

/// Builds the served surface over whichever catalog kind `catalogs` opened, and closes the one gap
/// left to its caller.
///
/// **The composition itself is [`crate::serve::started`]**, reused rather than duplicated - it
/// already matches over [`crate::serve::catalog::OpenedCatalogs`] and returns the erased `Surface`
/// this root and `sutura serve` both build a `LocalService` from. What this adds is the check that
/// root's own callers make right after it, outside the shared function: a `files` source this
/// process attached is compared against the bundle the erased service actually validated, which
/// [`sutura_app::preflight::refuse_unattached`] can only do once the service exists. Skipped for a
/// data system nothing was attached to (`OpenedWith::attached` is `None`), the same narrowing
/// `crate::commands::started` applies for `query`.
fn compose<W>(
    catalogs: &crate::serve::catalog::OpenedCatalogs,
    opened: OpenedWith<W>,
    settings: &sutura_config::Settings,
) -> Result<Arc<dyn Surface>, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    let service = crate::serve::started(catalogs, opened.engines, opened.broker, settings)?;
    if let Some(attached) = opened.attached {
        sutura_app::preflight::refuse_unattached(&sutura_app::preflight::served_tables(service.definitions()), &attached)
            .map_err(|changed| changed.to_string())?;
    }
    Ok(service)
}

/// Gives [`sutura_mcp::serve_stdio`] a SIZED `Surface` to hold without un-erasing.
///
/// `compose` above already erases the adapter, and now the catalog kind, to `Arc<dyn Surface>`;
/// `serve_stdio<S: Surface>` needs a concrete `S`. The identical shape `crate::serve::agent` builds
/// for the HTTP mount - not reused directly because that one also pushes a spend-headroom gauge
/// shared with the HTTP query route, which this stdio-only binary has no second transport to share
/// with.
struct ErasedService(Arc<dyn Surface>);

impl Surface for ErasedService {
    fn definitions(&self) -> &PinnedDefinitions {
        self.0.definitions()
    }
    fn answer(&self, context: &RequestContext, query: &Query, deadline: Deadline) -> Result<ToolOutcome, SurfaceFailure> {
        self.0.answer(context, query, deadline)
    }
    fn run_sql(&self, context: &RequestContext, statement: &RawStatement) -> Result<RawOutcome, SurfaceFailure> {
        self.0.run_sql(context, statement)
    }
    fn spend_headroom_bytes(&self) -> Option<u64> {
        self.0.spend_headroom_bytes()
    }
}

/// Starts the service over the erased surface and serves the agent surface on this process's pipes.
fn serve(service: Arc<dyn Surface>, settings: &sutura_config::Settings) -> Result<(), String> {
    let (service, prose, admission, reply, instructions) = mcp_service(service, settings)?;
    // The limit printed beside the mode, the way `banner::announce_token_class` prints the token
    // class: a pipe has no header a token could arrive in, so this surface grants every
    // capability to whoever can reach the process. Stated at startup, not left as a default
    // nobody declared. Standard error, which is the log channel, so the MCP stream on stdout
    // stays a pure protocol.
    //
    // Both bounds are printed with it, and for the same reason: they are the numbers an operator
    // configured, and the numbers a shed call and a given-up wait are about, so they belong where
    // the posture is stated rather than inside a semaphore and a timeout nobody can see.
    eprintln!(
        "sutura: serving the agent surface over stdin/stdout - it grants every capability to \
         whoever can launch or reach this process, answers at most {} questions at once, and gives \
         up on a question after {} seconds - waiting for a slot included, and the question itself \
         keeps running",
        admission.bound(),
        reply.seconds()
    );
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("no async runtime: {e}"))?;
    // The service is shared rather than moved in, and the reason is the one `serve_stdio`
    // documents: the engine's own `Drop` makes releasing it safe anywhere once a question is not
    // in flight, but a peer can disconnect while one IS answering on a pool thread - and
    // releasing the engine then would abort this process. The outer handle below releases it on
    // the main thread, once `shutdown_timeout` has let that in-flight answer finish.
    let service = Arc::new(ErasedService(service));
    // Every capability EXCEPT the raw SQL tool unless this deployment turned it on - the same
    // narrowing `sutura-http`'s capability layer applies, over the no-authentication case this
    // transport always is: a pipe has no header a token could arrive in, so scope alone cannot
    // keep the tool off, and `docs/adr/0013` requires it absent for every caller regardless.
    let permitted = sutura_app::Permitted::every_capability();
    let permitted = if settings.tools().run_sql_enabled() {
        permitted
    } else {
        permitted.without(sutura_app::Capability::RunSql)
    };
    let served = runtime
        .block_on(sutura_mcp::serve_stdio(
            Arc::clone(&service),
            permitted,
            prose,
            admission,
            reply,
            instructions,
        ))
        .map_err(|e| render(&e));
    // Bound the teardown the way `crate::serve`'s `stop` does: dropping a runtime with a
    // question still answering would wait for it however long it takes, and nothing here can
    // cancel one. This gives the pool a moment to finish and then exits on our own terms.
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    drop(service);
    served
}

/// The agent surface this command serves: the four transport-only decisions over an already-built
/// service.
///
/// **Literally the same composition, not a parallel one.** [`compose`] is where this binary builds
/// a service (through [`crate::serve::started`], which carries the audit sink and re-runs every
/// anchor); what is left here is the transport's own decisions - how catalog descriptions are
/// treated, how many questions may be executing at once, how long a peer waits for one of them, and
/// the rendered prompt a peer's `initialize` result carries. **It takes the whole `Settings` rather
/// than the values it needs**, so all four are READ here and not handed in - `#266`'s `H1` is what
/// a caller-supplied setting costs.
fn mcp_service(service: Arc<dyn Surface>, settings: &sutura_config::Settings) -> Result<Served, String> {
    let instructions = agent_instructions(service.definitions(), settings)?;
    Ok((
        service,
        catalog_prose(settings.prompt().catalog_prose()),
        Admission::from_settings(settings.runtime()),
        settings.server().request_timeout(),
        Arc::from(instructions),
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Served, compose, mcp_service};

    fn example() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
    }

    /// A client and a server joined by an in-memory pipe, with the peer permitted everything.
    ///
    /// The client and the server are the MCP SDK's own, so the bytes on that pipe are the protocol:
    /// nothing in this crate sits between the assertion and the wire. This is the same shape
    /// `sutura_mcp::server::tests` uses; the point here is that the service is THIS command's, over
    /// the real engine and the documented example, rather than a fake warehouse.
    async fn connected<S>(
        service: std::sync::Arc<S>,
        prose: sutura_app::prompt::CatalogProse,
        admission: sutura_runtime::Admission,
        reply: sutura_config::RequestTimeout,
        instructions: std::sync::Arc<str>,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()>
    where
        S: sutura_app::surface::Surface,
    {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let server = rmcp::serve_server(
            sutura_mcp::AgentSurface::new(
                service,
                sutura_mcp::Asking::TheProcessOwner {
                    permitted: sutura_app::Permitted::every_capability(),
                },
                prose,
                admission,
                reply,
                instructions,
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

    /// The documented example, composed exactly as [`super::mcp`] would, under a settings
    /// DOCUMENT.
    ///
    /// The overlay occupies the position a deployment's own file does, which is the whole point: the
    /// setting has to arrive the way an operator writes it, or the read under test is the test's own
    /// argument. `catalogs:` names the example's `catalog`/`data` directories with absolute paths,
    /// so this test does not depend on the process's working directory the way the spawned
    /// `tests/mcp.rs` suite does.
    fn example_composition(overlay: &str) -> (std::sync::Arc<dyn sutura_app::surface::Surface>, sutura_config::Settings) {
        let catalogs_overlay = format!(
            "catalogs:\n  - name: model\n    kind: markdown\n    dir: {:?}\n    data_dir: {:?}\n    version: unversioned\n{overlay}",
            example().join("catalog"),
            example().join("data"),
        );
        let settings = sutura_config::Settings::load(
            &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(&catalogs_overlay),
        )
        .expect("the test settings load");
        let outbound = crate::sources::resolve_outbound_anchors(&settings).expect("no outbound trust is declared");
        let catalogs =
            crate::serve::catalog::open_catalog(settings.catalogs(), outbound.as_ref()).expect("the example catalog opens");
        let pinned = crate::serve::catalog::load(&catalogs).expect("the example catalog loads");
        // An exhaustive match into an `Option` rather than a refutable `let`: `clippy::unreachable`
        // is denied here, and a match is also what makes a third linked adapter a compile error in
        // this test the way it is in the command itself.
        let opened = match crate::sources::open_engine(
            &pinned,
            &sutura_config::SourceRegistry::default(),
            settings.runtime(),
            settings.server().request_timeout(),
            Some(&example().join("data")),
            None,
        )
        .expect("the example catalog opens with nothing declared")
        {
            crate::sources::Opened::Files(opened) => {
                Some(compose(&catalogs, opened, &settings).expect("the example bundle is fit to serve"))
            }
            #[cfg(feature = "bigquery")]
            crate::sources::Opened::BigQuery(_) => None,
            #[cfg(feature = "postgres")]
            crate::sources::Opened::Postgres(_) => None,
            #[cfg(feature = "clickhouse")]
            crate::sources::Opened::ClickHouse(_) => None,
            #[cfg(feature = "oracle")]
            crate::sources::Opened::Oracle(_) => None,
        }
        .expect("the example declares a files source");
        (opened, settings)
    }

    /// THE claim of issue #110, end to end: the `mcp` command's own composition serves the two tools
    /// over the protocol. The schema snapshots in `sutura-mcp` already pin the tool inputs; what was
    /// missing is that a composition root links the surface and something answers.
    #[test]
    fn the_mcp_composition_serves_every_tool_the_surface_declares() {
        let (service, settings) = example_composition("");
        let (service, prose, admission, reply, instructions): Served =
            mcp_service(service, &settings).expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Quoted);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(
                std::sync::Arc::new(super::ErasedService(service)),
                prose,
                admission,
                reply,
                instructions,
            )
            .await;

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

    /// `telekom/sutura#776`: a served agent actually receives the bundle's knowledge at
    /// `initialize`, rather than the fixed six-line sentence that named none of it.
    #[test]
    fn the_mcp_composition_serves_the_rendered_prompt_as_initialize_instructions() {
        let (service, settings) = example_composition("");
        let (service, prose, admission, reply, instructions): Served =
            mcp_service(service, &settings).expect("the example bundle is fit to serve");
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(
                std::sync::Arc::new(super::ErasedService(service)),
                prose,
                admission,
                reply,
                instructions,
            )
            .await;
            let told = client
                .peer_info()
                .and_then(|info| info.instructions.clone())
                .expect("this deployment's `initialize` result carries instructions");
            assert!(
                told.contains("churn_rate_over_the_half_year"),
                "the served instructions must carry the pinned bundle's own knowledge, got: {told}"
            );
            drop(client.cancel().await);
        });
    }

    /// `telekom/sutura#971`: a client that reads NOTHING off `initialize` still reaches the
    /// knowledge sections and the operator's own instructions, through `describe_catalog`'s
    /// `tools/call` result alone - the gateway shape the issue names, over the in-memory pipe.
    #[test]
    fn a_tools_only_client_reads_the_glossary_and_the_operators_instructions_through_describe_catalog() {
        let dir = std::env::temp_dir().join(format!("sutura-cli-mcp-instructions-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        let house_rules = dir.join("house-rules.md");
        std::fs::write(&house_rules, "Prefer the month grain.\n").expect("the operator file is writable");
        let overlay = format!("prompt:\n  instructions_file: {:?}\n", house_rules);
        let (service, settings) = example_composition(&overlay);
        let (service, prose, admission, reply, instructions): Served =
            mcp_service(service, &settings).expect("the example bundle is fit to serve");
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(
                std::sync::Arc::new(super::ErasedService(service)),
                prose,
                admission,
                reply,
                instructions,
            )
            .await;
            // Deliberately never read: a tools-only client's whole point is that it never asks for
            // `initialize`'s own `instructions` field, or a gateway in front of it never forwards
            // one. `describe_catalog` is called with no other tool having been called first.
            let result = client
                .call_tool(rmcp::model::CallToolRequestParams::new(
                    sutura_app::Capability::DescribeCatalog.id(),
                ))
                .await
                .expect("the catalog tool answers");
            let text = result
                .content
                .first()
                .and_then(rmcp::model::ContentBlock::as_text)
                .map(|block| block.text.clone())
                .expect("the catalog carries a text block");
            assert!(
                text.contains("churn_rate_over_the_half_year"),
                "the tool result must carry the pinned bundle's own knowledge: {text}"
            );
            assert!(
                text.contains("Prefer the month grain."),
                "the tool result must carry the operator's own instructions: {text}"
            );
            drop(client.cancel().await);
        });
        drop(std::fs::remove_dir_all(&dir));
    }

    /// `prompt.catalog_prose: omitted` reaches the served agent surface from THIS command's root.
    #[test]
    fn the_mcp_composition_honours_the_prose_setting_it_was_configured_with() {
        let (service, settings) = example_composition("prompt:\n  catalog_prose: omitted\n");
        let (service, prose, admission, reply, instructions): Served =
            mcp_service(service, &settings).expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Omitted);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(
                std::sync::Arc::new(super::ErasedService(service)),
                prose,
                admission,
                reply,
                instructions,
            )
            .await;
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
            assert!(!structured.contains("Where the customer is."), "{structured}");
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
    #[test]
    fn the_mcp_composition_bounds_execution_with_the_number_it_was_configured_with() {
        let (service, settings) = example_composition("");
        let (_service, _prose, admission, _reply, _instructions): Served =
            mcp_service(service, &settings).expect("the example bundle is fit to serve");
        assert_eq!(admission.bound(), settings.runtime().max_concurrent_queries().count());
        assert_eq!(admission.wait(), settings.runtime().admission_timeout().duration());

        let (service, settings) = example_composition("runtime:\n  max_concurrent_queries: 3\n  admission_timeout_seconds: 1\n");
        let (_service, _prose, admission, _reply, _instructions): Served =
            mcp_service(service, &settings).expect("the example bundle is fit to serve");
        assert_eq!(admission.bound(), 3, "the configured bound did not reach the surface");
        assert_eq!(admission.wait(), std::time::Duration::from_secs(1));
    }

    /// `server.request_timeout_seconds` reaches the served agent surface from THIS command's root.
    #[test]
    fn the_mcp_composition_bounds_the_reply_with_the_number_it_was_configured_with() {
        let (service, settings) = example_composition("");
        let (_service, _prose, _admission, reply, _instructions): Served =
            mcp_service(service, &settings).expect("the example bundle is fit to serve");
        assert_eq!(reply, settings.server().request_timeout());

        let (service, settings) = example_composition("server:\n  request_timeout_seconds: 7\n");
        let (_service, _prose, _admission, reply, _instructions): Served =
            mcp_service(service, &settings).expect("the example bundle is fit to serve");
        assert_eq!(reply.seconds(), 7, "the configured reply deadline did not reach the surface");
    }
}
