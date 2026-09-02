//! How a declared data system becomes an open one, for the commands that answer questions.
//!
//! **The half of the composition root that reads a DECLARATION**, split out of [`crate::commands`]
//! when the hard-coded source name went away: that file holds the commands and their rendering, and
//! this one holds the one question those commands cannot answer themselves - which adapter opens the
//! data system a catalog names, and under which identity.
//!
//! # Two declarations, and the order between them
//!
//! A source is declared in one of two places, and this module reads them in this order:
//!
//! 1. **The deployment's `sources:` tree**, the same one `sutura-serve` reads, layered from the
//!    directory [`sutura_config::CONFIG_DIR_VARIABLE`] names. An entry says what KIND of data system
//!    this is, where it is, and which identity a query reaches it as - so a catalog whose models name
//!    `warehouse` is opened because the deployment said what `warehouse` is, not because the name
//!    happened to match a constant here.
//! 2. **This command's own built-in declaration**, for a source the tree does not name: a `files`
//!    source called [`BUILT_IN_SOURCE`], over the directory given on the command line, read as
//!    whoever ran the command. It is written in code where a reviewer can see it, which is the same
//!    standing [`single_user_posture`]'s sentence has.
//!
//! **The built-in declaration keeps its NAME, and that is the control this module did not drop.**
//! [`open_engine`] used to refuse any source not called `local`, and the reason was a real defect: an
//! engine named after whatever the catalog declared satisfied `sutura-app`'s
//! `plan.source() != warehouse.source()` guard by construction, so a catalog certifying a metric
//! against a real data system got that metric answered out of the caller's own CSV files under the
//! real bundle's digest. What replaces the name comparison for a DECLARED source is the declaration
//! itself - an operator who writes `sources.warehouse.kind: files` with a directory beside it has
//! said the thing the comparison was standing in for. What replaces it for an UNDECLARED one is
//! nothing, so the name still has to match: a bundle naming `production_warehouse` with no entry for
//! it is refused, and told which entry to write.
//!
//! # Which adapter, and which BUILD
//!
//! The `kind:` an entry declares is dispatched by an exhaustive match, so a third kind is a compile
//! error here. Which of the declared kinds THIS BINARY can open is a different question, and it is
//! answered by a `cfg`: [`bigquery::open`] has two definitions of one signature, and the one a build
//! without the `bigquery` feature reaches is a refusal naming that feature. `sutura_config` cannot
//! see a link and must not pretend to, which is why the vocabulary of kinds is the repository's and
//! the set a binary opens is this file's.
//!
//! `Result<_, String>` throughout, for the reason [`crate::commands`] states at its own head.

use std::collections::BTreeSet;
use std::path::Path;

use sutura_domain::model::{ModelName, SourceName, TableName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_exec_datafusion::DataFusionWarehouse;

use crate::commands::render;

/// The `BigQuery` half of this module: the composed adapter, and the refusal a build without it gives.
///
/// **Its own file because `cargo xtask max-lines` fails at 1000 lines rather than warning**, and this
/// one crossed it when the second adapter landed. The split follows the seam that was already there:
/// everything below this line is kind-agnostic or files-only, and everything in that file is behind
/// `#[cfg(feature = "bigquery")]` or is the refusal for its absence. Its suite travels with it, which
/// is deliberate - an impl file whose tests live in the parent is the partition `test-causality`
/// reads as a false *green against base*.
mod bigquery;

/// The data system this command declares for itself, and the name it answers to.
///
/// Read only where the deployment declared nothing: a `sources:` entry beats it, under any name.
/// Renaming this is a catalog edit in every example plus a new digest, not a code change here.
pub(crate) const BUILT_IN_SOURCE: &str = "local";

/// Which adapter this command opened its one source with, and everything the next step needs from it.
///
/// **One variant per LINKED adapter**, which is the shape `sutura-serve`'s `OpenedSources` already
/// holds and for the reason `sutura_app::warehouses` states: `sutura_app::Warehouses<W>` is generic in
/// ONE adapter, so this is not a heterogeneous registry and does not try to be. It is the choice of
/// which registry got built, made once, at the one place that can see both the declaration and the
/// link. A build without the `bigquery` feature has a one-variant enum, and the match in each caller
/// is still exhaustive - which is what makes adding a third adapter a compile error at those call
/// sites rather than a `SourceUnavailable` on the first question.
pub(crate) enum Opened {
    /// The in-process engine over a directory of files.
    Files(OpenedWith<DataFusionWarehouse>),
    /// A `BigQuery` dataset, reached over the wire.
    #[cfg(feature = "bigquery")]
    BigQuery(OpenedWith<bigquery::BigQuerySource>),
}

/// What a command opened over one adapter: the registry a plan is looked up in, what was attached,
/// and what mints for it.
///
/// **A named struct rather than a tuple, and the third field is why.** The broker and the engines have
/// to come from the same decision: a source opened from the `sources:` tree is minted for out of that
/// tree, and one opened from the built-in declaration is minted for out of the sentence beside it. Two
/// call sites deciding that separately is a command whose leg carries an acknowledgement no adapter
/// was opened with - which `Presented::agrees_with` would then refuse at query time, one step too
/// late. One value carries both, so they cannot disagree.
///
/// Generic in the adapter so both arms of [`Opened`] are the same shape, and so a caller writes the
/// answer path once: the monomorphisation ends at whichever `match` arm called it.
pub(crate) struct OpenedWith<W> {
    /// The data systems this command holds, keyed by the name a plan selects them with.
    pub(crate) engines: sutura_app::Warehouses<W>,
    /// Every table this command registered, or `None` for a data system it attached nothing to.
    ///
    /// `None` is not an empty set: an empty set means nothing was registered and the served bundle
    /// had better name nothing either, while `None` means the tables live in the data system and this
    /// process cannot enumerate them. [`refuse_unattached`] is skipped for the second, and the caller
    /// states that narrowing at its own call site.
    pub(crate) attached: Option<BTreeSet<TableName>>,
    /// The broker that mints for whatever was opened above.
    pub(crate) broker: sutura_config::StaticCredentialBroker,
}

/// The settings tree this process was configured with.
///
/// **The same door `sutura prompt` goes through, and that is deliberate rather than convenient.**
/// `Settings::load` runs the deployment refusals, so a configuration that will not serve will not
/// answer a question either - and the alternative is a second, weaker way into the settings whose
/// refusals could differ from the service's. A process with no `SUTURA_CONFIG_DIR` gets the embedded
/// defaults, which declare no source at all; that is the ordinary command-line case, and it is what
/// sends [`open_engine`] to the built-in declaration.
///
/// **The whole tree rather than its `sources:` alone**, because a networked adapter needs a second
/// value out of it: `server.request_timeout_seconds` is the one place a deployment says how long a
/// question may take, and a job bounded by a number invented here would be the drifting duplicate the
/// settings tree exists to prevent. Callers pass the two accessors, which is what `sutura-serve`'s own
/// root does.
///
/// # Errors
///
/// The environment or the configuration being unreadable, unparseable, or a deployment this build
/// refuses to serve - each naming the key to fix.
pub(crate) fn configured() -> Result<sutura_config::Settings, String> {
    let environment = sutura_config::environment_from_process().map_err(|cause| render(&cause))?;
    sutura_config::Settings::load(&sutura_config::Sources::from_process_environment(
        environment,
        sutura_config::config_dir_from_process(),
    ))
    .map_err(|cause| render(&cause))
}

/// Opens the one data system this catalog's models name, and returns what mints for it.
///
/// # Errors
///
/// A catalog with no models or with models on more than one data system; a declared kind this build
/// linked no adapter for; a source the deployment never declared and that is not the built-in one; a
/// posture the linked adapter cannot deliver; and, for a files source, a model with no file behind it.
pub(crate) fn open_engine(
    pinned: &PinnedDefinitions,
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
    data: Option<&Path>,
) -> Result<Opened, String> {
    let sources = sutura_app::sources(pinned);
    let named = match sources.as_slice() {
        [only] => (*only).clone(),
        [] => return Err(String::from("this catalog declares no models, so there is nothing to open")),
        many => {
            return Err(format!(
                "this catalog spans {} data systems, and this command answers one question against \
                 one. Serve it over HTTP, where a plan spanning two sources is split into legs",
                many.len()
            ));
        }
    };
    // A `let`-else rather than a match on the `Option`, because `clippy::option_if_let_else` asks for
    // `map_or_else` and the two closures it wants read as an expression where this reads as an order:
    // the deployment's declaration first, this command's own only if there was none.
    let Some(declared) = registry.get(&named) else {
        return from_the_built_in_declaration(pinned, &named, data);
    };
    from_the_registry(pinned, &named, declared, data, registry, request_timeout)
}

/// What one files source becomes: the registry a plan is looked up in, and the tables attached to it.
///
/// Named because the pair is over `clippy::type_complexity` once the generic warehouse is spelled
/// out - the same reason `mcp.rs` aliases the service it composes.
type OpenedFiles = (sutura_app::Warehouses<DataFusionWarehouse>, BTreeSet<TableName>);

/// Opens a source the deployment declared, under the identity that declaration names.
///
/// **An exhaustive match with no wildcard arm, and it is the one line where "which adapter opens a
/// declared kind" is decided** - the same shape `sutura-serve`'s own dispatcher holds, so a third kind
/// is a compile error in both composition roots rather than an arm that falls through in one of them.
fn from_the_registry(
    pinned: &PinnedDefinitions,
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    data: Option<&Path>,
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
) -> Result<Opened, String> {
    // Refused rather than resolved, because there is no honest order between the two. The argument is
    // what a person typed just now and the entry is what the deployment declared, so preferring
    // either one silently makes the other a value somebody wrote and nothing read.
    if let Some(given) = data {
        return Err(format!(
            "`sources.{source}` declares this data system as `kind: {}`, and {} was given on the \
             command line as well - two answers to one question. Drop the argument, or remove the \
             `sources.{source}` entry to read that directory as a files source",
            configured.kind().as_str(),
            given.display()
        ));
    }
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    match configured.kind() {
        sutura_config::SourceKind::Files => {
            let sutura_config::SourcePlacement::Files { ref data_dir } = *configured.placement() else {
                return Err(format!(
                    "`sources.{source}` reached the files attach step with a placement no linked \
                     adapter reads, which the match above should have dispatched elsewhere"
                ));
            };
            let (engines, attached) = files(source, identity.posture(), pinned, data_dir)?;
            Ok(Opened::Files(OpenedWith {
                engines,
                attached: Some(attached),
                // The deployment's own tree, so every source declared `shared-service-user` is minted
                // for under the acknowledgement its own entry carries - and one declared
                // `impersonation-at-source` gets nothing, which is a refused question rather than a
                // leg that runs as this process.
                broker: sutura_config::StaticCredentialBroker::from_registry(registry),
            }))
        }
        sutura_config::SourceKind::BigQuery => bigquery::open(source, configured, registry, request_timeout),
    }
}

/// Opens the built-in `files` declaration for a source the deployment did not declare.
///
/// The name comparison lives here and only here - see this module's own head for why it survived the
/// registry landing.
fn from_the_built_in_declaration(pinned: &PinnedDefinitions, source: &SourceName, data: Option<&Path>) -> Result<Opened, String> {
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
    let posture = single_user_posture()?;
    let (engines, attached) = files(source, &posture, pinned, data)?;
    let sutura_domain::source::SourcePosture::SharedServiceUser { declared } = posture else {
        return Err(String::from(
            "this command's built-in declaration is shared, one function below",
        ));
    };
    Ok(Opened::Files(OpenedWith {
        engines,
        attached: Some(attached),
        // Declared in code rather than in a file, which is what `for_one_shared_source` exists for:
        // what the leg presents and what the adapter was opened with come from ONE sentence.
        broker: sutura_config::StaticCredentialBroker::for_one_shared_source(source.clone(), declared),
    }))
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
fn files(
    source: &SourceName,
    posture: &sutura_domain::source::SourcePosture,
    pinned: &PinnedDefinitions,
    data: &Path,
) -> Result<OpenedFiles, String> {
    posture
        .deliverable_by(
            <DataFusionWarehouse as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(|cause| render(&cause))?;
    let engine = DataFusionWarehouse::new(source.clone(), posture.clone(), working_set()?).map_err(|cause| render(&cause))?;
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

/// Every table the served bundle's models sit behind.
pub(crate) fn served_tables(served: &PinnedDefinitions) -> BTreeSet<TableName> {
    served
        .definitions()
        .models()
        .values()
        .map(|model| model.table_name().clone())
        .collect()
}

/// The tables the served bundle names, against the tables the engine actually holds.
///
/// Copied from `sutura-serve`'s function of the same name, because the two composition roots are
/// separate binaries and neither may depend on the other. The two sets come from two `load()` calls
/// on the same directory; a model added between them is refused here rather than served with no
/// table behind it, which would fail the first question against it at query time.
///
/// # Errors
///
/// Either set holding a table the other does not.
pub(crate) fn refuse_unattached(serving: &BTreeSet<TableName>, attached: &BTreeSet<TableName>) -> Result<(), String> {
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

/// The posture this command's own built-in declaration carries.
///
/// The reason is the operator's, and here the operator is whoever typed the command: the sentence says
/// what is true of this tool rather than describing a deployment it is not. It goes through
/// `AcknowledgementReason::parse` like any other, so it is bounded and checked by the same code a
/// configuration file's is - the difference is who wrote the sentence, not whether one exists.
fn single_user_posture() -> Result<sutura_domain::source::SourcePosture, String> {
    let reason = sutura_domain::source::AcknowledgementReason::parse(
        "the sutura command reads the files of whoever ran it, as that person's own operating-system identity",
    )
    .map_err(|cause| render(&cause))?;
    Ok(sutura_domain::source::SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(reason),
    })
}

/// The working-set ceiling this command bounds the engine with.
///
/// **The embedded default, and not a flag.** A flag would be a second number an operator could set,
/// disagreeing with `runtime.working_set_max_bytes` in the tree the service reads. It goes through
/// `WorkingSetCeiling::parse` rather than constructing the wrapper from the constant directly, so this
/// reads the same number through the same checks the service does; a test asserts the constant and
/// `defaults.yaml` agree. `available_memory_bytes` is asked here too: a laptop with less memory than
/// the default ceiling should be told so rather than dying inside a join.
///
/// # Errors
///
/// A ceiling above the memory this process can reach.
pub(crate) fn working_set() -> Result<sutura_exec_datafusion::WorkingSet, String> {
    let ceiling = sutura_config::WorkingSetCeiling::parse(
        sutura_config::WorkingSetCeiling::DEFAULT_BYTES,
        sutura_config::available_memory_bytes(),
    )
    .map_err(|cause| render(&cause))?;
    Ok(sutura_exec_datafusion::WorkingSet::of_bytes(ceiling.bytes()))
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

/// A one-source registry as a deployment would have declared it.
///
/// `#[cfg(test)]`, and it goes through `Settings::load` with a YAML overlay rather than building a
/// [`sutura_config::SourceRegistry`] some other way - which it could not anyway, because that type's
/// parse is crate-visible there. The point of every test below that uses it is that the entry was
/// PARSED the way an operator's file is parsed.
#[cfg(test)]
fn declaring(alias: &str, body: &str, posture: &str) -> sutura_config::SourceRegistry {
    // `single-user` with its own reason, which is what lets a `shared-service-user` entry take the
    // mode's acknowledgement as its witness - a per-source `acknowledged_because` would work too, and
    // the mode is the honest one for a command-line tool.
    let overlay = format!(
        "security:\n  identity: single-user\n  single_user_because: \"one developer, one laptop, one \
         set of files\"\nsources:\n  {alias}:\n    posture: \"{posture}\"\n{body}\n"
    );
    sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("the test overlay is a servable deployment")
    .sources()
    .clone()
}

/// A pinned bundle whose one model claims to live in `source`, over a table the example's data
/// directory happens to have a file for.
///
/// Module level rather than inside [`tests`] because [`bigquery`]'s own suite needs it too, and
/// `#[cfg(test)]` so it costs a real build nothing. The FILE is what makes a refusal mean something:
/// without it the wrong branch would fail on a missing CSV and be indistinguishable from the branch
/// under test.
#[cfg(test)]
fn bundle_naming(source: &str) -> PinnedDefinitions {
    use sutura_domain::catalog::{Definitions, Description, Metric, Model};
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName};

    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("customers").expect("a test model is a model"),
        SourceName::parse(source).expect("a test source is a source"),
        TableName::parse("dim_customer").expect("a test table is a table"),
        std::collections::BTreeSet::from([column("customer_key"), column("signed_at")]),
        Description::default(),
    );
    let metric = Metric::new(
        MetricName::parse("customers_signed").expect("a test metric is a metric"),
        ModelName::parse("customers").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Count,
            column("customer_key"),
        ))),
        Vec::new(),
        column("signed_at"),
        std::collections::BTreeSet::from([Grain::Month]),
        std::collections::BTreeMap::new(),
        None,
        Description::default(),
    );
    pin(Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent"))
}

/// The pinning half of the bundle builders, so the manifest key is written once.
#[cfg(test)]
fn pin(definitions: sutura_domain::catalog::Definitions) -> PinnedDefinitions {
    use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion};

    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        sutura_domain::knowledge::Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse(crate::commands::CATALOG_SOURCE).expect("the built-in catalog name is a name"),
            Contribution::of(sutura_domain::capabilities::MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

/// The request timeout every case hands in: the EMBEDDED default, read through `Settings::load`,
/// which is the number a command with no configuration directory would use. Written this way rather
/// than as a literal so a change to `defaults.yaml` reaches these tests.
///
/// Module level for the reason [`bundle_naming`] gives.
#[cfg(test)]
fn timeout() -> sutura_config::RequestTimeout {
    sutura_config::Settings::load(&sutura_config::Sources::defaults(sutura_config::Environment::Development))
        .expect("the embedded defaults are a servable development deployment")
        .server()
        .request_timeout()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use sutura_domain::catalog::{Definitions, Description, Model};
    use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
    use sutura_domain::pinned::PinnedDefinitions;

    use sutura_exec_datafusion::DataFusionWarehouse;

    use super::{
        BUILT_IN_SOURCE, Opened, OpenedWith, bundle_naming, declaring, open_engine, pin, refuse_unattached, served_tables,
        timeout,
    };

    /// The files registry `open_engine` produced, or a failure saying which arm it took instead.
    ///
    /// An exhaustive match rather than an `if let`, so a third linked adapter is a compile error in
    /// this suite too - the same property the two commands' own matches carry.
    fn files_of(opened: Opened) -> OpenedWith<DataFusionWarehouse> {
        match opened {
            Opened::Files(opened) => Some(opened),
            #[cfg(feature = "bigquery")]
            Opened::BigQuery(_) => None,
        }
        .expect("this fixture declares a files source")
    }

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

    /// One model as a catalog document names it: the model, its data system, its table.
    type DeclaredModel<'raw> = (&'raw str, &'raw str, &'raw str);

    /// A pinned bundle over exactly the models given, and no metrics.
    ///
    /// Models are all [`open_engine`] reads: [`sutura_app::sources`] maps over them and the attach
    /// step is called once per model, so a metric would add nothing any arm of that function looks at.
    fn bundle_over(models: &[DeclaredModel<'_>]) -> PinnedDefinitions {
        let declared: Vec<Model> = models
            .iter()
            .map(|&(model, source, table)| {
                Model::new(
                    ModelName::parse(model).expect("a test model is a model"),
                    SourceName::parse(source).expect("a test source is a source"),
                    TableName::parse(table).expect("a test table is a table"),
                    BTreeSet::from([ColumnName::parse("customer_key").expect("a test column is a column")]),
                    Description::default(),
                )
            })
            .collect();
        pin(Definitions::assemble(declared, vec![], vec![]).expect("the test bundle is consistent"))
    }

    #[test]
    fn a_files_source_the_deployment_declared_is_opened_under_its_own_name() {
        // THE OUTCOME issue 121 asks for: a catalog whose models name `warehouse` is answered,
        // because the deployment said what `warehouse` is. Before this, this function compared the
        // declared source against one constant and refused everything else, so the only catalog the
        // published binary could open was one that happened to call its data system `local`.
        let opened = files_of(
            open_engine(&bundle_naming("warehouse"), &declaring_files("warehouse"), timeout(), None)
                .expect("a declared files source opens"),
        );
        assert_eq!(
            opened
                .engines
                .postures()
                .map(|(name, posture)| (name.as_str(), posture.as_str()))
                .collect::<Vec<(&str, &str)>>(),
            vec![("warehouse", "shared-service-user")],
            "the engine answers to the DECLARED name, under the identity that declaration carries"
        );
        assert!(
            opened.attached.is_some(),
            "a files source attaches, so there is a table set to compare a re-load against"
        );
        // The broker has to have come from the same decision, or the leg would carry an
        // acknowledgement no adapter was opened with. One entry, for the one declared source.
        assert_eq!(opened.broker.count(), 1, "the deployment's own tree is what mints for it");
    }

    #[test]
    fn a_source_the_deployment_never_declared_gets_the_built_in_files_source_only_under_its_own_name() {
        // THE BUG THE OLD NAME COMPARISON EXISTED FOR, kept: with nothing declared there is no
        // statement anywhere that `production_warehouse` is a directory of files, so an engine
        // wearing that name over the caller's own CSVs would answer that catalog's certified metric
        // out of them, stamped with the real bundle's version and digest - and `sutura-app`'s
        // `plan.source() != warehouse.source()` guard could not fire, because naming the engine after
        // the catalog satisfies it by construction.
        //
        // `dim_customer.csv` is in the example's data directory, so nothing else fails either.
        let error = open_engine(
            &bundle_naming("production_warehouse"),
            &nothing_declared(),
            timeout(),
            Some(&example().join("data")),
        )
        .map(|_| ())
        .expect_err("an undeclared source must not get the built-in declaration's engine");
        assert!(
            error.contains("sources.production_warehouse"),
            "the refusal must name the entry to write: {error}"
        );
        assert!(
            error.contains(sutura_config::CONFIG_DIR_VARIABLE),
            "the refusal must say where that entry goes: {error}"
        );
    }

    #[test]
    fn the_built_in_declaration_still_opens_the_documented_example() {
        // The other half: a gate is worth nothing if it also refuses the catalog the quickstart tells
        // a reader to run, with no configuration at all. This is the path
        // `crates/sutura-cli/tests/example.rs` and `docs/getting-started.md` both take.
        let pinned = crate::commands::load(&example().join("catalog")).expect("the example catalog loads");
        let opened = files_of(
            open_engine(&pinned, &nothing_declared(), timeout(), Some(&example().join("data")))
                .expect("the example catalog opens with nothing declared"),
        );
        assert_eq!(
            opened
                .engines
                .postures()
                .map(|(name, posture)| (name.as_str(), posture.as_str()))
                .collect::<Vec<(&str, &str)>>(),
            vec![(BUILT_IN_SOURCE, "shared-service-user")],
            "the built-in declaration keeps its own name and states the identity it reads under"
        );
        assert_eq!(
            opened
                .attached
                .as_ref()
                .map(|tables| tables.iter().map(TableName::as_str).collect::<Vec<&str>>()),
            Some(served_tables(&pinned).iter().map(TableName::as_str).collect::<Vec<&str>>()),
            "the attached set is what the served bundle names"
        );
    }

    #[test]
    fn a_declared_source_and_a_data_directory_on_the_command_line_is_two_answers_to_one_question() {
        // Neither ordering is honest, so neither is chosen. Preferring the argument makes
        // `sources.warehouse.data_dir` a value somebody wrote and nothing read; preferring the entry
        // makes the directory a person just typed do nothing. Both are silent, which is the failure
        // mode this repository refuses everywhere else.
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_files("warehouse"),
            timeout(),
            Some(&example().join("data")),
        )
        .map(|_| ())
        .expect_err("a declaration and an argument for one data system is a refusal");
        assert!(error.contains("two answers to one question"), "{error}");
        assert!(error.contains("sources.warehouse"), "the entry is not named: {error}");
    }

    #[test]
    fn the_built_in_declaration_with_no_directory_says_what_is_missing() {
        // The arm the argument being OPTIONAL created: `[data-dir]` may be absent because a fully
        // declared deployment needs none, so the case where it is absent AND nothing is declared has
        // to say which of the two to supply rather than failing on a path built from nothing.
        let error = open_engine(&bundle_naming(BUILT_IN_SOURCE), &nothing_declared(), timeout(), None)
            .map(|_| ())
            .expect_err("no declaration and no directory is nothing to read");
        assert!(error.contains("no data directory was given"), "{error}");
    }

    #[test]
    fn a_catalog_spanning_two_data_systems_gets_no_engine() {
        // The arm nothing else proves. This command answers one question against one data system, so
        // a bundle whose models name two gets no engine at all rather than one over whichever half
        // happens to be local.
        //
        // The built-in source is deliberately ONE OF THE PAIR, and its table has a real file in the
        // example's data directory. That is what makes this discriminate: an arm that took the first
        // source instead of refusing would find it, find `dim_customer.csv`, and hand back a working
        // engine serving half a catalog under the whole bundle's digest.
        let error = open_engine(
            &bundle_over(&[
                ("customers", BUILT_IN_SOURCE, "dim_customer"),
                ("products", "production_warehouse", "dim_product"),
            ]),
            &nothing_declared(),
            timeout(),
            Some(&example().join("data")),
        )
        .map(|_| ())
        .expect_err("a catalog spanning two data systems must not get an engine");
        assert!(
            error.contains("spans 2 data systems"),
            "the refusal must say how many it found: {error}"
        );
        // NOT the neighbouring arm, and this is the half that stops the test passing on the wrong
        // branch: `production_warehouse` is also undeclared, so a test that only checked for *a*
        // refusal would be green with the multi-source arm gone.
        assert!(
            !error.contains("sources.production_warehouse"),
            "this is the multi-source arm, not the undeclared-source one: {error}"
        );
    }

    #[test]
    fn a_catalog_declaring_no_models_gets_no_engine() {
        // The empty bundle. An engine opened over nothing would open successfully, because the attach
        // loop has nothing to iterate, and then answer every question as an unknown metric - which
        // reads as a question problem rather than as a catalog directory that holds no models.
        let error = open_engine(
            &bundle_over(&[]),
            &nothing_declared(),
            timeout(),
            Some(&example().join("data")),
        )
        .map(|_| ())
        .expect_err("a catalog with no models opens nothing");
        assert!(error.contains("declares no models"), "{error}");
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
            timeout(),
            Some(&example().join("data")),
        )
        .map(|_| ())
        .expect_err("a model with no file behind it must not open");
        assert!(error.contains("fct_order.csv"), "the CSV path is missing: {error}");
        assert!(error.contains("fct_order.parquet"), "the Parquet path is missing: {error}");
        assert!(error.contains("model orders"), "the model is not named: {error}");
    }

    #[test]
    fn a_catalog_that_changed_between_two_loads_is_refused() {
        // The check the `mcp` command runs after `LocalService::start` re-loads the catalog: the two
        // loads are two `read_all()` calls over one directory, and a model added between them would
        // be served with no table behind it and fail its first question at query time.
        let set = |tables: &[&str]| -> BTreeSet<TableName> {
            tables
                .iter()
                .map(|raw| TableName::parse(raw).expect("a test table is a table"))
                .collect()
        };
        let serving = set(&["orders", "customers"]);
        let attached = set(&["orders"]);
        let error = refuse_unattached(&serving, &attached)
            .expect_err("a table served but never attached is the whole point of the check");
        assert!(error.contains("customers"), "the missing table is named: {error}");
        // Both directions, so a silently dropped model is caught too - the "extra" arm exists because
        // whatever else drifted is the part nobody has looked at.
        let error =
            refuse_unattached(&attached, &serving).expect_err("an attached table no longer served is a bundle that changed");
        assert!(error.contains("customers"), "the extra table is named: {error}");
        // And the two agreeing is not an error.
        refuse_unattached(&serving, &serving).expect("matching sets are fine");
    }
}
