//! Exposing a file as a table: which formats this engine reads, and how a codec is decided.
//!
//! Its own module for the reason [`crate::translate`] and [`crate::collect`] are: `lib.rs` sits at
//! the 1000-line gate, and the gate's answer is a seam rather than a shorter change. The seam is a
//! real one - nothing here has seen a plan, a credential or a result, and nothing in `lib.rs`
//! decides a file format.
//!
//! # Compression is a DECLARED capability, not feature unification
//!
//! `docs/adr/0039` records the decision. Two halves, and they are bought by different things:
//!
//! * **Compressed Parquet needs nothing here.** Parquet carries its codec per column chunk inside
//!   the file, and the `parquet` crate's own codecs read it - a `.parquet` written with Snappy,
//!   GZIP or ZSTD pages has always read through [`DataFusionWarehouse::attach_parquet`]. So the compression suffix
//!   list below deliberately does NOT apply to Parquet: a `data.parquet.gz` is an outer wrapper
//!   around a format that is already compressed, and refusing it by name is better than reading a
//!   file nobody meant to write.
//! * **Compressed CSV and NDJSON is what `datafusion/compression` buys**, and it is the reason the
//!   feature is on and the reason `deny.toml` allows one more licence. Those two formats are plain
//!   text, so an outer codec is the only compression they have.
//!
//! # The codec comes from the path, and an unknown one is refused
//!
//! [`Codec::of_path`] is a parse, not a guess: the extension either names a codec this build
//! compiled or it is not a compression suffix at all, and there is no third answer. The engine's
//! own listing path would infer the same thing from the extension and report it as an opaque
//! `DataFusionError`; parsing it here is what lets a mistyped `.gzip` be
//! [`DataFusionError::UnknownCodec`] naming the suffix and the set that would have worked.

use std::path::Path;

use datafusion::datasource::file_format::file_compression_type::FileCompressionType;
use datafusion::execution::options::JsonReadOptions;
use datafusion::prelude::{CsvReadOptions, ParquetReadOptions};
use sutura_domain::model::TableName;

use crate::{DataFusionError, DataFusionWarehouse};

/// The outer codec a text file is wrapped in.
///
/// A closed enum over what `datafusion/compression` compiles rather than a re-export of
/// `FileCompressionType`: that type also has variants for codecs this build does not have, so
/// matching on it would mean an arm nothing can produce. Converted at the one call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// No outer codec: the bytes are the text.
    None,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
}

/// Every compression suffix this build reads, with the codec it names.
///
/// One table, so the parse and the candidate list a composition root offers an operator cannot
/// disagree about which suffixes work: `sutura_cli`'s file-source search reads the same suffixes
/// through [`Codec::suffixes`] rather than spelling its own.
const SUFFIXES: &[(&str, Codec)] = &[
    ("gz", Codec::Gzip),
    ("bz2", Codec::Bzip2),
    ("xz", Codec::Xz),
    ("zst", Codec::Zstd),
];

impl Codec {
    /// The codec a path's outermost extension names, or [`Self::None`] when it names a format.
    ///
    /// **`None` is returned for an extension that is not a compression suffix at all**, which is
    /// what makes `orders.csv` and `orders.csv.gz` both answer through one function. An extension
    /// that LOOKS like a codec and is not one - `.gzip`, `.bzip2`, `.lz4` - is
    /// [`DataFusionError::UnknownCodec`] rather than silently `None`, because reading a compressed
    /// file as text produces a schema inferred from binary rather than an error.
    pub fn of_path(path: &Path) -> Result<Self, DataFusionError> {
        let Some(extension) = path.extension().and_then(|raw| raw.to_str()) else {
            return Ok(Self::None);
        };
        let lowered = extension.to_ascii_lowercase();
        if let Some(&(_, codec)) = SUFFIXES.iter().find(|&&(suffix, _)| suffix == lowered) {
            return Ok(codec);
        }
        // A near-miss is refused; a real format extension is not a codec and needs no entry here.
        if NEAR_MISSES.contains(&lowered.as_str()) {
            return Err(DataFusionError::UnknownCodec {
                path: path.display().to_string(),
                suffix: String::from(extension),
            });
        }
        Ok(Self::None)
    }

    /// The suffixes, in the order a composition root should offer them.
    pub fn suffixes() -> impl Iterator<Item = &'static str> {
        SUFFIXES.iter().map(|&(suffix, _)| suffix)
    }
}

/// Extensions that name a codec this build cannot read, or spell a supported one differently.
///
/// **A closed list rather than "anything unrecognised", and the difference is a working path.** A
/// CSV may legitimately be called `orders.data` or `orders.txt`, so treating every unrecognised
/// extension as a bad codec would refuse files that read fine. What must not pass silently is an
/// extension whose bytes are certainly not text - so this names the compressed spellings only.
const NEAR_MISSES: &[&str] = &["gzip", "bzip2", "bz", "lzma", "lz4", "zstd", "br", "z", "7z", "zip"];

impl From<Codec> for FileCompressionType {
    fn from(codec: Codec) -> Self {
        match codec {
            Codec::None => Self::UNCOMPRESSED,
            Codec::Gzip => Self::GZIP,
            Codec::Bzip2 => Self::BZIP2,
            Codec::Xz => Self::XZ,
            Codec::Zstd => Self::ZSTD,
        }
    }
}

/// The file extension a text format is read under, once its codec suffix is stripped.
///
/// `DataFusion`'s listing path filters a path by extension before it reads it, so a `.csv.gz` has
/// to be registered with the extension it actually ends in or the table resolves to nothing. That
/// is the whole reason this exists: passing `.csv` for a gzipped file produces an empty table
/// rather than an error.
fn extension(path: &Path, default: &str) -> String {
    path.extension()
        .and_then(|raw| raw.to_str())
        .map_or_else(|| String::from(default), |found| format!(".{found}"))
}

/// Every file name one model's table could arrive under, in the order to prefer them.
///
/// **The engine owns this list, not a composition root**, and that is what stops a deployment
/// offering a candidate [`DataFusionWarehouse::attach_file`] then refuses - or missing one it
/// reads. Parquet first, because it carries its own schema and its own codecs; then the two
/// plain-text formats, each uncompressed and then once per codec.
///
/// The names are relative: a caller joins each onto its data directory and takes the first that is
/// a file. `sutura_cli`'s two file-source searches are the callers.
pub fn candidates(table: &TableName) -> impl Iterator<Item = String> {
    let stem = String::from(table.as_str());
    core::iter::once(format!("{stem}.parquet")).chain(TEXT_FORMATS.iter().flat_map(move |&(extension, _)| {
        let plain = format!("{stem}.{extension}");
        core::iter::once(plain.clone()).chain(Codec::suffixes().map(move |suffix| format!("{plain}.{suffix}")))
    }))
}

/// The plain-text formats, and which affordance reads each.
///
/// A table rather than a `match` in [`DataFusionWarehouse::attach_file`], because [`candidates`]
/// has to enumerate exactly the same set: one list read two ways cannot disagree.
const TEXT_FORMATS: &[(&str, TextFormat)] = &[("csv", TextFormat::Csv), ("ndjson", TextFormat::NdJson)];

/// Which of the two plain-text readers a file extension names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextFormat {
    Csv,
    NdJson,
}

/// Registering a file as a table.
///
/// `#[expect]` rather than a second type: these are inherent methods on the adapter, split from
/// `lib.rs` for the length gate, and a trait with one implementor would be the wrong answer.
#[expect(
    clippy::multiple_inherent_impl,
    reason = "the attach affordances live beside the format parse, split from lib.rs for the length gate"
)]
impl DataFusionWarehouse {
    /// Exposes whichever format a path's extensions name, compressed or not.
    ///
    /// The one entry point a composition root needs: it pairs with [`candidates`], so the set of
    /// names a deployment looks for and the set this dispatch accepts are the same list. An
    /// extension naming no format this engine reads is [`DataFusionError::UnknownFormat`] rather
    /// than a guess at CSV - inferring a schema from a file of the wrong shape is how a table
    /// resolves to columns nobody declared.
    pub fn attach_file(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError> {
        // Past the codec suffix, whether or not there is one, so `orders.csv.gz` reads `csv`.
        let stripped = match Codec::of_path(path)? {
            Codec::None => path.to_path_buf(),
            _ => path.with_extension(""),
        };
        let format = stripped.extension().and_then(|raw| raw.to_str()).map(str::to_ascii_lowercase);
        match format.as_deref() {
            Some("parquet") => self.attach_parquet(table, path),
            Some(found) => match TEXT_FORMATS.iter().find(|&&(extension, _)| extension == found) {
                Some(&(_, TextFormat::Csv)) => self.attach_csv(table, path),
                Some(&(_, TextFormat::NdJson)) => self.attach_json(table, path),
                None => Err(DataFusionError::UnknownFormat {
                    path: path.display().to_string(),
                    extension: String::from(found),
                }),
            },
            None => Err(DataFusionError::UnknownFormat {
                path: path.display().to_string(),
                extension: String::new(),
            }),
        }
    }

    /// Exposes a CSV file as a table, compressed or not. `DataFusion` handles inference.
    ///
    /// The codec comes from the path - see [`Codec::of_path`] - so `orders.csv` and `orders.csv.gz`
    /// are one call.
    pub fn attach_csv(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError> {
        let codec = Codec::of_path(path)?;
        let extension = extension(path, ".csv");
        let options = CsvReadOptions::new()
            .file_compression_type(codec.into())
            .file_extension(extension.as_str());
        self.register_csv(table, path.display().to_string(), options)
    }

    /// Exposes a newline-delimited JSON file as a table, compressed or not.
    ///
    /// The CSV affordance's twin for the other plain-text format, and the second half of what
    /// `datafusion/compression` buys. NDJSON rather than a JSON array: the engine reads one record
    /// per line, which is what streams and what a schema can be inferred from without holding the
    /// document.
    pub fn attach_json(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError> {
        let codec = Codec::of_path(path)?;
        let located = path.display().to_string();
        let extension = extension(path, ".ndjson");
        let options = JsonReadOptions::default()
            .file_compression_type(codec.into())
            .file_extension(extension.as_str());
        self.runtime()?
            .block_on(
                self.context
                    .register_json(crate::translate::table_reference(table), located.as_str(), options),
            )
            .map_err(|cause| DataFusionError::Attach {
                table: String::from(table.as_str()),
                path: located,
                cause,
            })
    }

    /// Exposes a deliberately simple conformance fixture CSV with exact shared types.
    ///
    /// Available only with the default-off `fixtures` feature.
    #[cfg(feature = "fixtures")]
    pub fn attach_fixture_csv(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError> {
        let schema = crate::fixture::schema(path).map_err(|cause| DataFusionError::Attach {
            table: String::from(table.as_str()),
            path: path.display().to_string(),
            cause: datafusion::error::DataFusionError::External(Box::new(cause)),
        })?;
        self.register_csv(table, path.display().to_string(), CsvReadOptions::new().schema(&schema))
    }

    fn register_csv(&self, table: &TableName, located: String, options: CsvReadOptions<'_>) -> Result<(), DataFusionError> {
        self.runtime()?
            .block_on(
                self.context
                    .register_csv(crate::translate::table_reference(table), located.as_str(), options),
            )
            .map_err(|cause| DataFusionError::Attach {
                table: String::from(table.as_str()),
                path: located,
                cause,
            })
    }

    /// Exposes a Parquet file as a table.
    ///
    /// **No codec argument, and that is the point rather than an omission.** A Parquet file records
    /// its own compression per column chunk and the `parquet` feature's codecs read it, so there is
    /// nothing for [`Codec`] to decide - and an outer `.gz` around a Parquet file is
    /// [`DataFusionError::UnknownCodec`] through [`Codec::of_path`], which is the honest answer to a
    /// file that should not have been written that way. The module header carries the distinction.
    pub fn attach_parquet(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError> {
        if Codec::of_path(path)? != Codec::None {
            return Err(DataFusionError::UnknownCodec {
                path: path.display().to_string(),
                suffix: String::from("an outer codec around Parquet, whose own codecs are inside the file"),
            });
        }
        let located = path.display().to_string();
        self.runtime()?
            .block_on(self.context.register_parquet(
                crate::translate::table_reference(table),
                located.as_str(),
                ParquetReadOptions::default(),
            ))
            .map_err(|cause| DataFusionError::Attach {
                table: String::from(table.as_str()),
                path: located,
                cause,
            })
    }
}

/// The codec parse, and a compressed file read end to end.
#[cfg(test)]
mod tests;
