//! The credential broker a deployment gets when its credentials are configuration.
//!
//! **The first implementor of `sutura_domain::identity::CredentialBroker`, and it is a shipping
//! deployment mode rather than test scaffolding.** `AGENTS.md`'s rule is that a port trait arrives
//! with its first implementor, because a trait with no implementor is a guess at a signature - and
//! `examples/multi-player/README.md` says the same thing about this port by name. What arrives with
//! it here is the static-credential broker single-user mode already needs: credentials as
//! configuration, one user, one host.
//!
//! # Why this crate
//!
//! Because the identity provider this broker talks to *is* the settings tree. It opens no socket,
//! reads no clock and holds no secret of its own: what it hands back for a shared source is the
//! operator's own acknowledgement witness, which this crate is the only place that can parse. A
//! broker that exchanges a token belongs in a crate that can make a network call, and that crate does
//! not exist yet - `docs/adr/0014` Decision 3 says why, and `sutura_domain::identity`'s own header
//! records what the port cannot express until it does.
//!
//! # What it refuses, and why that is the interesting half
//!
//! **A source declared `impersonation-at-source` gets no credential from this broker**, so a question
//! against one is refused as `CredentialUnavailable` rather than answered under the deployment's own
//! identity. That is the whole point of the port: the fallback is not forbidden by a rule, it is
//! absent from every signature, and a broker with nothing to present has to say so.
//!
//! It is also what makes the refusal provokable **without a network and without a fake** - by the
//! real implementor, from a real configuration - which is what `docs/adr/0008` part 6 asks of the one
//! refusal variant this design adds.

use std::collections::BTreeMap;

use sutura_domain::identity::{CredentialBroker, CredentialsDoNotCoverThePlan};
use sutura_domain::identity::{Expiry, LegCredentials, Minted, Presented, RequestContext, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::source::{SharedIdentityDeclared, SourcePosture};

use crate::sources::SourceRegistry;

/// A defect in this broker itself, which no configuration reaches.
///
/// **Stated rather than unwrapped, and the reason is worth a line.** The one thing minting can fail
/// on is `LegCredentials::minted` refusing a set that does not cover the sources it was asked about -
/// and this broker builds its map from that same set, one entry per source it was asked about, so the
/// check cannot fire here. It is still answered for rather than unwrapped: `unwrap_used` is denied,
/// and a panic on this path would be process death under `panic = "abort"` for a case a type already
/// describes. Nothing in this crate's suite can provoke it, and that is said here rather than left
/// for a reader to assume it is covered.
#[derive(Debug, thiserror::Error)]
pub enum StaticCredentialsUnusable {
    /// The credentials built here did not cover the sources they were asked about.
    #[error("the statically configured credentials did not cover the sources they were minted for")]
    Coverage {
        #[source]
        cause: CredentialsDoNotCoverThePlan,
    },
}

/// Mints from what an operator declared, and nothing else.
///
/// Holds one entry per source declared `shared-service-user`, carrying that source's acknowledgement
/// witness. A source declared `impersonation-at-source` is deliberately **absent**: there is no
/// static credential that could execute as an asking subject, and an entry that pretended otherwise
/// would be the fallback this port exists to remove.
///
/// **Built from the DECLARATION and never from an adapter.** `docs/adr/0008` part 4 requires that a
/// broker produce the shared shape only for a source configured shared, and the check that catches a
/// broker which did not is the adapter's own exhaustive match on what it received. Those two are only
/// an independent pair if they read different things: this reads the settings tree, and the adapter
/// holds what the composition root handed it. A broker that read the posture off the adapter would
/// make that check compare a value against itself.
#[derive(Debug, Clone)]
pub struct StaticCredentialBroker {
    shared: BTreeMap<SourceName, SharedIdentityDeclared>,
}

impl StaticCredentialBroker {
    /// Reads the declared sources.
    ///
    /// Infallible: a registry that parsed is a registry whose postures are declared, and a source
    /// this broker cannot mint for is a refusal at request time rather than a startup failure. The
    /// startup failures that DO belong to identity are already elsewhere and are not duplicated here -
    /// `Settings::refusals` refuses an unacknowledged shared source in multi-user mode, and the
    /// composition root refuses a posture the linked adapter cannot deliver.
    #[must_use]
    pub fn from_registry(registry: &SourceRegistry) -> Self {
        let mut shared = BTreeMap::new();
        for (alias, source) in registry.each() {
            if let Some(SourcePosture::SharedServiceUser { declared }) = source.posture() {
                drop(shared.insert(alias.clone(), declared.clone()));
            }
        }
        Self { shared }
    }

    /// One shared source, declared in code rather than in a file.
    ///
    /// **For a composition root that has no settings tree**, which is the `sutura` command: it answers
    /// one question and exits, reading the files of whoever ran it as that person's own
    /// operating-system identity, and there is no `sources:` entry for an operator to write. The
    /// acknowledgement is still an [`SharedIdentityDeclared`] that went through
    /// `AcknowledgementReason::parse`, so what the leg carries is bounded and checked by the same code
    /// a configuration file's is - the difference is who wrote the sentence, not whether one exists.
    ///
    /// It is here rather than as a second broker in that binary because one implementor of the port is
    /// what keeps its contract in one place: a second one would be a second thing to keep in step with
    /// what an adapter accepts.
    #[must_use]
    pub fn for_one_shared_source(source: SourceName, declared: SharedIdentityDeclared) -> Self {
        Self {
            shared: BTreeMap::from([(source, declared)]),
        }
    }

    /// How many sources this broker can mint for.
    ///
    /// Read by this crate's own suite, which is where the interesting assertion is: a source declared
    /// `impersonation-at-source` is ABSENT rather than mapped to a default, so the count is what shows
    /// that a broker holding nothing for a source is a broker that refuses it. Nothing in a composition
    /// root prints it today.
    #[must_use]
    pub fn count(&self) -> usize {
        self.shared.len()
    }
}

impl CredentialBroker for StaticCredentialBroker {
    type Error = StaticCredentialsUnusable;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let mut presented = BTreeMap::new();
        for source in sources.iter() {
            // A source with no static credential is a refusal and not an error, and not a leg that
            // runs as this process either. `docs/adr/0008` part 6: asking differently does not help,
            // so the caller is told which source rather than told to retry.
            let Some(declared) = self.shared.get(source) else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            drop(presented.insert(
                source.clone(),
                Presented::SharedServiceUser {
                    declared: declared.clone(),
                },
            ));
        }
        // Nothing here expires: a credential an operator wrote in a file has no lifetime, and
        // `Expiry` says that as a case rather than as a sentinel instant. A broker that exchanges a
        // token computes the earliest across what it minted instead.
        LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|cause| StaticCredentialsUnusable::Coverage { cause })
    }
}

#[cfg(test)]
mod tests;
