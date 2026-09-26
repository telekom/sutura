//! The `mcp` command: the agent surface, composed over the standard input and output of this
//! process.
//!
//! The composition root for [`sutura_mcp::serve_stdio`], and the point of issue #110: the agent
//! surface used to be reachable only from its own crate's tests. An agent client launches this
//! process and speaks the Model Context Protocol on its pipes.
//!
//! **Since issue #970 it reads `catalogs:` (and `sources:`) the way `crate::serve` does, over the
//! SAME opener - `crate::catalog::open_catalog`/`load`/`start_composed`** - instead of opening
//! only a directory argument. That is the point of #970: a shared-service-user deployment can now
//! switch its catalog by configuration on the stdio agent surface. The directory arguments are
//! gone, because `Settings::load` always yields a declared markdown catalog from `defaults.yaml`,
//! so there is no settings-less invocation left for them to name - a parallel argument to the
//! declared catalog is the legacy shim #970 removes, not a path that survives. `catalog.kind: okf`
//! is an unconditional dependency of this build, so it always opens; `openmetadata` opens behind
//! this binary's `openmetadata` feature and `rdbms` has no reader on any build - both are refused
//! by name with the same message `sutura serve` gives, because both roots dispatch the same
//! `crate::catalog::open_catalog`.
//!
//! Kept in its own module rather than inlined into `commands.rs` because it is a composition of its
//! own - the driving port over a pipe, with an async runtime the other commands do not want - and
//! because inlining it would push `commands.rs` over the 1000-line gate. `main.rs`'s command table
//! names it directly.

use std::process::ExitCode;

use sutura_app::prompt::CatalogProse;
use sutura_app::surface::Surface as _;
use sutura_config::RequestTimeout;
use sutura_runtime::Admission;

use crate::commands::{Composed, agent_instructions, catalog_prose, render, report};
use crate::sources::{Opened, OpenedWith, configured, open_engine};
// The `BigQuery` arm's half of the same question, and reached only from that arm - so the import is
// gated for the reason `sources::bigquery`'s are: `dead_code` is `deny` here, and this crate's
// default build links no BigQuery adapter.
#[cfg(feature = "bigquery")]
use crate::sources::refuse_absent_tables;

/// What serving the surface amounts to: the service, how catalog descriptions are treated, the two
/// bounds - how many questions may be executing at once, and how long a peer waits for one - the
/// rendered document a peer's `initialize` result carries, and the operator's own text the catalog
/// tool carries through `tools/call`.
///
/// Named because even with [`Composed`] aliased, the tuple stays over `clippy::type_complexity`
/// once the alias is expanded.
type Served<W> = (
    Composed<W>,
    CatalogProse,
    Admission,
    RequestTimeout,
    std::sync::Arc<str>,
    Option<std::sync::Arc<str>>,
);

/// `mcp`: serve the agent surface over standard input and output, reading `catalogs:`/`sources:`
/// the way [`crate::serve`] does.
/// # Errors
///
/// Whatever [`crate::catalog::open_catalog`] refuses for the declared catalog kind a build did not
/// link (the same message `sutura serve` gives, since both dispatch it), whatever
/// [`crate::catalog::load`] refuses while reading the declared directories, and whatever
/// [`crate::sources::open_engine`] refuses for the declared `sources:` tree. Serving never begins
/// before all three have succeeded, so a refusal is a message on standard error and a non-zero
/// exit, never a surface a peer has been told it can ask questions through.
pub(crate) fn mcp() -> ExitCode {
    report((|| {
        let settings = configured()?;
        // Read ONCE - `security.outbound`, `github.com/telekom/sutura#125` - and shared with every
        // `WireAgent` this surface's `bigquery` arm builds, and with the catalog opener below.
        let outbound = crate::serve::outbound::resolve(&settings)?;
        // **The declared catalog, through the ONE opener `sutura serve` uses.** A deployment that
        // names `catalog.kind: openmetadata` or `rdbms` without this binary linking the reader
        // (`openmetadata` is behind the `openmetadata` feature; `rdbms` has no reader on any build)
        // is refused here with the same operator-facing message `serve` gives - both roots call
        // `crate::catalog::open_catalog`, so the refusal cannot drift.
        let catalogs = crate::catalog::open_catalog(settings.catalogs(), outbound.as_ref())?;
        // The bundle the engine must attach for. `crate::catalog::load` composes the opened
        // catalogs; `open_engine` then reads the `sources:` tree. A no-source deployment has no
        // declared data system, which `open_engine` refuses by name.
        let pinned = crate::catalog::load(&catalogs)?;
        // The exhaustive match is here for the reason `crate::commands::query`'s is: the service is
        // monomorphised per adapter, so a third linked adapter is a compile error at this line.
        //
        // `data` is gone as an argument: this command reads `catalogs:`/`sources:`, so there is no
        // directory for a settings-less invocation to name the way `query` must. The engine is
        // opened from the declared sources alone, exactly as `sutura serve` opens it.
        match open_engine(
            &pinned,
            settings.sources(),
            settings.runtime(),
            settings.server().request_timeout(),
            None,
            outbound.as_ref(),
        )? {
            Opened::Files(opened) => serve(&catalogs, opened, &settings),
            #[cfg(feature = "bigquery")]
            Opened::BigQuery(opened) => {
                // **The pre-flight, and this line is where its ORDER is decided** - the same order
                // `crate::serve`'s root keeps, for the same reason. It runs after `open_engine`,
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
                // the window between this root's TWO loads: `crate::catalog::load` above is the
                // first, this pre-flight reads that bundle, and the second load inside
                // `crate::catalog::start_composed` (via `LocalService::start_composed`) is the
                // second. So a model added to the catalog directory between the two is caught on a
                // `files` source and caught by nothing on a `bigquery` one - the same gap
                // `crate::serve`'s root states at its own call site, open here for the same reason.
                // Closing it is an architecture decision rather than a call-site move:
                // `LocalService` exposes no accessor for the engines it was handed, so there is
                // nothing to re-ask once the second load has happened.
                refuse_absent_tables(&pinned, &opened.engines)?;
                serve(&catalogs, opened, &settings)
            }
            #[cfg(feature = "postgres")]
            Opened::Postgres(opened) => {
                // A `postgres` source attaches nothing and reports no table inventory, so
                // `refuse_absent_tables` has nothing to add - the same reasoning `crate::serve`'s own
                // arm carries. A mistyped `table:` is caught on the first question against it.
                serve(&catalogs, opened, &settings)
            }
            #[cfg(feature = "clickhouse")]
            Opened::ClickHouse(opened) => {
                // A `clickhouse` source attaches nothing and reports no table inventory, for the
                // `postgres` arm's reason exactly - `ClickHouseWarehouse` takes the port's default
                // `preflight`, so `refuse_absent_tables` would have nothing to add.
                serve(&catalogs, opened, &settings)
            }
            #[cfg(feature = "oracle")]
            Opened::Oracle(opened) => {
                // The `clickhouse` arm's reason: `OracleWarehouse` takes the port's default `preflight`.
                serve(&catalogs, opened, &settings)
            }
        }
    })())
}

/// Starts the service over one configured catalog and serves the agent surface on this process's
/// pipes.
/// Generic in the adapter, so the match arms above share every line after them - including the
/// runtime, the startup notice and the bounded teardown, none of which is a per-adapter decision.
fn serve<W>(
    catalogs: &crate::catalog::OpenedCatalogs,
    opened: OpenedWith<W>,
    settings: &sutura_config::Settings,
) -> Result<(), String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    let (service, prose, admission, reply, instructions, operator_instructions) = mcp_service(catalogs, opened, settings)?;
    // The limit printed beside the mode, the way `banner::announce_token_class` prints the token
    // class: a pipe has no header a token could arrive in, so this surface grants every
    // capability to whoever can reach the process. Stated at startup, not left as a default
    // nobody declared. Standard error, which is the log channel, so the MCP stream on stdout
    // stays a pure protocol.
    //
    // Both bounds are printed with it, and for the same reason: they are the numbers an operator
    // configured, and the numbers a shed call and a given-up wait are about, so they belong where
    // the posture is stated rather than inside a semaphore and a timeout nobody can see. The reply
    // deadline in particular did not exist until `telekom/sutura#339`, so a deployment reading this
    // line is reading the difference.
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
    let service = std::sync::Arc::new(service);
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
            std::sync::Arc::clone(&service),
            permitted,
            prose,
            settings.prompt().list_physical_schema(),
            admission,
            reply,
            instructions,
            operator_instructions,
        ))
        .map_err(|e| render(&e));
    // Bound the teardown the way `crate::serve`'s `stop` does: dropping a runtime with a
    // question still answering would wait for it however long it takes, and nothing here can
    // cancel one. This gives the pool a moment to finish and then exits on our own terms.
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    drop(service);
    served
}

/// The agent surface this command serves: the `query` composition behind the driving port.
///
/// **Literally the same composition, not a parallel one.** [`crate::catalog::start_composed`] is
/// the place this binary builds a service from the declared catalogs - the constructor that takes
/// an audit sink and re-runs every anchor, plus the unattached-table check that closes the gap its
/// second catalog load leaves. Issue #970 is what moved that one constructor to the crate root so
/// `sutura mcp` uses it the way `sutura serve` does. `sutura query` still goes through
/// `commands::started` (`LocalService::start`, not `start_composed`) - a third, simpler
/// composition over a single directory argument rather than declared `catalogs:`, unchanged by
/// this issue.
///
/// A locally launched process installs no subscriber either way, so those records go nowhere for the
/// whole session - the honest default rather than a claim that a record was kept when none was, and
/// the limit the invariants row states. The sink is only the writer, so any composition that does
/// install a subscriber must send it to standard error: on this transport standard output is the
/// protocol channel, which is why the startup notice is an `eprintln!`.
///
/// What is left here is the transport's own decisions, and there are four - how catalog
/// descriptions are treated, how many questions may be executing at once, how long a peer waits
/// for one of them, and the rendered prompt a peer's `initialize` result carries. **It takes the
/// whole `Settings` rather than the values it needs**, so all four are READ here and not handed
/// in; `#266`'s `H1` is what a caller-supplied setting costs.
fn mcp_service<W>(
    catalogs: &crate::catalog::OpenedCatalogs,
    opened: OpenedWith<W>,
    settings: &sutura_config::Settings,
) -> Result<Served<W>, String>
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
    //
    // **The service is built by [`crate::catalog::start_composed`], the ONE
    // `LocalService::start_composed` match `sutura serve` shares** (issue #970 moved it to the
    // crate root). It takes the declared catalogs, the engines this root opened and the broker the
    // declaration minted, and re-loads every catalog and re-runs every anchor - the same contract
    // `commands::started` stated. The unattached-table check is what closes the gap its second
    // catalog load leaves, exactly as it does there and in `crate::serve`.
    let service = crate::catalog::start_composed(catalogs, opened.engines, opened.broker, settings)?;
    if let Some(attached) = opened.attached {
        sutura_app::preflight::refuse_unattached(&sutura_app::preflight::served_tables(service.definitions()), &attached)
            .map_err(|changed| changed.to_string())?;
    }
    // Read off the SERVICE rather than a second catalog load: `service.definitions()` is the exact
    // bundle `Surface::answer` computes against, so what this composes the prompt over cannot drift
    // from what it certifies over - `telekom/sutura#776`.
    //
    // One read, two uses: `agent_instructions` both renders the prompt and returns the operator's
    // raw text it read to do so, so the surface gets the SAME text the prompt folded in - never a
    // second read of the same path that could disagree with the first.
    let (instructions, operator_instructions) = agent_instructions(service.definitions(), settings, true)?;
    let operator_instructions = operator_instructions.map(std::sync::Arc::from);
    Ok((
        service,
        catalog_prose(settings.prompt().catalog_prose()),
        Admission::from_settings(settings.runtime()),
        // **The peer's wait, and `telekom/sutura#339` is that it had no bound at all.** The same key
        // `open_engine` above already divides into a `bigquery` job deadline, so before this line
        // the engine on this transport gave up against a number the PEER was not bounded by. Read
        // here for the reason the other two are, and the agent surface applies it where it awaits
        // the port - it has no layer to hang it on.
        settings.server().request_timeout(),
        std::sync::Arc::from(instructions),
        operator_instructions,
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
    /// The prose treatment and the two bounds are the caller's, because they are what this command
    /// resolves from the settings and a test that hardcoded any of them could not tell two settings
    /// documents apart.
    async fn connected<S>(
        service: std::sync::Arc<S>,
        prose: sutura_app::prompt::CatalogProse,
        admission: sutura_runtime::Admission,
        reply: sutura_config::RequestTimeout,
        instructions: std::sync::Arc<str>,
        operator_instructions: Option<std::sync::Arc<str>>,
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
                operator_instructions,
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

    /// The documented example, openable as a deployment declares it: `catalogs:` and `sources:`
    /// in a settings DOCUMENT, opened through the SAME `crate::catalog` opener `sutura mcp` and
    /// `sutura serve` share.
    ///
    /// The settings arrive as a document the way an operator writes them, which is the whole point:
    /// a test that handed `mcp_service` the catalog values it wanted to see would pass with the read
    /// back to a constant one frame up. The same construction `sutura_http::harness::settings` uses,
    /// for the same reason.
    ///
    /// Extracted because several tests compose it and the composition is the thing under test in
    /// each: a second copy would be a second answer to *what does this command build*.
    ///
    /// `overlay` is appended after the catalog and source declarations, so a test that varies a
    /// bound (`runtime:`, `server:`, `prompt:`) layers it on top.
    fn example_composition(
        overlay: &str,
    ) -> (
        crate::catalog::OpenedCatalogs,
        crate::sources::OpenedWith<sutura_exec_datafusion::DataFusionWarehouse>,
        sutura_config::Settings,
    ) {
        let example = example();
        let settings = sutura_config::Settings::load(
            &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(format!(
                "security:\n  identity: \"single-user\"\n  single_user_because: \"a unit test reads its \
                 own fixture files as one identity\"\n\
                 catalogs:\n  \
                   - name: \"model\"\n    \
                     kind: \"markdown\"\n    \
                     dir: \"{}\"\n    \
                     data_dir: \"{}\"\n    \
                     version: \"local-working-tree\"\n\
                 sources:\n  \
                   local:\n    \
                     kind: \"files\"\n    \
                     data_dir: \"{}\"\n    \
                     posture: \"shared-service-user\"\n\
                 {overlay}",
                example.join("catalog").display(),
                example.join("data").display(),
                example.join("data").display(),
            )),
        )
        .expect("the test settings load");
        // **Open the declared catalog the way the command does** - `crate::catalog::open_catalog`,
        // which is what a `markdown`-kind deployment of every feature combination opens here. The
        // load composes the bundle the engine must attach for.
        let catalogs = crate::catalog::open_catalog(settings.catalogs(), None).expect("the example markdown catalog opens");
        let pinned = crate::catalog::load(&catalogs).expect("the example catalog loads");
        // An exhaustive match into an `Option` rather than a refutable `let`: `clippy::unreachable`
        // is denied here, and a match is also what makes a third linked adapter a compile error in
        // this test the way it is in the command itself. The engine is opened from the declared
        // `sources:` alone - no `data` directory, which is the point of #970.
        let opened = match crate::sources::open_engine(
            &pinned,
            settings.sources(),
            settings.runtime(),
            settings.server().request_timeout(),
            None,
            None,
        )
        .expect("the example catalog opens its declared source")
        {
            crate::sources::Opened::Files(opened) => Some(opened),
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
        (catalogs, opened, settings)
    }

    /// THE claim of issue #110, end to end: the `mcp` command's own composition serves the two tools
    /// over the protocol. The schema snapshots in `sutura-mcp` already pin the tool inputs; what was
    /// missing is that a composition root links the surface and something answers.
    ///
    /// A plain `test` rather than `#[tokio::test]`, deliberately. `mcp_service` opens the engine and
    /// re-runs its anchors, and the engine drives its OWN runtime with `block_on` - which panics when
    /// called from within a runtime, exactly as `crate::serve`'s `run` documents. So the service is
    /// built before any runtime exists, and the runtime is entered only for the handshake.
    ///
    /// The service is kept in this scope, NOT created inside the async block, so the caller's own
    /// handle releases it after its runtime is gone - the same shape the `mcp` command's shutdown
    /// takes, where a main-thread handle frees the engine once blocking work has settled.
    #[test]
    fn the_mcp_composition_serves_every_tool_the_surface_declares() {
        let (catalog, opened, settings) = example_composition("");
        let (service, prose, admission, reply, instructions, operator_instructions) =
            mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Quoted);
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(
                std::sync::Arc::clone(&service),
                prose,
                admission,
                reply,
                instructions,
                operator_instructions,
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
                            "metrics": ["recurring_revenue"],
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
    ///
    /// Reads the SDK's own `peer_info()` off the connected client - the exact
    /// `initialize.instructions` field a real client sees - and asserts it carries a name the
    /// example catalog's own knowledge declares: `churn_rate_over_the_half_year` is a worked
    /// example's `name:` frontmatter, rendered verbatim as a heading by
    /// `sutura_app::prompt::render`. A fixed const could never contain it; only the rendered
    /// document, over THIS bundle, can.
    #[test]
    fn the_mcp_composition_serves_the_rendered_prompt_as_initialize_instructions() {
        let (catalog, opened, settings) = example_composition("");
        let (service, prose, admission, reply, instructions, operator_instructions) =
            mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(
                std::sync::Arc::clone(&service),
                prose,
                admission,
                reply,
                instructions,
                operator_instructions,
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
        let (service, prose, admission, reply, instructions, operator_instructions) =
            mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Omitted);
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(
                std::sync::Arc::clone(&service),
                prose,
                admission,
                reply,
                instructions,
                operator_instructions,
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
        let (service, _prose, admission, _reply, _instructions, _operator_instructions) =
            mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(admission.bound(), settings.runtime().max_concurrent_queries().count());
        assert_eq!(admission.wait(), settings.runtime().admission_timeout().duration());
        drop(service);

        let (catalog, opened, settings) =
            example_composition("runtime:\n  max_concurrent_queries: 3\n  admission_timeout_seconds: 1\n");
        let (service, _prose, admission, _reply, _instructions, _operator_instructions) =
            mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(admission.bound(), 3, "the configured bound did not reach the surface");
        assert_eq!(admission.wait(), std::time::Duration::from_secs(1));
        drop(service);
    }

    /// `server.request_timeout_seconds` reaches the served agent surface from THIS command's root.
    ///
    /// **The composition half of `telekom/sutura#339`**, and the sibling of the admission test above
    /// it in every respect: the bound no test in `sutura-mcp` can reach is that a root builds one at
    /// all, and this root built NONE - `serve_stdio` took no deadline, so a peer that got an
    /// execution slot waited for as long as the data system took while the HTTP surface answered
    /// `408` off this very key.
    ///
    /// Two documents rather than one, because that is what tells a READ from a constant: the first
    /// is the settings tree as an operator inherits it and is compared against what those settings
    /// say, and the second names a number no default carries. A root that returned a constant passes
    /// the first and fails the second.
    ///
    /// The same number `open_engine` divides into a `bigquery` job deadline, which is the reason it
    /// is this key and not one of the transport's own - stated at `sutura_mcp::server`.
    ///
    /// No runtime here, deliberately: this asserts what the composition RESOLVES, and the effect of
    /// that number on a question in flight is `sutura_mcp::server`'s own suite, over a fake that can
    /// be held.
    #[test]
    fn the_mcp_composition_bounds_the_reply_with_the_number_it_was_configured_with() {
        let (catalog, opened, settings) = example_composition("");
        let (service, _prose, _admission, reply, _instructions, _operator_instructions) =
            mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(reply, settings.server().request_timeout());
        drop(service);

        let (catalog, opened, settings) = example_composition("server:\n  request_timeout_seconds: 7\n");
        let (service, _prose, _admission, reply, _instructions, _operator_instructions) =
            mcp_service(&catalog, opened, &settings).expect("the example bundle is fit to serve");
        assert_eq!(reply.seconds(), 7, "the configured reply deadline did not reach the surface");
        drop(service);
    }

    /// An `okf`-kind deployment, composed the way this command does: `crate::catalog::open_catalog`
    /// over a directory of Table Schema descriptors, `crate::sources::open_engine` over the
    /// `files` source it names - issue #970's own acceptance for this kind. `dir` is a scratch
    /// directory the CALLER owns and clears - the same split `crate::catalog::tests::scratch` and
    /// `served/okf.rs`'s `CatalogDir` use - so this fn's return stays the three-tuple
    /// `example_composition`'s own is, rather than a fourth element only to hand the path back.
    fn okf_composition(
        dir: &Path,
    ) -> (
        crate::catalog::OpenedCatalogs,
        crate::sources::OpenedWith<sutura_exec_datafusion::DataFusionWarehouse>,
        sutura_config::Settings,
    ) {
        std::fs::write(
            dir.join("subscriptions.yaml"),
            "description: Subscriptions, one row per active plan.\nfields:\n  - name: subscription_id\n  \
             - name: amount_cents\n",
        )
        .expect("the descriptor is writable");
        std::fs::write(dir.join("subscriptions.csv"), "subscription_id,amount_cents\nS1,1999\n").expect("the CSV is writable");
        let settings = sutura_config::Settings::load(
            &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(format!(
                "security:\n  identity: \"single-user\"\n  single_user_because: \"a unit test reads its \
                 own fixture files as one identity\"\n\
                 catalogs:\n  \
                   - name: \"physical\"\n    \
                     kind: \"okf\"\n    \
                     dir: \"{d}\"\n    \
                     data_dir: \"{d}\"\n    \
                     version: \"local-working-tree\"\n\
                 sources:\n  \
                   physical:\n    \
                     kind: \"files\"\n    \
                     data_dir: \"{d}\"\n    \
                     posture: \"shared-service-user\"\n",
                d = dir.display(),
            )),
        )
        .expect("the okf test settings load");
        let catalogs = crate::catalog::open_catalog(settings.catalogs(), None).expect("the okf catalog opens");
        let pinned = crate::catalog::load(&catalogs).expect("the okf catalog loads");
        let opened = match crate::sources::open_engine(
            &pinned,
            settings.sources(),
            settings.runtime(),
            settings.server().request_timeout(),
            None,
            None,
        )
        .expect("the okf deployment opens its declared source")
        {
            crate::sources::Opened::Files(opened) => Some(opened),
            #[cfg(feature = "bigquery")]
            crate::sources::Opened::BigQuery(_) => None,
            #[cfg(feature = "postgres")]
            crate::sources::Opened::Postgres(_) => None,
            #[cfg(feature = "clickhouse")]
            crate::sources::Opened::ClickHouse(_) => None,
            #[cfg(feature = "oracle")]
            crate::sources::Opened::Oracle(_) => None,
        }
        .expect("the okf deployment declares a files source");
        (catalogs, opened, settings)
    }

    /// `sutura mcp` opens a declared `catalog.kind: okf` the same way `sutura serve` does, and
    /// serves its tools over the protocol - the acceptance issue #970 states for this kind. `okf`
    /// declares no measure (its own module header), so this asserts `tools/list` carries every
    /// capability and `describe_catalog` answers with zero metrics rather than a refusal.
    #[test]
    fn the_mcp_composition_opens_a_declared_okf_catalog_and_serves_its_tools() {
        let dir = std::env::temp_dir().join(format!("sutura-cli-mcp-okf-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("the okf scratch directory is creatable");
        let (catalog, opened, settings) = okf_composition(&dir);
        let (service, prose, admission, reply, instructions, operator_instructions) =
            mcp_service(&catalog, opened, &settings).expect("the okf bundle is fit to serve");
        let service = std::sync::Arc::new(service);
        let runtime = tokio::runtime::Runtime::new().expect("a runtime starts");

        runtime.block_on(async {
            let client = connected(
                std::sync::Arc::clone(&service),
                prose,
                admission,
                reply,
                instructions,
                operator_instructions,
            )
            .await;
            let tools = client.list_all_tools().await.expect("tools/list answers");
            let advertised: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
            let expected: Vec<&str> = sutura_app::Capability::every().map(sutura_app::Capability::id).collect();
            assert_eq!(advertised, expected, "{advertised:?}");

            let result = client
                .call_tool(rmcp::model::CallToolRequestParams::new(
                    sutura_app::Capability::DescribeCatalog.id(),
                ))
                .await
                .expect("describe_catalog is not a protocol error");
            assert_ne!(result.is_error, Some(true), "{result:?}");
            let structured = result
                .structured_content
                .as_ref()
                .expect("an okf listing carries structured content");
            assert_eq!(
                structured["metrics"].as_array().map(Vec::len),
                Some(0),
                "an okf catalog declares no measure, so a served listing must carry none: {structured}"
            );
            drop(client.cancel().await);
        });
        drop(std::fs::remove_dir_all(&dir));
    }
}
