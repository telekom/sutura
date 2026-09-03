//! One `files` source becomes the in-process engine: a declared directory, or the built-in one.
//!
//! **The half of [`crate::sources`] that belongs to a single kind**, split out when that file
//! crossed the 1000-line cap. The parent keeps what belongs to no kind in particular: the shared
//! shapes, the settings door, the `kind:` dispatch and the two-load table check.
//!
//! Its suite is at the bottom of this file rather than in the parent, and that is deliberate: an
//! impl file whose tests live one module up is the partition `cargo xtask test-causality` reads as a
//! false *green against base behaviour*.

use std::collections::BTreeSet;
use std::path::Path;

use sutura_domain::model::{ModelName, SourceName, TableName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_exec_datafusion::DataFusionWarehouse;

use crate::commands::render;
use crate::sources::{BUILT_IN_SOURCE, Opened, working_set};

/// What one files source becomes: the registry a plan is looked up in, and the tables attached to it.
///
/// Named because the pair is over `clippy::type_complexity` once the generic warehouse is spelled
/// out - the same reason `mcp.rs` aliases the service it composes.
pub(super) type OpenedFiles = (sutura_app::Warehouses<DataFusionWarehouse>, BTreeSet<TableName>);
/// Refuses a command-line directory that DISAGREES with the one a files entry declared.
///
/// **Agreement is not a conflict, and that is the second half of the fix.** The quickstart line -
/// `sutura query <catalog-dir> <question.yaml> <data-dir>`, which `README.md` and
/// `docs/getting-started.md` both print - names the same directory an operator's own
/// `sources.local.data_dir` would; refusing it made the documented command fail for anybody who also
/// runs `sutura-serve` and therefore has `SUTURA_CONFIG_DIR` exported. So two answers that AGREE are
/// one answer, and only a genuine disagreement is refused.
///
/// **Canonicalised where the filesystem allows it, compared literally where it does not.** A
/// directory that is not there cannot be canonicalised, and that case belongs to the attach step -
/// which names the file it wanted - rather than to a comparison that would report the wrong problem.
///
/// The remedy is the one that always works: drop the argument. The other one an earlier version
/// offered - remove the entry - is deliberately absent, because it is true only for the source the
/// built-in declaration answers to.
///
/// # Errors
///
/// The two directories naming different places.
pub(super) fn refuse_a_second_directory(source: &SourceName, declared: &Path, given: Option<&Path>) -> Result<(), String> {
    let Some(given) = given else {
        return Ok(());
    };
    let same = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if same(declared) == same(given) {
        return Ok(());
    }
    Err(format!(
        "`sources.{source}.data_dir` is {}, and {} was given on the command line - two answers to \
         one question, and they do not name the same directory. Drop the argument: the entry is \
         what a served deployment would read, so it is the one that has to be right",
        declared.display(),
        given.display()
    ))
}

/// Opens the built-in `files` declaration for a source the deployment did not declare.
///
/// The name comparison lives here and only here - see this module's own head for why it survived the
/// registry landing.
pub(super) fn from_the_built_in_declaration(
    pinned: &PinnedDefinitions,
    source: &SourceName,
    data: Option<&Path>,
    runtime: sutura_config::RuntimeSettings,
) -> Result<Opened, String> {
    if source.as_str() != BUILT_IN_SOURCE {
        return Err(format!(
            "this catalog reads from {source}, and no `sources.{source}` entry declares where that \
             data system is or which identity a query reaches it as. This command's own declaration \
             is a files source called {BUILT_IN_SOURCE} over the directory given to it, so it will \
             not answer {source}'s certified numbers out of that directory. Declare \
             `sources.{source}` in the configuration directory {} names, or point the models at \
             {BUILT_IN_SOURCE}",
            sutura_config::CONFIG_DIR_VARIABLE
        ));
    }
    let data = data.ok_or_else(|| {
        format!(
            "this catalog reads from {source}, no `sources.{source}` entry declares it, and no data \
             directory was given - so there is nothing to read. Pass the directory holding one CSV or \
             Parquet file per model"
        )
    })?;
    let declared = single_user_acknowledgement()?;
    let posture = sutura_domain::source::SourcePosture::SharedServiceUser {
        declared: declared.clone(),
    };
    let (engines, attached) = open(source, &posture, pinned, data, runtime)?;
    Ok(Opened {
        engines,
        attached: Some(attached),
        // Declared in code rather than in a file, which is what `for_one_shared_source` exists for:
        // what the leg presents and what the adapter was opened with come from ONE sentence.
        broker: sutura_config::StaticCredentialBroker::for_one_shared_source(source.clone(), declared),
    })
}

/// The engine over a directory, with one file registered per model on this source.
///
/// The engine reads the files itself, so there is no database to create and nothing to keep in step
/// with the CSVs. A `.parquet` beside a model's table name is preferred over a `.csv` because it
/// carries its own types; a CSV has to be sniffed.
///
/// **The posture is an argument rather than a constant, and the cross-check is here rather than in
/// `sutura-config`.** Which posture a deployment asks for is configuration; whether the LINKED adapter
/// can carry a per-subject credential at all is a property of the build, and the settings tree cannot
/// see which adapters were compiled in. `SourcePosture::deliverable_by` is the one function that
/// compares them, and it is called against THIS adapter's own constant - the same call
/// `sutura-serve`'s `build_engine` makes.
///
/// **The table set comes back because it is evidence rather than bookkeeping.** [`refuse_unattached`]
/// compares it against the bundle a service re-loads, and the two bundles are two loads.
pub(super) fn open(
    source: &SourceName,
    posture: &sutura_domain::source::SourcePosture,
    pinned: &PinnedDefinitions,
    data: &Path,
    runtime: sutura_config::RuntimeSettings,
) -> Result<OpenedFiles, String> {
    posture
        .deliverable_by(
            <DataFusionWarehouse as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(|cause| render(&cause))?;
    let engine =
        DataFusionWarehouse::new(source.clone(), posture.clone(), working_set(runtime)).map_err(|cause| render(&cause))?;
    let mut attached: BTreeSet<TableName> = BTreeSet::new();
    for model in pinned
        .definitions()
        .models()
        .values()
        .filter(|model| model.source() == source)
    {
        // Refused here rather than at query time, so a catalog this command cannot serve fails the
        // command instead of failing the question. The engine registers one file per model with
        // nothing above it, so there is nothing for a dataset or a project to name - and dropping the
        // qualifier would read the file of that name and answer about it.
        if model.table().qualifier().is_some() {
            return Err(format!(
                "model {} names the table {}, and this command registers one file per model with \
                 nothing above it. A qualified table needs a data system that resolves one",
                model.name(),
                model.table()
            ));
        }
        attach(&engine, model.name(), model.table_name(), data)?;
        // Collected AFTER the successful attach, like `sutura-serve`'s `open_files`: `attach` fails
        // the command on a missing file, so this set is what the engine holds rather than what was
        // asked for.
        attached.insert(model.table_name().clone());
    }
    Ok((sutura_app::Warehouses::of(engine), attached))
}
/// The acknowledgement this command's own built-in declaration carries.
///
/// The reason is the operator's, and here the operator is whoever typed the command: the sentence says
/// what is true of this tool rather than describing a deployment it is not. It goes through
/// `AcknowledgementReason::parse` like any other, so it is bounded and checked by the same code a
/// configuration file's is - the difference is who wrote the sentence, not whether one exists.
///
/// **It returns the WITNESS rather than the posture**, which is what removes a branch that could not
/// be taken: the caller needs both the posture (for the adapter) and the witness (for the broker), and
/// a function returning the posture forced a `let ... else` back out of it whose message named a
/// function rather than anything an operator could do. One value, built once, used twice.
fn single_user_acknowledgement() -> Result<sutura_domain::source::SharedIdentityDeclared, String> {
    let reason = sutura_domain::source::AcknowledgementReason::parse(
        "the sutura command reads the files of whoever ran it, as that person's own operating-system identity",
    )
    .map_err(|cause| render(&cause))?;
    Ok(sutura_domain::source::SharedIdentityDeclared::of(reason))
}
/// Registers one model's file, preferring Parquet.
fn attach(engine: &DataFusionWarehouse, model: &ModelName, table: &TableName, data: &Path) -> Result<(), String> {
    let parquet = data.join(format!("{table}.parquet"));
    if parquet.is_file() {
        return engine.attach_parquet(table, &parquet).map_err(|cause| render(&cause));
    }
    let csv = data.join(format!("{table}.csv"));
    if csv.is_file() {
        return engine.attach_csv(table, &csv).map_err(|cause| render(&cause));
    }
    Err(format!(
        "model {model} needs {} or {}, and neither is there",
        csv.display(),
        parquet.display()
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::sources::{BUILT_IN_SOURCE, bundle_naming, bundle_over, declaring, open_engine, runtime};

    /// The example deployment this suite reads its catalog and data from.
    fn example() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
    }

    /// The example's own data directory, as an absolute path.
    ///
    /// Absolute because `sources.<alias>.data_dir` is parsed absolute - a service's working directory
    /// is whatever its supervisor chose - so a relative one is a settings refusal rather than a
    /// declaration this suite could make.
    fn data_dir() -> String {
        example()
            .join("data")
            .canonicalize()
            .expect("the example's data directory is there")
            .to_string_lossy()
            .into_owned()
    }

    /// A files entry over the example's data, for one alias.
    fn declaring_files(alias: &str) -> sutura_config::SourceRegistry {
        declaring(
            alias,
            &format!("    kind: files\n    data_dir: \"{}\"", data_dir()),
            "shared-service-user",
        )
    }

    /// An empty registry: the ordinary command-line case, where nothing was configured.
    fn nothing_declared() -> sutura_config::SourceRegistry {
        sutura_config::SourceRegistry::default()
    }

    #[test]
    fn a_declared_directory_and_a_different_one_on_the_command_line_is_two_answers_to_one_question() {
        // Neither ordering is honest, so neither is chosen. Preferring the argument makes
        // `sources.warehouse.data_dir` a value somebody wrote and nothing read; preferring the entry
        // makes the directory a person just typed do nothing. Both are silent, which is the failure
        // mode this repository refuses everywhere else.
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_files("warehouse"),
            runtime(),
            Some(&example().join("catalog")),
        )
        .map(|_| ())
        .expect_err("a declaration and a DIFFERENT argument for one data system is a refusal");
        assert!(error.contains("two answers to one question"), "{error}");
        assert!(
            error.contains("sources.warehouse.data_dir"),
            "the entry is not named: {error}"
        );
        // **THE REMEDY IS ASSERTED, which is the half that was missing.** Review reproduced a closed
        // loop: the old message offered "remove the `sources.<alias>` entry to read that directory as
        // a files source", and following it got the undeclared-source refusal telling you to put the
        // entry back - true only for a source named `local`, which is the one case where the entry is
        // redundant anyway. So that remedy is gone and this asserts the one that always works.
        assert!(
            error.contains("Drop the argument"),
            "the refusal must name a remedy that works: {error}"
        );
        assert!(
            !error.contains("remove the"),
            "the remedy that only works for the built-in name must not be offered: {error}"
        );
    }
    #[test]
    fn a_declared_directory_and_the_same_one_on_the_command_line_is_one_answer() {
        // **The quickstart, reproduced.** `sutura query <catalog-dir> <question.yaml> <data-dir>` is
        // the line `README.md` and `docs/getting-started.md` both print, and an operator who also
        // runs `sutura-serve` has `SUTURA_CONFIG_DIR` exported - so refusing the documented command
        // for having a configuration directory was a real regression, found by review. Two answers
        // that AGREE are one answer.
        let opened = open_engine(
            &bundle_naming("warehouse"),
            &declaring_files("warehouse"),
            runtime(),
            Some(&example().join("data")),
        )
        .expect("a declared directory and the same one on the command line is not a conflict");
        assert_eq!(
            opened
                .engines
                .postures()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<&str>>(),
            vec!["warehouse"],
            "the entry is still what was opened"
        );
    }
    #[test]
    fn a_declared_posture_the_engine_cannot_deliver_is_refused_before_anything_opens() {
        // `SourcePosture::deliverable_by` against the ENGINE's own constant, which is the call the
        // body lists as a shape note and which review found untested in this crate. The engine
        // declares `NoPlaceForASubject`, so `impersonation-at-source` is a refusal - and it is the
        // adapter's capability that decides, not the deployment's wish.
        let registry = declaring(
            BUILT_IN_SOURCE,
            &format!(
                "    kind: files\n    data_dir: \"{}\"\n{}    verification_identity: \
                 \"sutura_anchor_reader\"\n",
                data_dir(),
                wif_for_files()
            ),
            "impersonation-at-source",
        );
        let error = open_engine(&bundle_naming(BUILT_IN_SOURCE), &registry, runtime(), None)
            .map(|_| ())
            .expect_err("a posture the linked engine cannot carry must not open");
        assert!(error.contains(BUILT_IN_SOURCE), "the refusal must name the source: {error}");
        assert!(
            error.contains("impersonation-at-source"),
            "the refusal must name the posture that cannot be delivered: {error}"
        );
    }

    /// The `workload_identity` block, for a FILES entry that declares the impersonating posture.
    ///
    /// The parse requires the block for any impersonating entry, whatever its kind.
    fn wif_for_files() -> String {
        String::from(
            "    workload_identity:\n      audience: \"//iam.googleapis.com/projects/1/locations/global/\
             workloadIdentityPools/p/providers/sso\"\n      scope: \"https://www.googleapis.com/auth/\
             bigquery.readonly\"\n",
        )
    }
    #[test]
    fn a_model_with_no_file_behind_it_gets_no_engine() {
        // The attach step runs per model AFTER the source is accepted, so this arm is reachable only
        // by a catalog this command can otherwise open. Both candidate paths are asserted because the
        // message is the only thing an operator can act on: that neither extension is present is the
        // failure, and naming the two that were looked for is the difference between a fixable
        // message and "and neither is there".
        let error = open_engine(
            &bundle_over(&[("orders", BUILT_IN_SOURCE, "fct_order")]),
            &nothing_declared(),
            runtime(),
            Some(&example().join("data")),
        )
        .map(|_| ())
        .expect_err("a model with no file behind it must not open");
        assert!(error.contains("fct_order.csv"), "the CSV path is missing: {error}");
        assert!(error.contains("fct_order.parquet"), "the Parquet path is missing: {error}");
        assert!(error.contains("model orders"), "the model is not named: {error}");
    }
}
