//! The FILES half of this composition root: declared directories become one open engine, with the
//! tables it registered beside it.
//!
//! **Split out of the composition root when a fourth `SourceKind` pushed that file past the
//! unexemptable 1000-line cap**, into the shape its two siblings already had: the dispatcher stays
//! in `serve.rs` and each kind's open-and-build pair lives in a file of its own. Unlike
//! `bigquery.rs` and `postgres.rs` there is no feature-off twin here - the engine is a non-optional
//! dependency of this crate, so `files` is the one kind every build can open.
//!
//! [`Opened`] carries `pub(super)` fields rather than private ones, which is what the move cost:
//! the parent reads both, and a private field is visible to a module's DESCENDANTS rather than to
//! its parent.

use std::collections::BTreeSet;

use sutura_domain::model::{SourceName, TableName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_exec_datafusion::DataFusionWarehouse;

use super::{configured_source, flatten};

/// The open data systems, and the tables they actually hold.
///
/// A named pair rather than a tuple: the second field is evidence for a startup refusal and `.1`
/// would say nothing about which of the two it is. `clippy::type_complexity` asks for the same thing
/// from the other direction.
pub(crate) struct Opened {
    pub(super) engines: sutura_app::Warehouses<DataFusionWarehouse>,
    pub(super) attached: BTreeSet<TableName>,
}

/// Opens the in-process engine for every declared `files` source and registers one file per model.
pub(super) fn open_files(
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
