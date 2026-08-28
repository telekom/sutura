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
//! Because the shipped artifact is one executable with no server in it. The release derivations
//! build `--package sutura-cli`, the image holds that one binary, and adding an HTTP surface to it
//! would put a multi-threaded runtime, an I/O driver, a web framework and a browser asset bundle
//! into all four cross-compiled targets. Whether to pay that is a decision, and it is not this
//! file's to make.
//!
//! # `Result<_, String>` below the surface
//!
//! Used freely, and the boundary gate exempts a binary on purpose: the audience for these messages
//! is a person reading standard error, not code matching on a variant. Every typed error from a
//! library crate is flattened with its whole `#[source]` chain on the way out, because the outermost
//! message is the one that says least.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use sutura_app::surface::Surface as _;
use sutura_catalog_local::LocalCatalog;
use sutura_config::{Environment, Settings, Sources, TlsMaterial};
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog as _};
use sutura_exec_datafusion::DataFusionWarehouse;
use sutura_http::{LocalService, ServiceState};
use sutura_runtime::{Shutdown, banner, shutdown, telemetry};

/// The only data system this build can open.
///
/// The engine is this process, over the files in the configured data directory. A catalog naming
/// anything else gets no engine rather than one wearing that data system's name over local files -
/// the same refusal the command-line tool makes, for the same reason.
const ENGINE_SOURCE: &str = "local";

/// The variable that points at a configuration directory.
///
/// Optional: the defaults embedded in `sutura-config` are complete, so a deployment with no files at
/// all is a loopback development service rather than a failure.
const CONFIG_DIR_VARIABLE: &str = "SUTURA_CONFIG_DIR";

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
    let settings = Settings::load(&Sources::from_process_environment(environment, config_dir())).map_err(flatten)?;

    // 4. The log, then the panic hook.
    telemetry::install(settings.telemetry()).map_err(flatten)?;
    sutura_runtime::install_panic_hook();

    // 5. What was resolved, and what this service does not do.
    banner::announce(&settings);

    // 6. The adapters, then the service. Both ports are named exactly here.
    let catalog = LocalCatalog::new(PathBuf::from(settings.catalog().dir()), settings.catalog().version().clone());
    let pinned = catalog.load().map_err(flatten)?;
    let opened = open_engine(&pinned, settings.catalog().data_dir(), settings.runtime().engine_workers())?;
    // `LocalService::start` loads through the catalog port a SECOND time rather than being handed
    // the bundle above, and that is deliberate: the bundle it validates has to be the bundle it
    // serves, and the only way to guarantee that is for the same call to do both. The load above
    // exists so the engine can be opened for the sources the catalog actually names, which has to
    // happen first.
    let service = LocalService::start(&catalog, opened.engine).map_err(flatten)?;
    // And this closes the gap between the two loads. `attached` is what the FIRST bundle's models
    // needed; the service serves the SECOND. A model added to the catalog directory between the two
    // calls is therefore served with no table registered behind it, and `answer` cannot see that -
    // its only check on the engine is that the source NAME matches. The failure would arrive as a
    // query-time error for whoever asked first, which is precisely the trade this startup sequence
    // exists to avoid: a bundle that does not hold together must stop the process, not one question.
    refuse_unattached(&served_tables(service.definitions()), &opened.attached)?;
    tracing::info!(
        definition_version = %settings.catalog().version(),
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
    let state = ServiceState::new(Arc::new(service), Arc::new(settings));
    let router = sutura_http::router(&state).map_err(flatten)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|cause| format!("the async runtime could not be built: {cause}"))?;
    // Built out here rather than inside the served future, because what happens AFTER serving
    // returns needs to know how much of the grace period the drain spent. `tokio::sync` needs no
    // runtime entered, so this is safe on this side of `block_on`.
    let stopping = Shutdown::with_grace(grace);
    let served = runtime.block_on(serve_until_stopped(router, address, material, stopping.clone()));
    stop(runtime, &stopping);
    served
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
    stopping: Shutdown,
) -> Result<(), String> {
    // Detached on purpose: the task's only job is to translate the first signal into the shared
    // flag, and `serve` below is what waits on it. Joining it would mean waiting for a signal that
    // may never arrive.
    drop(tokio::spawn(shutdown::listen(stopping.clone())));
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
    println!("  2. <dir>/base.yaml, if {CONFIG_DIR_VARIABLE} names a directory holding one");
    println!("  3. <dir>/<environment>.yaml");
    println!("  4. one variable per key, such as SUTURA__SERVER__PORT");
    println!();
    println!("  {:<22} one of: {}", sutura_config::ENVIRONMENT_VARIABLE, environments());
    println!("  {CONFIG_DIR_VARIABLE:<22} a directory of YAML overrides, optional");
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

/// The configuration directory, if one was named.
fn config_dir() -> Option<PathBuf> {
    std::env::var_os(CONFIG_DIR_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// An open engine, and the tables it actually holds.
///
/// A named pair rather than a tuple: the second field is evidence for a startup refusal and `.1`
/// would say nothing about which of the two it is. `clippy::type_complexity` asks for the same thing
/// from the other direction.
struct Opened {
    engine: DataFusionWarehouse,
    attached: BTreeSet<TableName>,
}

/// Every table the served bundle's models sit behind.
fn served_tables(served: &PinnedDefinitions) -> BTreeSet<TableName> {
    served
        .definitions()
        .models()
        .values()
        .map(|model| model.table().clone())
        .collect()
}

/// The tables the served bundle names, against the tables the engine actually holds.
///
/// Two sets rather than a bundle and a set, so the comparison is unit-testable without a digest, a
/// knowledge declaration and an engine - [`served_tables`] is the other half and is one map over a
/// public accessor.
///
/// Both directions are refused, and the second is not pedantry: a table attached for a model the
/// served bundle no longer names means the catalog directory changed between two loads seconds
/// apart, and whatever else moved with it is the part nobody has looked at.
fn refuse_unattached(serving: &BTreeSet<TableName>, attached: &BTreeSet<TableName>) -> Result<(), String> {
    let missing = names(serving.difference(attached));
    let extra = names(attached.difference(serving));
    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }
    Err(format!(
        "the catalog changed while this process was starting: the engine was opened for the bundle \
         loaded first, and the bundle being served names different tables. Served with no table \
         attached: [{missing}]. Attached and no longer served: [{extra}]. Refusing to serve a model \
         whose questions would fail at query time"
    ))
}

/// One line of table names, for a message an operator has to act on.
fn names<'table>(tables: impl Iterator<Item = &'table TableName>) -> String {
    tables.map(TableName::as_str).collect::<Vec<&str>>().join(", ")
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
fn open_engine(
    pinned: &PinnedDefinitions,
    data: &std::path::Path,
    workers: sutura_config::EngineWorkers,
) -> Result<Opened, String> {
    let declared = match sutura_app::sources(pinned).as_slice() {
        [only] => (*only).clone(),
        [] => return Err(String::from("this catalog declares no models, so there is nothing to open")),
        many => {
            return Err(format!(
                "this catalog spans {} data systems, and a plan runs against one",
                many.len()
            ));
        }
    };
    let engine_source =
        SourceName::parse(ENGINE_SOURCE).map_err(|cause| format!("the built-in engine source name is not a name: {cause}"))?;
    if declared != engine_source {
        return Err(format!(
            "this catalog reads from {declared}, and this build has no adapter for it: the only \
             data system it can open is {engine_source}, the in-process engine over the CSV and \
             Parquet files in {}",
            data.display()
        ));
    }
    // `NonZeroUsize::MIN` is unreachable: `EngineWorkers::parse` refuses a zero and resolves an
    // absent key from the machine, which reports at least one. Written as a fallback rather than an
    // unwrap because the workspace denies both, and because one worker is the safe direction to fail
    // in - a narrow engine is slow, and a zero-width runtime does not build.
    let width = core::num::NonZeroUsize::new(workers.count()).unwrap_or(core::num::NonZeroUsize::MIN);
    let engine = DataFusionWarehouse::with_worker_threads(engine_source, width).map_err(flatten)?;
    let mut attached: BTreeSet<TableName> = BTreeSet::new();
    for model in pinned.definitions().models().values() {
        attach(&engine, model.table(), data)?;
        // Collected AFTER the attach, so this set is what the engine holds rather than what was
        // asked for. `attach` fails the startup on a missing file, so the two cannot diverge here -
        // and recording it from the successful call rather than from the model list is what keeps
        // that true if it ever gains a path that can skip one.
        attached.insert(model.table().clone());
    }
    Ok(Opened { engine, attached })
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sutura_domain::model::TableName;

    use super::refuse_unattached;

    fn tables(names: &[&str]) -> BTreeSet<TableName> {
        names
            .iter()
            .map(|raw| TableName::parse(raw).expect("a test table is a table"))
            .collect()
    }

    #[test]
    fn a_model_the_engine_has_no_table_for_stops_the_process() {
        // The startup sequence loads the catalog TWICE - the engine is opened for the first bundle
        // and the service validates and serves the second - so a model added to the catalog
        // directory between the two calls was served with nothing attached behind it. `answer`
        // cannot catch that: its only check on the engine is that the source NAME matches, so the
        // first question about the new metric came back as an error from the engine rather than as a
        // refusal at startup.
        let err = refuse_unattached(
            &tables(&["fact_subscription", "dim_customer"]),
            &tables(&["fact_subscription"]),
        )
        .expect_err("a served model with no attached table does not serve");
        assert!(err.contains("Served with no table attached: [dim_customer]"), "{err}");
        assert!(err.contains("Attached and no longer served: []"), "{err}");
        assert!(err.contains("the catalog changed while this process was starting"), "{err}");
    }

    #[test]
    fn a_table_attached_for_a_model_no_longer_served_stops_it_too() {
        // The other direction, and not pedantry: it means the catalog directory changed between two
        // loads seconds apart. This one would answer every question correctly, which is exactly why
        // it has to be loud - whatever else moved in that edit is the part nobody has looked at.
        let err = refuse_unattached(
            &tables(&["fact_subscription"]),
            &tables(&["fact_subscription", "dim_customer"]),
        )
        .expect_err("an attached table for nothing served does not serve");
        assert!(err.contains("Served with no table attached: []"), "{err}");
        assert!(err.contains("Attached and no longer served: [dim_customer]"), "{err}");
    }

    #[test]
    fn the_two_bundles_agreeing_is_the_ordinary_case_and_starts() {
        // The check has to be silent when nothing changed, which is every start. An empty catalog is
        // already refused earlier, by `open_engine`, so the empty pair is not a case this decides.
        refuse_unattached(&tables(&["fact_subscription"]), &tables(&["fact_subscription"]))
            .expect("two bundles that agree start");
        assert!(
            refuse_unattached(
                &tables(&["dim_customer", "fact_subscription"]),
                &tables(&["fact_subscription", "dim_customer"])
            )
            .is_ok(),
            "the comparison is over sets, so declaration order is not a difference"
        );
    }
}
