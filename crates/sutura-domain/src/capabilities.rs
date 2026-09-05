//! What a metadata provider declares it can supply, and what it declares it cannot.
//!
//! **The absence is the point.** A `SemanticCatalog` adapter over a directory of markdown documents
//! written here can carry every field [`crate::catalog::Definitions`] has, because the format is this
//! repository's own and grows with the domain. An adapter over a fixed external schema cannot: a
//! metadata service that holds a measure as a raw expression string in a dialect nothing here renders
//! supplies structure, prose and join columns, and supplies no measure this repository will execute.
//! Those two must not look alike, and an empty collection cannot tell them apart - so the adapter
//! says, here, and the content is then just content.
//!
//! This is [`crate::knowledge::KnowledgeCapabilities`]'s argument applied to the other half of a
//! bundle, and the two halves are declared together in [`MetadataCapabilities`] because an adapter is
//! one thing. `docs/adr/0011-pluggable-by-declaration.md` decided the shape and
//! `docs/adr/0016-what-datahub-can-carry.md` is the measurement that scheduled it: the first source
//! measured against this port provides part of a model rather than all of one.
//!
//! # What this module does NOT do
//!
//! It does not refuse a load. [`crate::knowledge::Knowledge::assemble`] refuses content for an
//! undeclared knowledge capability, and nothing here refuses anything: a declaration is a property of
//! the **code** rather than of the bundle, so it is not under the definition digest and no
//! composition root reads it yet. What holds it honest is [`MetadataCapabilities::checked_against`],
//! which a conformance suite runs over a real adapter's real bundle. **That is a test rather than an
//! invariant, and it is written down that way deliberately** - the mechanism that cannot be omitted
//! is the declaration itself, which the port requires with no default.

use std::collections::BTreeSet;

use crate::catalog::Definitions;
use crate::knowledge::{Capability, Knowledge, KnowledgeCapabilities};

/// One kind of thing a catalog's *definitions* can carry.
///
/// A closed set, for the reason [`crate::knowledge::Capability`] is one: the alternative is a string,
/// and a provider that declared `"metrics "` would silently declare nothing at all.
///
/// **Nine kinds, and the test for whether one belongs here is whether a real source can be missing it
/// on its own:** a metadata service can have tables and no metrics, metrics and no definitional
/// filters, joins whose cardinality it does not vouch for, and dimensions with no reviewed value list.
///
/// **`Grains` is the exception and it is stated rather than smoothed over.**
/// [`Definitions::assemble`] refuses a metric declaring no grain as `NoGrains`, so a bundle cannot
/// hold a metric without one - which means the definition side never *observes* `Grains` absent while
/// `Metrics` is present, and the fidelity check below therefore cannot catch a wrong claim about
/// grains independently of the claim about metrics. It is still worth declaring: a source with no
/// grain vocabulary cannot produce a metric at all, and the declaration is what says *why* the
/// metrics are missing rather than leaving a reader to guess. What it is not is an independently
/// checkable claim, and describing it as one would be the overstatement this file's own rules name as
/// a defect.
///
/// **The variants are named after what a caller loses**, not after a struct field. `Cardinality` is
/// the clearest case: every [`crate::catalog::Relationship`] holds a [`crate::model::JoinType`]
/// because the type has no other shape, so what a source can fail to supply is not the field but the
/// *warrant* - and what a caller sees when the warrant is missing is that no dimension is reachable
/// through a relationship. [`MetadataCapabilities::produced`] observes exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionKind {
    /// Physical models: a table, and the column set it exposes.
    Structure,
    /// Prose about a model, a metric or a dimension.
    Descriptions,
    /// Declared joins between models: the two endpoints and their columns.
    Relationships,
    /// A join's cardinality, vouched for well enough to license a dimension reached through it.
    Cardinality,
    /// Metrics, each with a measure from the closed vocabulary.
    Metrics,
    /// Predicates that are part of what a metric MEANS.
    RequiredFilters,
    /// The time resolutions a metric may be asked at. Declarable, and not independently observable -
    /// see this enum's own doc comment for why, and do not read a green fidelity test as covering it.
    Grains,
    /// The reviewed set of values a dimension may be filtered on.
    AllowedValues,
    /// The number a metric produced when it was certified.
    Anchors,
}

// `DefinitionKind::every` is seeded with `Structure`, and this is what makes that a derived fact
// rather than a hand-written guess. ON THE DISCRIMINANT rather than on `previous()`, for the reason
// `crate::knowledge`'s equivalent guard gives in full: an assertion on `previous().is_none()` catches
// the careful author who writes `Structure => Some(Foo)` and misses the mechanical one who writes
// `Foo => None` and leaves `Structure => None`, satisfying every exhaustive match while `every()`
// still starts at `Structure` and never yields `Foo`.
//
// A fieldless enum casts without `#[repr]`, so a variant declared ahead of `Structure` shifts the
// discriminant off zero and this fails to compile however the matches are spelled.
//
// `const _` rather than a named constant: a name would be an item nothing reads, and `dead_code` is
// denied in this workspace.
const _: () = assert!(
    DefinitionKind::Structure as u8 == 0,
    "DefinitionKind::every is seeded with Structure, so no kind may be declared before it"
);

impl DefinitionKind {
    /// The next kind in declaration order, or `None` at the end.
    ///
    /// **This function exists in order not to compile.** [`Self::every`] is derived from it, so a
    /// tenth kind is a compile error here rather than a variant that quietly never appears in a
    /// list - which is the bug a hand-written `ALL` array has by construction.
    const fn next(self) -> Option<Self> {
        match self {
            Self::Structure => Some(Self::Descriptions),
            Self::Descriptions => Some(Self::Relationships),
            Self::Relationships => Some(Self::Cardinality),
            Self::Cardinality => Some(Self::Metrics),
            Self::Metrics => Some(Self::RequiredFilters),
            Self::RequiredFilters => Some(Self::Grains),
            Self::Grains => Some(Self::AllowedValues),
            Self::AllowedValues => Some(Self::Anchors),
            Self::Anchors => None,
        }
    }

    /// The previous kind in declaration order, or `None` at the start.
    ///
    /// The inverse WITNESS, not the seed guard - the seed is checked on the discriminant above this
    /// `impl` block, and the comment there says why an assertion here would not have held. What this
    /// buys is the round-trip test, which asserts [`Self::next`] and this function are inverses over
    /// the whole chain and therefore that `next` is a bijection rather than merely total.
    ///
    /// Scoped to test builds because that test is its only reader and `dead_code` is denied here.
    #[cfg(test)]
    const fn previous(self) -> Option<Self> {
        match self {
            Self::Structure => None,
            Self::Descriptions => Some(Self::Structure),
            Self::Relationships => Some(Self::Descriptions),
            Self::Cardinality => Some(Self::Relationships),
            Self::Metrics => Some(Self::Cardinality),
            Self::RequiredFilters => Some(Self::Metrics),
            Self::Grains => Some(Self::RequiredFilters),
            Self::AllowedValues => Some(Self::Grains),
            Self::Anchors => Some(Self::AllowedValues),
        }
    }

    /// Every kind there is, in declaration order.
    ///
    /// Derived from [`Self::next`] rather than listed, and seeded by the one variant the assertion
    /// above this `impl` block pins to discriminant zero.
    pub fn every() -> impl Iterator<Item = Self> {
        core::iter::successors(Some(Self::Structure), |current| current.next())
    }

    /// The word this kind answers to, in a message and in a declaration.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Structure => "structure",
            Self::Descriptions => "descriptions",
            Self::Relationships => "relationships",
            Self::Cardinality => "cardinality",
            Self::Metrics => "metrics",
            Self::RequiredFilters => "required filters",
            Self::Grains => "grains",
            Self::AllowedValues => "allowed values",
            Self::Anchors => "anchors",
        }
    }
}

impl core::fmt::Display for DefinitionKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which definition kinds one provider declares.
///
/// A `BTreeSet` rather than nine booleans, for the reason
/// [`crate::knowledge::KnowledgeCapabilities`] is one: the order is deterministic, and adding a kind
/// does not add a field to every construction site.
///
/// The constructor is infallible. Any set of kinds is a legitimate declaration - a source that
/// carries nothing but tables is a real source, and 0016 checked that a bundle of models with no
/// metrics assembles, pins and validates. What is not legitimate is a bundle that disagrees with the
/// declaration, and that is [`MetadataCapabilities::checked_against`]'s to report.
///
/// **`conditional` is 0011's *declared-and-empty* state, and it is separate from `declared` on
/// purpose.** A source maps a schema the deployment authors (datahub's deployment-defined
/// structured property is the first), so whether a KIND is produced is a property of the deployment
/// rather than of the code:
/// the adapter declares the kind, and a bundle that carries none is a faithful bundle rather than an
/// aspirational declaration. [`Self::of_may_provide`] is what such an adapter writes. Everything
/// else - the reference adapter, the goldens, an adapter over a fixed external schema - declares
/// unconditionally through [`Self::of`], which is why the serialized form below carries only the
/// declared set and why no existing digest moves.
///
/// **The conditional marking is a property of the CODE, not of the serialized declaration.** It is
/// deliberately absent from the `Serialize`/`Deserialize` below, which emit and read the declared
/// set exactly as the previous newtype did - the contribution manifest's digest therefore records
/// which kinds a source declared (so widening any declaration moves the digest) and not whether a
/// kind was conditional (a property `sutura-app`'s assembler and the conformance suite read off the
/// adapter's own `capabilities()`, never off a wire). A value that round-trips through serde loses
/// the marking and reads as unconditionally declared, which is the stricter direction and the honest
/// one: nothing in this repository deserializes a live declaration to serve with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionCapabilities {
    declared: BTreeSet<DefinitionKind>,
    /// Kinds in `declared` that a bundle may lawfully carry nothing of.
    ///
    /// `docs/adr/0011`'s *declared-and-empty*: absent from a bundle is a faithful bundle, and
    /// present is still covered by the declared half. Not serialized - see the type's own doc, which
    /// says why it traveling with the value but not the wire is the point.
    conditional: BTreeSet<DefinitionKind>,
}

impl serde::Serialize for DefinitionCapabilities {
    /// The declared set alone, preserving the shape the newtype had so a manifest digest is unmoved
    /// for any adapter that declares unconditionally.
    ///
    /// The `expect` is the price of a hand-written impl in a workspace with the whole `restriction`
    /// menu on: the signature `S: serde::Serializer` is forced by the trait and cannot be spelled
    /// inline.
    #[expect(
        clippy::inline_trait_bounds,
        reason = "the generic bound is part of the serde trait's own signature and cannot be inlined"
    )]
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.declared.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for DefinitionCapabilities {
    /// Read as unconditionally declared: a code property does not survive the wire, and the stricter
    /// reading is the honest one. Symmetric with `Serialize` over the vocabulary both emit.
    #[expect(
        clippy::inline_trait_bounds,
        reason = "the generic bound is part of the serde trait's own signature and cannot be inlined"
    )]
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            declared: BTreeSet::deserialize(deserializer)?,
            conditional: BTreeSet::new(),
        })
    }
}

impl DefinitionCapabilities {
    /// The kinds a provider says it supplies, unconditionally.
    ///
    /// **What an adapter over a fixed external schema writes**, so that a tenth kind added here
    /// leaves its declaration alone rather than silently widening it.
    pub fn of(kinds: impl IntoIterator<Item = DefinitionKind>) -> Self {
        Self {
            declared: kinds.into_iter().collect(),
            conditional: BTreeSet::new(),
        }
    }

    /// The kinds a provider may supply, where the deployment decides which a bundle carries.
    ///
    /// **What a source whose content is deployment-authored writes** - datahub's
    /// deployment-defined structured property - where the adapter can carry a kind but every given
    /// bundle may carry none of it.
    /// The kinds are declared (the *not declared* direction still catches content), and absent from
    /// a bundle is a faithful bundle rather than an aspirational declaration.
    pub fn of_may_provide(kinds: impl IntoIterator<Item = DefinitionKind>) -> Self {
        let kinds: BTreeSet<DefinitionKind> = kinds.into_iter().collect();
        Self {
            conditional: kinds.clone(),
            declared: kinds,
        }
    }

    /// Adds kinds a bundle may lawfully omit to this declaration, leaving the rest unchanged.
    ///
    /// What a source writes whose kinds split by whether the deployment authors them: structure,
    /// prose and joins are per-instance unconditional, while the deployment-authored content is
    /// declared-and-empty until a bundle carries any of it.
    #[must_use]
    pub fn and_may_provide(mut self, kinds: impl IntoIterator<Item = DefinitionKind>) -> Self {
        for kind in kinds {
            self.declared.insert(kind);
            self.conditional.insert(kind);
        }
        self
    }

    /// A provider with none of them.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            declared: BTreeSet::new(),
            conditional: BTreeSet::new(),
        }
    }

    /// Every kind there is.
    ///
    /// **What a REFERENCE adapter declares, and it means more than "all nine today".** A provider
    /// calling this says it supplies whatever kinds exist, including ones added later - which is true
    /// of a catalog format defined in this repository and is not true of anything mapping a schema
    /// somebody else owns.
    #[must_use]
    pub fn all() -> Self {
        Self::of(DefinitionKind::every())
    }

    /// Does this provider supply that kind at all?
    #[inline]
    pub fn declares(&self, kind: DefinitionKind) -> bool {
        self.declared.contains(&kind)
    }

    /// Everything declared, in a deterministic order.
    #[inline]
    pub const fn declared(&self) -> &BTreeSet<DefinitionKind> {
        &self.declared
    }

    /// Is a declared kind one a bundle may lawfully omit?
    #[inline]
    pub fn is_conditional(&self, kind: DefinitionKind) -> bool {
        self.conditional.contains(&kind)
    }

    /// Is nothing at all declared?
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.declared.is_empty()
    }
}

/// One kind of content, either half of a bundle.
///
/// Exists so [`MetadataCapabilities::checked_against`] is one function with one failure type rather
/// than two that a caller has to remember to run both of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclarableKind {
    /// Something the definitions carry.
    Definition(DefinitionKind),
    /// Something the knowledge carries.
    Knowledge(Capability),
}

impl core::fmt::Display for DeclarableKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Definition(kind) => write!(f, "{kind}"),
            Self::Knowledge(capability) => write!(f, "{capability}"),
        }
    }
}

/// Why a bundle does not match what the adapter that produced it declared.
///
/// **Two variants because they are two different defects with two different readers.** A bundle
/// carrying something undeclared means a caller was told an absence that is not one, and whoever
/// reads the declaration to decide what to trust was misled. A declaration claiming something the
/// bundle does not carry means the declaration is aspirational, and the next person to widen it will
/// not know which half of it was ever true.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnfaithfulDeclaration {
    /// The bundle carries content of a kind the adapter did not declare.
    #[error("the bundle carries {kind}, and this adapter does not declare it")]
    Undeclared { kind: DeclarableKind },
    /// The adapter declared a kind and the bundle carries nothing of it.
    #[error("this adapter declares {kind}, and the bundle carries none")]
    Unprovided { kind: DeclarableKind },
}

/// What one `SemanticCatalog` adapter declares it can supply.
///
/// Both halves of a bundle in one value, because an adapter is one thing and a caller deciding what
/// to trust reads one declaration. The knowledge half is
/// [`crate::knowledge::KnowledgeCapabilities`] verbatim rather than a second vocabulary over the same
/// four kinds: there is already a closed set for those, it is already rendered into the agent-facing
/// prompt, and a copy of it would be a second thing to keep in step.
///
/// **This value duplicates nothing and derives nothing.** The knowledge capabilities a *bundle*
/// carries ([`crate::knowledge::Knowledge::declares`]) are under the definition digest and travel
/// with the answer; the declaration here is a property of the linked code. That the two agree is
/// exactly what [`Self::checked_against`] checks, and it is a check rather than a derivation because
/// a derivation could not fail.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MetadataCapabilities {
    definitions: DefinitionCapabilities,
    knowledge: KnowledgeCapabilities,
}

impl MetadataCapabilities {
    /// The declaration one adapter makes.
    #[must_use]
    pub const fn of(definitions: DefinitionCapabilities, knowledge: KnowledgeCapabilities) -> Self {
        Self { definitions, knowledge }
    }

    /// Everything there is, in both halves.
    ///
    /// **What a REFERENCE adapter declares.** `sutura-catalog-local` already argued this for its
    /// knowledge half and the argument generalises unchanged: a catalog format defined in this
    /// repository supplies whatever kinds the domain grows, so a tenth definition kind or a fifth
    /// knowledge capability needs no edit at that adapter. Anything mapping a schema somebody else
    /// owns writes [`Self::of`] with two explicit lists instead.
    #[must_use]
    pub fn everything() -> Self {
        Self::of(DefinitionCapabilities::all(), KnowledgeCapabilities::all())
    }

    /// A provider that declares nothing.
    ///
    /// Legitimate, and not a synonym for a broken adapter: a source that supplies nothing this port
    /// models is one whose bundle is empty, and the pair still has to agree. What it is NOT is a
    /// default - [`crate::pinned::SemanticCatalog::capabilities`] has none, so nothing reaches this
    /// by omission.
    #[must_use]
    pub const fn nothing() -> Self {
        Self::of(DefinitionCapabilities::none(), KnowledgeCapabilities::none())
    }

    /// The definition half.
    #[inline]
    pub const fn definitions(&self) -> &DefinitionCapabilities {
        &self.definitions
    }

    /// The knowledge half.
    #[inline]
    pub const fn knowledge(&self) -> &KnowledgeCapabilities {
        &self.knowledge
    }

    /// Does this declaration cover that kind?
    #[inline]
    pub fn declares(&self, kind: DeclarableKind) -> bool {
        match kind {
            DeclarableKind::Definition(definition) => self.definitions.declares(definition),
            DeclarableKind::Knowledge(capability) => self.knowledge.declares(capability),
        }
    }

    /// Every kind either half could declare, in a deterministic order.
    ///
    /// Derived from the two vocabularies' own walks rather than listed, so a kind added to either one
    /// arrives here without an edit. Definitions first, then knowledge, and only because a reader has
    /// to be told some order - nothing depends on which.
    pub fn every_kind() -> impl Iterator<Item = DeclarableKind> {
        DefinitionKind::every()
            .map(DeclarableKind::Definition)
            .chain(Capability::every().map(DeclarableKind::Knowledge))
    }

    /// What a bundle actually carries, read off the content.
    ///
    /// **Not what the bundle says it carries.** The knowledge half here is observed from the four
    /// collections and never from [`crate::knowledge::Knowledge::declares`], which is what lets
    /// [`Self::checked_against`] catch a bundle whose own declaration and content disagree rather
    /// than comparing one claim against a copy of itself.
    ///
    /// Two of the nine definition kinds are read through their consequence rather than their field,
    /// and both are worth stating because a reader will otherwise look for the field:
    ///
    /// - **`Cardinality`** is observed as *some dimension is reached through a relationship*. Every
    ///   [`crate::catalog::Relationship`] holds a [`crate::model::JoinType`] because the type has no
    ///   other shape, so the presence of the field says nothing; what a source failing to vouch for
    ///   cardinality costs a caller is that no relationship licenses a join, and
    ///   [`Definitions::assemble`] is what turns an unvouched-for declaration into that.
    /// - **`Descriptions`** is observed as *some description is non-empty*, over models, metrics and
    ///   dimensions alike. A bundle of empty descriptions is a bundle with no prose in it, whatever
    ///   the fields are.
    ///
    /// The other seven are the presence of the thing itself.
    #[must_use]
    pub fn produced(definitions: &Definitions, knowledge: &Knowledge) -> Self {
        Self::of(
            DefinitionCapabilities::of(DefinitionKind::every().filter(|kind| carried(definitions, *kind))),
            KnowledgeCapabilities::of(Capability::every().filter(|capability| recorded(knowledge, *capability))),
        )
    }

    /// Declaration fidelity: is this declaration exactly what `produced` was produced?
    ///
    /// The assertion a **declaring** adapter gets in place of the golden adapters' oracle, and it is
    /// two claims rather than one: everything declared was produced, so a declaration is not
    /// aspirational; and nothing of an undeclared kind appears, so a declared absence is visibly
    /// absent rather than silently missing.
    ///
    /// **One exemption, and it is the point of[`DefinitionCapabilities::of_may_provide`].** A kind
    /// an adapter marks conditional - declared, yet absent from a bundle is lawful because whether a
    /// bundle carries it is the deployment's decision - does not fail the *unprovided* direction. The
    /// *undeclared* direction is unaffected, so a bundle carrying a conditional kind is still
    /// checked against the declared half of it. A declaration that lost its marking (one that
    /// round-tripped through the wire) reads as unconditional, which fails on an absent kind - the
    /// stricter and therefore safe direction.
    ///
    /// **The undeclared direction is checked first, and the order is not cosmetic.** That one is the
    /// safety failure - a caller was told an absence that is not one - and reporting it first means a
    /// suite that stops at the first error stops on the worse of the two. Named in the error either
    /// way, so a reader is never left to infer which happened.
    ///
    /// One kind per call, deliberately. A `Vec` of every discrepancy would be a presentation of an
    /// error rather than an error, and the workspace's rule is that the variant is the contract.
    ///
    /// # Errors
    ///
    /// [`UnfaithfulDeclaration`], naming the first kind the two disagree about.
    pub fn checked_against(&self, produced: &Self) -> Result<(), UnfaithfulDeclaration> {
        for kind in Self::every_kind() {
            if produced.declares(kind) && !self.declares(kind) {
                return Err(UnfaithfulDeclaration::Undeclared { kind });
            }
        }
        for kind in Self::every_kind() {
            if self.declares(kind) && !produced.declares(kind) && !Self::absent_is_conditional(&self.definitions, kind) {
                return Err(UnfaithfulDeclaration::Unprovided { kind });
            }
        }
        Ok(())
    }

    /// Is an absent kind a lawful absence under this declaration's conditional marking?
    ///
    /// Knowledge kinds are never conditional - [`crate::knowledge::KnowledgeCapabilities`] carries
    /// no may-provide state, so a knowledge declaration is unconditional.
    fn absent_is_conditional(definitions: &DefinitionCapabilities, kind: DeclarableKind) -> bool {
        match kind {
            DeclarableKind::Definition(definition) => definitions.is_conditional(definition),
            DeclarableKind::Knowledge(_) => false,
        }
    }
}

/// Does `definitions` carry anything of that kind?
///
/// One exhaustive match, so a tenth [`DefinitionKind`] is a compile error here rather than a kind
/// nothing observes - which would make [`MetadataCapabilities::checked_against`] silently blind to
/// it in both directions.
fn carried(definitions: &Definitions, kind: DefinitionKind) -> bool {
    match kind {
        DefinitionKind::Structure => !definitions.models().is_empty(),
        DefinitionKind::Descriptions => describes_anything(definitions),
        DefinitionKind::Relationships => !definitions.relationships().is_empty(),
        DefinitionKind::Cardinality => dimensions(definitions).any(|dimension| dimension.via().is_some()),
        DefinitionKind::Metrics => !definitions.metrics().is_empty(),
        DefinitionKind::RequiredFilters => definitions
            .metrics()
            .values()
            .any(|metric| !metric.required_filters().is_empty()),
        // Written as a read of the field rather than as `!metrics().is_empty()`, even though
        // `assemble`'s `NoGrains` refusal makes the two equal today: if that refusal is ever relaxed,
        // this arm is already right, and the equality is a fact about another function rather than
        // about this one.
        DefinitionKind::Grains => definitions.metrics().values().any(|metric| !metric.grains().is_empty()),
        DefinitionKind::AllowedValues => dimensions(definitions).any(|dimension| dimension.allowed_values().is_some()),
        DefinitionKind::Anchors => definitions.metrics().values().any(|metric| metric.anchor().is_some()),
    }
}

/// Every dimension of every metric.
fn dimensions(definitions: &Definitions) -> impl Iterator<Item = &crate::catalog::Dimension> {
    definitions.metrics().values().flat_map(|metric| metric.dimensions().values())
}

/// Is there prose anywhere in these definitions?
///
/// Models, metrics and dimensions, because a source can carry prose about one and not another and
/// this kind is one declaration covering all three. Splitting it into three kinds would be three
/// declarations nobody can act on differently.
fn describes_anything(definitions: &Definitions) -> bool {
    let models = definitions.models().values().any(|model| !model.description().is_empty());
    let metrics = definitions.metrics().values().any(|metric| !metric.description().is_empty());
    models || metrics || dimensions(definitions).any(|dimension| !dimension.description().is_empty())
}

/// Does `knowledge` record anything of that capability?
///
/// The fourth exhaustive match over [`Capability`] in this workspace, and it is one on purpose: a
/// fifth knowledge kind has to be assigned an observation here or this stops compiling.
fn recorded(knowledge: &Knowledge, capability: Capability) -> bool {
    match capability {
        Capability::Glossary => !knowledge.glossary().is_empty(),
        Capability::Caveats => !knowledge.caveats().is_empty(),
        Capability::Absences => !knowledge.absences().is_empty(),
        Capability::Examples => !knowledge.examples().is_empty(),
    }
}

#[cfg(test)]
mod tests;
