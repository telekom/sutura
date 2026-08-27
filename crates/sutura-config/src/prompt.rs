//! What goes into the agent-facing system prompt that this deployment hands out.
//!
//! Two keys, and each one is read by something: `sutura_app::prompt::render` is the consumer, and
//! `sutura prompt` is the command that reaches it. That is a requirement rather than a remark - this
//! crate has shipped a group of keys that were parsed, range-checked, refused on a bad value and
//! consumed by nothing, and it was a finding. A key nobody reads reads as a control that is in
//! place.
//!
//! # Why an operator can add to the prompt and cannot replace it
//!
//! [`PromptSettings::instructions_file`] is layered *on top of* the derived text and appended as its
//! last section. There is deliberately no key that substitutes for the derived part.
//!
//! The derived part carries the refusal guidance, which is the one thing an agent talking to this
//! surface most needs and least often has: a refusal arrives as a successful result, and an agent
//! that reads it as an outage retries until something works - which is precisely the behaviour the
//! refusal exists to prevent. A key whose worst setting silently deletes that paragraph would be a
//! key whose failure mode is invisible, and this crate's whole shape is arranged against those. If
//! wholesale replacement is ever wanted it should arrive as its own named key with its own argument,
//! not as an omission from this one.
//!
//! # Why there is no environment-derived default here
//!
//! [`ApiSettings`](crate::api::ApiSettings) and [`LogFormat`](crate::telemetry::LogFormat) default by
//! [`Environment`](crate::Environment) and record whether an operator wrote the value down, so the
//! startup log can tell "somebody chose this" from "nobody did". Neither key here does, and the
//! reason is that neither decision is a function of the environment.
//!
//! Whether a catalog's authors are trusted enough to quote their prose into an agent's context is a
//! fact about who writes the catalog, not about whether the process is on a laptop. A default that
//! dropped the prose in production would be worse than either fixed answer: an agent with no
//! descriptions does not stop, it infers a metric's meaning from its name and reports the inference.
//! And the interesting value - [`CatalogProse::Omitted`] - is never a default, so a deployment
//! running with it is visible from the value itself. That is the case an explicitness flag exists to
//! make legible, and here the value already is.
//!
//! # Why a configured instructions file that is missing is not tolerated
//!
//! The reference implementation this prompt is modelled on reads `<project>/instructions.md` when it
//! is there and silently omits the section when it is not, which is right for a *convention*: no
//! file means nobody wrote one. Here it is a *configured path*, so absence means the operator wrote
//! a path down and the file behind it is not there - and quietly serving a prompt without the
//! operator's rules in it would be the failure this crate refuses everywhere else. Existence is not
//! checked at parse time, for the reason [`CatalogSettings`](crate::catalog::CatalogSettings) does
//! not check its directories: a check here is a claim that is already stale by the time the file is
//! read. The read is what fails, loudly, at the composition root.

use std::path::{Path, PathBuf};

/// Whether the catalog's own prose is quoted into the prompt.
///
/// The word an operator writes. The type that does the work is
/// `sutura_app::prompt::CatalogProse`, and the split is the one
/// [`LogFilter`](crate::telemetry::LogFilter) already uses: this crate parses what was written down,
/// and the crate that acts on it owns the type that acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogProse {
    /// Quoted in, with `> ` at the start of every line and the trust boundary named above the
    /// block. The default.
    ///
    /// **A description is untrusted content and this is not a claim that it is safe.** A per-line
    /// prefix stops catalog text from reaching column zero, so it cannot emit a heading or close a
    /// block; it does nothing about prose that persuades without escaping. `SECURITY.md` treats
    /// catalog content as untrusted, and `sutura_app::prompt` states the residual gap.
    Quoted,
    /// Left out. For a deployment whose catalog authors are not the people who decide what its
    /// agents are told.
    ///
    /// The prompt then says the descriptions exist and were not included, rather than rendering a
    /// list of metric names with no meaning attached: silence is what makes an agent infer a
    /// definition from a name.
    Omitted,
}

/// The word was neither spelling.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` is not a catalog-prose setting - use one of: {}", CatalogProse::NAMES.join(", "))]
pub struct UnknownCatalogProse {
    found: String,
}

impl CatalogProse {
    /// Every accepted spelling.
    pub const NAMES: &'static [&'static str] = &["quoted", "omitted"];

    /// Reads the word.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownCatalogProse> {
        let raw = raw.as_ref().trim();
        match raw.to_ascii_lowercase().as_str() {
            "quoted" | "included" => Ok(Self::Quoted),
            "omitted" | "excluded" | "none" => Ok(Self::Omitted),
            _ => Err(UnknownCatalogProse {
                found: String::from(raw),
            }),
        }
    }

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quoted => "quoted",
            Self::Omitted => "omitted",
        }
    }

    #[inline]
    pub const fn is_quoted(self) -> bool {
        matches!(self, Self::Quoted)
    }
}

impl Default for CatalogProse {
    /// Quoted. A prompt whose metrics have names and no meanings is a prompt that makes an agent
    /// guess, and guessing is the failure this document exists to prevent.
    #[inline]
    fn default() -> Self {
        Self::Quoted
    }
}

impl TryFrom<String> for CatalogProse {
    type Error = UnknownCatalogProse;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for CatalogProse {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where the operator's own prompt text lives.
///
/// A newtype rather than a `PathBuf` so the one thing that can be wrong about it is wrong in one
/// place. The field is private and [`Self::parse`] is the only way in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionsFile(PathBuf);

/// Why the prompt configuration is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPromptSettings {
    /// The path was present and empty.
    ///
    /// Empty is not absent: an unset variable in a shell arrives as `""`, and an empty path resolves
    /// to the process working directory, so this would read a *directory* as an instructions file on
    /// whichever host the process happens to be on. Absent means "no operator text"; empty means
    /// somebody meant to write a path.
    #[error("prompt.instructions_file is empty - write the path, or remove the key")]
    EmptyPath,
}

impl InstructionsFile {
    /// Reads the path.
    ///
    /// Existence is deliberately not checked - see this module's documentation. The read at the
    /// composition root is what fails when a configured file is not there, and it fails loudly
    /// rather than omitting the section.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidPromptSettings> {
        let raw = raw.as_ref().trim();
        if raw.is_empty() {
            return Err(InvalidPromptSettings::EmptyPath);
        }
        Ok(Self(PathBuf::from(raw)))
    }

    #[inline]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl core::fmt::Display for InstructionsFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// Everything that goes into the prompt beyond the pinned bundle and the tool list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSettings {
    instructions_file: Option<InstructionsFile>,
    catalog_prose: CatalogProse,
}

impl PromptSettings {
    /// Assembles the group from parts that have each already been parsed.
    ///
    /// Infallible, like the other groups here: there is no cross-field rule inside it. `omitted`
    /// prose with no operator text is a coherent deployment - it says the metric names and the rules
    /// and nothing about meaning - so it is a choice rather than a refusal.
    #[inline]
    pub const fn new(instructions_file: Option<InstructionsFile>, catalog_prose: CatalogProse) -> Self {
        Self {
            instructions_file,
            catalog_prose,
        }
    }

    /// The operator's own text, if a path was configured.
    #[inline]
    pub const fn instructions_file(&self) -> Option<&InstructionsFile> {
        self.instructions_file.as_ref()
    }

    #[inline]
    pub const fn catalog_prose(&self) -> CatalogProse {
        self.catalog_prose
    }
}

impl Default for PromptSettings {
    /// No operator text, prose quoted. What a deployment that configured nothing gets.
    #[inline]
    fn default() -> Self {
        Self::new(None, CatalogProse::Quoted)
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogProse, InstructionsFile, InvalidPromptSettings, PromptSettings};

    #[test]
    fn both_spellings_of_each_answer_parse_and_an_unknown_one_names_the_alternatives() {
        assert_eq!(CatalogProse::parse("quoted"), Ok(CatalogProse::Quoted));
        assert_eq!(CatalogProse::parse(" INCLUDED "), Ok(CatalogProse::Quoted));
        assert_eq!(CatalogProse::parse("omitted"), Ok(CatalogProse::Omitted));
        assert_eq!(CatalogProse::parse("none"), Ok(CatalogProse::Omitted));
        for name in CatalogProse::NAMES {
            assert!(CatalogProse::parse(name).is_ok(), "{name}");
        }
        let error = CatalogProse::parse("verbatim").expect_err("an unsupported word is an error");
        assert!(error.to_string().contains("quoted"), "{error}");
        assert!(error.to_string().contains("omitted"), "{error}");
    }

    #[test]
    fn the_default_quotes_the_prose_rather_than_dropping_it() {
        // The direction matters and is argued in this module's documentation: an agent handed metric
        // names with no meanings does not stop, it infers a definition from a name and reports the
        // inference. Dropping the prose has to be a decision somebody made.
        assert_eq!(CatalogProse::default(), CatalogProse::Quoted);
        assert!(PromptSettings::default().catalog_prose().is_quoted());
        assert!(PromptSettings::default().instructions_file().is_none());
    }

    #[test]
    fn an_empty_instructions_path_is_refused_rather_than_read_as_absent() {
        // The bug this catches: an unset variable arrives as `""`, an empty path resolves to `.`,
        // and the composition root then tries to read the working DIRECTORY as the operator's
        // prompt text. Absent means no operator text; empty means somebody meant to write a path.
        assert_eq!(InstructionsFile::parse("   "), Err(InvalidPromptSettings::EmptyPath));
        assert_eq!(
            InstructionsFile::parse(" prompts/house-rules.md ")
                .expect("a path is a path")
                .path()
                .to_string_lossy(),
            "prompts/house-rules.md"
        );
    }

    #[test]
    fn a_file_that_does_not_exist_yet_is_accepted() {
        // Deliberate, and the same argument `CatalogSettings` makes about its directories: an
        // existence check here is a claim that is stale by the time the file is read, and it would
        // make a settings test need a filesystem. The read is what fails.
        let configured = InstructionsFile::parse("/nowhere/instructions.md").expect("a path is a path");
        assert_eq!(configured.path().to_string_lossy(), "/nowhere/instructions.md");
    }
}
