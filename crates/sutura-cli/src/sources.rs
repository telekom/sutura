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
//!    standing `files::single_user_acknowledgement`'s sentence has.
//!
//! **The built-in declaration keeps its NAME, and that is the control this module did not drop.**
//! [`open_engine`] used to refuse any source not called `local`, and the reason is a LOOKUP rather
//! than a comparison - which is worth stating precisely, because an earlier version of this paragraph
//! named a `plan.source() != warehouse.source()` guard that does not exist anywhere in the tree, and
//! `sutura_app`'s own source says the opposite in as many words: *"The plan SELECTS its warehouse -
//! it is not compared against one."*
//!
//! What actually happens is `warehouses.get(plan.source())` - in `sutura_app::answer` and again in
//! `verify_anchors` - keyed on the name the CATALOG declared. So an engine registered under whatever
//! the catalog said makes that lookup succeed by construction: nothing refuses, and a bundle
//! certifying a metric against a real data system gets that metric answered out of the caller's own
//! CSV files under the real bundle's digest. An engine registered under a FIXED name misses instead,
//! and a miss is `RefusalReason::SourceUnavailable` / `NotExecutedReason::SourceNotConfigured` - a
//! clean refusal.
//!
//! `files::open` names the engine after the declared source, which is what a registry is for and is what
//! makes the name check load-bearing HERE: what replaces it for a DECLARED source is the declaration
//! itself - an operator who writes `sources.warehouse.kind: files` with a directory beside it has
//! said where that data system is. What replaces it for an UNDECLARED one is nothing, so the name
//! still has to match: a bundle naming `production_warehouse` with no entry for it is refused, and
//! told which entry to write.
//!
//! # The limit a declared `files` source introduces, stated next to the claim
//!
//! A declaration says where a data system is. It does **not** say the files there hold the data the
//! bundle certifies, and nothing in this module tree checks that: the only thing that does is an ANCHOR,
//! `sutura_app::verify_anchors` walks `pinned.anchored_metrics()`, and [`refuse_unattached`] compares
//! table NAMES and never content. So a bundle whose metrics declare no anchor is answered under its
//! real digest out of whatever directory the entry points at. That is parity with `sutura-serve`
//! rather than a hole this module opened - and it is stated here because this is the change that
//! makes it the documented command-line workflow.
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

use sutura_domain::model::{SourceName, TableName};
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

/// The FILES half of this module: the in-process engine over a directory, and the built-in
/// declaration that answers when the deployment declared nothing.
///
/// **Its own file for the reason `bigquery`'s is** - `cargo xtask max-lines` fails at 1000 lines
/// rather than warning, and this one crossed it a second time when the review fixes landed. What
/// stays here is what belongs to NEITHER kind: the shared shapes, the settings door, the `kind:`
/// dispatch and the two-load table check both kinds' callers run.
mod files;

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
/// **The limit this introduces, which review found stated nowhere and reproduced.**
/// `Settings::refusals` is a SERVER's refusal set - TLS termination, an access token, rate limiting,
/// an ephemeral port - and `Settings::load` runs all of it. So
/// `SUTURA_ENVIRONMENT=production sutura query …` now stops on `security.access_token`, and a
/// `SUTURA__SERVER__HOST` set off-loopback in a shell stops it on TLS termination, from a command
/// that binds nothing. That worked before this door existed.
///
/// It is kept, because a second weaker door into the settings could refuse differently from the
/// service's and `sutura prompt` already goes through this one. What is FIXED is the message: a
/// refusal an operator cannot act on is the defect, so the wording below says whose refusals these
/// are and names the two things that make this command answer again.
///
/// # Errors
///
/// The environment or the configuration being unreadable, unparseable, or a deployment this build
/// refuses to SERVE - each naming the key to fix, under a line saying why a command-line tool is
/// reporting them.
pub(crate) fn configured() -> Result<sutura_config::Settings, String> {
    let environment = sutura_config::environment_from_process().map_err(|cause| render(&cause))?;
    sutura_config::Settings::load(&sutura_config::Sources::from_process_environment(
        environment,
        sutura_config::config_dir_from_process(),
    ))
    .map_err(|cause| unservable(&cause))
}

/// A settings failure, said in terms a person running a COMMAND can act on.
///
/// The refusals underneath are a deployment's and are rendered unchanged - they name their own keys,
/// and this command reads the same tree a service would precisely so the two cannot disagree. What is
/// added is the sentence that makes them actionable here: this command binds nothing, so a refusal
/// about a listener is about the configuration it was pointed at rather than about the question.
fn unservable(cause: &sutura_config::SettingsError) -> String {
    format!(
        "{}\nthis command reads the same configuration a deployment would, so a refusal about \
         serving stops it too - it binds no listener of its own. Point \
         {} somewhere else, or unset {}, to answer from the directory on the command line instead",
        render(cause),
        sutura_config::CONFIG_DIR_VARIABLE,
        sutura_config::ENVIRONMENT_VARIABLE
    )
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
            // **No remedy is offered, and that is a correction rather than terseness.** An earlier
            // version said "serve it over HTTP, where a plan spanning two sources is split into
            // legs", and no shipped build delivers that: `Warehouse::EXECUTES_LEGS` defaults to
            // `false`, the only implementor setting it `true` is the DuckDB dev vehicle, so
            // `sutura-serve` refuses the same question as `FederationNotExecutable`. Sending an
            // operator to stand up a server that refuses them again is worse than saying nothing.
            return Err(format!(
                "this catalog spans {} data systems, and this command answers one question against \
                 one. No shipped binary answers a two-source question either - every adapter a \
                 release links declares it executes no leg - so this is a catalog to split rather \
                 than a surface to move to",
                many.len()
            ));
        }
    };
    // A `let`-else rather than a match on the `Option`, because `clippy::option_if_let_else` asks for
    // `map_or_else` and the two closures it wants read as an expression where this reads as an order:
    // the deployment's declaration first, this command's own only if there was none.
    let Some(declared) = registry.get(&named) else {
        return files::from_the_built_in_declaration(pinned, &named, data);
    };
    from_the_registry(pinned, &named, declared, data, registry, request_timeout)
}

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
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    // **The argument is checked PER KIND, and that is two corrections rather than one.** It used to
    // be one check above this match, which made both of its remedies wrong somewhere: "remove the
    // `sources.<alias>` entry to read that directory as a files source" only works when the catalog
    // names the source `local`, because otherwise the built-in declaration refuses that name - a
    // closed loop, where one refusal says delete the entry and the next says put it back. And a
    // directory offered to a DATASET is not "two answers to one question" at all; it is an argument
    // that means nothing for that kind. Each arm now says the one true thing about itself.
    match configured.kind() {
        sutura_config::SourceKind::Files => {
            let sutura_config::SourcePlacement::Files { ref data_dir } = *configured.placement() else {
                return Err(format!(
                    "`sources.{source}` reached the files attach step with a placement no linked \
                     adapter reads, which the match above should have dispatched elsewhere"
                ));
            };
            files::refuse_a_second_directory(source, data_dir, data)?;
            let (engines, attached) = files::open(source, identity.posture(), pinned, data_dir)?;
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
        sutura_config::SourceKind::BigQuery => {
            if let Some(given) = data {
                return Err(format!(
                    "`sources.{source}` is a BigQuery dataset, and {} was given on the command line \
                     as a data directory - a dataset has none, so the argument selects nothing. Drop \
                     it; the dataset that entry declares is what will be read",
                    given.display()
                ));
            }
            bigquery::open(source, configured, registry, request_timeout)
        }
    }
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
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName};

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

/// One model as a catalog document names it: the model, its data system, its table.
#[cfg(test)]
type DeclaredModel<'raw> = (&'raw str, &'raw str, &'raw str);

/// A pinned bundle over exactly the models given, and no metrics.
///
/// Models are all [`open_engine`] reads: [`sutura_app::sources`] maps over them and the attach step
/// is called once per model, so a metric would add nothing any arm of that function looks at.
///
/// Module level for the reason [`bundle_naming`] gives: `files.rs`'s suite needs it too.
#[cfg(test)]
fn bundle_over(models: &[DeclaredModel<'_>]) -> PinnedDefinitions {
    use sutura_domain::catalog::{Definitions, Description, Model};
    use sutura_domain::model::{ColumnName, ModelName};

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

/// One `sources:` entry for a `BigQuery` dataset, with every key that kind is opened with.
///
/// The credential file points at a path that is not there ON PURPOSE, and each test that uses it says
/// what it is proving with that: a refusal naming that key is proof the composition reached the
/// credential layer, which is the furthest a test with no project can get. The shape is
/// `sutura-serve`'s own `bigquery_entry`, because the composition under test is the same one.
///
/// Module level, so `bigquery.rs`'s suite and the dataset-argument case in [`tests`] - which belongs
/// here, beside the arm that refuses it - share ONE entry builder.
#[cfg(test)]
fn declaring_bigquery(posture: &str, extra: &str) -> sutura_config::SourceRegistry {
    declaring(
        "warehouse",
        &format!(
            "    kind: bigquery\n    billing_project: \"acme-analytics\"\n    dataset: \"marts\"\n    \
             credential_file: \"/nonexistent/sutura-cli-test-bigquery.json\"\n    max_bytes_billed: \
             1073741824\n{extra}"
        ),
        posture,
    )
}

/// The `workload_identity` block an `impersonation-at-source` entry must carry.
#[cfg(feature = "bigquery")]
#[cfg(test)]
fn wif() -> String {
    String::from(
        "    workload_identity:\n      audience: \"//iam.googleapis.com/projects/1/locations/global/\
         workloadIdentityPools/p/providers/sso\"\n      scope: \"https://www.googleapis.com/auth/\
         bigquery.readonly\"\n",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use sutura_domain::model::{MetricName, TableName};

    use sutura_exec_datafusion::DataFusionWarehouse;

    use super::{
        BUILT_IN_SOURCE, Opened, OpenedWith, bundle_naming, bundle_over, declaring, declaring_bigquery, open_engine,
        refuse_unattached, served_tables, timeout,
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
        // out of them, stamped with the real bundle's version and digest - because
        // `warehouses.get(plan.source())` is a LOOKUP by the name the catalog declared, and an engine
        // registered under that name makes it succeed. See this module's head, which states why the
        // guard an earlier version of this comment named does not exist.
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
    fn a_question_is_answered_through_a_declared_source_under_the_witness_that_entry_carries() {
        // **THE OUTCOME issue 121 asks for, end to end - and the test review proved was missing.**
        // Everything else here builds an `Opened` and stops. This one verifies the bundle against the
        // engine the REGISTRY arm opened and answers a certified question through it, which is the
        // only thing that exercises the seam `OpenedWith`'s doc comment claims: the broker mints for
        // the same decision the engines came from.
        //
        // Review demonstrated the gap with a mutation - minting the built-in acknowledgement while
        // the engine carries the registry entry's - and all 34 committed tests passed, because the
        // only broker assertion was `count() == 1` and both constructors give 1 on a one-source
        // registry. `Presented::agrees_with` is what actually catches that, at answer time, so an
        // answer is what has to be asked for.
        //
        // The EXAMPLE catalog rather than a hand-built bundle, deliberately: its models say
        // `source: local`, so declaring `sources.local` sends it through the registry arm with real
        // CSVs behind it. A fixture bundle cannot answer - `bundle_naming` names a `signed_at` column
        // `dim_customer.csv` does not have, which review also measured.
        let pinned = crate::commands::load(&example().join("catalog")).expect("the example catalog loads");
        let opened = files_of(
            open_engine(&pinned, &declaring_files(BUILT_IN_SOURCE), timeout(), None)
                .expect("the example catalog opens through a declared files source"),
        );
        let validated = sutura_app::verify_and_validate(pinned, &opened.engines).expect("every anchor reproduces");
        // The example's own certified window, which is the range `examples/single-player` documents
        // and the one its anchor covers.
        let question = sutura_domain::query::Query::new(
            MetricName::parse("recurring_revenue").expect("a test metric is a metric"),
            sutura_domain::model::Grain::Month,
            sutura_domain::calendar::TimeRange::new(
                sutura_domain::calendar::Date::parse("2026-01-01").expect("a test date is a date"),
                sutura_domain::calendar::Date::parse("2026-07-01").expect("a test date is a date"),
            )
            .expect("a test range is not empty"),
            Vec::new(),
            Vec::new(),
        );
        let outcome = sutura_app::answer(
            &validated,
            &question,
            &sutura_domain::identity::RequestContext::of(sutura_domain::identity::PrincipalChain::of(
                sutura_domain::identity::Subject::TheDeploymentItself,
            )),
            &opened.broker,
            &opened.engines,
            1 << 30,
        )
        .expect("a declared source answers rather than failing")
        .into_outcome();
        // An ANSWER and not a refusal: a broker minting a witness the adapter was not opened with is
        // a `SurfaceFailure`, and a posture disagreement is what `Presented::agrees_with` returns.
        assert!(
            matches!(outcome, sutura_domain::query::ToolOutcome::Answer { .. }),
            "a certified question through a declared source must be answered: {outcome:?}"
        );
    }

    #[test]
    fn a_data_directory_offered_to_a_dataset_is_refused_as_an_argument_that_selects_nothing() {
        // NOT "two answers to one question", which is what the single pre-match check called it: a
        // dataset has no directory, so the argument is not a competing answer - it is an argument
        // that means nothing for that kind. Review reproduced the old message's remedy leading to a
        // second refusal about the feature, which is a different problem than the one it named.
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_bigquery("shared-service-user", ""),
            timeout(),
            Some(&example().join("data")),
        )
        .map(|_| ())
        .expect_err("a data directory for a dataset source is refused");
        assert!(error.contains("a dataset has none"), "{error}");
        assert!(
            !error.contains("two answers to one question"),
            "a dataset's directory is not a competing answer: {error}"
        );
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
        // **The remedy it must NOT offer.** An earlier version said "serve it over HTTP, where a plan
        // spanning two sources is split into legs", and no shipped build delivers that: every adapter
        // a release links declares `EXECUTES_LEGS = false`, so `sutura-serve` refuses the same
        // question as `FederationNotExecutable`. Review reproduced the round trip. Asserted as an
        // ABSENCE, because that is what the defect was.
        assert!(
            !error.to_lowercase().contains("over http"),
            "the refusal must not send an operator to a surface that refuses them again: {error}"
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
