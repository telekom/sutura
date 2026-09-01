//! The contribution manifest: which metadata sources composed a bundle, and what each declared.
//!
//! [`PinnedDefinitions`]' digest is taken over a canonical form of the definitions, the knowledge
//! and this manifest, so the digest covers the **composition** and not only the assembly -
//! `docs/adr/0011`'s "two different compositions that assemble identically are indistinguishable"
//! is the gap this closes. Decision and serialized form: `docs/adr/0011`, *The contribution
//! manifest is built, and its serialized form is decided*.
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

/// Whether a contributor is required for this deployment to serve, or may be absent.
///
/// **Every value this code can produce is [`Self::Required`]** - no settings shape declares an
/// optional source yet, so a bundle exists only when every configured source loaded. The variant is
/// carried because the availability rule `docs/adr/0011` decided lands on top of it: a deployment
/// that can declare `optional` is the diff that first writes [`Self::Optional`], and the digest's
/// job is to make that run look different from the one that included the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RequiredOrOptional {
    /// The deployment does not serve without this source.
    Required,
    /// The deployment may serve without it, and a bundle that did differs in digest from one that
    /// did not - nothing here can produce one yet, because no declaration says so.
    Optional,
}

/// One metadata source's record in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Contribution {
    capabilities: MetadataCapabilities,
    required_or_optional: RequiredOrOptional,
    reached: bool,
}

impl Contribution {
    /// The record for a source that was configured, declared these capabilities, and loaded.
    ///
    /// **`reached = true` is what a served bundle records.** A source an optional deployment could
    /// not reach is a manifest entry with `reached = false`, which this constructor's `of` does not
    /// produce - see [`Self::missing`].
    pub const fn of(capabilities: MetadataCapabilities) -> Self {
        Self {
            capabilities,
            required_or_optional: RequiredOrOptional::Required,
            reached: true,
        }
    }

    /// The record for a configured source this bundle serves without.
    ///
    /// The availability case `docs/adr/0011` prices: optional and unreachable at startup, recorded
    /// as such so the digest differs from a run that included it. Nothing in this repository can
    /// produce one today - no deployment declares an optional source - so it is the shape a future
    /// declaration fills, and it stops this constructor being omitted.
    pub const fn missing(capabilities: MetadataCapabilities) -> Self {
        Self {
            capabilities,
            required_or_optional: RequiredOrOptional::Optional,
            reached: false,
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

/// Which metadata sources composed a bundle, keyed by each source's declared name.
///
/// **A `BTreeMap`, so collection order is content order** - the same determinism requirement
/// [`crate::catalog::Definitions`] and [`crate::knowledge::Knowledge`] carry, for the same reason:
/// the digest is taken over the serialized form, and an unordered map serializes in whatever order
/// its hasher chose this run.
///
/// **A single-source deployment carries a one-entry manifest** rather than none, because a shape
/// that differed between one source and N would put the interesting case on the untested path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContributionManifest {
    entries: BTreeMap<SourceName, Contribution>,
}

impl ContributionManifest {
    /// A bundle read from exactly one source.
    pub fn single(source: SourceName, contribution: Contribution) -> Self {
        Self {
            entries: BTreeMap::from([(source, contribution)]),
        }
    }

    /// A bundle read from several sources, in declaration order.
    pub fn of(entries: impl IntoIterator<Item = (SourceName, Contribution)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
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
