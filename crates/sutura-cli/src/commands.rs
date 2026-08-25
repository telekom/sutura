//! The commands, and the only place adapters are chosen.
//!
//! This is the composition root: it is where a directory becomes a [`LocalCatalog`] and a file
//! becomes a `DuckDB` connection. Nothing above it names an adapter, which is what lets the same
//! service code be exercised against a fake.
//!
//! `Result<_, String>` throughout, deliberately. The boundary gate fails that in a library crate and
//! exempts a binary, because here the error's audience is a person reading stderr rather than code
//! matching on a variant. Every message is built from a typed error's own `Display`, so the variant
//! is still what decided the wording.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sutura_catalog_local::LocalCatalog;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _, Validated};
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::warehouse::Value;
use sutura_semantic::{Compiled, Dialect};

#[cfg(feature = "exec-duckdb")]
use sutura_domain::model::TableName;
#[cfg(feature = "exec-duckdb")]
use sutura_exec_duckdb::DuckDbWarehouse;

/// The version a locally-read catalog is stamped with when the caller did not say.
///
/// Named rather than derived from the digest: the digest already says what the content is, and a
/// version that repeats it leaves no way to tell two builds of identical content apart. A real
/// deployment passes a commit id.
const DEFAULT_VERSION: &str = "local-working-tree";

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
            println!(
                "{name}\n  measure    {}({})\n  grains     {}\n  dimensions {}\n  anchor     {}",
                metric.measure().aggregate(),
                metric.measure().column(),
                grains.join(", "),
                if dimensions.is_empty() {
                    String::from("none")
                } else {
                    dimensions.join(", ")
                },
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
        println!("  measure    {}({})", metric.measure().aggregate(), metric.measure().column());
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
        match sutura_semantic::compile(&question, &pinned, dialect).map_err(|e| render(&e))? {
            Compiled::Refused { reason } => {
                println!("refused: {reason:?}");
            }
            Compiled::Statement { plan, query } => {
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
#[cfg(feature = "exec-duckdb")]
pub(crate) fn query(args: &[String]) -> ExitCode {
    report((|| {
        let usage = "query <catalog-dir> <question.yaml> <data-dir>";
        let root = arg(args, 0, "catalog-dir", usage)?;
        let question_path = arg(args, 1, "question.yaml", usage)?;
        let data = arg(args, 2, "data-dir", usage)?;

        let pinned = load(Path::new(&root))?;
        let warehouse = open_duckdb(&pinned, Path::new(&data))?;

        // The order is the governance: anchors first, and `Validated::new` is the only way to get a
        // bundle the service will answer from. A corrupted anchor stops here rather than answering.
        let report = sutura_app::verify_anchors(&pinned, &warehouse, Dialect::DuckDb);
        let validated =
            Validated::new(pinned, &report).map_err(|e| format!("{}\nthis bundle is not fit to serve", render(&e)))?;

        let question = read_question(Path::new(&question_path))?;
        let outcome = sutura_app::answer(&validated, &question, &warehouse, Dialect::DuckDb).map_err(|e| render(&e))?;
        print_outcome(&outcome);
        Ok(())
    })())
}

/// Opens an in-memory `DuckDB` and exposes one CSV per model.
///
/// In memory and rebuilt per run on purpose: a database file in a repository is a binary nobody
/// reviews, and a fixture built from the CSV every time cannot drift from it.
#[cfg(feature = "exec-duckdb")]
fn open_duckdb(pinned: &PinnedDefinitions, data: &Path) -> Result<DuckDbWarehouse, String> {
    let sources = sutura_app::sources(pinned);
    let source = match sources.as_slice() {
        [only] => (*only).clone(),
        [] => return Err(String::from("this catalog declares no models, so there is nothing to open")),
        many => {
            return Err(format!(
                "this catalog spans {} data systems, and this build opens one",
                many.len()
            ));
        }
    };
    let warehouse = DuckDbWarehouse::in_memory(source).map_err(|e| render(&e))?;
    for model in pinned.definitions().models().values() {
        let csv = data.join(format!("{}.csv", model.table()));
        if !csv.is_file() {
            return Err(format!("model {} needs {}, which is not there", model.name(), csv.display()));
        }
        attach(&warehouse, model.table(), &csv)?;
    }
    Ok(warehouse)
}

#[cfg(feature = "exec-duckdb")]
fn attach(warehouse: &DuckDbWarehouse, table: &TableName, csv: &Path) -> Result<(), String> {
    warehouse.attach_csv(table, csv).map_err(|e| render(&e))
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

/// The stub that stands in for `query` when the adapter is not compiled.
///
/// A command that says why it is absent, rather than an unknown-command error that reads like a
/// typo. The adapter is default-off because there is no musl `libduckdb` to link the cross builds
/// against, so a binary without it is the normal case rather than a mistake.
#[cfg(not(feature = "exec-duckdb"))]
pub(crate) fn query(_args: &[String]) -> ExitCode {
    eprintln!("sutura: this build has no data-system adapter compiled in.");
    eprintln!("  Rebuild with `--features exec-duckdb` to answer questions from a DuckDB file.");
    eprintln!("  `compile` works without one: it renders the statement and stops.");
    ExitCode::from(2)
}
