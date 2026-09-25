//! `sutura import wren <dir> <out>`.
//!
//! `<dir>` is a `WrenAI` project directory - read as `<dir>/manifest.json`, the MDL manifest
//! `wren-core-base::mdl::manifest` defines (layout version 2; [`wire`]'s own header names the
//! upstream source this was read from). Everything this converter knows about a wren project is in
//! that one file: no second reader for a `views/` directory or a knowledge export, because nothing
//! upstream of the manifest declares one that this converter's own module header does not already
//! name as out of scope.

mod convert;
mod plan;
mod recognize;
mod render;
mod wire;

use std::fs;
use std::path::Path;

/// What ran, for the two lines the command prints.
pub(crate) struct Summary {
    pub(crate) models: usize,
    pub(crate) relationships: usize,
    pub(crate) metrics: usize,
    pub(crate) refusals: usize,
}

/// Converts the wren project at `source` into markdown catalog documents and a refusal report
/// under `destination`.
///
/// # Errors
///
/// If `<source>/manifest.json` cannot be read or is not a wren manifest this converter's [`wire`]
/// module can parse, or if `destination` cannot be written to.
pub(crate) fn import(source: &Path, destination: &Path) -> Result<Summary, String> {
    let manifest_path = source.join("manifest.json");
    let text = fs::read_to_string(&manifest_path).map_err(|e| format!("could not read {}: {e}", manifest_path.display()))?;
    let manifest: wire::Manifest =
        serde_json::from_str(&text).map_err(|e| format!("{} is not a wren MDL manifest: {e}", manifest_path.display()))?;
    let converted = convert::convert(&manifest);

    for model in &converted.models {
        write_document(destination, "models", model.name.as_str(), &render::model_document(model))?;
    }
    for relationship in &converted.relationships {
        write_document(
            destination,
            "relationships",
            relationship.name.as_str(),
            &render::relationship_document(relationship),
        )?;
    }
    for metric in &converted.metrics {
        write_document(destination, "metrics", metric.name.as_str(), &render::metric_document(metric))?;
    }
    write_report(destination, &converted)?;

    Ok(Summary {
        models: converted.models.len(),
        relationships: converted.relationships.len(),
        metrics: converted.metrics.len(),
        refusals: converted.refusals.len(),
    })
}

/// Writes one catalog document under `<destination>/<subdirectory>/<name>.md`.
///
/// The subdirectory is a convenience for a person browsing the output - `sutura-catalog-local`
/// dispatches on each document's own `kind:` tag and walks every `.md` file regardless of which
/// directory holds it, exactly as `document.rs`'s own header states.
fn write_document(destination: &Path, subdirectory: &str, name: &str, text: &str) -> Result<(), String> {
    let dir = destination.join(subdirectory);
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{name}.md"));
    fs::write(&path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Writes the refusal report as `<destination>/report.txt`.
///
/// `.txt`, not `.md`: `sutura-catalog-local`'s walk loads every `.md` file as a catalog document,
/// and a report has no `kind:` tag to be one - naming it `.md` would make the converted catalog
/// fail to load over its own report.
fn write_report(destination: &Path, converted: &convert::Converted) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| format!("could not create {}: {e}", destination.display()))?;
    let path = destination.join("report.txt");
    fs::write(&path, render::report(converted)).map_err(|e| format!("could not write {}: {e}", path.display()))
}
