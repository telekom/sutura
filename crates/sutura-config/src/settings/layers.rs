//! File-layer discovery and the context retained across a configuration load.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{DEFAULTS, SettingsError, Sources, VARIABLE_PREFIX, VARIABLE_SEPARATOR};
use crate::raw::RawSettings;

/// Which configuration files were observed, in application order.
///
/// **The answer to a question the resolved values cannot be asked.** Every file layer is optional, so
/// a mistyped configuration directory and a deployment with no files produce the same settings - and
/// the startup report described those settings in detail while naming no source, which is a report
/// that cannot distinguish "the operator's file is in effect" from "the operator's file was never
/// found". An operator reading a value they did not write has nothing to look at.
///
/// A type rather than a bare `Vec<PathBuf>` for one reason: [`Display`](std::fmt::Display) is the
/// single owner of the wording, including the empty case, so the startup log and the `prompt` command
/// cannot describe the same deployment differently.
///
/// **Paths only, and never a value.** A path is not a credential; a value can be one, and
/// `security.access_token` is set by exactly this mechanism. Nothing read out of a file reaches this
/// type - there is nowhere in it for a value to go. The origin it now also carries is the same rule
/// applied to provenance: a per-key source label is not the value that came from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLayers {
    files: Vec<PathBuf>,
    /// Which layer supplied each resolved key (dotted path), so a refusal can name it.
    ///
    /// Built by [`read`] from the `config` crate's own cache, which is the thing that used to be
    /// thrown away the line after it was built. The map has one entry for every resolved leaf,
    /// because the embedded defaults define them all - a value that is refused is a value someone
    /// set, so a refusal that does not know who is the refusal this list exists to prevent.
    origins: BTreeMap<String, LayerOrigin>,
}

/// Which of [`read`]'s sources supplied a resolved key.
///
/// The `config` crate stamps the origin of every value it produces and [`read`] used to discard
/// that stamp the moment it deserialised the tree it carried. This is the same value, kept in the
/// one place a refusal can look it up. It carries no value - only where a value came from - so a
/// credential-shaped key is as safe here as it is in [`ConfigLayers`]' file list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LayerOrigin {
    /// A `SUTURA__*` variable set the value.
    Environment,
    /// A file layer set the value; the path, so the message can say which file.
    File(PathBuf),
    /// The embedded defaults (or, in tests, a supplied overlay) set the value.
    Defaults,
}

impl LayerOrigin {
    /// The label a refusal appends after a key's value, given the key it applies to.
    ///
    /// For the environment the label is the variable NAME reconstructed from the key -
    /// `server.host` becomes `SUTURA__SERVER__HOST` - because the `config` crate stamps every
    /// environment value with the single label "the environment" and the whole reason this exists
    /// is to tell an operator which of several exported variables actually carried the value.
    #[must_use]
    pub(super) fn describe(&self, key: &str) -> String {
        match self {
            Self::Environment => {
                let variable = key
                    .split('.')
                    .map(str::to_ascii_uppercase)
                    .collect::<Vec<_>>()
                    .join(VARIABLE_SEPARATOR);
                format!("{VARIABLE_PREFIX}{VARIABLE_SEPARATOR}{variable}")
            }
            Self::File(path) => path.display().to_string(),
            Self::Defaults => String::from("embedded defaults"),
        }
    }
}

impl ConfigLayers {
    /// The files that were found, in application order: `base.yaml`, then `<environment>.yaml`.
    #[inline]
    #[must_use]
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Which layer supplied `key` (a dotted path such as `server.host`).
    ///
    /// `None` only when the key was never resolved, which for a key the refusal is naming is the
    /// sign of a bug rather than a deployment a message can help with - the refusal text currently
    /// falls back to the embedded defaults when it happens.
    #[inline]
    #[must_use]
    pub(super) fn origin_of(&self, key: &str) -> Option<&LayerOrigin> {
        self.origins.get(key)
    }

    /// Was no file layer observed?
    ///
    /// True for a deployment configured entirely by the embedded defaults and the environment, and
    /// equally true for one whose configuration directory is wrong. The two are indistinguishable
    /// here on purpose - that is the fact, and [`Display`](std::fmt::Display) says it plainly rather
    /// than leaving a blank field.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl core::fmt::Display for ConfigLayers {
    /// The observed file layers, or a sentence saying there are none.
    ///
    /// Written out rather than left to a `Debug` of an empty vector, because `[]` in a log line is
    /// read as "the field is not implemented yet" and not as "this process is running on defaults
    /// nobody wrote down".
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.files.is_empty() {
            return f.write_str("embedded defaults only");
        }
        for (index, path) in self.files.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", path.display())?;
        }
        Ok(())
    }
}

/// A failed configuration load and the file layers observed before it failed.
///
/// The source remains typed; this message adds only file context. Observed paths do not prove
/// that each file parsed or contributed a value. Variables and text overlays have no file path.
#[derive(Debug, thiserror::Error)]
#[error("configuration file layers: {layers}")]
pub struct SettingsLoadError {
    // Boxed: the error half of `read`'s `Result` must stay small (`result_large_err` is on here
    // by choice), and `ConfigLayers` now carries a per-key origin table. The successful half holds
    // the same type inline, where there is no such pressure.
    layers: Box<ConfigLayers>,
    #[source]
    reason: SettingsError,
}

impl SettingsLoadError {
    pub(super) fn new(layers: ConfigLayers, reason: SettingsError) -> Self {
        Self {
            layers: Box::new(layers),
            reason,
        }
    }

    /// The files observed in application order, including on a failed read or parse.
    #[inline]
    pub fn layers(&self) -> &ConfigLayers {
        &self.layers
    }

    /// The typed refusal, retaining its own source chain.
    #[inline]
    pub const fn reason(&self) -> &SettingsError {
        &self.reason
    }
}

/// The raw tree, and which file layers were observed.
///
/// Named rather than written inline, because the pair is the thing: a tree without its provenance is
/// what [`read`] used to return, and the whole point of the change is that the two travel together.
type Layered = (RawSettings, ConfigLayers);

/// Builds the layered configuration and deserializes it into the raw tree.
///
/// **Retains which files were observed, whether the load succeeds or fails.** Both file layers
/// are `.required(false)`, so a mistyped `--config` directory, a volume that failed to mount, and a
/// deployment that genuinely has no files are the same silent success - and the process then starts
/// on embedded defaults with nothing in the log to distinguish the three. What is returned here is
/// what [`ConfigLayers`] carries into the startup report.
///
/// **Files, and only files.** The overlay layer is supplied as text by a test and has no path, and the
/// variable layer has no path either - so neither can appear in the list, and a deployment configured
/// entirely by `SUTURA__*` variables reports no layers. That is the honest answer to "which files",
/// not a claim that nothing overrode the defaults.
pub(super) fn read(sources: &Sources) -> Result<Layered, SettingsLoadError> {
    let mut builder = config::Config::builder().add_source(config::File::from_str(DEFAULTS, config::FileFormat::Yaml));
    let mut layers = Vec::new();

    if let Some(directory) = sources.directory.as_deref() {
        for stem in [BASE_STEM, sources.environment.as_str()] {
            // `layer_path` is the only place a layer's filename is constructed, so what is reported
            // as found and what is added as a source cannot name different files.
            //
            // **The limit, stated where the claim is:** this records what was on disk at this
            // instant. A file that appears or disappears between here and the build, or one that
            // exists and cannot be read, is not covered - the first is a race nothing here closes,
            // and the second becomes `SettingsError::Source` a few lines below.
            let path = layer_path(directory, stem);
            if path.is_file() {
                layers.push(path);
            }
            builder = builder.add_source(optional_file(directory, stem));
        }
    }
    if let Some(overlay) = sources.overlay.as_deref() {
        builder = builder.add_source(config::File::from_str(overlay, config::FileFormat::Yaml));
    }

    let mut variables = config::Environment::with_prefix(VARIABLE_PREFIX)
        .prefix_separator(VARIABLE_SEPARATOR)
        .separator(VARIABLE_SEPARATOR);
    if let Some(ref supplied) = sources.variables {
        // An explicit map, including an EMPTY one, replaces the process environment. That is what
        // makes a test hermetic: without it, a `SUTURA__*` variable in the developer's shell would
        // change what the test asserts.
        variables = variables.source(Some(supplied.clone().into_iter().collect()));
    }
    builder = builder.add_source(variables);

    // Build once and keep the `Config` long enough to walk its cache, then deserialise it. The
    // `config` crate stamps every value it resolves with the layer it came from - a variable, a
    // file, or the embedded defaults - and that stamp used to die on this line, because the only
    // thing `try_deserialize` returned was the raw tree. `collect_origins` reads the stamp first,
    // so a refusal can say where the value it is refusing came from.
    let config = match builder.build() {
        Ok(config) => config,
        Err(cause) => {
            return Err(SettingsLoadError::new(
                ConfigLayers {
                    files: layers,
                    origins: BTreeMap::new(),
                },
                SettingsError::Source { cause: Box::new(cause) },
            ));
        }
    };
    let origins = collect_origins(&config.cache);
    let layers = ConfigLayers { files: layers, origins };
    match config.try_deserialize() {
        Ok(raw) => Ok((raw, layers)),
        Err(cause) => Err(SettingsLoadError::new(
            layers,
            SettingsError::Source { cause: Box::new(cause) },
        )),
    }
}

/// Walks the merged value tree, keeping the per-key origin that `read` used to drop.
///
/// The `config` crate's `cache` is a nested tree whose leaves each remember the URI they came
/// from: `the environment`, a resolved file path, or nothing for the embedded defaults and text
/// overlays (both of which supply their contents as strings). Only leaves are recorded - a table
/// has no origin of its own, because the leaves under it each do.
///
/// **Keyed by the dotted path, once, on the winning value.** A key overridden by a later layer
/// has exactly one leaf here, carrying the origin of the layer that won, which is the layer a
/// refusal about that key has to name.
fn collect_origins(cache: &config::Value) -> BTreeMap<String, LayerOrigin> {
    fn walk(value: &config::Value, path: &mut Vec<String>, origins: &mut BTreeMap<String, LayerOrigin>) {
        if let config::ValueKind::Table(table) = &value.kind {
            // The merged tree is a hash map; sort the row so the walk is deterministic - the
            // destination `BTreeMap` is order-independent, but iteration order is not something to
            // leave to hashing.
            let mut row: Vec<(String, &config::Value)> = table.iter().map(|(k, v)| (k.clone(), v)).collect();
            row.sort_by(|a, b| a.0.cmp(&b.0));
            for (key, child) in row {
                path.push(key);
                if !matches!(&child.kind, config::ValueKind::Table(_)) {
                    let origin = match child.origin() {
                        Some("the environment") => LayerOrigin::Environment,
                        Some(uri) => LayerOrigin::File(PathBuf::from(uri)),
                        None => LayerOrigin::Defaults,
                    };
                    origins.insert(path.join("."), origin);
                }
                walk(child, path, origins);
                path.pop();
            }
        }
    }
    let mut origins = BTreeMap::new();
    walk(cache, &mut Vec::new(), &mut origins);
    origins
}

/// The stem of the layer every environment reads first.
const BASE_STEM: &str = "base";

/// Where a layer's file would be. The one place a layer filename is spelled.
fn layer_path(directory: &Path, stem: &str) -> PathBuf {
    directory.join(format!("{stem}.yaml"))
}

/// A `<stem>.yaml` in `directory`, if it is there.
///
/// Optional rather than required, which is the opposite of what the reference project does, and
/// deliberately: the defaults here are embedded and complete, so a missing file means "nothing to
/// override" rather than "the deployment is half-configured". A required file would make the
/// binary unable to start without a filesystem it does not otherwise need.
///
/// The cost of that choice is that absence is indistinguishable from a wrong path, which is why
/// [`ConfigLayers`] exists: the posture stays fail-open and the log stops being silent about it.
fn optional_file(directory: &Path, stem: &str) -> config::File<config::FileSourceFile, config::FileFormat> {
    config::File::from(layer_path(directory, stem)).required(false)
}
