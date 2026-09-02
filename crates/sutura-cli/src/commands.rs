//! The commands: their arguments, what they read, and how an outcome is printed.
//!
//! Half of the composition root - the half that turns a directory into a [`LocalCatalog`]. The other
//! half is [`crate::sources`], which turns a DECLARED data system into an open one, and it is its own
//! module because the two answer different questions and because this file is under the same
//! 1000-line cap everything else is. Nothing above either of them names an adapter, which is what
//! lets the same service code be exercised against a fake.
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
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::Value;
use sutura_semantic::Compiled;
use sutura_sql::Dialect;

/// The version a locally-read catalog is stamped with when the caller did not say.
///
/// Named rather than derived from the digest: the digest already says what the content is, and a
/// version that repeats it leaves no way to tell two builds of identical content apart. A real
/// deployment passes a commit id.
const DEFAULT_VERSION: &str = "local-working-tree";

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

/// The name the CLI's single catalog is recorded under in its contribution manifest.
///
/// The CLI reads a raw directory and is markdown by construction - there is no `catalogs:`
/// declaration to dispatch, and therefore no name an operator wrote. It still needs a manifest key,
/// because a single-source deployment carries a one-entry manifest, so it is a constant here the way
/// [`crate::sources::BUILT_IN_SOURCE`] is for the data side.
///
/// The two spell the same word and are not the same declaration: this one names where the DEFINITIONS
/// came from, and that one names the data system a question executes against. A deployment can
/// declare the second in its `sources:` tree under any name it likes; nothing declares this one,
/// because the `sutura` command reads a raw directory of markdown and there is no `catalogs:` entry
/// to carry an operator's name for it.
pub(crate) const CATALOG_SOURCE: &str = "local";

/// The catalog a command reads, built from its directory on the command line.
///
/// Split out of [`load`] so a command that must serve a `LocalService` - the `mcp` one - has the
/// catalog itself to hand to the service's constructor, which loads and validates it, rather than
/// rebuilding it. Named `catalog_reader` because `catalog` is already this module's listing
/// subcommand. Everything else keeps using `load`.
pub(crate) fn catalog_reader(root: &Path) -> Result<LocalCatalog, String> {
    let version =
        DefinitionVersion::parse(DEFAULT_VERSION).map_err(|e| format!("the built-in default version is not a version: {e}"))?;
    let name = SourceName::parse(CATALOG_SOURCE).map_err(|e| format!("the built-in catalog name is not a name: {e}"))?;
    Ok(LocalCatalog::new(name, PathBuf::from(root), version))
}

/// Reads a catalog directory into a pinned bundle.
pub(crate) fn load(root: &Path) -> Result<PinnedDefinitions, String> {
    catalog_reader(root)?.load().map_err(|e| render(&e))
}

/// A typed error and every cause beneath it, on one line each.
///
/// Written out rather than relying on `Display`, which shows only the outermost message. The causes
/// are where the useful part is: "could not read the frontmatter of x.md as a metric" is a location,
/// and its source is the reason.
pub(crate) fn render(error: &dyn core::error::Error) -> String {
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
pub(crate) fn report(outcome: Result<(), String>) -> ExitCode {
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("sutura: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The next argument, or a usage message.
pub(crate) fn arg(args: &[String], index: usize, name: &str, usage: &str) -> Result<String, String> {
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
        // **The positional wins, and the VARIABLE is the fallback - which is a fix rather than a
        // convenience.** Review reproduced one binary reading the deployment configuration from two
        // places: `sutura prompt <catalog>` printed `configuration from embedded defaults only`
        // while `sutura query <catalog> <question>` refused on the content of the directory
        // `SUTURA_CONFIG_DIR` names, on one machine, at the same moment. Two subcommands resolving
        // different settings is exactly what exporting the variable name from `sutura_config` was
        // meant to stop.
        let settings = sutura_config::Settings::load(&sutura_config::Sources::from_process_environment(
            environment,
            args.get(1).map(PathBuf::from).or_else(sutura_config::config_dir_from_process),
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

/// `query <dir> <question> [data-dir]`: check the anchors, then answer.
///
/// **The data directory is OPTIONAL now, and that is what reading the `sources:` tree bought.** A
/// deployment that declares its data system - kind, location, identity - has already said where the
/// data is, so a third argument there would be a second answer to one question and is refused as
/// one. A caller with no configuration at all is the ordinary command-line case and still passes the
/// directory, which is why `crates/sutura-cli/tests/example.rs` and the quickstart are unchanged.
pub(crate) fn query(args: &[String]) -> ExitCode {
    report((|| {
        let usage = "query <catalog-dir> <question.yaml> [data-dir]";
        let root = arg(args, 0, "catalog-dir", usage)?;
        let question_path = arg(args, 1, "question.yaml", usage)?;
        let data = args.get(2).map(PathBuf::from);

        let pinned = load(Path::new(&root))?;
        let question = read_question(Path::new(&question_path))?;
        let settings = crate::sources::configured()?;
        // **The exhaustive match is the caller's, and that is what erasing later would have cost.**
        // `sutura_app::Warehouses<W>` is generic in ONE adapter, so the answer path is monomorphised
        // per kind; naming both arms here is what makes a third linked adapter a compile error at
        // this line rather than a `SourceUnavailable` on the first question. On a build without the
        // `bigquery` feature the enum has one variant and this reads as it always did.
        match crate::sources::open_engine(
            &pinned,
            settings.sources(),
            settings.server().request_timeout(),
            data.as_deref(),
        )? {
            crate::sources::Opened::Files(opened) => answered(pinned, &question, &opened),
            #[cfg(feature = "bigquery")]
            crate::sources::Opened::BigQuery(opened) => answered(pinned, &question, &opened),
        }
    })())
}

/// Verifies the bundle against the data system that was opened, answers the question, and prints it.
///
/// Generic in the adapter, so the two arms above share every line after them. It takes the bundle by
/// value because `verify_and_validate` consumes it: the only constructor of `Validated` is the one
/// that re-ran every anchor, which is what stops an arrangement of these lines that skips the check.
fn answered<W>(pinned: PinnedDefinitions, question: &Query, opened: &crate::sources::OpenedWith<W>) -> Result<(), String>
where
    W: sutura_domain::warehouse::Warehouse,
{
    // The governance is not an order this function has to remember. One call runs the anchors against
    // the engine it was handed and hands back a bundle only if every one reproduced its number;
    // `sutura_app::answer` takes nothing else. A corrupted anchor stops here rather than answering.
    let validated = sutura_app::verify_and_validate(pinned, &opened.engines)
        .map_err(|e| format!("{}\nthis bundle is not fit to serve", render(&e)))?;
    // `Subject::TheDeploymentItself` is the honest subject: there is no transport and no caller, and
    // the identity the data system is reached under is the process's own.
    let context = RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself));
    // The broker comes off the same value the engines did, which is the whole point of `OpenedWith`
    // carrying it: there is no path here that executes without a credential - `Warehouse::execute`
    // has no signature for it - and what the leg presents agrees with what the adapter was opened
    // under because ONE decision produced both.
    //
    // `into_outcome` because this command writes no audit record: the deadline `Answered` also
    // carries is for a sink, and this binary answers one question on a terminal and exits. The
    // working-set number is the config default: this command answers against one data system and
    // never federates, so `answer` never reads it here.
    let outcome = sutura_app::answer(&validated, question, &context, &opened.broker, &opened.engines, 1 << 30)
        .map_err(|e| render(&e))?
        .into_outcome();
    print_outcome(&outcome)
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
    use sutura_domain::model::{DimensionName, MetricName};
    use sutura_domain::query::{MAX_RANGE_DAYS, RefusalReason};

    use super::{prompt_inputs, render_refusal};

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
