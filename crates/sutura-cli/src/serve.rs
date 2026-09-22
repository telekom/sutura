//! `sutura serve`: the HTTP surface's composition root, and nothing else.
//!
//! # The order, and why it is this order
//!
//! 1. **The environment.** It decides the log format, which configuration file is layered and how
//!    strict the startup refusals are, so it has to be known before any of those.
//! 2. **The banner**, to standard output, before any subscriber exists. It answers "what is this and
//!    which build" for somebody looking at a terminal or the top of a container log.
//! 3. **The configuration.** A refusal here is a process that does not start - which is the whole
//!    posture: an unsafe deployment fails loudly rather than serving behind a warning nobody read.
//! 4. **The log**, then the panic hook. In that order, so a panic during the rest of startup is
//!    traced rather than being the one that gets lost.
//! 5. **What was resolved**, including the line saying there is no per-caller identity.
//! 6. **The adapters**, then the service. The catalog is loaded through its port and every anchor is
//!    re-executed against the data system; a bundle whose anchors do not hold starts nothing.
//! 7. **The router**, the signal listener, and then serving.
//!
//! # Why a subcommand rather than a second binary
//!
//! It used to be a second binary, `sutura-serve`, on the argument that a shipped artifact holds
//! one executable and folding the HTTP surface into `sutura` would put a multi-threaded runtime,
//! an I/O driver, a web framework and a browser asset bundle into every invocation of
//! `sutura compile`. `github.com/telekom/sutura#685` step 2 folded it in anyway: the owner
//! ruling was that a deployment installing `sutura` should never need a second binary's name to
//! serve one, and the cost that argument priced - every `sutura` invocation now links `axum` and
//! the HTTP surface's runtime dependencies, whether or not `serve` is the command run - is paid
//! once, in the closure a `cargo build` produces, not per invocation: `doctor`, `compile` and
//! `query` still run one process each and start no listener.
//!
//! **This surface IS published**, which is unchanged by the fold: `nix/shipped.nix` lists one
//! binary now rather than two; `checks.one-binary` and `checks.shipped-features` are what assert
//! what the artefact holds - one executable of the expected name, and no `tls`, `bigquery`,
//! `postgres` or `datahub`, all being default-off features that cost a rustls closure across two
//! musl triples and refuse at startup by name.
//!
//! # `Result<_, String>` below the surface
//!
//! Used freely, and the boundary gate exempts a binary on purpose: the audience for these messages
//! is a person reading standard error, not code matching on a variant. Every typed error from a
//! library crate is flattened with its whole `#[source]` chain on the way out, because the outermost
//! message is the one that says least.

use std::collections::BTreeSet;
use std::sync::Arc;

use sutura_app::surface::Surface;
use sutura_config::{Settings, Sources, StaticCredentialBroker, TlsMaterial};
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_exec_datafusion::DataFusionWarehouse;
use sutura_http::{LocalService, ServiceState};
use sutura_runtime::{Admission, Shutdown, TracingAuditSink, banner, shutdown, telemetry};

/// How a declared `catalogs:` becomes the catalog this build serves.
mod catalog;

/// The refusals this root makes by reading the bundle. `main.rs` keeps the ORDER they run in.
mod boot;

/// The broker a `bigquery` deployment is served under. `cfg`-gated like the adapter: a build that
/// links none of `sutura-exec-bigquery` has no `DeclaredPrincipalBroker` to attach.
#[cfg(feature = "bigquery")]
mod broker;

/// One kind's open-and-build pair, so the composition root keeps the dispatch and the refusals.
mod bigquery;

/// The same, for the `Postgres` connection this root opens and secures.
mod postgres;

/// The closed enum over the shipped warehouse KINDS - `github.com/telekom/sutura#112` - so this
/// root can hold more than one at once. Unconditional, like [`OpenedSources`] itself: a build with
/// neither optional adapter feature still has to REFUSE a mixed catalog by naming the missing
/// feature, not by never reaching that code.
mod kind;

/// The agent-surface transport, under the `agent` feature. `cfg`-gated like `broker`: a build that
/// links none of `sutura_mcp` has no `sutura_mcp::http::service` to attach.
#[cfg(feature = "agent")]
mod agent;

/// `security.outbound`, resolved once at boot - `github.com/telekom/sutura#125`/`#911`.
mod outbound;

/// The alias the example deployment and this crate's tests use for their one source.
///
/// **No longer a check, and that is the change worth reading.** It used to be the only source name
/// this build would open: a catalog naming anything else got no engine rather than one wearing that
/// data system's name over local files. That argument held while the catalog was the only signal, and
/// it stops holding once the DEPLOYMENT declares each source - an operator who writes
/// `sources.warehouse.kind: files` with a directory beside it has said what the name comparison was
/// standing in for, and under the old rule that legitimate deployment could not be served at all.
///
/// What carries the fact instead is `sutura_config::SourceKind`: the vocabulary of kinds is the
/// vocabulary of adapters, an unknown word is a parse refusal, and which adapter opens a declared kind
/// is an exhaustive match in [`build_engine`] - so a second kind is a compile error there rather than
/// an arm that falls through.
///
/// `#[cfg(test)]`, which is the honest consequence: nothing in the serving path reads it any more, and
/// `dead_code` is `deny` here - so leaving it compiled would have been a constant that looks like a
/// rule and is a string.
#[cfg(test)]
const ENGINE_SOURCE: &str = "local";

/// Startup, synchronously, and then one `block_on` for the server.
///
/// **`#[tokio::main]` is deliberately NOT used here, and this was found by running it rather than
/// by reading.** The in-process engine holds its own current-thread runtime and `block_on`s it for
/// every attach and every statement. `Runtime::block_on` panics when the calling thread is already
/// inside a runtime, so with `#[tokio::main]` the whole of this function was inside one and
/// registering the first CSV aborted the process before it ever listened:
///
/// ```text
/// Cannot start a runtime from within a runtime. This happens because a function (like `block_on`)
/// attempted to block the current thread while the thread is being used to drive asynchronous tasks.
/// ```
///
/// So the shape below is not a style choice: everything that touches a port runs with no runtime
/// entered, and the runtime is built afterwards, for serving and nothing else. The request path has
/// the same constraint and answers it differently - see the query handler, which moves the call
/// onto the blocking pool.
///
/// **`pub(crate)`, called from [`crate::COMMANDS`]'s `serve` entry.** Takes no arguments: this
/// command reads its whole configuration from the settings tree, the same posture
/// `handled_immediately`/`usage` used to enforce for the standalone binary's `--version`/`--help`,
/// dropped at the fold because `crate::vet` and `crate::usage` already answer both uniformly for
/// every command in the table, including this one. What that trades away is documented in
/// `docs/serving.md` rather than repeated here: the settings tree's layering order, its
/// environment variables and the identity caveat were all in `sutura-serve --help`'s own text and
/// are in that page's, in more detail and kept in step with the settings crate rather than with
/// this file's own copy.
pub(crate) fn run() -> Result<(), String> {
    // 1. The environment.
    let environment = sutura_config::environment_from_process().map_err(flatten)?;

    // 2. The banner, before any subscriber.
    banner::print(env!("CARGO_PKG_VERSION"), environment);

    // 3. The configuration. A posture refusal lands here.
    let settings = Settings::load(&Sources::from_process_environment(
        environment,
        sutura_config::config_dir_from_process(),
    ))
    .map_err(flatten)?;

    // A deployment that asked for the agent surface on a build without the transport must refuse
    // rather than silently serve no `/mcp` - the same "configured for a build it does not have"
    // refusal `serve_as_configured` gives for TLS. `agent_refused_if_enabled` is the feature-off
    // twin of `agent::mount`: under the `agent` feature the same switch just builds the mount; on
    // the default build it is this refusal. `check-default-feature-tests` runs the cell that holds
    // it.
    #[cfg(not(feature = "agent"))]
    agent_refused_if_enabled(&settings)?;

    // 4. The log, then the panic hook.
    telemetry::install(settings.telemetry()).map_err(flatten)?;
    sutura_runtime::install_panic_hook();

    // 5. What was resolved, and what this service does not do.
    banner::announce(&settings);

    // `security.outbound.transport_anchors`, before any adapter is opened - the same order leg 1's
    // key set is read in, for the same reason: an unreadable declaration stops the process rather
    // than becoming a deployment that answers every question while its startup log says it verifies
    // a trust set nobody can name.
    let outbound = outbound::resolve(&settings)?;

    // 6. The adapters, then the service. Both ports are named exactly here.
    // The ONE boot-time `outbound` value flows to BOTH the catalog adapter and the source wire: a
    // deployment that declares `security.outbound.transport_anchors` verifies its `datahub` catalog
    // reader against the same CA set its `bigquery` wire is verified against - never a second read
    // of the bundle (`outbound::resolve` resolved it once, above).
    let catalogs = catalog::open_catalog(settings.catalogs(), outbound.as_ref())?;
    let pinned = catalog::load(&catalogs)?;
    // The `sources:` tree rather than `catalog.data_dir`: a deployment declares each data system, its
    // location and which identity a query reaches it as, and the engine is opened per declaration.
    // `catalog.data_dir` stays what it always was - the catalog's own directory - and is no longer
    // where a source's files are found.
    let opened = open_engine(
        &pinned,
        settings.sources(),
        settings.runtime(),
        settings.server().request_timeout(),
        outbound.as_ref(),
    )?;
    // `LocalService::start_composed` loads the declared catalogs a SECOND time - and composes them -
    // rather than being handed the bundle above, and that is deliberate: the bundle it validates has
    // to be the bundle it serves, and the only way to guarantee that is for the same call to do
    // both. The load above exists so the engine can be opened for the sources the catalog actually
    // names, which has to happen first. For N catalogs the assembler is what makes the two bundles
    // the same value rather than each source's own.
    //
    // The audit sink is named here too, and it is the third port this root attaches. A deployment
    // that wants records somewhere else replaces this one argument; a deployment that attaches
    // nothing gets the structured writer over the subscriber installed at step 4, which is the sink
    // this crate can promise exists. What that log pipeline retains is the deployment's - sutura
    // writes a record per outcome and keeps nothing.
    // The credential broker, which is the fourth port and the one that decides what a question
    // executes as. Which broker this build attaches is decided per ARM below, because an
    // impersonating source can only be served by a broker that EXCHANGES a subject's credential, and
    // only a `bigquery` build links one:
    //
    // - A deployment with no impersonating source (the `files` arm, and a `bigquery` arm with none)
    //   is served under `sutura_config::StaticCredentialBroker`, which reads the `sources:` tree this
    //   root already parsed: every source declared `shared-service-user` is served under the identity
    //   this process holds, and a source the broker holds nothing for is refused as
    //   `credential_unavailable` rather than answered as this process.
    // - A `bigquery` deployment with an impersonating source is served under
    //   `sutura_exec_bigquery::DeclaredPrincipalBroker`, which holds BOTH shapes - a declared
    //   witness for shared sources and, for an impersonating one, the asking subject's own verified
    //   assertion for the driver to federate - because one plan may read one of each and the broker
    //   is per answer, not per source. **The EXCHANGING broker this line used to name is deleted**
    //   (`docs/adr/0018`, eighth amendment): its HTTP hops went with the `wire` transport, leaving a
    //   type no root could reach whose every exchange was a test fake.
    //
    // **In every arm, a subject with no credential at a source is refused as `credential_unavailable`
    // rather than answered under the deployment's own identity** - the fallback the port exists to
    // make unrepresentable.
    let working_set_ceiling_bytes = settings.runtime().working_set().bytes().get() as u64;
    let spend_budget = settings.spend_budget();
    let row_ceiling = settings.row_ceiling();
    // **One `Arc<dyn Surface>` out of up to four adapter shapes, and the erasure is where it always
    // was.** `sutura_app::Warehouses<W>` is generic in ONE adapter, so a single-kind arm still
    // monomorphises `started` over its own concrete type - and `ServiceState` takes
    // `Arc<dyn Surface>`, so every shape meets one line later either way. `telekom/sutura#112`
    // added the fourth: `kind::AnyWarehouse` IS the closed enum `sutura_app::warehouses` names as
    // the remedy for a heterogeneous set, and it is `W` for the `Mixed` arm alone - the other three
    // arms stay exactly as generic-free as this comment used to claim of all of them.
    let (service, attached) = match opened {
        OpenedSources::Files(files) => {
            let broker = StaticCredentialBroker::from_registry(settings.sources());
            (
                started(
                    &catalogs,
                    files.engines,
                    broker,
                    working_set_ceiling_bytes,
                    spend_budget,
                    row_ceiling,
                )?,
                Some(files.attached),
            )
        }
        #[cfg(feature = "bigquery")]
        OpenedSources::BigQuery(engines) => {
            // **The pre-flight, and this line is where its ORDER is decided.** It runs after
            // `open_engine` - which read the credential for every declared source, so an operator
            // whose credential file is wrong is told about the credential file and not about a table
            // they would then go and not fix - and before the listener opens, several statements
            // below.
            //
            // This comment used to add *and only `open_engine` produces a registry, which is what
            // makes the first half of that order a type rather than a convention*, and review
            // disproved it twice: `Warehouses::of` and `::and` are both `pub`, and this very file
            // calls `of` further down, in `open_bigquery`. **And the replacement claim - that the
            // `BigQuerySource` alias's `BigQueryWire<Credential>` transport held it, because
            // `Credential::read` was its one public constructor - died with the `wire` half: there
            // is no credential to read, and `AdbcBigQuery::new` takes a `DriverLocation`. So the
            // first half of the order is held by NOTHING today.** What `open_bigquery` still reads
            // at boot is the driver this artefact carries (or the one a source build mounted) and
            // the declared scope, so an unusable one of either is a startup failure; that is a
            // smaller claim than the one this comment used to make.
            //
            // **The second half is held by `check-boot-order`**, which `just hygiene` runs, and it
            // is there because this comment used to close by calling the order *a
            // convention this line keeps* - which is a rule with no mechanism, and `AGENTS.md` does
            // not accept one. The gate reads the order of three call sites in this file; its own
            // header states what that is worth and what it cannot see.
            boot::refuse_absent_tables(&pinned, &engines)?;
            // **The broker that makes an impersonating source answerable, and it is the only one
            // this crate can attach.** The exchanging broker's HTTP hops went with the `wire` half
            // and the broker itself is deleted; the ADBC driver cannot take a subject's bearer
            // either. What it CAN take is a principal to become, so this attaches
            // `DeclaredPrincipalBroker` - the declared subject-to-account map, presented as the
            // identity each job runs as. `crate::serve::broker` carries what that does not cover,
            // and refuses at boot every declaration this build cannot honour.
            let broker = broker::build_broker(settings.sources())?;
            (
                started(
                    &catalogs,
                    engines,
                    broker,
                    working_set_ceiling_bytes,
                    spend_budget,
                    row_ceiling,
                )?,
                None,
            )
        }
        #[cfg(feature = "postgres")]
        OpenedSources::Postgres(engines) => {
            // A `postgres` source attaches nothing - the tables live in the database - so there is
            // no attach step whose absence at boot would set the served bundle's tables. Its
            // `preflight` is `NotReported` by construction, which `refuse_absent_tables` treats as a
            // WARN rather than a refusal; calling the bigquery-shaped table check would add nothing
            // but a misleading permission sentence. Verification of a mistyped `table:` therefore
            // happens on the first question against it, as the port itself documents.
            //
            // **The static broker, not the exchanging one.** `PostgresWarehouse` declares
            // `NoPlaceForASubject` (one connection under the deployment's declared identity), and
            // `build_postgres` refuses an impersonating source at the posture cross-check - so the
            // only identity a question is answered under is the one this process holds. The static
            // broker is exactly that: every declared `shared-service-user` source is served as
            // itself, and nothing is ever exchanged.
            let broker = StaticCredentialBroker::from_registry(settings.sources());
            (
                started(
                    &catalogs,
                    engines,
                    broker,
                    working_set_ceiling_bytes,
                    spend_budget,
                    row_ceiling,
                )?,
                None,
            )
        }
        OpenedSources::Mixed(mixed) => {
            // One registry, so one pre-flight - generic in the adapter, so it runs the same way
            // over whichever kinds this mix opened. A `files` entry answers `NotReported` here for
            // the first time (its `preflight` takes the port's default), which `boot.rs`'s own doc
            // says is informational rather than a defect.
            boot::refuse_absent_tables(&pinned, &mixed.engines)?;
            // **One broker per ANSWER, so the choice is per BUILD and not per kind.** A mix may
            // read a shared source of one kind and an impersonating one of another, and
            // `build_broker` scans the whole registry rather than the `bigquery` entries - so "does
            // this mix need the principal broker" is exactly "does this build link the adapter that
            // can deliver one". With no impersonating source declared it holds the same shared map
            // the static broker would, and refuses the same sources.
            #[cfg(feature = "bigquery")]
            let served = {
                let broker = broker::build_broker(settings.sources())?;
                started(
                    &catalogs,
                    mixed.engines,
                    broker,
                    working_set_ceiling_bytes,
                    spend_budget,
                    row_ceiling,
                )?
            };
            // No `BigQuery` adapter linked, so no adapter in this build declares
            // `PerSubjectCredential` and every impersonating entry is already refused at its own
            // posture cross-check. The static broker is then the whole truth: every declared shared
            // source served as itself, and nothing else mintable.
            #[cfg(not(feature = "bigquery"))]
            let served = {
                let broker = StaticCredentialBroker::from_registry(settings.sources());
                started(
                    &catalogs,
                    mixed.engines,
                    broker,
                    working_set_ceiling_bytes,
                    spend_budget,
                    row_ceiling,
                )?
            };
            (served, mixed.attached)
        }
    };
    // And this closes the gap between the two loads. `attached` is what the FIRST bundle's models
    // needed; the service serves the SECOND. A model added to the catalog directory between the two
    // calls is therefore served with no table registered behind it, and `answer` cannot see that -
    // its only check on the engine is that the source NAME matches. The failure would arrive as a
    // query-time error for whoever asked first, which is precisely the trade this startup sequence
    // exists to avoid: a bundle that does not hold together must stop the process, not one question.
    //
    // **`None` for a data system this process attached nothing to, and what that costs has CHANGED
    // rather than gone away - which is the whole of issue 120.** The check compares the served
    // bundle's tables against the tables the engine holds, and the engine holds them because `attach`
    // put them there. A `BigQuery` source has no attach step: the tables live in the dataset. That
    // used to mean a `bigquery` deployment whose catalog names a table the dataset does not hold
    // STARTED, and the first question against that model failed - where a `files` deployment in the
    // same state did not start at all. `boot::refuse_absent_tables`, in the `BigQuery` arm above
    // (and, since `telekom/sutura#112`, in the mixed one too), is that asymmetry closed: one
    // metadata read per dataset, and a refusal naming the model and the table.
    //
    // **What is still narrower here than on the files path, stated because it is the whole remaining
    // gap:** the pre-flight reads the bundle loaded FIRST, so a model added to the catalog directory
    // between this root's two loads is caught below on `files` and is not caught at all on
    // `bigquery`.
    if let Some(attached) = attached {
        sutura_app::preflight::refuse_unattached(&sutura_app::preflight::served_tables(service.definitions()), &attached)
            .map_err(|changed| changed.to_string())?;
    }
    tracing::info!(
        definition_version = %pinned.version(),
        metrics = pinned.definitions().metrics().len(),
        "catalog loaded and every anchor reproduced its number"
    );

    // 7. The router, then the runtime, then serving. In that order: see the note on this function.
    let address = settings.server().bind().socket();
    // Read out before the settings are moved into the state. The first two are about the LISTENER
    // rather than about a request, and the third is about stopping, so this is the last place any of
    // them is looked at.
    let material = settings.server().tls().cloned();
    let grace = settings.runtime().shutdown_grace().duration();
    // Leg 1, and it is built HERE rather than inside the state for one reason: building it reads the
    // key set the declaration names, so it can fail - and an unreadable key set has to stop the process
    // rather than become a deployment that answers `401` to everybody while its startup log says it
    // establishes a caller identity. `sutura_http::router` refuses to assemble when a declaration has
    // no gate, so this cannot be forgotten in a later edit; the `?` here is what makes it a refusal to
    // start rather than that refusal firing at assembly.
    let inbound = inbound_gate(&settings)?;
    // **THE process's one execution bound, built here because this is the only place that can say
    // there is one of it** - `telekom/sutura#340`. It used to be derived inside `ServiceState::new`,
    // which made a second state a second permit set; the state takes one now, so this line is what
    // decides the number and `cargo xtask check-one-bound` is what holds *once per root*. Read
    // beside `grace` above it and for the same reason: both are numbers about the process rather
    // than about a request, and this is the last place the settings are looked at before they move.
    let admission = Admission::from_settings(settings.runtime());
    // Built first: the state owns this replica's spend gauge (#892), and the agent surface is handed a handle to it, so both surfaces drive one spend series.
    let mut state = ServiceState::new(service, Arc::new(settings), admission);
    #[cfg(feature = "agent")]
    let agent_mount = agent_mount(&state)?;
    // Kept beside the state so the key-set watch, reached over the same `Arc`, can be armed once the runtime exists.
    let mut watching: Option<Arc<sutura_http::InboundGate>> = None;
    if let Some(gate) = inbound {
        let gate = Arc::new(gate);
        watching = Some(Arc::clone(&gate));
        state = state.with_inbound_identity(gate);
    }
    #[cfg(feature = "agent")]
    if let Some(mount) = agent_mount {
        state = state.with_agent_surface(mount);
    }
    let router = sutura_http::router(&state).map_err(flatten)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|cause| format!("the async runtime could not be built: {cause}"))?;
    // Built out here rather than inside the served future, because what happens AFTER serving
    // returns needs to know how much of the grace period the drain spent. `tokio::sync` needs no
    // runtime entered, so this is safe on this side of `block_on`.
    let stopping = Shutdown::with_grace(grace);
    let served = runtime.block_on(serve_until_stopped(router, address, material, watching, stopping.clone()));
    stop(runtime, &stopping);
    served
}

/// The agent surface's transport, built AFTER the state exists - `github.com/telekom/sutura#892`.
///
/// **Why after, when `main`'s pre-#892 order built it before.** The state owns this replica's spend
/// gauge, and the agent surface is handed a handle to the same one, so both surfaces drive ONE spend
/// series rather than two that disagree. That handle only exists once the state does, which is what
/// moves this call below `ServiceState::new` and costs the `settings`-still-owned convenience the
/// previous order had. `sutura_http::SpendHeadroomPush::of` is the only route to it, and
/// `AgentMount::new` will not build without one - so the ordering is a consequence of the types
/// rather than of this comment.
///
/// `None` is the deployment having left the surface off: a build carrying the `agent` feature is
/// still off by default, and `sutura_http::router` refuses to assemble a mount with no leg-1 gate
/// attached, so "the agent surface is only served where a caller can be verified" cannot be
/// un-paired by a later edit.
#[cfg(feature = "agent")]
fn agent_mount(state: &ServiceState) -> Result<Option<sutura_http::AgentMount>, String> {
    if !state.settings().server().agent_surface_enabled() {
        return Ok(None);
    }
    Ok(Some(agent::mount(
        state.surface(),
        state.settings(),
        state.admission().clone(),
        sutura_http::SpendHeadroomPush::of(state),
    )?))
}

/// Leg 1, for a deployment that declared one.
///
/// **`None` is a posture and not a gap.** A deployment with no `security.inbound` block is a
/// single-player deployment - `docs/adr/0008` part 5a calls that a first-class shape - and every
/// deployment that existed before leg 1 is one. `sutura_config` refuses a block that does not say
/// which mode, so there is no third answer here.
///
/// The key set is read on this line, before the listener opens. What that buys is the difference
/// between a process that does not start and a process that starts and authenticates nobody.
fn inbound_gate(settings: &Settings) -> Result<Option<sutura_http::InboundGate>, String> {
    let Some(declared) = settings.security().inbound() else {
        return Ok(None);
    };
    let gate = sutura_http::InboundGate::from_declaration(declared).map_err(flatten)?;
    tracing::info!(
        inbound_mode = declared.mode(),
        header = gate.header(),
        "leg 1 is armed: the key set was read and a caller's token will be verified against it"
    );
    Ok(Some(gate))
}

/// A build without the `agent` feature: an enabled switch is a startup refusal.
///
/// The feature-off twin of [`agent::mount`] and of [`serve_as_configured`]'s no-`tls` body - the
/// "configured for a build it does not have" refusal. Clearing this function (or its call in `run`)
/// is exactly the regression `crate::tests::agent::an_enabled_agent_surface_is_refused_by_a_build_without_the_feature`
/// exists to hold, and the cell that runs it is `check-default-feature-tests` (this body only
/// compiles without the `agent` feature, so `just test`'s `--all-features` never sees it).
#[cfg(not(feature = "agent"))]
fn agent_refused_if_enabled(settings: &Settings) -> Result<(), String> {
    if settings.server().agent_surface_enabled() {
        return Err(String::from(
            "server.agent_surface.enabled is set and this binary was built without the `agent` \
             feature, so it has no agent surface to mount. Rebuild with `--features agent`, or \
             remove the key.",
        ));
    }
    Ok(())
}

/// Gives the blocking pool what is left of the grace period, and then stops waiting.
///
/// **Dropping a `tokio` runtime waits for every blocking task, without a bound.** That is the hole
/// this closes, and it is the same hole the query handler's slot is about from the other side: a
/// started `spawn_blocking` task cannot be aborted, so a question still running when the process is
/// asked to stop keeps the runtime's `Drop` waiting for it - however long it takes, and whatever the
/// connection drain already spent. So a process with a fifteen second grace period could be killed
/// by its orchestrator mid-answer while looking, in its own log, like it had drained cleanly.
///
/// `shutdown_timeout` is the bound. What it guarantees is the *wait*: a blocking task still running
/// when it expires is left running and the runtime stops waiting, which means the process gets to
/// exit on its own terms - it does not mean the query was cancelled, because nothing can cancel it.
///
/// The budget is what the drain did not spend, not another full grace period. See
/// `Shutdown::remaining_grace`: the number an operator wrote was chosen against their
/// orchestrator's kill timer, and spending it twice is being killed anyway.
fn stop(runtime: tokio::runtime::Runtime, stopping: &Shutdown) {
    let left = stopping.remaining_grace();
    tracing::info!(
        // Milliseconds, and not seconds like every other bound in this process: this one is a
        // REMAINDER, so whole seconds truncate a four second budget that a fast drain barely
        // touched to "3" and read like a second went missing.
        blocking_wait_ms = left.as_millis(),
        grace_seconds = stopping.grace_period().as_secs(),
        "waiting out what is left of the grace period for questions already executing"
    );
    runtime.shutdown_timeout(left);
    tracing::info!("stopped");
}

/// Spawns the signal listener and serves until it fires.
async fn serve_until_stopped(
    router: axum::Router,
    address: std::net::SocketAddr,
    material: Option<TlsMaterial>,
    inbound: Option<Arc<sutura_http::InboundGate>>,
    stopping: Shutdown,
) -> Result<(), String> {
    // Detached on purpose: the task's only job is to translate the first signal into the shared
    // flag, and `serve` below is what waits on it. Joining it would mean waiting for a signal that
    // may never arrive.
    drop(tokio::spawn(shutdown::listen(stopping.clone())));
    // Armed HERE and not where the gate was built, for the reason the TLS renewal watch is: the gate
    // is built before the runtime exists, and a `tokio::spawn` on that side would panic. What it buys
    // is a bound on how long a REVOKED key keeps verifying while nothing is being asked - the gate's
    // own age check covers the case where requests are arriving.
    if let Some(gate) = inbound {
        gate.watch_keys_until_shutdown(stopping.clone());
    }
    serve_as_configured(router, address, stopping, material).await
}

/// Serves plaintext, or terminates TLS here, according to what was configured.
///
/// **Two bodies, chosen by the `tls` feature, and they are not equivalent.** The configuration
/// cannot ask for something a build cannot do - `sutura-cli`'s `tls` feature turns on
/// `sutura-config`'s, so a binary without it refuses `security.tls_termination: in-process` at
/// startup with `NotFitToServe::InProcessTlsNotCompiledIn`, naming the feature. That refusal is the
/// gate; this is not a second copy of it.
///
/// The check in the second body is narrower and is about the one failure that must never be quiet:
/// it keys on the MATERIAL rather than on the declaration, so serving plaintext on a port that was
/// given a certificate is impossible in this file rather than impossible two crates away. A `cfg`
/// that silently fell through to `serve` would be exactly the silent fallback this is not allowed to
/// have.
#[cfg(feature = "tls")]
async fn serve_as_configured(
    router: axum::Router,
    address: std::net::SocketAddr,
    stopping: Shutdown,
    material: Option<TlsMaterial>,
) -> Result<(), String> {
    match material.as_ref() {
        Some(material) => sutura_http::serve_tls(router, address, stopping, material)
            .await
            .map_err(flatten),
        None => sutura_http::serve(router, address, stopping).await.map_err(flatten),
    }
}

/// The same decision in a build with no TLS listener in it.
#[cfg(not(feature = "tls"))]
async fn serve_as_configured(
    router: axum::Router,
    address: std::net::SocketAddr,
    stopping: Shutdown,
    material: Option<TlsMaterial>,
) -> Result<(), String> {
    if material.is_some() {
        return Err(String::from(
            "server.tls_certificate and server.tls_key are set and this binary was built without \
             the `tls` feature, so it has no TLS listener at all. Rebuild with `--features tls`, or \
             terminate TLS in front of this process and remove the paths. Refusing to serve \
             plaintext on a port that was configured to be encrypted",
        ));
    }
    sutura_http::serve(router, address, stopping).await.map_err(flatten)
}

/// The open data systems, and the tables they actually hold.
///
/// A named pair rather than a tuple: the second field is evidence for a startup refusal and `.1`
/// would say nothing about which of the two it is. `clippy::type_complexity` asks for the same thing
/// from the other direction.
pub(crate) struct Opened {
    engines: sutura_app::Warehouses<DataFusionWarehouse>,
    attached: BTreeSet<TableName>,
}

/// The adapter(s) this process opened its sources with, and everything the next step needs from
/// them.
///
/// **One variant per LINKED adapter, and the enum is here rather than in `sutura-app` for the reason
/// that crate's `warehouses` module states: which adapters a process holds is a property of the
/// BUILD.** `sutura_app::Warehouses<W>` is generic in one `W`, so the first three variants are each
/// a single-kind registry - the choice of which one got built, made once, at the one place that can
/// see both the declarations and the link.
///
/// **[`Self::Mixed`] is `telekom/sutura#112`'s closed enum, and it is what makes the other three
/// variants a fast path rather than the whole decision.** `open_engine` still takes them when every
/// declared source shares a kind - no erasure, no `crate::serve::kind::AnyWarehouse` in the type -
/// and reaches for [`Self::Mixed`] the moment more than one of `kind::group_by_kind`'s three groups
/// is non-empty. A catalog whose models sit on a `files` source AND a `bigquery` source now opens
/// both, instead of the startup refusal this comment used to describe.
pub(crate) enum OpenedSources {
    /// The in-process engine over directories of files.
    Files(Opened),
    /// A `BigQuery` dataset per source, reached through the ADBC driver.
    ///
    /// Nothing is attached, so there is no table set beside it - see the note at the call site of
    /// [`sutura_app::preflight::refuse_unattached`], which states what that costs.
    #[cfg(feature = "bigquery")]
    BigQuery(sutura_app::Warehouses<BigQuerySource>),
    /// A `PostgreSQL` connection per source, reached over the declared channel.
    ///
    /// Nothing is attached - the tables live in the database - so like `BigQuery` there is no table
    /// set beside it. The channel (plaintext on a local host, or TLS over the declared anchors) is
    /// resolved when the engine opens, so a TLS refusal stops the process before the listener binds.
    #[cfg(feature = "postgres")]
    Postgres(sutura_app::Warehouses<PostgresSource>),
    /// More than one kind, erased behind [`kind::AnyWarehouse`] - unconditional, so a build with
    /// neither optional feature still refuses a genuinely mixed catalog by naming the missing
    /// feature rather than never reaching that arm.
    Mixed(kind::Mixed),
}

/// A `Postgres` source as this binary composes it: one connection under the deployment's declared
/// identity, secured as the source declares.
///
/// Named once for the reason `BigQuerySource` is: it appears in a registry type, a `Warehouse`
/// bound and a constructor's return, and the three layers ARE the composition.
#[cfg(feature = "postgres")]
pub(crate) type PostgresSource = sutura_exec_postgres::PostgresWarehouse;

/// A `BigQuery` source as this binary composes it: the adapter, over the ADBC driver.
///
/// Named once because it appears in a registry type, a `Warehouse` bound and a constructor's return.
/// TWO layers now and not three: the credential layer the `wire` half carried is gone - the driver
/// authenticates itself - so the composition is the adapter over the one reachable transport.
#[cfg(feature = "bigquery")]
pub(crate) type BigQuerySource = sutura_exec_bigquery::BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery>;

/// The started service, with the adapter it was built over erased.///
/// Named because `Result<Arc<dyn Surface>, String>` is over the `type_complexity` threshold this
/// workspace tightened - the same reason `sutura_exec_bigquery`'s `Mapped` exists - and because the
/// erasure is the thing worth naming: what the transport takes is a trait object, so which adapter
/// answered stops being visible in a type exactly here.
type Serving = Arc<dyn Surface>;

/// Loads the catalogs a second time through their ports, composes them, verifies every anchor, and
/// erases the adapter.
///
/// Generic in the adapter AND the broker, and the second generic is what lets the two arms below
/// differ: a `files` deployment has no impersonating source, so its broker is the static one; a
/// `bigquery` deployment with an impersonating source gets the exchanging broker. Returning
/// `Arc<dyn Surface>` is what lets the shared lines after each arm stop caring which of those it
/// was - the transport takes a trait object, so the monomorphisation ends here rather than through
/// the router.
///
/// **Also generic over which of the two monomorphic catalog vectors `catalog::OpenedCatalogs`
/// carries**, matched once here rather than at each of this function's call sites: every arm below
/// builds the exact same `LocalService<W, TracingAuditSink, B>`, because `start_composed`'s catalog
/// type parameter is consumed while loading and never stored - see `catalog.rs`'s module header for
/// why `OpenedCatalogs` is an enum of two vectors rather than one vector of a shared type.
fn started<W, B>(
    catalogs: &catalog::OpenedCatalogs,
    engines: sutura_app::Warehouses<W>,
    broker: B,
    working_set_bytes: u64,
    spend_budget: Option<sutura_config::SpendBudget>,
    row_ceiling: sutura_domain::plan::RowCeiling,
) -> Result<Serving, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
    B: sutura_domain::identity::CredentialBroker + Send + Sync + 'static,
    B::Error: Send + Sync,
{
    // The combiner, built once for this replica - the served root's half of `docs/adr/0007`'s
    // second driven port. Built here rather than handed in, for the reason the audit sink is: which
    // implementor a process holds is a property of the BUILD, and this is the build.
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new()
        .map_err(|cause| format!("{cause}\ncould not build the federation combiner"))?;
    match catalogs {
        catalog::OpenedCatalogs::Markdown(catalogs) => LocalService::start_composed(
            catalogs,
            engines,
            TracingAuditSink::new(),
            broker,
            combiner,
            working_set_bytes,
        )
        .map(|service| {
            Arc::new(
                service
                    .with_spend_ledger(spend_ledger(spend_budget))
                    .with_row_ceiling(row_ceiling),
            ) as Serving
        })
        .map_err(flatten),
        #[cfg(feature = "datahub")]
        catalog::OpenedCatalogs::Datahub(catalogs) => LocalService::start_composed(
            catalogs,
            engines,
            TracingAuditSink::new(),
            broker,
            combiner,
            working_set_bytes,
        )
        .map(|service| {
            Arc::new(
                service
                    .with_spend_ledger(spend_ledger(spend_budget))
                    .with_row_ceiling(row_ceiling),
            ) as Serving
        })
        .map_err(flatten),
    }
}

/// The spend ledger this replica answers under: unbounded if `governance.per_replica_spend_ceiling`
/// is absent, which is `docs/adr/0030`'s decision for every deployment before this key existed.
fn spend_ledger(spend_budget: Option<sutura_config::SpendBudget>) -> sutura_app::SpendLedger {
    sutura_app::SpendLedger::new(spend_budget.map(|budget| sutura_app::SpendBudget::new(budget.ceiling_bytes(), budget.window())))
}

/// Starts the engine and registers one file per model, returning what it attached.
///
/// The engine reads the files itself, so there is no database to create and nothing to keep in step
/// with them. Parquet is preferred over CSV where both are present, because it carries its own types
/// and a CSV has to be sniffed.
///
/// The set of tables comes back with the engine because it is evidence rather than bookkeeping: it is
/// what [`sutura_app::preflight::refuse_unattached`] compares the SERVED bundle against, and the two bundles are two loads.
///
/// **`with_worker_threads` and not `new`, and that is the whole of what `runtime.engine_worker_threads`
/// does.** The engine drives its own runtime and every request `block_on`s it from a blocking-pool
/// thread, so a single-threaded one is a ceiling every concurrent question shares - measured flat at
/// one caller's throughput however many are asking. The command-line tool keeps `new`: it answers one
/// question and exits. See `DataFusionWarehouse::with_worker_threads` for the numbers.
///
/// **It also hands over `runtime.working_set_max_bytes`**, which is what stops the engine installing
/// its unbounded memory pool - and under `panic = "abort"` an unbounded pool makes a large enough join
/// process death for every caller in flight rather than a refusal for the one who asked. The whole
/// group is taken rather than two of its values, because a third bound would otherwise mean a third
/// parameter here.
fn open_engine(
    pinned: &PinnedDefinitions,
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
    request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedSources, String> {
    let declared = sutura_app::sources(pinned);
    if declared.is_empty() {
        return Err(String::from("this catalog declares no models, so there is nothing to open"));
    }
    // Before the engines are built, and the order is the classic one: the cheap check that reads the
    // parsed tree runs before the expensive one that starts a runtime and a memory pool. It also puts
    // the more actionable message first - an anchor with no identity to run it as names the metric.
    boot::refuse_unverifiable_anchors(pinned, registry)?;
    // **Sorted into its kind, and then a fast path or the mix.** `sutura_config::SourceKind`'s
    // vocabulary is separate from the set of adapters a given binary LINKED - the vocabulary is
    // the repository's and the link is this file's - so `kind::group_by_kind` is exhaustive with
    // no wildcard arm, and a third kind is a compile error there rather than a case that falls
    // through. Every declared source sharing one kind takes the ORIGINAL single-registry path -
    // no erasure, no `kind::AnyWarehouse` anywhere in the type - and [`kind::open_mixed`] is
    // reached only once more than one group is non-empty.
    let grouped = kind::group_by_kind(&declared, registry)?;
    match (
        grouped.files.is_empty(),
        grouped.bigquery.is_empty(),
        grouped.postgres.is_empty(),
    ) {
        (false, true, true) => open_files(pinned, &grouped.files, registry, runtime).map(OpenedSources::Files),
        (true, false, true) => bigquery::open_bigquery(&grouped.bigquery, registry, request_timeout, outbound),
        (true, true, false) => postgres::open_postgres(&grouped.postgres, registry),
        // Unreachable: `declared` is non-empty (checked above) and every entry falls into exactly
        // one of the three groups, so this arm can only be `(true, true, true)` if nothing ran -
        // which cannot happen. Written as a fallback rather than an unwrap the workspace denies.
        (true, true, true) => Err(String::from("this catalog declares no models, so there is nothing to open")),
        _ => kind::open_mixed(&grouped, pinned, registry, runtime, request_timeout, outbound).map(OpenedSources::Mixed),
    }
}

/// Opens the in-process engine for every declared `files` source and registers one file per model.
fn open_files(
    pinned: &PinnedDefinitions,
    declared: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<Opened, String> {
    let mut engines: Option<sutura_app::Warehouses<DataFusionWarehouse>> = None;
    let mut attached: BTreeSet<TableName> = BTreeSet::new();
    for source in declared {
        let configured = configured_source(source, registry)?;
        let engine = build_engine(source, configured, runtime)?;
        // Matched rather than read off a `data_dir()` every kind had to have. `build_engine` has
        // already refused every kind this binary cannot open, so the other arm is unreachable here -
        // written as a branch rather than an `expect` because the workspace denies both, and because a
        // second openable kind should arrive as a compile error at this line too.
        let sutura_config::SourcePlacement::Files { ref data_dir } = *configured.placement() else {
            return Err(format!(
                "`sources.{source}` reached the attach step with a placement no linked adapter reads, \
                 which `build_engine` should have refused first"
            ));
        };
        for model in pinned
            .definitions()
            .models()
            .values()
            .filter(|model| model.source() == *source)
        {
            // Refused at BOOT, which is where it belongs: this binary links the in-process engine
            // and nothing else, and the engine registers one file per model with no catalog and no
            // schema above it. So a qualified model is a bundle this deployment cannot serve, and a
            // deployment that cannot serve its bundle should not start. `DataFusionWarehouse::scan`
            // refuses the same thing again for a model that could only arrive after this loop.
            if model.table().qualifier().is_some() {
                return Err(format!(
                    "model {} names the table {}, and the in-process engine registers one file per \
                     model with nothing above it. Refusing to serve a bundle whose questions would \
                     fail at query time",
                    model.name(),
                    model.table()
                ));
            }
            attach(&engine, model.table_name(), data_dir)?;
            // Collected AFTER the attach, so this set is what the engines hold rather than what was
            // asked for. `attach` fails the startup on a missing file, so the two cannot diverge
            // here - and recording it from the successful call rather than from the model list is
            // what keeps that true if it ever gains a path that can skip one.
            attached.insert(model.table_name().clone());
        }
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns, so this is the loop's own
    // invariant written as a fallback rather than an unwrap the workspace denies.
    let engines = engines.ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))?;
    Ok(Opened { engines, attached })
}

/// The declaration for one source the catalog names, or a refusal saying what is missing.
///
/// Two refusals, and they are two facts rather than one: a source with no entry is a deployment that
/// never said where that data system is, and a source declared shared with nobody's acknowledgement is
/// a deployment `Settings::refusals` already refused. The second is unreachable through
/// `Settings::load` and is written out anyway - `sutura_config::ConfiguredSource::identity` returns an
/// `Option` precisely so a root cannot read the absence as permission, and treating it as such here
/// would be the hole that accessor exists to keep open to review.
fn configured_source<'registry>(
    source: &SourceName,
    registry: &'registry sutura_config::SourceRegistry,
) -> Result<&'registry sutura_config::ConfiguredSource, String> {
    let configured = registry.get(source).ok_or_else(|| {
        format!(
            "this catalog reads from {source}, and no `sources.{source}` entry declares where that \
             data system is or which identity a query reaches it as. Declare it, or remove the models \
             that name it"
        )
    })?;
    if configured.identity().is_none() {
        return Err(format!(
            "`sources.{source}` is `shared-service-user` in a multi-user deployment and carries no \
             acknowledgement, so there is no declared identity to serve it under"
        ));
    }
    Ok(configured)
}

/// Builds the engine for one declared source, after checking this build can deliver its posture.
///
/// **The kind is no longer matched here, and that is a move rather than a removal:** the exhaustive
/// match lives in [`open_engine`], which is where a declared kind is DISPATCHED to an adapter. It was
/// here while the second kind's answer was a refusal, because a refusal per source reads the same
/// wherever it is written; once the answer is a different registry, only the dispatcher can hold it.
/// Reaching this function is therefore a statement that `one_kind` said `files`.
///
/// **The cross-check is here and not in `sutura-config`, and the split follows what each half can
/// see.** Configuration says which posture the deployment is asking for; whether the LINKED adapter can
/// carry a per-subject credential at all is a property of the build, and the settings tree cannot see
/// which adapters were compiled in. So the comparison lives where both are in scope, which is here, and
/// `SourcePosture::deliverable_by` is the one function that makes it.
fn build_engine(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    runtime: sutura_config::RuntimeSettings,
) -> Result<DataFusionWarehouse, String> {
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    identity
        .posture()
        .deliverable_by(
            <DataFusionWarehouse as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(flatten)?;
    // `NonZeroUsize::MIN` is unreachable: `EngineWorkers::parse` refuses a zero and resolves an
    // absent key from the machine, which reports at least one. Written as a fallback rather than an
    // unwrap because the workspace denies both, and because one worker is the safe direction to fail
    // in - a narrow engine is slow, and a zero-width runtime does not build.
    let width = core::num::NonZeroUsize::new(runtime.engine_workers().count()).unwrap_or(core::num::NonZeroUsize::MIN);
    // The ceiling arrives already parsed - `WorkingSetCeiling::parse` refused a zero and refused a
    // value above the memory this process can reach - so there is nothing left to check here. The
    // wrapper exists so a thread count and a quantity of memory cannot be swapped at this call.
    let working_set = sutura_exec_datafusion::WorkingSet::of_bytes(runtime.working_set().bytes());
    DataFusionWarehouse::with_worker_threads(source.clone(), identity.posture().clone(), width, working_set).map_err(flatten)
}

/// Registers one model's file: Parquet first, then CSV or NDJSON, plain or compressed.
///
/// The candidate names come from the engine, for the reason `sources::files`'s twin gives.
fn attach(engine: &DataFusionWarehouse, table: &TableName, data: &std::path::Path) -> Result<(), String> {
    let mut tried = Vec::new();
    for name in sutura_exec_datafusion::candidates(table) {
        let candidate = data.join(name);
        if candidate.is_file() {
            return engine.attach_file(table, &candidate).map_err(flatten);
        }
        tried.push(candidate.display().to_string());
    }
    Err(format!(
        "the model behind table {table} needs one of {}, and none is there",
        tried.join(", ")
    ))
}

/// A typed error and every cause beneath it, one per line.
///
/// Written out rather than relying on `Display`, which prints the outermost message and stops. The
/// causes are where the useful part is: "the configuration sources could not be read" is a category,
/// and its source is the key.
fn flatten(error: impl core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

/// The startup refusals, in their own file.
///
/// **Moved here by the file-length gate**, not by a change of mind about where a composition root's
/// tests live: this file crossed 1000 lines when the source registry landed, and `cargo xtask
/// max-lines` fails rather than warning. `devco/max-lines-ignore` cannot exempt anything under
/// `crates/`, which is the rule working as intended - the answer to a long file is to split it, not to
/// shorten the fix that made it long.
#[cfg(test)]
mod tests;
