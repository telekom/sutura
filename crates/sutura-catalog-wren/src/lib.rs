#![forbid(unsafe_code)]
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

use std::path::{Path, PathBuf};
use std::{fs, io};

/// What ran, for the two lines the command prints.
#[derive(Debug)]
pub struct Summary {
    models: usize,
    relationships: usize,
    metrics: usize,
    refusals: usize,
}

impl Summary {
    #[must_use]
    pub const fn models(&self) -> usize {
        self.models
    }

    #[must_use]
    pub const fn relationships(&self) -> usize {
        self.relationships
    }

    #[must_use]
    pub const fn metrics(&self) -> usize {
        self.metrics
    }

    #[must_use]
    pub const fn refusals(&self) -> usize {
        self.refusals
    }
}

/// Why an import wrote nothing, or stopped part-way through writing.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("could not read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{} is not a wren MDL manifest: {source}", path.display())]
    NotAManifest { path: PathBuf, source: serde_json::Error },
    /// `<out>` already holds files. Refused rather than written into, because a document an
    /// earlier run wrote and this manifest no longer produces would sit beside the new output as if
    /// this run had converted it - and deleting the operator's files is not this command's call.
    #[error("{} is not empty - import into a new or empty directory", .0.display())]
    DestinationNotEmpty(PathBuf),
    #[error("could not write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
}

/// Converts the wren project at `source` into markdown catalog documents and a refusal report
/// under `destination`, which must be absent or empty.
///
/// # Errors
///
/// If `<source>/manifest.json` cannot be read or is not a wren MDL manifest this converter's [`wire`]
/// module can parse, if `destination` already holds a file, or if it cannot be written to.
pub fn import(source: &Path, destination: &Path) -> Result<Summary, ImportError> {
    let manifest_path = source.join("manifest.json");
    let text = fs::read_to_string(&manifest_path).map_err(|source| ImportError::Read {
        path: manifest_path.clone(),
        source,
    })?;
    let manifest: wire::Manifest = serde_json::from_str(&text).map_err(|source| ImportError::NotAManifest {
        path: manifest_path.clone(),
        source,
    })?;
    match fs::read_dir(destination) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(ImportError::DestinationNotEmpty(destination.to_owned()));
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(ImportError::Read {
                path: destination.to_owned(),
                source,
            });
        }
    }
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
fn write_document(destination: &Path, subdirectory: &str, name: &str, text: &str) -> Result<(), ImportError> {
    let dir = destination.join(subdirectory);
    fs::create_dir_all(&dir).map_err(|source| ImportError::Write {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join(format!("{name}.md"));
    fs::write(&path, text).map_err(|source| ImportError::Write { path, source })
}

/// Writes the refusal report as `<destination>/report.txt`.
///
/// `.txt`, not `.md`: `sutura-catalog-local`'s walk loads every `.md` file as a catalog document,
/// and a report has no `kind:` tag to be one - naming it `.md` would make the converted catalog
/// fail to load over its own report.
fn write_report(destination: &Path, converted: &convert::Converted) -> Result<(), ImportError> {
    fs::create_dir_all(destination).map_err(|source| ImportError::Write {
        path: destination.to_owned(),
        source,
    })?;
    let path = destination.join("report.txt");
    fs::write(&path, render::report(converted)).map_err(|source| ImportError::Write { path, source })
}

#[cfg(test)]
mod tests;
