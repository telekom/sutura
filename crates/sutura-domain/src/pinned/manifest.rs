//! The contribution manifest: which metadata sources composed a bundle, and what each declared.
//!
//! [`PinnedDefinitions`](crate::pinned::PinnedDefinitions)' digest is taken over a canonical form
//! of the definitions, the knowledge and this manifest, so the digest covers the **composition**
//! and not only the assembly - `docs/adr/0011`'s "two different compositions that assemble
//! identically are indistinguishable" is the gap this closes. Decision and serialized form:
//! `docs/adr/0011`, *The contribution manifest is built, and its serialized form is decided*.
//!
//! **The manifest says what was configured and reached, not what a source returned.** Each entry is
//! the source's own declared capability list, its required-or-optional declaration, and whether it
//! was reached for this bundle - no host, no credential, no URL. Fidelity between the declaration
//! and a bundle's content is `MetadataCapabilities::checked_against`, which `sutura_app`'s
//! metadata assembler runs per contributor.
//!
//! Split out of `pinned.rs` for `cargo xtask max-lines`, the catalog precedent applied to the
//! pinned-bundle half of the module.

use std::collections::BTreeMap;

use crate::capabilities::MetadataCapabilities;
use crate::model::SourceName;

/// Whether a contributor is required for this deployment to serve.
///
/// **One variant, and that is the whole of what this code can say.** It carried an `Optional` half
/// and a `Contribution::missing` constructor to produce it, and nothing produced one: no settings
/// shape declares an optional source, so a bundle exists only when every configured source loaded.
/// Both are deleted rather than kept against a declaration that does not exist -
/// `github.com/telekom/sutura#639` is where that was decided, and the reasoning is that a variant no
/// deployment can reach is a combination the type admits and the constructors do not produce.
///
/// **The FIELD stays, and the limit is worth stating exactly.** `docs/adr/0011` decided the
/// manifest's serialized form, the digest is taken over it, and dropping the key changes every
/// pinned digest - so the shape is what a deployment that declares availability fills in, and that
/// diff brings back the second variant beside its producer. What is gone is the pre-built half, not
/// the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum RequiredOrOptional {
    /// The deployment does not serve without this source.
    Required,
}

/// One metadata source's record in the manifest.
///
/// **No `Deserialize`, and that is what closes the last way in.** A derived one admitted every
/// combination of the three fields - `Required` beside `reached: false`, an availability state no
/// constructor produces - while nothing in the workspace reads a manifest back:
/// [`PinnedDefinitions`](crate::pinned::PinnedDefinitions) derives `Serialize` alone, because the
/// serialized form exists to be DIGESTED rather than to be parsed. So the derive was unused surface
/// that could construct what [`Self::of`] cannot, and it is deleted rather than routed through a
/// `try_from`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Contribution {
    capabilities: MetadataCapabilities,
    required_or_optional: RequiredOrOptional,
    reached: bool,
}

impl Contribution {
    /// The record for a source that was configured, declared these capabilities, and loaded.
    ///
    /// **The only constructor, so `reached = true` and `Required` are the only state a
    /// [`Contribution`] has.** It had a `missing` twin for the optional-and-unreachable case and
    /// nothing called it; [`RequiredOrOptional`] says why both are gone and what brings them back.
    pub const fn of(capabilities: MetadataCapabilities) -> Self {
        Self {
            capabilities,
            required_or_optional: RequiredOrOptional::Required,
            reached: true,
        }
    }

    /// What this source declared it supplies, the manifest record of
    /// [`SemanticCatalog::capabilities`](crate::pinned::SemanticCatalog::capabilities).
    #[inline]
    pub const fn capabilities(&self) -> &MetadataCapabilities {
        &self.capabilities
    }

    /// Whether this source was declared required for the deployment to serve.
    #[inline]
    pub const fn required_or_optional(&self) -> RequiredOrOptional {
        self.required_or_optional
    }

    /// Whether this source was reached for this bundle.
    #[inline]
    pub const fn reached(&self) -> bool {
        self.reached
    }
}

/// Why a set of contributions is not a manifest.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidManifest {
    /// Nothing was contributed, so the manifest would record no composition at all.
    #[error("a manifest records at least one contributor")]
    NoContributors,
    /// Two contributions name one source, so one of them would not be recorded.
    ///
    /// `source_name` and not `source`, for [`FederatedPlanError`](crate::plan::FederatedPlanError)'s
    /// reason: `thiserror` reads a field called `source` as the error's cause.
    #[error("two contributions name the source `{source_name}`, and a manifest records each one once")]
    DuplicateSource { source_name: SourceName },
}

/// Which metadata sources composed a bundle, keyed by each source's declared name.
///
/// **A `BTreeMap`, so collection order is content order** - the same determinism requirement
/// [`crate::catalog::Definitions`] and [`crate::knowledge::Knowledge`] carry, for the same reason:
/// the digest is taken over the serialized form, and an unordered map serializes in whatever order
/// its hasher chose this run.
///
/// **A single-source deployment carries a one-entry manifest** rather than none, because a shape
/// that differed between one source and N would put the interesting case on the untested path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ContributionManifest {
    entries: BTreeMap<SourceName, Contribution>,
}

impl ContributionManifest {
    /// A bundle read from exactly one source.
    ///
    /// Infallible by construction rather than by a skipped check, which is
    /// [`StatementTables::only`](crate::plan::StatementTables::only)'s argument: one entry cannot be
    /// no entries and has no second name to collide with.
    pub fn single(source: SourceName, contribution: Contribution) -> Self {
        Self {
            entries: BTreeMap::from([(source, contribution)]),
        }
    }

    /// A bundle read from several sources, in declaration order.
    ///
    /// **The canonical constructor, and it refuses two states it used to absorb.** It collected
    /// straight into the map, so a repeated source name OVERWROTE the earlier entry and an empty
    /// iterator produced an empty manifest - a bundle whose manifest records fewer contributors than
    /// composed it, which is precisely the "two different compositions that assemble identically are
    /// indistinguishable" gap `docs/adr/0011` built the manifest to close. A silent overwrite in the
    /// thing whose job is to make compositions distinguishable is worse than a refusal.
    ///
    /// **The limit, next to the claim:** neither refusal is reachable from the one caller today.
    /// `sutura_app`'s assembler already refuses an empty composition with its own `Empty`, and it
    /// composes one contributor per configured source. So this is defence in depth on a public
    /// constructor rather than a bug being fixed on a live path, and both variants are provoked by a
    /// test on this constructor rather than by a deployment.
    pub fn parse(entries: impl IntoIterator<Item = (SourceName, Contribution)>) -> Result<Self, InvalidManifest> {
        let mut map: BTreeMap<SourceName, Contribution> = BTreeMap::new();
        for (source, contribution) in entries {
            if map.insert(source.clone(), contribution).is_some() {
                return Err(InvalidManifest::DuplicateSource { source_name: source });
            }
        }
        if map.is_empty() {
            return Err(InvalidManifest::NoContributors);
        }
        Ok(Self { entries: map })
    }

    /// Every entry, keyed on each contributor's declared name.
    #[inline]
    pub const fn entries(&self) -> &BTreeMap<SourceName, Contribution> {
        &self.entries
    }

    /// One contributor's record, or `None` if the name was not configured.
    #[inline]
    pub fn get(&self, source: &SourceName) -> Option<&Contribution> {
        self.entries.get(source)
    }

    /// How many sources composed this bundle.
    #[must_use]
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}
