//! The service binary. The composition root, and nothing else.
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
//! # Why this is a second binary rather than a subcommand of `sutura`
//!
//! Because a shipped artifact holds one executable, and `sutura` is a command-line tool a person
//! runs while this is a service a platform schedules. Folding the HTTP surface into that binary
//! would put a multi-threaded runtime, an I/O driver, a web framework and a browser asset bundle
//! into every invocation of `sutura compile`.
//!
//! **This binary IS published now**, which is a change from what this comment used to say: it named
//! the release derivations' `--package sutura-cli` and concluded that nothing shipped a server, which
//! was true and was the defect `github.com/telekom/sutura#111` records. `nix/shipped.nix` lists both
//! binaries; `checks.one-binary` and `checks.shipped-features` are what assert what each artefact
//! holds - one executable of the expected name, and no `tls` or `bigquery`, both being default-off
//! features that cost a rustls closure across two musl triples and refuse at startup by name.
//!
//! # `Result<_, String>` below the surface
//!
//! Used freely, and the boundary gate exempts a binary on purpose: the audience for these messages
//! is a person reading standard error, not code matching on a variant. Every typed error from a
//! library crate is flattened with its whole `#[source]` chain on the way out, because the outermost
//! message is the one that says least.

// mimalloc as the global allocator, on Linux only - the same decision, for the same measurements, as
// `crates/sutura-cli/src/main.rs`, whose comment is the long form and is not repeated here.
//
// IT IS HERE BECAUSE THIS BINARY IS NOW SHIPPED, and the argument is sharper for a server than for
// the tool: two of the four release triples are musl, mallocng serialises the whole process on one
// lock word, and a server's unit of work is a warehouse round trip fanned out over threads - the
// shape that measured 20.7x slower on 48 cores. `#[global_allocator]` on a static is a safe
// attribute, so this needs no `unsafe`.
#[cfg(target_os = "linux")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::BTreeSet;
use std::process::ExitCode;
use std::sync::Arc;

use sutura_app::surface::Surface;
use sutura_catalog_local::LocalCatalog;
use sutura_config::{Environment, Settings, Sources, StaticCredentialBroker, TlsMaterial};
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_exec_datafusion::DataFusionWarehouse;
use sutura_http::{LocalService, ServiceState};
use sutura_runtime::{Shutdown, TracingAuditSink, banner, shutdown, telemetry};

/// How a declared `catalogs:` becomes the catalog this build serves.
mod catalog;

/// The refusals this root makes by reading the bundle. `main.rs` keeps the ORDER they run in.
mod boot;

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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // Standard error, and `eprintln` rather than `tracing`: this path includes every
            // failure that happens before a subscriber exists, and a message emitted into a
            // subscriber that was never installed goes nowhere.
            eprintln!("sutura-serve: {message}");
            ExitCode::FAILURE
        }
    }
}

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
fn run() -> Result<(), String> {
    if let Some(code) = handled_immediately() {
        return code;
    }

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

    // 4. The log, then the panic hook.
    telemetry::install(settings.telemetry()).map_err(flatten)?;
    sutura_runtime::install_panic_hook();

    // 5. What was resolved, and what this service does not do.
    banner::announce(&settings);

    // 6. The adapters, then the service. Both ports are named exactly here.
    let catalogs = catalog::open_catalog(settings.catalogs())?;
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
    // executes as. `StaticCredentialBroker` reads the `sources:` tree this root already parsed:
    // every source declared `shared-service-user` is served under the identity this process holds,
    // and a source declared `impersonation-at-source` gets no credential from it - so a question
    // against one is refused as `credential_unavailable` rather than answered as this process. That
    // is the shipping single-user shape, and the deployment that needs the other one is the
    // deployment that needs a broker which can perform a token exchange.
    let broker = StaticCredentialBroker::from_registry(settings.sources());
    // The working-set ceiling this deployment configured, threaded to the federated combiner so a
    // combined answer is counted against the same bound the engine's operators are refused by.
    let working_set_ceiling_bytes = settings.runtime().working_set().bytes().get() as u64;
    // **One `Arc<dyn Surface>` out of two adapter types, and the erasure is where it always was.**
    // `sutura_app::Warehouses<W>` is generic in ONE adapter, so the service is monomorphised per kind
    // - and `ServiceState` takes `Arc<dyn Surface>`, so the two shapes meet one line later either
    // way. That is the whole reason this deployment does not need the closed enum over adapters that
    // `sutura_app::warehouses` describes: nothing above this line is generic.
    let (service, attached) = match opened {
        OpenedSources::Files(files) => (
            started(&catalogs, files.engines, broker, working_set_ceiling_bytes)?,
            Some(files.attached),
        ),
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
            // calls `of` further down, in `open_bigquery`. What holds the first half is the
            // `BigQuerySource` alias declared beside `OpenedSources`, whose transport parameter is
            // `BigQueryWire<Credential>` and whose `Credential` has one public constructor,
            // `Credential::read`.
            //
            // **The second half is held by `check-boot-order`**, which `just hygiene` runs, and it
            // is there because this comment used to close by calling the order *a
            // convention this line keeps* - which is a rule with no mechanism, and `AGENTS.md` does
            // not accept one. The gate reads the order of three call sites in this file; its own
            // header states what that is worth and what it cannot see.
            boot::refuse_absent_tables(&pinned, &engines)?;
            (started(&catalogs, engines, broker, working_set_ceiling_bytes)?, None)
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
    // same state did not start at all. `boot::refuse_absent_tables`, in the arm above, is that
    // asymmetry closed: one metadata read per dataset, and a refusal naming the model and the table.
    //
    // **What is still narrower here than on the files path, stated because it is the whole remaining
    // gap:** the pre-flight reads the bundle loaded FIRST, so a model added to the catalog directory
    // between this root's two loads is caught below on `files` and is not caught at all on
    // `bigquery`.
    if let Some(attached) = attached {
        boot::refuse_unattached(&boot::served_tables(service.definitions()), &attached)?;
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
    let mut state = ServiceState::new(service, Arc::new(settings));
    // Kept beside the state so the key-set watch can be armed once the runtime exists. `Arc` because
    // the state holds one and the watch needs to reach the same cache.
    let mut watching: Option<Arc<sutura_http::InboundGate>> = None;
    if let Some(gate) = inbound {
        let gate = Arc::new(gate);
        watching = Some(Arc::clone(&gate));
        state = state.with_inbound_identity(gate);
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
/// cannot ask for something a build cannot do - `sutura-serve`'s `tls` feature turns on
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

/// `--version` and `--help`, answered without loading anything.
///
/// Returns `None` when there is real work to do. Deliberately tiny: this binary takes no options,
/// because every knob it has is configuration, and a flag that shadowed a configuration key would be
/// a second way to set it.
fn handled_immediately() -> Option<Result<(), String>> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        None => None,
        Some("--version" | "-V") => {
            println!("sutura-serve {}", env!("CARGO_PKG_VERSION"));
            Some(Ok(()))
        }
        Some("--help" | "-h") => {
            usage();
            Some(Ok(()))
        }
        Some(unknown) => Some(Err(format!(
            "unknown argument `{unknown}`. This binary takes no options; everything is \
             configuration. Run with --help."
        ))),
    }
}

fn usage() {
    println!("usage: sutura-serve");
    println!();
    println!("Serves the semantic surface over HTTP. It takes no options: everything is");
    println!("configuration, layered in this order, later beating earlier.");
    println!();
    println!("  1. the defaults compiled into this binary");
    println!(
        "  2. <dir>/base.yaml, if {} names a directory holding one",
        sutura_config::CONFIG_DIR_VARIABLE
    );
    println!("  3. <dir>/<environment>.yaml");
    println!("  4. one variable per key, such as SUTURA__SERVER__PORT");
    println!();
    println!("  {:<22} one of: {}", sutura_config::ENVIRONMENT_VARIABLE, environments());
    println!(
        "  {:<22} a directory of YAML overrides, optional",
        sutura_config::CONFIG_DIR_VARIABLE
    );
    println!(
        "  {:<22} overrides telemetry.filter",
        sutura_runtime::telemetry::FILTER_VARIABLE
    );
    println!();
    println!("It binds loopback by default and refuses to start in production without an");
    println!("access token and rate limiting. That token authenticates the DEPLOYMENT and not");
    println!("the caller: sutura has no per-caller identity, so every question is answered with");
    println!("whatever access this process already had, whoever asked it.");
}

fn environments() -> String {
    Environment::NAMES.join(", ")
}

/// The open data systems, and the tables they actually hold.
///
/// A named pair rather than a tuple: the second field is evidence for a startup refusal and `.1`
/// would say nothing about which of the two it is. `clippy::type_complexity` asks for the same thing
/// from the other direction.
struct Opened {
    engines: sutura_app::Warehouses<DataFusionWarehouse>,
    attached: BTreeSet<TableName>,
}

/// The adapter this process opened its sources with, and everything the next step needs from it.
///
/// **One variant per LINKED adapter, and the enum is here rather than in `sutura-app` for the reason
/// that crate's `warehouses` module states: which adapters a process holds is a property of the
/// BUILD.** `sutura_app::Warehouses<W>` is generic in one `W`, so this is not a heterogeneous
/// registry and does not try to be - it is the choice of which registry got built, made once, at the
/// one place that can see both the declarations and the link.
///
/// The consequence is a refusal rather than a silence, and [`one_kind`] is where it is made: a
/// catalog whose models sit on a `files` source AND a `bigquery` source cannot be served by this
/// process. Federating across two kinds needs a closed enum over the adapter types or dynamic
/// dispatch, which `sutura_app::warehouses` records as an architecture decision with a record - so
/// what this enum does is make the limit a startup refusal naming both entries, instead of a
/// `SourceUnavailable` on the first question against whichever source lost.
enum OpenedSources {
    /// The in-process engine over directories of files.
    Files(Opened),
    /// A `BigQuery` dataset per source, reached over the wire.
    ///
    /// Nothing is attached, so there is no table set beside it - see the note at the call site of
    /// [`refuse_unattached`], which states what that costs.
    #[cfg(feature = "bigquery")]
    BigQuery(sutura_app::Warehouses<BigQuerySource>),
}

/// A `BigQuery` source as this binary composes it: the adapter, over the wire, over a credential file.
///
/// Named once because it appears in a registry type, a `Warehouse` bound and a constructor's return,
/// and because the three layers ARE the composition - `docs/adr/0018` is the record for the inner two.
#[cfg(feature = "bigquery")]
type BigQuerySource = sutura_exec_bigquery::BigQueryWarehouse<
    sutura_exec_bigquery::wire::BigQueryWire<sutura_exec_bigquery::wire::credential::Credential>,
>;

/// The started service, with the adapter it was built over erased.///
/// Named because `Result<Arc<dyn Surface>, String>` is over the `type_complexity` threshold this
/// workspace tightened - the same reason `sutura_exec_bigquery`'s `Mapped` exists - and because the
/// erasure is the thing worth naming: what the transport takes is a trait object, so which adapter
/// answered stops being visible in a type exactly here.
type Serving = Arc<dyn Surface>;

/// Loads the catalogs a second time through their ports, composes them, verifies every anchor, and
/// erases the adapter.
///
/// Generic in the adapter and returning `Arc<dyn Surface>`, which is what lets the two arms above
/// share every line after them: the transport takes a trait object, so the monomorphisation ends
/// here rather than travelling through the router.
fn started<W>(
    catalogs: &[LocalCatalog],
    engines: sutura_app::Warehouses<W>,
    broker: StaticCredentialBroker,
    working_set_bytes: u64,
) -> Result<Serving, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    LocalService::start_composed(catalogs, engines, TracingAuditSink::new(), broker, working_set_bytes)
        .map(|service| Arc::new(service) as Serving)
        .map_err(flatten)
}

/// Starts the engine and registers one file per model, returning what it attached.
///
/// The engine reads the files itself, so there is no database to create and nothing to keep in step
/// with them. Parquet is preferred over CSV where both are present, because it carries its own types
/// and a CSV has to be sniffed.
///
/// The set of tables comes back with the engine because it is evidence rather than bookkeeping: it is
/// what [`refuse_unattached`] compares the SERVED bundle against, and the two bundles are two loads.
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
) -> Result<OpenedSources, String> {
    let declared = sutura_app::sources(pinned);
    if declared.is_empty() {
        return Err(String::from("this catalog declares no models, so there is nothing to open"));
    }
    // Before the engines are built, and the order is the classic one: the cheap check that reads the
    // parsed tree runs before the expensive one that starts a runtime and a memory pool. It also puts
    // the more actionable message first - an anchor with no identity to run it as names the metric.
    boot::refuse_unverifiable_anchors(pinned, registry)?;
    // **An exhaustive match with no wildcard arm, and it is the one line where "which adapter opens a
    // declared kind" is decided.** A third kind is a compile error here rather than a case that falls
    // through, which is the whole reason `sutura_config::SourceKind`'s vocabulary is separate from the
    // set of adapters a given binary LINKED: the vocabulary is the repository's and the link is this
    // file's. It moved OUT of `build_engine` when the second kind stopped being a refusal - a refusal
    // per source read the same whichever function held it, and a dispatch does not.
    match one_kind(&declared, registry)? {
        sutura_config::SourceKind::Files => open_files(pinned, &declared, registry, runtime).map(OpenedSources::Files),
        sutura_config::SourceKind::BigQuery => open_bigquery(&declared, registry, request_timeout),
    }
}

/// The one kind every declared source is, or the refusal that says this process opens one at a time.
///
/// **A refusal and not a fan-out, and the reason is a type rather than an opinion.**
/// `sutura_app::Warehouses<W>` is generic in one adapter, so a process holds two file sources or two
/// datasets and cannot hold one of each; that limit is documented where the registry is, and this is
/// where it becomes something an operator is told at startup instead of discovering as a
/// `SourceUnavailable` on the first question against whichever source lost.
///
/// It names BOTH entries and both kinds, because the fix is a choice between two deployments rather
/// than an edit to one line.
fn one_kind(declared: &[&SourceName], registry: &sutura_config::SourceRegistry) -> Result<sutura_config::SourceKind, String> {
    // The first source decides, and every other one is compared against it - so the refusal names the
    // pair that disagreed rather than reporting a set. `declared` is non-empty at every call site;
    // written as a fallback rather than an index because the workspace denies both.
    let mut chosen: Option<(&SourceName, sutura_config::SourceKind)> = None;
    for source in declared {
        let kind = configured_source(source, registry)?.kind();
        match chosen {
            None => chosen = Some((source, kind)),
            Some((_, expected)) if expected == kind => {}
            Some((first, expected)) => {
                return Err(format!(
                    "`sources.{first}` is `kind: {}` and `sources.{source}` is `kind: {}`, and this \
                     process opens one kind of data system at a time - the registry it holds is \
                     generic in one adapter type. Serve the two from two deployments, or move the \
                     models so one catalog reads one kind",
                    expected.as_str(),
                    kind.as_str()
                ));
            }
        }
    }
    chosen
        .map(|(_, kind)| kind)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// Opens one `BigQuery` adapter per declared source, over the wire, under a declared credential.
///
/// **Nothing is attached and nothing is registered, which is the difference from [`open_files`] that
/// matters:** the tables live in the dataset. What this function does instead is everything that can
/// fail before a listener is bound - the posture cross-check, the two bounds, and READING the
/// credential file, which is the one step that would otherwise fail on the first question.
#[cfg(feature = "bigquery")]
fn open_bigquery(
    declared: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
) -> Result<OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<BigQuerySource>> = None;
    for source in declared {
        let configured = configured_source(source, registry)?;
        let engine = build_bigquery(source, configured, request_timeout)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns. Written as a fallback for the
    // reason `open_files` gives - the workspace denies `unwrap` and `expect`.
    engines
        .map(OpenedSources::BigQuery)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the `BigQuery` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, so the dispatcher above
/// has exactly one call and the compiler decides which of these it reaches. The parameters this body
/// does not read are named for it, which is what lets both signatures stay identical under
/// `dead_code = "deny"`.
///
/// The message names the FEATURE and not just the kind, because the two things an operator can do are
/// in two different files: change the `kind:`, or build with `--features bigquery`. A message that
/// only said "this binary links no `BigQuery` adapter" sent them to the first when they wanted the
/// second - which was this refusal's shape before the adapter was registered at all.
#[cfg(not(feature = "bigquery"))]
fn open_bigquery(
    declared: &[&SourceName],
    _registry: &sutura_config::SourceRegistry,
    _request_timeout: sutura_config::RequestTimeout,
) -> Result<OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: bigquery`, and this binary was built without the `bigquery` \
         feature - so it links no BigQuery adapter and composes the in-process engine only. Build \
         `sutura-serve` with `--features bigquery`, or declare a `files` source"
    ))
}

/// Builds one `BigQuery` adapter, after checking this build can deliver the source's posture.
///
/// **Every value it needs is declared, and the two that are not on the source entry say where they
/// come from.** The billing project, the dataset, the credential file and the bytes-billed ceiling are
/// the entry's; the query deadline is `server.request_timeout_seconds`, which is what
/// `sutura_exec_bigquery::wire::QueryDeadline` asks a composition root for by name - a job that
/// outlives the request it is answering is billed for a result nobody is waiting for.
///
/// The ceiling is parsed HERE and not in `sutura-config`, and that is the single-owner rule rather
/// than laziness: the range belongs to the adapter, so a second copy of it in the settings tree would
/// be the duplicate that drifts. What the settings tree owns is that the key was written.
#[cfg(feature = "bigquery")]
fn build_bigquery(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    request_timeout: sutura_config::RequestTimeout,
) -> Result<BigQuerySource, String> {
    use sutura_exec_bigquery::transport::{DatasetId as WireDataset, ProjectId as WireProject};
    use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
    use sutura_exec_bigquery::wire::{BigQueryWire, BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};

    // Matched rather than read off accessors every kind would have to have, for the reason
    // `open_files` gives at the same shape: `one_kind` has already decided which arm this is, and a
    // second openable kind should arrive as a compile error at this line too.
    let sutura_config::SourcePlacement::BigQuery {
        ref billing_project,
        ref dataset,
        ref credential_file,
        max_bytes_billed,
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the BigQuery attach step with a placement no BigQuery adapter \
             reads, which `one_kind` should have dispatched elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    // The same cross-check `open_files` makes and against a DIFFERENT constant, which is the point of
    // it being per adapter rather than per deployment: this adapter declares `PerSubjectCredential`,
    // so a `shared-service-user` entry is deliverable and an `impersonation-at-source` entry passes
    // the adapter's capability half - which is the change issue 87 landed. Passing the adapter's half
    // is not the whole story, and the composition's half is below.
    identity
        .posture()
        .deliverable_by(<BigQuerySource as sutura_domain::warehouse::Warehouse>::IMPERSONATION, source)
        .map_err(flatten)?;
    // **The adapter can carry a subject, and this composition does not yet wire a broker that mints
    // one.** The port, the `WorkloadIdentityBroker` and the real `StsExchange` all exist and are
    // tested; attaching a broker to a served source is the step that awaits a deployable GCP project.
    // Until then an `impersonation-at-source` entry would be opened and served under the credential
    // the deployment declared - every row as this process while a reviewer believed a subject's
    // authorization was evaluated - which is the confusion `docs/adr/0014` names. Refuse it before
    // the credential file is read, so an operator fixes the posture rather than a file.
    // **An exhaustive MATCH and not an `==`**, for the reason `sutura-cli`'s copy states at length:
    // a third `SourcePosture` would fall through an `==` and be OPENED. Both roots, one edit.
    match *identity.posture() {
        sutura_domain::source::SourcePosture::SharedServiceUser { .. } => {}
        sutura_domain::source::SourcePosture::ImpersonationAtSource => {
            return Err(format!(
                "`sources.{source}` is `impersonation-at-source`, and this build does not attach a \
                 broker that exchanges a subject's credential to a served `BigQuery` source - \
                 refusing rather than reading every row as this process; no fallback"
            ));
        }
    }
    // **`within_request_timeout` and NOT `parse`, and the difference is a bug that would only show up
    // under load.** What a job may spend is not the request timeout: an answer makes
    // `QueryDeadline::CALLS_PER_ANSWER` calls and each pays a connect margin on top of its own budget,
    // so a 30-second deadline inside a 30-second request timeout overruns the transport that promised
    // it. That arithmetic lives in the adapter, next to the constant it depends on, which is why a
    // composition root asks for the SHARE rather than computing one.
    let deadline = QueryDeadline::within_request_timeout(request_timeout.seconds())
        .map_err(|cause| format!("`server.request_timeout_seconds` leaves no BigQuery job deadline: {cause}"))?;
    let ceiling = BytesBilledCeiling::parse(max_bytes_billed)
        .map_err(|cause| format!("`sources.{source}.max_bytes_billed` is not a usable ceiling: {cause}"))?;
    let bounds = JobBounds::of(deadline, ceiling);
    // Read at BOOT rather than on the first question, which is the same argument the inbound key set
    // is read before the listener opens: a credential file that is missing, unreadable or not a
    // credential has to stop the process, not become a deployment that answers every question with a
    // failure while its startup log says it opened a dataset.
    // ONE agent, cloned, and not two `pinned` calls - which is what `Credential::read` taking an agent
    // is for: the token exchange and the job then share one connection pool and one set of pins by
    // construction rather than because two call sites happened to pass the same bounds. `WireAgent` is
    // `Clone` and a `ureq::Agent`'s clone shares its pool, so the clone is the cheap half of that.
    let agent = WireAgent::pinned(bounds);
    let credentials = Credential::read(&CredentialFile::at(credential_file.clone()), agent.clone())
        .map_err(|cause| format!("`sources.{source}.credential_file` could not be read: {}", flatten(cause)))?;
    // The two resource newtypes are parsed a SECOND time here, and that is not a redundant check: the
    // settings tree's `BillingProject` and the transport's `ProjectId` are two types in two crates,
    // and the one whose value is written into a request path is the transport's. Neither can be
    // reached from the other without going through a `parse`.
    let project = WireProject::parse(billing_project.as_str())
        .map_err(|cause| format!("`sources.{source}.billing_project` is not a usable project id: {cause}"))?;
    let dataset = WireDataset::parse(dataset.as_str())
        .map_err(|cause| format!("`sources.{source}.dataset` is not a usable dataset id: {cause}"))?;
    Ok(sutura_exec_bigquery::BigQueryWarehouse::new(
        source.clone(),
        identity.posture().clone(),
        project,
        dataset,
        BigQueryWire::new(agent, credentials),
    ))
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

/// Registers one model's file, preferring Parquet.
fn attach(engine: &DataFusionWarehouse, table: &TableName, data: &std::path::Path) -> Result<(), String> {
    let parquet = data.join(format!("{table}.parquet"));
    if parquet.is_file() {
        return engine.attach_parquet(table, &parquet).map_err(flatten);
    }
    let csv = data.join(format!("{table}.csv"));
    if csv.is_file() {
        return engine.attach_csv(table, &csv).map_err(flatten);
    }
    Err(format!(
        "the model behind table {table} needs {} or {}, and neither is there",
        csv.display(),
        parquet.display()
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
