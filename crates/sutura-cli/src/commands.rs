//! The commands, and the only place adapters are chosen.
//!
//! This is the composition root: it is where a directory becomes a [`LocalCatalog`] and a set of
//! files becomes a running engine. Nothing above it names an adapter, which is what lets the same
//! service code be exercised against a fake.
//!
//! `Result<_, String>` throughout, deliberately. The boundary gate fails that in a library crate and
//! exempts a binary, because here the error's audience is a person reading stderr rather than code
//! matching on a variant. Every message is built from a typed error's own `Display`, so the variant
//! is still what decided the wording.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sutura_app::prompt::{CatalogProse, PromptInputs, Tool};
use sutura_catalog_local::LocalCatalog;
use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};
use sutura_domain::measure::RequiredFilter;
use sutura_domain::model::{ModelName, SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::Value;
use sutura_exec_datafusion::DataFusionWarehouse;
use sutura_semantic::Compiled;
use sutura_sql::Dialect;

/// The version a locally-read catalog is stamped with when the caller did not say.
///
/// Named rather than derived from the digest: the digest already says what the content is, and a
/// version that repeats it leaves no way to tell two builds of identical content apart. A real
/// deployment passes a commit id.
const DEFAULT_VERSION: &str = "local-working-tree";

/// The one data system this build can open, and the name it answers to.
///
/// **A constant here rather than whatever the catalog declared, and that is the whole of the fix.**
/// The engine is the in-process one: it reads the CSV and Parquet files in the directory the CALLER
/// passed on the command line. Naming it after the catalog's declared source made
/// `plan.source() != warehouse.source()` in `sutura-app` true by construction, so the one guard that
/// stops a plan running against the wrong data system was satisfied rather than checked - and a
/// catalog declaring a real data system got an engine that answered its certified metric out of the
/// caller's files, under that bundle's provenance and digest. Fixed at build time, the app's check is
/// a check again and [`open_engine`] refuses everything else.
///
/// The value is what every catalog in this repository already declares, so the gate arrives without
/// moving any bundle's digest. Renaming it is a catalog edit in every example plus a new digest, not
/// a code change here.
const ENGINE_SOURCE: &str = "local";

/// The metric's definitional filters, for a person reading a catalog.
///
/// Worth showing rather than hiding: a caller cannot choose these and they change what the number
/// means, so somebody reading what a metric is needs to see them beside the measure.
fn render_filters(filters: &[RequiredFilter]) -> String {
    if filters.is_empty() {
        return String::from("none");
    }
    filters.iter().map(ToString::to_string).collect::<Vec<String>>().join(", ")
}

/// Reads a catalog directory into a pinned bundle.
fn load(root: &Path) -> Result<PinnedDefinitions, String> {
    let version =
        DefinitionVersion::parse(DEFAULT_VERSION).map_err(|e| format!("the built-in default version is not a version: {e}"))?;
    LocalCatalog::new(PathBuf::from(root), version).load().map_err(|e| render(&e))
}

/// A typed error and every cause beneath it, on one line each.
///
/// Written out rather than relying on `Display`, which shows only the outermost message. The causes
/// are where the useful part is: "could not read the frontmatter of x.md as a metric" is a location,
/// and its source is the reason.
fn render(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

/// Reads a question file.
fn read_question(path: &Path) -> Result<Query, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read the question at {}: {e}", path.display()))?;
    serde_norway::from_str(&text).map_err(|e| format!("{} is not a question: {e}", path.display()))
}

/// Turns a `Result` into an exit code, printing the message on failure.
fn report(outcome: Result<(), String>) -> ExitCode {
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("sutura: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The next argument, or a usage message.
fn arg(args: &[String], index: usize, name: &str, usage: &str) -> Result<String, String> {
    args.get(index)
        .cloned()
        .ok_or_else(|| format!("missing <{name}>\nusage: sutura {usage}"))
}

/// `catalog <dir>`: what this catalog defines.
pub(crate) fn catalog(args: &[String]) -> ExitCode {
    report((|| {
        let root = arg(args, 0, "catalog-dir", "catalog <catalog-dir>")?;
        let pinned = load(Path::new(&root))?;
        println!("version {}", pinned.version());
        println!("digest  {}", pinned.digest().as_str());
        println!();
        for (name, metric) in pinned.definitions().metrics() {
            let grains: Vec<&str> = metric.grains().iter().map(|g| g.as_str()).collect();
            let dimensions: Vec<&str> = metric
                .dimensions()
                .keys()
                .map(sutura_domain::model::DimensionName::as_str)
                .collect();
            println!("{name}");
            println!("  measure    {}", metric.measure());
            println!("  filters    {}", render_filters(metric.required_filters()));
            println!("  grains     {}", grains.join(", "));
            println!(
                "  dimensions {}",
                if dimensions.is_empty() {
                    String::from("none")
                } else {
                    dimensions.join(", ")
                }
            );
            println!(
                "  anchor     {}",
                metric
                    .anchor()
                    .map_or_else(|| String::from("none"), |a| format!("{} over {}", a.value(), a.range()))
            );
        }
        Ok(())
    })())
}

/// `describe <dir> <metric>`: one metric in full, prose included.
pub(crate) fn describe(args: &[String]) -> ExitCode {
    report((|| {
        let usage = "describe <catalog-dir> <metric>";
        let root = arg(args, 0, "catalog-dir", usage)?;
        let wanted = arg(args, 1, "metric", usage)?;
        let pinned = load(Path::new(&root))?;
        let name =
            sutura_domain::model::MetricName::parse(&wanted).map_err(|e| format!("{wanted:?} is not a metric name: {e}"))?;
        let metric = pinned
            .definitions()
            .metric(&name)
            .ok_or_else(|| format!("this catalog defines no metric called {wanted}"))?;
        println!("{name}");
        println!("  model      {}", metric.model());
        println!("  measure    {}", metric.measure());
        println!("  filters    {}", render_filters(metric.required_filters()));
        println!("  time       {}", metric.time_column());
        for (dimension_name, dimension) in metric.dimensions() {
            println!(
                "  dimension  {dimension_name} -> {}{}{}",
                dimension.column(),
                dimension.via().map_or_else(String::new, |via| format!(" via {via}")),
                if dimension.is_filterable() {
                    " (filterable)"
                } else {
                    " (group-by only)"
                }
            );
        }
        if !metric.description().is_empty() {
            println!();
            println!("{}", metric.description());
        }
        Ok(())
    })())
}

/// `prompt <dir> [config-dir]`: the system prompt an agent should be given.
///
/// **The reachable consumer of the `prompt` configuration group**, and that is why it exists as a
/// command rather than only as an endpoint. A key that is parsed, range-checked and read by nothing
/// reads as a control that is in place; this is what reads it. An operator pipes the output into an
/// agent's configuration.
///
/// Two arguments, and the second one is the deployment's configuration directory - the same
/// `base.yaml` plus `<environment>.yaml` a service would read, layered under the same
/// `SUTURA__PROMPT__*` variables. So the text rendered here is the text that deployment would hand
/// out, rather than a second rendering with its own flags that could disagree.
///
/// **A configuration that will not serve will not describe what it serves either.** `Settings::load`
/// runs the posture refusals, so `SUTURA_ENVIRONMENT=production` with no access token configured
/// fails here exactly as it would at startup. That is deliberate: the alternative is a second,
/// weaker door into the settings, and the refusal names the key to fix.
pub(crate) fn prompt(args: &[String]) -> ExitCode {
    report((|| {
        let usage = "prompt <catalog-dir> [config-dir]";
        let root = arg(args, 0, "catalog-dir", usage)?;
        let environment = sutura_config::environment_from_process().map_err(|e| render(&e))?;
        let settings = sutura_config::Settings::load(&sutura_config::Sources::from_process_environment(
            environment,
            args.get(1).map(PathBuf::from),
        ))
        .map_err(|e| render(&e))?;
        // **Standard error, and that is not a detail.** This command's standard output is piped into
        // an agent's configuration, so a provenance line on it would become part of the prompt. The
        // same text the startup report logs, for the same reason: this command renders what a
        // deployment WOULD hand out, and a prompt rendered from a configuration directory that was
        // never found is the failure it exists to make visible.
        eprintln!("sutura: configuration from {}", settings.layers());
        let (prose, instructions) = prompt_inputs(settings.prompt())?;
        let pinned = load(Path::new(&root))?;
        // Every operation, because the HTTP surface mounts every operation. A transport that hid one
        // passes the subset it mounts and the workflow drops the step rather than telling an agent
        // to call something that is not there.
        let inputs = PromptInputs::new(Tool::ALL, prose, instructions.as_deref());
        print!("{}", sutura_app::prompt::render(&pinned, &inputs));
        Ok(())
    })())
}

/// How the catalog's prose is treated, and the operator's own text if a path was configured.
///
/// A named alias because the inline tuple is over the complexity threshold in `clippy.toml`, and
/// naming it is the better half of that trade: the pair is what the settings resolve to.
type ResolvedPromptText = (CatalogProse, Option<String>);

/// The prompt's two non-catalog inputs, resolved from the settings.
///
/// **A configured instructions file that cannot be read is an error, not an omitted section.** The
/// implementation this prompt is modelled on omits its `instructions.md` silently when the file is
/// absent, which is right for a convention - no file means nobody wrote one. Here the path was
/// written down, so absence means the operator's rules are missing from a document that says it
/// carries them, and serving that quietly is the failure this repository refuses everywhere else.
fn prompt_inputs(settings: &sutura_config::PromptSettings) -> Result<ResolvedPromptText, String> {
    let prose = if settings.catalog_prose().is_quoted() {
        CatalogProse::Quoted
    } else {
        CatalogProse::Omitted
    };
    let instructions = match settings.instructions_file() {
        None => None,
        Some(configured) => {
            let path = configured.path();
            Some(std::fs::read_to_string(path).map_err(|e| {
                format!(
                    "prompt.instructions_file is {} and it could not be read: {e}\nremove the key to \
                     render the prompt without an operator section",
                    path.display()
                )
            })?)
        }
    };
    Ok((prose, instructions))
}

/// `compile <dir> <question> [dialect]`: the statement, without a data system.
pub(crate) fn compile(args: &[String]) -> ExitCode {
    report((|| {
        let usage = "compile <catalog-dir> <question.yaml> [dialect]";
        let root = arg(args, 0, "catalog-dir", usage)?;
        let question_path = arg(args, 1, "question.yaml", usage)?;
        let dialect = match args.get(2) {
            Some(name) => Dialect::parse(name).map_err(|e| e.to_string())?,
            None => Dialect::DuckDb,
        };
        let pinned = load(Path::new(&root))?;
        let question = read_question(Path::new(&question_path))?;
        match sutura_semantic::compile(&question, &pinned).map_err(|e| render(&e))? {
            Compiled::Refused { reason } => {
                println!("{}", render_refusal(&reason)?);
            }
            Compiled::Planned { plan } => {
                let query = sutura_sql::generate(&plan, dialect).map_err(|e| render(&e))?;
                println!("-- dialect {dialect}");
                println!("{}", query.sql());
                println!();
                for (index, param) in query.params().iter().enumerate() {
                    println!("-- ${} = {}", index.saturating_add(1), param.render());
                }
                println!();
                println!("-- plan");
                let rendered = serde_norway::to_string(&*plan).map_err(|e| format!("the plan could not be rendered: {e}"))?;
                print!("{rendered}");
            }
            Compiled::Federated { plan } => {
                println!("-- federated, one statement per leg for {dialect}");
                for leg in plan.legs() {
                    let query = sutura_sql::generate_leg(leg, dialect).map_err(|e| render(&e))?;
                    println!("{}", query.sql());
                    println!();
                }
                println!("-- plan");
                let rendered = serde_norway::to_string(&*plan).map_err(|e| format!("the plan could not be rendered: {e}"))?;
                print!("{rendered}");
            }
        }
        Ok(())
    })())
}

/// `query <dir> <question> <data-dir>`: check the anchors, then answer.
pub(crate) fn query(args: &[String]) -> ExitCode {
    report((|| {
        let usage = "query <catalog-dir> <question.yaml> <data-dir>";
        let root = arg(args, 0, "catalog-dir", usage)?;
        let question_path = arg(args, 1, "question.yaml", usage)?;
        let data = arg(args, 2, "data-dir", usage)?;

        let pinned = load(Path::new(&root))?;
        let engine = open_engine(&pinned, Path::new(&data))?;

        // The governance is not an order this function has to remember any more. One call runs the
        // anchors against the engine it was handed and hands back a bundle only if every one
        // reproduced its number; `sutura_app::answer` takes nothing else. A corrupted anchor stops
        // here rather than answering, and there is no arrangement of these lines that skips it.
        let validated = sutura_app::verify_and_validate(pinned, &engine)
            .map_err(|e| format!("{}\nthis bundle is not fit to serve", render(&e)))?;

        let question = read_question(Path::new(&question_path))?;
        // The credential this question executes with, and it is the same declaration the engine was
        // opened under: one user, one host, the files that person already has access to. There is no
        // path here that executes without one - `Warehouse::execute` has no signature for it - so
        // "this command runs as whoever typed it" is now a value a reviewer can read rather than a
        // property of there being no parameter.
        let broker = single_user_broker()?;
        // `Subject::TheDeploymentItself` is the honest subject: there is no transport and no caller,
        // and the identity the files are read under is the process's own.
        let context = RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself));
        // `into_outcome` because this command writes no audit record: the deadline `Answered` also
        // carries is for a sink, and this binary answers one question on a terminal and exits.
        let outcome = sutura_app::answer(&validated, &question, &context, &broker, &engine)
            .map_err(|e| render(&e))?
            .into_outcome();
        print_outcome(&outcome)?;
        Ok(())
    })())
}

/// Starts the engine and registers one file per model.
///
/// The engine reads the files itself, so there is no database to create and nothing to keep in step
/// with the CSVs. A `.parquet` beside a model's table name is preferred over a `.csv` because it
/// carries its own types; a CSV has to be sniffed.
///
/// It refuses a catalog that names anything other than [`ENGINE_SOURCE`]. The engine has its own
/// identity and does not borrow the catalog's: a catalog naming a data system nothing here can open
/// gets no engine, rather than one wearing that data system's name over the caller's files.
///
/// **The posture is written here, in code, and that is not a default nobody chose.** This command has
/// no settings tree on its query path - `sutura prompt` is the only subcommand that loads one - so
/// there is no `sources:` entry for an operator to write. What replaces it is a declaration this file
/// makes and a reviewer can see: `sutura` is a single-user tool by construction. It reads the files
/// the person running it already has access to, as that person's own operating-system identity, and
/// there is no second caller for that identity to be wrong for. `ImpersonationCapability` on the
/// adapter says the same thing from the other side, so the two agree by construction rather than by a
/// check this command could skip.
fn open_engine(pinned: &PinnedDefinitions, data: &Path) -> Result<sutura_app::Warehouses<DataFusionWarehouse>, String> {
    let sources = sutura_app::sources(pinned);
    let declared = match sources.as_slice() {
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
        SourceName::parse(ENGINE_SOURCE).map_err(|e| format!("the built-in engine source name is not a name: {e}"))?;
    if declared != engine_source {
        return Err(format!(
            "this catalog reads from {declared}, and this build has no adapter for it: the only data \
             system it can open is {engine_source}, the in-process engine over the CSV and Parquet \
             files in the directory given to this command"
        ));
    }
    let engine = DataFusionWarehouse::new(engine_source, single_user_posture()?, working_set()?).map_err(|e| render(&e))?;
    for model in pinned.definitions().models().values() {
        attach(&engine, model.name(), model.table(), data)?;
    }
    // One data system, registered under its own name - which is what `answer` looks a plan up in.
    Ok(sutura_app::Warehouses::of(engine))
}

/// The posture this command runs its one data system under.
///
/// The reason is the operator's, and here the operator is whoever typed the command: the sentence says
/// what is true of this tool rather than describing a deployment it is not. It goes through
/// `AcknowledgementReason::parse` like any other, so it is bounded and checked by the same code a
/// configuration file's is.
fn single_user_posture() -> Result<sutura_domain::source::SourcePosture, String> {
    let reason = sutura_domain::source::AcknowledgementReason::parse(
        "the sutura command reads the files of whoever ran it, as that person's own operating-system identity",
    )
    .map_err(|e| render(&e))?;
    Ok(sutura_domain::source::SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(reason),
    })
}

/// The credential broker this command answers with.
///
/// The static one, holding the one source this build can open under the same acknowledgement
/// [`single_user_posture`] declares - so what the leg presents and what the adapter was opened with
/// come from one sentence rather than two. A `SharedServiceUser` posture is the only shape the engine
/// can execute with, and it is the honest one here: there is no second caller for this identity to be
/// wrong for.
fn single_user_broker() -> Result<sutura_config::StaticCredentialBroker, String> {
    let source = SourceName::parse(ENGINE_SOURCE).map_err(|e| format!("the built-in engine source name is not a name: {e}"))?;
    let sutura_domain::source::SourcePosture::SharedServiceUser { declared } = single_user_posture()? else {
        return Err(String::from("this command opens its engine shared, one function above"));
    };
    Ok(sutura_config::StaticCredentialBroker::for_one_shared_source(source, declared))
}

/// The working-set ceiling this command bounds the engine with.
///
/// **The embedded default, and not a flag.** This command answers one question and exits, and it has
/// no settings tree in scope - `sutura prompt` is the only subcommand that loads one, deliberately, so
/// the prompt an operator pipes into an agent is rendered from the configuration the service would
/// read. Giving the query path a flag would be a second number an operator could set, disagreeing with
/// the one the service uses.
///
/// It goes through `WorkingSetCeiling::parse` rather than constructing the wrapper from the constant
/// directly, so this reads the same number through the same checks the service does; a test asserts the
/// constant and `defaults.yaml` agree. `available_memory_bytes` is asked here too: a laptop with less
/// memory than the default ceiling should be told so rather than dying inside a join.
fn working_set() -> Result<sutura_exec_datafusion::WorkingSet, String> {
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
        return engine.attach_parquet(table, &parquet).map_err(|e| render(&e));
    }
    let csv = data.join(format!("{table}.csv"));
    if csv.is_file() {
        return engine.attach_csv(table, &csv).map_err(|e| render(&e));
    }
    Err(format!(
        "model {model} needs {} or {}, and neither is there",
        csv.display(),
        parquet.display()
    ))
}

/// A refused question, for a person: what it means, what to do about it, and the refusal's own
/// fields.
///
/// **The wording is not written here and is not written in this crate.**
/// [`sutura_app::prompt::guidance`] is an accessor over the one table the agent-facing prompt renders
/// from, so a person at a terminal and an agent reading that document are told the same thing about
/// the same refusal. What this replaced was `println!("refused: {reason:?}")` - the Rust `Debug` of a
/// governance decision, which names the variant and says nothing an operator can act on.
///
/// **The typed fields stay, and are not what was wrong.** `TimeRangeTooLong`'s remedy says outright
/// that both day counts are carried so the split can be computed rather than guessed, so a rendering
/// that dropped them would leave the remedy pointing at nothing. They arrive through `serde_norway`,
/// the way the plan does in [`compile`] and the way the example suite pins them - not as a `Debug`
/// dump, which is the part that goes.
///
/// A `Result`, because the serialization is fallible and this is a binary where the alternative is a
/// silently missing detail line. Nothing in a `RefusalReason` can actually fail to serialize today;
/// the branch is here so that a variant carrying something that could does not lose the field
/// quietly.
fn render_refusal(reason: &RefusalReason) -> Result<String, String> {
    let (meaning, remedy) = sutura_app::prompt::guidance(reason);
    let fields = serde_norway::to_string(reason).map_err(|e| format!("the refusal could not be rendered: {e}"))?;
    let mut lines = fields.lines();
    // The first line is the variant, which this serializer writes as the YAML type tag `!Variant`;
    // the fields follow it at column zero. Both are reshaped here rather than taken as they come: the
    // tag marker is noise to a person, and the fields are indented so the block reads as one refusal.
    //
    // The variant name itself is kept: it is the machine-readable identity of the refusal, it is what
    // the prompt tells an agent to expect, and it is the one part of the old `Debug` output that was
    // worth anything.
    //
    // NOT the same string the HTTP surface sends, and an earlier version of this comment said it was.
    // The wire `code` is snake_case - `metric_unknown` - assigned by the exhaustive match in
    // `sutura_http::wire::refusal`; this is the PascalCase Rust variant. Same identity, two spellings,
    // and a caller matching on one must not be told it is the other.
    let variant = lines.next().unwrap_or_default().trim_start_matches('!').trim_end_matches(':');
    let mut out = format!("refused: {variant}\n  {meaning}");
    for field in lines {
        out.push_str("\n  ");
        out.push_str(field);
    }
    out.push_str("\n  remedy: ");
    out.push_str(remedy);
    Ok(out)
}

/// Prints an outcome as a table, or as the refusal it is.
fn print_outcome(outcome: &ToolOutcome) -> Result<(), String> {
    match *outcome {
        ToolOutcome::Refusal { ref reason } => println!("{}", render_refusal(reason)?),
        ToolOutcome::Answer {
            ref provenance,
            ref rows,
        } => {
            println!("-- definitions {} {}", provenance.version(), provenance.digest().as_str());
            println!("{}", rows.columns().join("\t"));
            for row in rows.rows() {
                let cells: Vec<String> = row.iter().map(Value::render).collect();
                println!("{}", cells.join("\t"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use sutura_domain::catalog::{Definitions, Description, Metric, Model};
    use sutura_domain::knowledge::Knowledge;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, SourceName, TableName};
    use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};

    use sutura_domain::query::{MAX_RANGE_DAYS, RefusalReason};

    use super::{ENGINE_SOURCE, load, open_engine, prompt_inputs, render_refusal};

    fn example() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
    }

    /// A bundle whose one model claims to live in a data system nothing here can open, over a table
    /// the example's data directory happens to have a file for.
    ///
    /// The file is what makes the test mean something: without it the old code failed on a missing
    /// CSV and the refusal would be indistinguishable from that.
    fn bundle_naming_another_data_system() -> PinnedDefinitions {
        let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
        let model = Model::new(
            ModelName::parse("customers").expect("a test model is a model"),
            SourceName::parse("production_warehouse").expect("a test source is a source"),
            TableName::parse("dim_customer").expect("a test table is a table"),
            BTreeSet::from([column("customer_key"), column("signed_at")]),
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
            BTreeSet::from([Grain::Month]),
            BTreeMap::new(),
            None,
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent");
        PinnedDefinitions::pin(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            definitions,
            Knowledge::none(),
        )
        .expect("the test definitions hash")
    }

    #[test]
    fn a_catalog_naming_another_data_system_gets_no_engine() {
        // THE BUG THIS EXISTS FOR, and it was reachable from the command line. `open_engine` named
        // the engine after whatever source the catalog declared, so a catalog saying
        // `source: production_warehouse` got an in-process engine calling itself
        // `production_warehouse` and reading the files in the directory the CALLER passed. Then
        // `sutura query` answered that catalog's certified metric out of those files, stamped with
        // the real bundle's version and digest - and `sutura-app`'s own
        // `plan.source() != warehouse.source()` guard could not fire, because naming the engine
        // after the catalog satisfied it by construction.
        //
        // Before the fix this call RETURNED AN ENGINE: `dim_customer.csv` is there, so nothing else
        // failed either.
        let error = open_engine(&bundle_naming_another_data_system(), &example().join("data"))
            .expect_err("a catalog naming another data system must not get this engine");
        assert!(
            error.contains("production_warehouse") && error.contains("no adapter"),
            "the refusal must name the data system it has no adapter for: {error}"
        );
    }

    #[test]
    fn the_documented_example_still_opens() {
        // The other half: the gate is worth nothing if it also refuses the catalog the quickstart
        // tells a reader to run. This is the one test that exercises `open_engine`'s own happy path,
        // which the example suite reaches only through the libraries.
        let pinned = load(&example().join("catalog")).expect("the example catalog loads");
        let engine = open_engine(&pinned, &example().join("data")).expect("the example catalog opens");
        assert_eq!(
            engine
                .postures()
                .map(|(name, posture)| (name.as_str(), posture.as_str()))
                .collect::<Vec<(&str, &str)>>(),
            vec![(ENGINE_SOURCE, "shared-service-user")],
            "the engine answers to its own name, not to the catalog's - and this command declares what \
             identity it reads under rather than leaving it at a default"
        );
    }

    /// One model as a catalog document names it: the model, its data system, its table.
    type DeclaredModel<'raw> = (&'raw str, &'raw str, &'raw str);

    /// A pinned bundle over exactly the models given, and no metrics.
    ///
    /// Models are all `open_engine` reads: [`sutura_app::sources`] maps over them and `attach` is
    /// called once per model, so a metric would add nothing any arm of that function looks at.
    /// Leaving them out is what lets one helper stand behind the empty catalog, the catalog spanning
    /// two data systems and the model with no file alike.
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
        let definitions = Definitions::assemble(declared, vec![], vec![]).expect("the test bundle is consistent");
        PinnedDefinitions::pin(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            definitions,
            Knowledge::none(),
        )
        .expect("the test definitions hash")
    }

    #[test]
    fn a_catalog_spanning_two_data_systems_gets_no_engine() {
        // The arm nothing proved. A plan runs against one data system - the CLI takes one data
        // directory and reads no source registry, so a question that would span two is refused - and
        // this is that rule at startup: a bundle whose models name
        // two systems gets no engine at all, rather than one over whichever half happens to be local.
        //
        // `local` is deliberately ONE OF THE PAIR, and its table has a real file in the example's
        // data directory. That is what makes this discriminate: an arm that took the first source
        // instead of refusing would find `local`, find `dim_customer.csv`, and hand back a working
        // engine serving half a catalog under the whole bundle's digest.
        let error = open_engine(
            &bundle_over(&[
                ("customers", ENGINE_SOURCE, "dim_customer"),
                ("products", "production_warehouse", "dim_product"),
            ]),
            &example().join("data"),
        )
        .expect_err("a catalog spanning two data systems must not get an engine");
        assert!(
            error.contains("spans 2 data systems"),
            "the refusal must say how many it found: {error}"
        );
        // NOT the neighbouring arm, and this is the half that stops the test passing on the wrong
        // branch: `production_warehouse` is also a source this build has no adapter for, so a test
        // that only checked for *a* refusal would be green with the multi-source arm gone.
        assert!(
            !error.contains("no adapter"),
            "this is the multi-source arm, not the wrong-name one: {error}"
        );
    }

    #[test]
    fn a_catalog_declaring_no_models_gets_no_engine() {
        // The empty bundle. `sutura-serve`'s own `refuse_unattached` tests state in a comment that
        // this case is "already refused earlier, by `open_engine`" - a claim neither binary had a
        // test for. An engine opened over nothing would open successfully, because the attach loop
        // has nothing to iterate, and then answer every question as an unknown metric - which reads
        // as a question problem rather than as a catalog directory that holds no models.
        let error = open_engine(&bundle_over(&[]), &example().join("data")).expect_err("a catalog with no models opens nothing");
        assert!(error.contains("declares no models"), "{error}");
        assert!(
            !error.contains("no adapter"),
            "this is the empty arm, not the wrong-name one: {error}"
        );
    }

    #[test]
    fn a_model_with_no_file_behind_it_gets_no_engine() {
        // `attach` runs per model AFTER the source name is accepted, so this arm is reachable only by
        // a catalog this build can otherwise open - which is why it names the engine source. Both
        // candidate paths are asserted because the message is the only thing an operator can act on:
        // that neither extension is present is the failure, and naming the two that were looked for
        // is the difference between a fixable message and "and neither is there".
        let error = open_engine(
            &bundle_over(&[("orders", ENGINE_SOURCE, "fct_order")]),
            &example().join("data"),
        )
        .expect_err("a model with no file behind it must not open");
        assert!(error.contains("fct_order.csv"), "the CSV path is missing: {error}");
        assert!(error.contains("fct_order.parquet"), "the Parquet path is missing: {error}");
        assert!(error.contains("model orders"), "the model is not named: {error}");
    }

    #[test]
    fn the_prompt_settings_reach_the_renderer() {
        // The point of the whole configuration group: what an operator wrote down is what the
        // rendered prompt is built from. Both keys, both directions, and no process environment
        // involved - `PromptSettings` is constructed directly so this stays hermetic.
        let (prose, instructions) = prompt_inputs(&sutura_config::PromptSettings::new(None, sutura_config::CatalogProse::Quoted))
            .expect("no operator file is not an error");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Quoted);
        assert!(instructions.is_none());

        let (prose, _) = prompt_inputs(&sutura_config::PromptSettings::new(
            None,
            sutura_config::CatalogProse::Omitted,
        ))
        .expect("no operator file is not an error");
        assert_eq!(prose, sutura_app::prompt::CatalogProse::Omitted);
    }

    #[test]
    fn a_configured_instructions_file_that_is_not_there_is_an_error_and_not_a_missing_section() {
        // The divergence from the implementation this prompt is modelled on, asserted. That one
        // omits its `instructions.md` in silence when the file is absent, which is right for a
        // CONVENTION. Here a path was written down, so silence would serve a document that claims
        // to carry the operator's rules and does not.
        let configured = sutura_config::InstructionsFile::parse("/nowhere/house-rules.md").expect("a path is a path");
        let error = prompt_inputs(&sutura_config::PromptSettings::new(
            Some(configured),
            sutura_config::CatalogProse::Quoted,
        ))
        .expect_err("a configured file that cannot be read is an error");
        assert!(error.contains("/nowhere/house-rules.md"), "{error}");
        assert!(error.contains("remove the key"), "the error does not say what to do: {error}");
    }

    #[test]
    fn the_operator_text_is_read_from_the_configured_path() {
        // The other half, so the assertion above is not merely "reading a missing file fails". The
        // scratch directory is named after the process, which is the shape the catalog adapter's own
        // filesystem tests use: `CARGO_TARGET_TMPDIR` is defined for an integration target and not
        // for a unit test in `src/`.
        let dir = std::env::temp_dir().join(format!("sutura-cli-prompt-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        let path = dir.join("house-rules.md");
        std::fs::write(&path, "Prefer the month grain.\n").expect("a scratch file is writable");
        let configured = sutura_config::InstructionsFile::parse(path.to_string_lossy().as_ref()).expect("a path is a path");
        let (_, instructions) = prompt_inputs(&sutura_config::PromptSettings::new(
            Some(configured),
            sutura_config::CatalogProse::Quoted,
        ))
        .expect("a readable file is read");
        assert_eq!(instructions.as_deref(), Some("Prefer the month grain.\n"));
        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn a_refused_question_is_printed_as_a_sentence_and_a_remedy_and_not_as_a_debug_dump() {
        // THE BUG. Both refusal paths - `compile` and `query` - printed `refused: {reason:?}`, so what
        // a person got for a governance decision was
        // `DimensionValueNotAllowed { metric: MetricName("recurring_revenue"), .. }`: the variant's
        // name, the newtype wrappers, and nothing about what to do next. Two renderings of this exact
        // set already existed in the workspace, which is what makes it a duplication bug rather than a
        // missing feature.
        //
        // Asserted against `sutura_app::prompt::guidance` rather than against pasted text, on purpose:
        // a copy of the wording here would be the fourth one, and would let this test pass while the
        // command and the prompt disagreed.
        let reason = RefusalReason::DimensionValueNotAllowed {
            metric: MetricName::parse("recurring_revenue").expect("a test metric is a metric"),
            dimension: DimensionName::parse("region").expect("a test dimension is a dimension"),
        };
        let rendered = render_refusal(&reason).expect("a refusal renders");
        let (meaning, remedy) = sutura_app::prompt::guidance(&reason);
        assert!(rendered.contains(meaning), "the sentence is missing:\n{rendered}");
        assert!(rendered.contains(remedy), "the remedy is missing:\n{rendered}");
        // The variant survives as the identity a client branches on, and the newtype wrappers around
        // it do not: `MetricName("..")` in the output is the `Debug` dump coming back.
        assert!(rendered.starts_with("refused: DimensionValueNotAllowed\n"), "{rendered}");
        assert!(!rendered.contains("MetricName("), "the Debug dump is back:\n{rendered}");
    }

    #[test]
    fn the_whole_block_is_pinned_including_the_day_counts_a_split_is_computed_from() {
        // The layout, end to end, and the reason the typed fields are kept rather than replaced by
        // the sentence: `TimeRangeTooLong`'s remedy tells the caller the refusal carries both day
        // counts so the split can be computed rather than guessed, so a rendering that printed only
        // the sentence and the remedy would leave that remedy pointing at nothing.
        //
        // `assert_eq!` over the whole string rather than four `contains` calls, deliberately. The
        // `Debug` dump this replaced ALSO contains `days: 3652058` and `limit: 3653` - it is
        // `TimeRangeTooLong { days: 3652058, limit: 3653 }` - so a test built from `contains` on the
        // fields passes against the bug it exists to catch. The expected text is assembled from
        // `guidance` for the same reason the test above is: the wording is not copied here.
        let reason = RefusalReason::TimeRangeTooLong {
            days: 3_652_058,
            limit: MAX_RANGE_DAYS,
        };
        let (meaning, remedy) = sutura_app::prompt::guidance(&reason);
        assert_eq!(
            render_refusal(&reason).expect("a refusal renders"),
            format!("refused: TimeRangeTooLong\n  {meaning}\n  days: 3652058\n  limit: {MAX_RANGE_DAYS}\n  remedy: {remedy}")
        );
    }
}
