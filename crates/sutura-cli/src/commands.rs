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

use sutura_catalog_local::LocalCatalog;
use sutura_domain::measure::RequiredFilter;
use sutura_domain::model::{ModelName, SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::warehouse::Value;
use sutura_exec_datafusion::DataFusionWarehouse;
use sutura_semantic::{Compiled, Dialect};

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
                println!("refused: {reason:?}");
            }
            Compiled::Planned { plan } => {
                let query = sutura_semantic::generate::generate(&plan, dialect).map_err(|e| render(&e))?;
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
        let outcome = sutura_app::answer(&validated, &question, &engine).map_err(|e| render(&e))?;
        print_outcome(&outcome);
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
fn open_engine(pinned: &PinnedDefinitions, data: &Path) -> Result<DataFusionWarehouse, String> {
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
    let engine = DataFusionWarehouse::new(engine_source).map_err(|e| render(&e))?;
    for model in pinned.definitions().models().values() {
        attach(&engine, model.name(), model.table(), data)?;
    }
    Ok(engine)
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

/// Prints an outcome as a table, or as the refusal it is.
fn print_outcome(outcome: &ToolOutcome) {
    match *outcome {
        ToolOutcome::Refusal { ref reason } => println!("refused: {reason:?}"),
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
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use sutura_domain::catalog::{Definitions, Metric, Model};
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
    use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};

    use super::{ENGINE_SOURCE, load, open_engine};

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
            String::new(),
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
            String::new(),
        );
        let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent");
        PinnedDefinitions::pin(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            definitions,
            sutura_catalog_local::digest_of,
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
            sutura_domain::warehouse::Warehouse::source(&engine).as_str(),
            ENGINE_SOURCE,
            "the engine answers to its own name, not to the catalog's"
        );
    }
}
