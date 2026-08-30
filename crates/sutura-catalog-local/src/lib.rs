//! A [`SemanticCatalog`] over a directory of markdown documents with YAML frontmatter.
//!
//! One document per model, relationship and metric: the frontmatter is the definition and the prose
//! is the description that travels with an answer. It is the catalog for the case where there is no
//! upstream semantic layer to take a rendered statement from, which is the case a person is in the
//! first time they try this. `docs/adr/0001-first-party-semantic-models.md` is the decision.
//!
//! Three properties of the load are worth stating, because each is a mechanism rather than an
//! intention:
//!
//! **[`LocalCatalog::load`] takes no request context**, because the trait does not have one to give
//! it. A catalog that could see the caller could return a different definition per caller, and the
//! digest that travels with an answer would then describe something other than what produced it.
//!
//! **The walk is sorted**, so the same directory produces the same bundle. `read_dir` returns
//! entries in whatever order the filesystem chose, and a digest that moves between two runs over
//! unchanged files is a digest nobody can act on.
//!
//! **An empty directory is an error.** A mistyped root that happens to exist would otherwise load a
//! catalog with no metrics and refuse every question, and the refusal would name the metric rather
//! than the path.

pub mod document;
pub mod frontmatter;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{
    Definitions, Description, InconsistentDefinitions, InvalidDescription, Metric, Model, Relationship,
};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{
    Absence, Caveat, Example, GlossaryEntry, InconsistentKnowledge, InvalidNoteBody, Knowledge, KnowledgeCapabilities,
    KnowledgeInput, NoteBody,
};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};

use crate::document::knowledge::{CaveatDoc, ExampleDoc, GlossaryDoc, NotDefinedDoc};
use crate::document::{DocumentKind, InvalidMetricDocument, KindProbe, MetricDoc, ModelDoc, RelationshipDoc};
use crate::frontmatter::{MalformedDocument, Split};

/// The extension a catalog document has to have.
const DOCUMENT_EXTENSION: &str = "md";

/// The two halves of a bundle's content, read and checked but not yet pinned.
///
/// An alias because the pair appears in two signatures and `clippy.toml` sets
/// `type-complexity-threshold` to 100 against the default 250 - so
/// `Result<(Definitions, Knowledge), LocalCatalogError>` is a lint asking to be named. It is also the
/// better name: this is what a catalog IS, and pinning is what happens to it next.
type Content = (Definitions, Knowledge);

/// Why a directory could not be read as a catalog.
///
/// Every variant carries the path, because a catalog is many files and "invalid type: integer" with
/// no file name is a message that sends the reader to read all of them.
///
/// **The variant is the contract, so a variant that names the wrong failure is a false contract even
/// with a truthful `#[source]` beneath it.** This enum had one that did: every failure of the
/// kind-probe deserialization became `UnknownKind`, whose message was "declares no `kind`". A
/// document saying `kind: dashboard`, one saying `kind: 3`, and one whose YAML did not parse at all
/// were three different problems reported as the same missing key - and a caller matching on the
/// variant, which is the only thing a caller can match on, was told something untrue in two cases out
/// of three. Keeping the parse error reachable through the chain did not fix that; it only meant the
/// truth was available to whoever thought to look past the variant.
///
/// It is two variants now, and the line between them is a mechanism rather than a guess at an error
/// message: [`Self::MalformedFrontmatter`] is raised when the block does not parse as YAML at all,
/// and [`Self::IdentifyKind`] when it parses and still does not identify the document. They are two
/// rather than four - missing, unrecognised, wrong type - because nothing in this workspace matches
/// on any of them, so a split finer than the remedy is a branch nobody takes: "your frontmatter is
/// not YAML" and "your frontmatter does not say what this is" send a reader to different places, and
/// "the `kind` key is missing" versus "its value is not one of three" send them to the same one. A
/// finer split is a cheap change if a caller ever needs the branch.
#[derive(Debug, thiserror::Error)]
pub enum LocalCatalogError {
    #[error("the catalog root {path} is not a directory")]
    NotADirectory { path: PathBuf },
    #[error("could not read {path}")]
    Io {
        path: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    #[error("{path} is not a catalog document")]
    Malformed {
        path: PathBuf,
        #[source]
        cause: MalformedDocument,
    },
    #[error("could not read the frontmatter of {path} as a {kind}")]
    Frontmatter {
        path: PathBuf,
        kind: &'static str,
        #[source]
        cause: serde_norway::Error,
    },
    #[error("the frontmatter of {path} is not YAML")]
    MalformedFrontmatter {
        path: PathBuf,
        #[source]
        cause: serde_norway::Error,
    },
    #[error("the frontmatter of {path} does not say what kind of document it is")]
    IdentifyKind {
        path: PathBuf,
        #[source]
        cause: serde_norway::Error,
    },
    #[error("{path} is not a usable metric")]
    Metric {
        path: PathBuf,
        #[source]
        cause: InvalidMetricDocument,
    },
    /// The prose of a definition document is not a usable description.
    ///
    /// **The variant that did not exist, and its absence was the hole.** A model's and a metric's
    /// prose used to reach [`sutura_domain::catalog`] as `String::from(split.body())` - no character
    /// check, no length check, nothing - while the prose of a note beside it went through
    /// [`NoteBody`]. So the one channel that carried reviewed prose into an agent's context without a
    /// parse was the definitional one, which is the prose an agent is most likely to act on. It is a
    /// separate variant from [`Self::NoteBody`] for the same reason that one is separate from
    /// [`Self::Frontmatter`]: the remedy is a different part of a different file.
    #[error("the prose of {path} is not a usable description")]
    Description {
        path: PathBuf,
        #[source]
        cause: InvalidDescription,
    },
    /// The prose of a knowledge document is not a usable note body: nothing at all, or more of it
    /// than a note may carry.
    ///
    /// Its own variant rather than folded into [`Self::Frontmatter`], because it is a failure of the
    /// BODY and the path is not enough to find it: a reader told "could not read the frontmatter"
    /// would go and look at the frontmatter, which is fine.
    #[error("the prose of {path} is not a usable note body")]
    NoteBody {
        path: PathBuf,
        #[source]
        cause: InvalidNoteBody,
    },
    #[error("the catalog does not hold together")]
    Inconsistent {
        #[source]
        cause: InconsistentDefinitions,
    },
    /// The notes do not hold together with the definitions they are about.
    ///
    /// Separate from [`Self::Inconsistent`] because they are two checks over two halves of the
    /// bundle, and the remedies are different documents: one sends a reader to a metric, the other to
    /// a glossary entry that names a value the metric does not permit.
    ///
    /// The cause carries no path, and that is a real limit rather than an oversight. A note's
    /// inconsistency is a fact about the note AND the definitions together, so it is found after both
    /// have been read - and by then this adapter no longer knows which file each note came from. What
    /// the message does carry is the note's own name or term, which is unique across the catalog and
    /// is what a `grep` finds.
    #[error("the catalog's knowledge does not hold together")]
    UncheckableKnowledge {
        #[source]
        cause: InconsistentKnowledge,
    },
    #[error("the catalog at {path} holds no documents")]
    Empty { path: PathBuf },
    /// The domain could not hash the definitions.
    ///
    /// One variant rather than the two this used to have. Those two - the canonical form failing to
    /// serialize, and the resulting hex failing to parse as a digest - are now both inside the
    /// domain's own hashing, and neither is a fact about reading a directory. The chain still says
    /// which one happened.
    #[error("the definitions could not be hashed")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

// The canonical form and its hash used to live here, and a review showed why they could not: while
// `PinnedDefinitions::pin` took the hashing FUNCTION, safe public code could hand it one that
// ignored its argument and pair any digest with any set of definitions. The hash is now the domain's
// own, so this adapter reads documents and nothing else - and there is one canonical form rather
// than one per adapter, which is what a second catalog adapter would otherwise have had to
// reimplement or import from here.

/// A catalog read from a directory of documents.
#[derive(Debug, Clone)]
pub struct LocalCatalog {
    root: PathBuf,
    version: DefinitionVersion,
}

impl LocalCatalog {
    /// Points a catalog at a directory.
    ///
    /// The version is supplied rather than derived, because what identifies a snapshot of a
    /// directory is not something the directory knows: it is a commit id, a build number or a tag,
    /// and only the caller has it. Deriving it from the digest would make the two say the same thing
    /// twice and leave no way to tell two builds of identical content apart.
    pub const fn new(root: PathBuf, version: DefinitionVersion) -> Self {
        Self { root, version }
    }

    #[inline]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every document under the root, in sorted order.
    ///
    /// Depth-first with the entries of each directory sorted, so the traversal is a function of the
    /// tree rather than of the filesystem. Files that are not documents are skipped rather than
    /// refused, so a `README.md`-adjacent `.gitkeep` or a stray image does not break a load; a
    /// document with the right extension and the wrong content still fails loudly.
    ///
    /// **A symlink is one of the things that is not a document, and that is what bounds the walk.**
    /// Catalog content is untrusted, and a link pointing at an ancestor is a cycle: the walk descends
    /// into it, finds the link again one level down, descends again, and stops only where the kernel
    /// refuses to resolve any more links in one path - 40 of them on Linux, measured rather than
    /// assumed. So the failure is not a hang; it is the same document collected once per level, which
    /// is 41 copies of one metric handed to `Definitions::assemble` and a digest that depends on the
    /// link structure rather than on the definitions. Nothing about that reads as "this catalog has a
    /// loop in it". A loop built out of real directories - a bind mount of an ancestor - has no such
    /// kernel limit and would not terminate; only a visited set catches that one, and nothing in this
    /// repository mounts anything into a catalog.
    ///
    /// The decision is made from the directory entry's own type rather than from the path, because
    /// `Path::is_dir` follows the link and answers about the target.
    fn documents(&self) -> Result<Vec<PathBuf>, LocalCatalogError> {
        if !self.root.is_dir() {
            return Err(LocalCatalogError::NotADirectory { path: self.root.clone() });
        }
        let mut found = BTreeSet::new();
        let mut pending = vec![self.root.clone()];
        while let Some(directory) = pending.pop() {
            let entries = std::fs::read_dir(&directory).map_err(|cause| LocalCatalogError::Io {
                path: directory.clone(),
                cause,
            })?;
            for entry in entries {
                let entry = entry.map_err(|cause| LocalCatalogError::Io {
                    path: directory.clone(),
                    cause,
                })?;
                let path = entry.path();
                // The entry's OWN type, which says nothing about what a link points at. That is the
                // whole fix: `path.is_dir()` follows the link, so a link to an ancestor came back
                // as a directory and the walk descended into itself.
                let kind = entry.file_type().map_err(|cause| LocalCatalogError::Io {
                    path: path.clone(),
                    cause,
                })?;
                // `is_file` and not `!is_dir()`, which is what `filetype_is_file` asks for: the lint's
                // point is that `is_file` is false for a socket, a FIFO or a device node, and being
                // false for those is exactly what this wants. A catalog document is a regular file;
                // anything else carrying a `.md` name is one of the things this walk skips.
                #[expect(
                    clippy::filetype_is_file,
                    reason = "a document is a regular file - a link, a socket or a device node is not, and skipping those is the point"
                )]
                let is_document = kind.is_file() && path.extension().is_some_and(|ext| ext == DOCUMENT_EXTENSION);
                if kind.is_dir() {
                    pending.push(path);
                } else if is_document {
                    // A `BTreeSet` rather than a sort at the end: the ordering is the point, and
                    // making it a property of the collection means it cannot be forgotten.
                    found.insert(path);
                }
            }
        }
        if found.is_empty() {
            return Err(LocalCatalogError::Empty { path: self.root.clone() });
        }
        Ok(found.into_iter().collect())
    }

    /// Reads every document and turns it into domain types.
    ///
    /// **Both halves in one walk, because a document says what it is.** A glossary note and a metric
    /// arrive through the same read, the same frontmatter split and the same `kind:` tag; only the
    /// dispatch differs. A second walk over a `knowledge/` subdirectory would make the directory
    /// layout part of the format, and the layout is the one thing about this adapter that another
    /// adapter - a metadata service with no directories at all - cannot reuse.
    fn read_all(&self) -> Result<Content, LocalCatalogError> {
        let mut collected = Collected::default();

        for path in self.documents()? {
            let text = std::fs::read_to_string(&path).map_err(|cause| LocalCatalogError::Io {
                path: path.clone(),
                cause,
            })?;
            let split = frontmatter::split(&text).map_err(|cause| LocalCatalogError::Malformed {
                path: path.clone(),
                cause,
            })?;
            // Two passes, and the first one exists to tell two failures apart. Deserializing
            // `KindProbe` straight from the text collapses "this is not YAML" and "this YAML does not
            // identify itself" into one error, and the variant then has to claim one of them for
            // both. Parsing to a `Value` first answers the YAML question on its own terms, so each
            // failure gets the variant that is true of it.
            //
            // **The probe's own error cannot be trusted to make that distinction, and this was
            // measured rather than assumed.** `kind: [metric` followed by another key is an
            // unterminated flow sequence - not YAML at all - and the probe reports "invalid type:
            // sequence" for it, a message about a data shape in a document that has no shape. It can
            // do that because it stops caring once it has read one key, so it never reaches the point
            // where the document falls apart. Parsing to a `Value` has to read the whole block, which
            // is exactly why its verdict is the one worth having.
            //
            // The parsed value is not kept: `KindProbe` reads one key, and `from_value` takes a
            // `Value` by move, so threading it through would cost a clone of the whole block to save
            // a parse of it. What this pass produces is the verdict, not the data.
            if let Err(cause) = serde_norway::from_str::<serde_norway::Value>(split.frontmatter()) {
                return Err(LocalCatalogError::MalformedFrontmatter {
                    path: path.clone(),
                    cause,
                });
            }
            let probe: KindProbe =
                serde_norway::from_str(split.frontmatter()).map_err(|cause| LocalCatalogError::IdentifyKind {
                    path: path.clone(),
                    cause,
                })?;
            collected.absorb(&path, &split, probe.kind())?;
        }

        collected.assemble()
    }

    fn parse<T>(path: &Path, frontmatter: &str, kind: DocumentKind) -> Result<T, LocalCatalogError>
    where
        T: serde::de::DeserializeOwned,
    {
        serde_norway::from_str(frontmatter).map_err(|cause| LocalCatalogError::Frontmatter {
            path: PathBuf::from(path),
            kind: kind.as_str(),
            cause,
        })
    }
}

/// Everything read so far, in the two groups it will be checked in.
///
/// A value rather than seven locals in [`LocalCatalog::read_all`], and the reason is a limit rather
/// than taste: seven `Vec`s threaded through a dispatch function is more arguments than
/// `clippy.toml`'s `too-many-arguments-threshold` permits, and the alternative - one function holding
/// the walk and both dispatches - is over `too_many_lines`. Both limits are pointing at the same
/// thing: reading a document and deciding what it is are two jobs.
///
/// Private, with private fields, so a struct literal cannot build a half-collected catalog outside
/// the one walk that fills it.
#[derive(Default)]
struct Collected {
    models: Vec<Model>,
    relationships: Vec<Relationship>,
    metrics: Vec<Metric>,
    glossary: Vec<GlossaryEntry>,
    caveats: Vec<Caveat>,
    absences: Vec<Absence>,
    examples: Vec<Example>,
}

impl Collected {
    /// One document, into whichever half it belongs to.
    fn absorb(&mut self, path: &Path, split: &Split<'_>, kind: DocumentKind) -> Result<(), LocalCatalogError> {
        match kind {
            DocumentKind::Model | DocumentKind::Relationship | DocumentKind::Metric => self.absorb_definition(path, split, kind),
            DocumentKind::Glossary | DocumentKind::Caveat | DocumentKind::NotDefined | DocumentKind::Example => {
                self.absorb_note(path, split, kind)
            }
        }
    }

    /// A document that decides what executes.
    ///
    /// The body becomes a [`Description`] before anything else, which is what
    /// [`Self::absorb_note`] already did with a [`NoteBody`] - and the symmetry is the fix. Both
    /// halves of a catalog carry authored prose into the same rendered prompt, and only one of them
    /// used to be parsed.
    ///
    /// It is parsed for a relationship document too, whose prose this adapter then discards -
    /// [`Relationship`] has no description field. Deliberately: the rule a reviewed definition
    /// document is held to should not depend on which of its fields the current domain types happen
    /// to read, and the day a relationship grows a description the check is already where it belongs.
    fn absorb_definition(&mut self, path: &Path, split: &Split<'_>, kind: DocumentKind) -> Result<(), LocalCatalogError> {
        let description = Description::parse(split.body()).map_err(|cause| LocalCatalogError::Description {
            path: PathBuf::from(path),
            cause,
        })?;
        match kind {
            DocumentKind::Model => {
                let doc: ModelDoc = LocalCatalog::parse(path, split.frontmatter(), kind)?;
                self.models.push(doc.into_domain(description));
            }
            DocumentKind::Relationship => {
                let doc: RelationshipDoc = LocalCatalog::parse(path, split.frontmatter(), kind)?;
                self.relationships.push(doc.into_domain());
            }
            // Every other kind is a note, and `absorb` is what decides which of the two this is. A
            // wildcard rather than four unreachable arms, because the exhaustiveness that matters is
            // the one in `absorb`: a kind added there with no arm does not compile.
            _ => {
                let doc: MetricDoc = LocalCatalog::parse(path, split.frontmatter(), kind)?;
                self.metrics
                    .push(doc.into_domain(description).map_err(|cause| LocalCatalogError::Metric {
                        path: PathBuf::from(path),
                        cause,
                    })?);
            }
        }
        Ok(())
    }

    /// A document that decides what a reader understands.
    ///
    /// The body becomes a [`NoteBody`] before anything else, because that is where the size caps are
    /// and they are the same caps for all four kinds. A note over the cap is an error naming the
    /// file; nothing here shortens one.
    fn absorb_note(&mut self, path: &Path, split: &Split<'_>, kind: DocumentKind) -> Result<(), LocalCatalogError> {
        let body = NoteBody::parse(split.body()).map_err(|cause| LocalCatalogError::NoteBody {
            path: PathBuf::from(path),
            cause,
        })?;
        match kind {
            DocumentKind::Glossary => {
                let doc: GlossaryDoc = LocalCatalog::parse(path, split.frontmatter(), kind)?;
                self.glossary.push(doc.into_domain(body));
            }
            DocumentKind::Caveat => {
                let doc: CaveatDoc = LocalCatalog::parse(path, split.frontmatter(), kind)?;
                self.caveats.push(doc.into_domain(body));
            }
            DocumentKind::NotDefined => {
                let doc: NotDefinedDoc = LocalCatalog::parse(path, split.frontmatter(), kind)?;
                self.absences.push(doc.into_domain(body));
            }
            // As above: `absorb` has already decided this is a note, so the remaining kind is the
            // example.
            _ => {
                let doc: ExampleDoc = LocalCatalog::parse(path, split.frontmatter(), kind)?;
                self.examples.push(doc.into_domain(body));
            }
        }
        Ok(())
    }

    /// The definitions, and the knowledge checked against them.
    ///
    /// **This adapter declares every knowledge capability there is, and that is a statement about the
    /// ADAPTER rather than about the directory it read.** A markdown catalog in git is a reviewed
    /// first-party catalog: it can carry a glossary, a caveat, a reviewed list of what is deliberately
    /// undefined and a worked question, so a tree that happens to hold none of one of them has an
    /// empty list rather than no such concept - and the prompt may still say the absence list is
    /// authoritative, because somebody keeps it. A metadata-service adapter is the other case: it has
    /// glossary terms with synonyms and no way at all to record an absence, so it will declare the two
    /// it can represent and never the other two. `sutura_domain::knowledge` argues why the two must
    /// not look alike.
    ///
    /// [`KnowledgeCapabilities::all`] rather than a list of the four, deliberately: it says "this
    /// provider supports whatever kinds exist", which is what makes this the reference adapter and
    /// what keeps a fifth kind from needing an edit here. An adapter mapping a fixed external schema
    /// gets the opposite treatment - `of([..])`, so a new kind leaves its declaration alone.
    fn assemble(self) -> Result<Content, LocalCatalogError> {
        let definitions = Definitions::assemble(self.models, self.relationships, self.metrics)
            .map_err(|cause| LocalCatalogError::Inconsistent { cause })?;
        let knowledge = Knowledge::assemble(
            &definitions,
            KnowledgeInput::new(
                KnowledgeCapabilities::all(),
                self.glossary,
                self.caveats,
                self.absences,
                self.examples,
            ),
        )
        .map_err(|cause| LocalCatalogError::UncheckableKnowledge { cause })?;
        Ok((definitions, knowledge))
    }
}

impl SemanticCatalog for LocalCatalog {
    type Error = LocalCatalogError;

    /// **Everything, and that is a statement about the ADAPTER rather than about the directory it
    /// read.** The markdown format is defined in this repository and grows with the domain, so this
    /// adapter supplies whatever kinds exist - a tenth definition kind or a fifth knowledge
    /// capability gets a document shape and needs no edit on this line. That is what makes this the
    /// reference adapter, and it is the same argument [`KnowledgeCapabilities::all`] carries in
    /// [`Self::read_all`]'s doc comment, generalised to the other half of the bundle by
    /// `docs/adr/0016-what-datahub-can-carry.md`.
    ///
    /// An adapter mapping a fixed external schema gets the opposite treatment -
    /// `MetadataCapabilities::of` with two explicit lists, so a new kind leaves its declaration
    /// alone rather than silently widening it.
    ///
    /// **The limit, next to the claim.** Declaring every kind says nothing about the directory: a
    /// tree with no relationships in it produces a bundle with none, and this declaration is what
    /// tells a reader that the emptiness is the corpus's rather than the format's.
    /// `sutura-app`'s golden suite checks the pair over the example catalog, which does carry every
    /// kind - so a claim wider than what this adapter can actually read fails there.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::everything()
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let (definitions, knowledge) = self.read_all()?;
        // `pin` hashes the content it is about to store, using the domain's own canonical form. This
        // adapter no longer supplies the hasher, and that is the point: while it did, safe public
        // code could pass a function that ignored its argument and pair any digest with any
        // definitions. There is nothing to pass now, so there is nothing to get wrong. The knowledge
        // goes under the same digest, because a glossary decides which metric a question is about.
        PinnedDefinitions::pin(self.version.clone(), definitions, knowledge).map_err(|cause| LocalCatalogError::Digest { cause })
    }
}

#[cfg(test)]
mod tests {
    use crate::LocalCatalog;
    use std::path::PathBuf;
    use sutura_domain::pinned::DefinitionVersion;

    fn catalog(root: PathBuf) -> LocalCatalog {
        LocalCatalog::new(root, DefinitionVersion::parse("test-1").expect("a test version is a version"))
    }

    /// An empty directory of this test's own, cleared on the way IN.
    ///
    /// `tempfile` is not a dependency of this workspace and one test is not the argument for adding
    /// one; `xtask` builds its scratch directories the same way. Cleared before rather than after so
    /// a failing run leaves its evidence on disk and the next run still starts clean.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-catalog-local-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// What one document loads to, given the whole document.
    ///
    /// A catalog of exactly one file, so the error is about that file and nothing else: an empty
    /// directory is its own error here, and a second document would let `Definitions::assemble` fail
    /// first for a reason these tests are not about.
    ///
    /// The whole outcome rather than the failure, because a test asserting that a document is refused
    /// says nothing on its own: the same assertion passes for a document refused two guards earlier,
    /// for a reason the test is not about. The twin - the same document with the one field corrected,
    /// loading - is what makes it a statement about that field, and it needs the `Ok` side.
    fn outcome_for(name: &str, document: &str) -> Result<crate::Content, crate::LocalCatalogError> {
        outcome_of(name, &[("doc.md", document)])
    }

    /// What a whole small catalog loads to.
    ///
    /// More than one document, for the case where the failure under test is a check over the WHOLE
    /// bundle rather than over one file: a note is about the definitions beside it, so the twin that
    /// shows the refusal is about the note's own field needs a definition for the note to be about.
    #[expect(
        clippy::unwrap_in_result,
        reason = "the panic is the scratch directory being unwritable, which is the harness failing rather than \
                  a catalog being unreadable - folding it into LocalCatalogError would give every test a second \
                  failure mode indistinguishable from the one it asserts"
    )]
    fn outcome_of(name: &str, documents: &[(&str, &str)]) -> Result<crate::Content, crate::LocalCatalogError> {
        let root = scratch(name);
        for &(file, document) in documents {
            std::fs::write(root.join(file), document).expect("a document is writable");
        }
        let outcome = catalog(root.clone()).read_all();
        drop(std::fs::remove_dir_all(&root));
        outcome
    }

    /// What a document failed to be, given the whole document.
    fn error_for(name: &str, document: &str) -> crate::LocalCatalogError {
        outcome_for(name, document).expect_err("this document cannot load")
    }

    /// Unix only, and that is a portability statement rather than a gap in the suite.
    ///
    /// The walk is what these tests exercise, and provoking the case that broke it needs a symlink.
    /// Creating one on Windows requires either developer mode or an elevated process, so a test that
    /// created one there would fail on a plain checkout for a reason that has nothing to do with this
    /// code. Everything that gates - the container and CI - is Linux, so the case is covered where the
    /// verdict is taken; the fix itself is `DirEntry::file_type`, which does not follow a link on either
    /// platform.
    #[cfg(unix)]
    mod symlinks {
        use super::{catalog, scratch};

        #[test]
        fn a_symlink_pointing_at_an_ancestor_does_not_make_the_walk_descend_into_itself() {
            // The bug: `path.is_dir()` follows the link, so `root/loop` answered "directory" and the
            // walk pushed it, found `root/loop/loop` one level down, pushed that, and kept going. What
            // this asserted before the fix is worth recording, because it is not what it looks like:
            // the walk terminated - the kernel stops resolving after 40 links in one path - and
            // returned `revenue.md` 41 times, once per level. So the failure was not a hang, it was one
            // metric defined 41 times and a digest that depended on the link structure.
            let root = scratch("symlink-loop");
            std::fs::write(root.join("revenue.md"), "---\nkind: metric\n---\n").expect("a document is writable");
            std::os::unix::fs::symlink(&root, root.join("loop")).expect("a symlink to the root is creatable");

            let found = catalog(root.clone())
                .documents()
                .expect("the walk terminates and reports the one document");

            assert_eq!(found, vec![root.join("revenue.md")]);
            drop(std::fs::remove_dir_all(&root));
        }

        #[test]
        fn a_symlink_to_a_document_outside_the_root_is_skipped_rather_than_followed() {
            // The same rule seen from the other side, and the reason it is a rule rather than a
            // cycle-detector: a visited set would still have followed this one. A catalog is what is IN
            // the directory, so a link out of it is not a document - and the alternative is a catalog
            // whose digest depends on a file the tree does not contain.
            let root = scratch("symlink-out");
            let outside = scratch("symlink-out-target");
            let target = outside.join("elsewhere.md");
            std::fs::write(&target, "---\nkind: metric\n---\n").expect("a document is writable");
            std::fs::write(root.join("revenue.md"), "---\nkind: metric\n---\n").expect("a document is writable");
            std::os::unix::fs::symlink(&target, root.join("linked.md")).expect("a symlink to a file is creatable");

            let found = catalog(root.clone()).documents().expect("the walk reports the real document");

            assert_eq!(found, vec![root.join("revenue.md")]);
            drop(std::fs::remove_dir_all(&root));
            drop(std::fs::remove_dir_all(&outside));
        }
    }

    /// The variant a document's failure lands in, which is the part a caller matches on.
    mod kinds {
        use super::error_for;
        use crate::LocalCatalogError;

        #[test]
        fn a_frontmatter_block_that_is_not_yaml_is_not_reported_as_a_missing_kind() {
            // The finding. Every failure of the kind probe used to become one variant whose message
            // read "declares no `kind`", so a syntax error at line 2 was reported as an absent key -
            // a false statement about the file, in the one field a caller can branch on. The source
            // chain carried the truth, which is not the same as the error stating it.
            let broken = error_for("kind-broken-yaml", "---\nkind: [metric\nname: revenue\n---\nProse.\n");
            assert!(
                matches!(broken, LocalCatalogError::MalformedFrontmatter { .. }),
                "a frontmatter block that is not YAML must say so: {broken:?}"
            );
            assert!(
                core::error::Error::source(&broken).is_some(),
                "the parse failure names the line, so it has to stay reachable: {broken:?}"
            );
        }

        #[test]
        fn a_document_that_parses_and_does_not_identify_itself_is_one_neutral_variant() {
            // Three different ways of not saying what a document is, and one variant for all three,
            // because nothing in this workspace branches on the difference and each one sends the
            // author to the same line of the same file. The variant is neutral for that reason: it
            // says the frontmatter does not identify the document rather than claiming which of the
            // three it was.
            for (name, document) in [
                ("kind-unknown", "---\nkind: dashboard\nname: revenue\n---\nProse.\n"),
                ("kind-wrong-type", "---\nkind: 3\nname: revenue\n---\nProse.\n"),
                ("kind-missing", "---\nname: revenue\n---\nProse.\n"),
            ] {
                let err = error_for(name, document);
                assert!(
                    matches!(err, LocalCatalogError::IdentifyKind { .. }),
                    "{name} must be an IdentifyKind failure: {err:?}"
                );
                assert!(
                    core::error::Error::source(&err).is_some(),
                    "{name} must keep the parse failure reachable as a source"
                );
            }
        }

        #[test]
        fn malformed_yaml_and_an_unrecognised_kind_are_different_variants() {
            // The assertion the finding actually asks for, made on the discriminant rather than on a
            // message: two failures that a caller has to be able to tell apart must not be one
            // variant, whatever their messages say.
            let broken = error_for("split-broken-yaml", "---\nkind: [metric\n---\nProse.\n");
            let unknown = error_for("split-unknown-kind", "---\nkind: dashboard\n---\nProse.\n");
            assert_ne!(
                core::mem::discriminant(&broken),
                core::mem::discriminant(&unknown),
                "these must not collapse into one variant: {broken:?} / {unknown:?}"
            );
        }
    }

    /// The three refusals that are about the CONTENT of a catalog rather than about its YAML.
    ///
    /// **What was untested here is not the refusal, it is the wiring to it.** Every refusal
    /// underneath these three variants - `InvalidDescription`, `InvalidNoteBody` and all of
    /// `InconsistentKnowledge` - is provoked in `sutura_domain` over a value built by hand, and a
    /// hand-built value is not evidence that a directory on disk reaches the parse. The parse is on
    /// this side of the boundary: `absorb_definition` and `absorb_note` each call it on
    /// `Split::body()` before touching the frontmatter, and `assemble` calls
    /// `Knowledge::assemble` after both halves are read. Nothing in the domain's suite would notice
    /// if one of those calls went away.
    ///
    /// **Every test here carries the twin, and the twin is what makes it a test.** "This document is
    /// refused" passes for a document refused two guards earlier over something else, so each case
    /// below loads the same document with the one field corrected. Measured, not assumed: deleting
    /// the `Description::parse` call and passing `Description::default()` instead leaves the
    /// twin green and turns the refusal red, which is the failure this shape is for.
    mod content {
        use super::{outcome_for, outcome_of};
        use crate::LocalCatalogError;
        use sutura_domain::catalog::InvalidDescription;
        use sutura_domain::knowledge::{InconsistentKnowledge, InvalidNoteBody};

        /// A model document, which is the smallest DEFINITION document that loads on its own.
        ///
        /// A metric would need its model beside it or `Definitions::assemble` fails first, for a
        /// reason these tests are not about - and a model's prose goes through the same
        /// `absorb_definition` a metric's does, which is the call under test.
        const MODEL: &str =
            "---\nkind: model\nname: orders\nsource: local\ntable: fct_order\ncolumns: [amount_cents, order_date]\n---\n";

        /// A metric over that model. Two documents together are the smallest catalog a note can be
        /// about.
        const METRIC: &str = "---\nkind: metric\nname: revenue\nmodel: orders\nmeasure:\n  simple: { aggregate: sum, column: amount_cents }\ntime_column: order_date\ngrains: [month]\n---\nNet revenue, in minor units.\n";

        /// A knowledge document with no referent of its own, so it loads beside no definitions.
        const NOT_DEFINED: &str = "---\nkind: not_defined\nphrase: revenue forecast\n---\n";

        /// The file a failure has to name, since a catalog is many of them.
        fn names_the_document(err: &LocalCatalogError) -> bool {
            err.to_string().contains("doc.md")
        }

        #[test]
        fn the_prose_of_a_definition_document_is_parsed_as_a_description() {
            // The twin first, so what follows is a statement about the prose and not about the
            // frontmatter above it.
            drop(outcome_for("prose-model-ok", &format!("{MODEL}Net revenue, in minor units.\n")).expect("this model loads"));

            // The reachable case, and the reason this is not a theoretical one: a CRLF working tree
            // gives every line of every description a trailing `\r`. `frontmatter::split` strips one
            // at the fence lines alone and `cargo xtask line-endings` sees tracked files only, so a
            // catalog directory an operator mounted from a Windows editor arrives here like this -
            // and `sutura_app::prompt::quote` would DROP it, which is the alteration at render the
            // description type exists to forbid.
            let crlf = outcome_for("prose-model-crlf", &format!("{MODEL}Net revenue.\r\nIn minor units.\n"))
                .expect_err("a carriage return in the prose is not a description");
            assert!(
                matches!(
                    crlf,
                    LocalCatalogError::Description {
                        cause: InvalidDescription::ControlCharacter { code: 0x0D },
                        ..
                    }
                ),
                "{crlf:?}"
            );
            assert!(names_the_document(&crlf), "{crlf}");

            // And the other half of the same rule, at the other set: a code point the renderer KEEPS
            // and a reader cannot see. This is the one the type was added for.
            let invisible = outcome_for(
                "prose-model-invisible",
                &format!("{MODEL}Revenue where status = 'act\u{202E}ive'.\n"),
            )
            .expect_err("an invisible code point in the prose is not a description");
            assert!(
                matches!(
                    invisible,
                    LocalCatalogError::Description {
                        cause: InvalidDescription::InvisibleCharacter { code: 0x202E },
                        ..
                    }
                ),
                "{invisible:?}"
            );
            assert!(names_the_document(&invisible), "{invisible}");
        }

        #[test]
        fn the_prose_of_a_knowledge_document_is_parsed_as_a_note_body() {
            // The twin: the same frontmatter, with prose under it.
            drop(outcome_for("body-ok", &format!("{NOT_DEFINED}Nothing here forecasts anything.\n")).expect("this note loads"));

            // A document with no prose at all. The frontmatter is complete, so the only thing wrong
            // with it is that the note says nothing - which would render as a heading over blank
            // space in the agent-facing prompt.
            let empty = outcome_for("body-empty", NOT_DEFINED).expect_err("a note with no prose is not a note");
            assert!(
                matches!(
                    empty,
                    LocalCatalogError::NoteBody {
                        cause: InvalidNoteBody::Empty,
                        ..
                    }
                ),
                "{empty:?}"
            );
            assert!(names_the_document(&empty), "{empty}");

            // And a body that is not empty in bytes and is empty on the page, which is the case the
            // emptiness check is decided on what a reader will see for. Same variant, reached from
            // the other direction.
            let blank = outcome_for("body-blank", &format!("{NOT_DEFINED}\u{200B}\u{FEFF}\u{2060}\n"))
                .expect_err("a note that draws nothing is not a note");
            assert!(
                matches!(
                    blank,
                    LocalCatalogError::NoteBody {
                        cause: InvalidNoteBody::Empty,
                        ..
                    }
                ),
                "{blank:?}"
            );

            // One mixed into a sentence is the other refusal, and the order between the two is
            // asserted in the domain. Here it is that the adapter carries whichever fired.
            let hidden = outcome_for(
                "body-invisible",
                &format!("{NOT_DEFINED}Nothing here for\u{200B}ecasts anything.\n"),
            )
            .expect_err("an invisible code point in a note is not a note");
            assert!(
                matches!(
                    hidden,
                    LocalCatalogError::NoteBody {
                        cause: InvalidNoteBody::InvisibleCharacter { code: 0x200B },
                        ..
                    }
                ),
                "{hidden:?}"
            );
        }

        #[test]
        fn a_note_that_does_not_hold_together_with_the_definitions_fails_the_load() {
            // The one variant here whose check is over the bundle rather than over a file, which is
            // why this case needs a catalog rather than a document: `Knowledge::assemble` runs after
            // both halves are read, and the twin has to be a caveat about a metric that exists.
            let scoped = "---\nkind: caveat\nname: revenue_is_in_minor_units\nabout:\n  - { metric: revenue }\n---\nEvery revenue figure here is in minor units.\n";
            drop(
                outcome_of(
                    "knowledge-ok",
                    &[("model.md", MODEL), ("metric.md", METRIC), ("caveat.md", scoped)],
                )
                .expect("a caveat about a metric that exists loads"),
            );

            // The same document with its scope emptied. An unscoped caveat is the shape that would
            // make the catalog an arbitrary text channel into the prompt's preamble, so it is refused
            // - and this asserts that the refusal survives a real directory rather than only a
            // hand-built bundle.
            let unscoped = "---\nkind: caveat\nname: revenue_is_in_minor_units\nabout: []\n---\nEvery revenue figure here is in minor units.\n";
            let err = outcome_of(
                "knowledge-unscoped",
                &[("model.md", MODEL), ("metric.md", METRIC), ("caveat.md", unscoped)],
            )
            .expect_err("a caveat about nothing is not a caveat");
            assert!(
                matches!(
                    err,
                    LocalCatalogError::UncheckableKnowledge {
                        cause: InconsistentKnowledge::CaveatAboutNothing { .. }
                    }
                ),
                "{err:?}"
            );
            // The cause carries no path - that limit is on the variant, and it is stated there - so
            // what a reader gets instead is the note's own name, which is unique across the catalog.
            assert!(
                core::error::Error::source(&err).is_some_and(|cause| cause.to_string().contains("revenue_is_in_minor_units")),
                "{err:?}"
            );
        }
    }
}
