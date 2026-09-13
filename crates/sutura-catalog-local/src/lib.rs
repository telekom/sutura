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
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

use crate::document::knowledge::{CaveatDoc, ExampleDoc, GlossaryDoc, NotDefinedDoc};
use crate::document::{DocumentKind, InvalidMetricDocument, KindProbe, MetricDoc, ModelDoc, RelationshipDoc};
use crate::frontmatter::{MalformedDocument, Split};

/// The extension a catalog document has to have.
const DOCUMENT_EXTENSION: &str = "md";

/// The most documents a local catalog root may hold.
///
/// A startup bound, not a request-time one: this directory is operator-controlled content read once
/// at boot, not a value a caller supplies per question. The walk in [`LocalCatalog::documents`]
/// refuses as soon as it finds one document past this count, rather than finishing the tree and
/// refusing afterward - so a directory built to be large does not get walked to the end before the
/// refusal fires. 1,000 is a round number well above the largest corpus this format has been
/// exercised against (the widest example under `examples/` is 50 documents); a real deployment with
/// more documents than this is the case to raise the constant for, not to work around.
const MAX_CATALOG_DOCUMENTS: usize = 1_000;

/// The most bytes a local catalog's documents may sum to.
///
/// Checked from each file's own metadata in [`LocalCatalog::read_all`], before that file is read to
/// a `String` - so the file that would cross the bound is never read into memory. 16 MiB is a round
/// number, and a generous one: every document here is markdown with a small, capped body
/// ([`sutura_domain::knowledge::MAX_NOTE_BODY_BYTES`], [`sutura_domain::catalog::MAX_DESCRIPTION_BYTES`],
/// both 4 KiB), so `MAX_CATALOG_DOCUMENTS` bodies alone could not exceed roughly 4 MiB even at the
/// document cap - this bounds the AGGREGATE across a directory of many small documents, not any one
/// of them, which already has its own cap.
const MAX_CATALOG_BYTES: u64 = 16 * 1024 * 1024;

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
    /// The walk found more documents than `MAX_CATALOG_DOCUMENTS` permits.
    ///
    /// A startup bound: `path` is the catalog root, `found` is how many document-shaped entries the
    /// walk had counted when it stopped - which may be less than the directory's true total, because
    /// the walk refuses as soon as it crosses `limit` rather than finishing the tree first.
    #[error("the catalog at {path} holds more than {limit} documents ({found} found before the walk stopped)")]
    TooManyDocuments { path: PathBuf, found: usize, limit: usize },
    /// The documents read so far sum to more bytes than `MAX_CATALOG_BYTES` permits.
    ///
    /// `found` is the running total, checked from each file's own metadata and INCLUDING the file
    /// that crossed `limit` - which is refused before that file is read into memory, not after.
    #[error("the catalog at {path} holds more than {limit} bytes of documents ({found} found before the read stopped)")]
    TooLarge { path: PathBuf, found: u64, limit: u64 },
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
///
/// Carries a declared NAME, the way a `sources:` entry or a `catalogs:` entry carries an alias: it
/// is the key the contribution manifest records this contributor under. `sutura-serve` hands it the
/// configured `catalogs:.<key>`; `sutura-cli` names its single directory a constant. The adapter
/// can no more guess it than a data adapter can guess its source alias.
#[derive(Debug, Clone)]
pub struct LocalCatalog {
    name: SourceName,
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
    pub const fn new(name: SourceName, root: PathBuf, version: DefinitionVersion) -> Self {
        Self { name, root, version }
    }

    /// The declared name this contributor is recorded under in a bundle's contribution manifest.
    #[inline]
    pub const fn name(&self) -> &SourceName {
        &self.name
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
                    // Checked on every insert, not once after the walk finishes: a directory built
                    // to be large is refused as soon as it is large enough, rather than walked to
                    // its end first. `pending`'s remaining entries are dropped with the early
                    // return, so a directory with more documents past this one is never listed.
                    if found.len() > MAX_CATALOG_DOCUMENTS {
                        return Err(LocalCatalogError::TooManyDocuments {
                            path: self.root.clone(),
                            found: found.len(),
                            limit: MAX_CATALOG_DOCUMENTS,
                        });
                    }
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
        let mut total_bytes: u64 = 0;

        for path in self.documents()? {
            // Checked from the file's own metadata, before it is read to a `String` - so the file
            // that crosses the aggregate bound is refused rather than allocated. A `stat` is the
            // cost of this check; reading the file whole to measure it first would be the cost this
            // check exists to avoid.
            let size = std::fs::metadata(&path)
                .map_err(|cause| LocalCatalogError::Io {
                    path: path.clone(),
                    cause,
                })?
                .len();
            total_bytes = total_bytes.saturating_add(size);
            if total_bytes > MAX_CATALOG_BYTES {
                return Err(LocalCatalogError::TooLarge {
                    path,
                    found: total_bytes,
                    limit: MAX_CATALOG_BYTES,
                });
            }
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

    /// The reference: this format defines the model, so this adapter is held to producing the whole
    /// of it - which is what the golden adapters' agreement-with-the-oracle assertion is for.
    const KIND: CatalogKind = CatalogKind::Golden;

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
        // goes under the same digest, because a glossary decides which metric a question is about,
        // and so does the contribution manifest, because a bundle's digest has to cover which source
        // composed it. A single-source deployment carries a one-entry manifest - `docs/adr/0011`'s
        // shape - and this adapter stamps its own declared name and its own capability declaration,
        // which is the one piece of composition knowledge a single source has.
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| LocalCatalogError::Digest { cause })
    }
}

#[cfg(test)]
mod tests;
