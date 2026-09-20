//! The broker a served `BigQuery` deployment impersonates through: a verified subject in, the
//! principal a source declared for that subject out.
//!
//! **Why this exists beside [`crate::sts`] rather than inside it.** `WorkloadIdentityBroker`
//! EXCHANGES - it hands the caller's own assertion to a token service and presents what comes back
//! as the leg's bearer. Its two HTTP hops went away with the `wire` transport, and the ADBC driver
//! cannot take a bearer anyway, so that broker has no implementor a composition root can reach. What
//! the ADBC transport needs is narrower and different in kind: not a token to exchange, but the
//! asking subject's OWN assertion for the driver to federate against the pool this source declares.
//! No socket and no cache - Google's token service performs the exchange - but an EXPIRY, because
//! what this presents is credential material and credential material ages out.
//!
//! Forking the exchanging broker for that would have put two answers to *what does this leg present*
//! inside one 800-line type. Two brokers, each with one answer, is the shape `docs/adr/0008` part 4
//! already assumes: a composition root picks one.
//!
//! # What it refuses, which is the half that matters
//!
//! - **A source it holds neither half for** - refused as `credential_unavailable`, so a forgotten
//!   attachment cannot read every row as the deployment.
//! - **A caller the source's declaration does not name** - refused, never widened to a bare
//!   deployment identity. `docs/adr/0032` states the same rule for the exchanging broker's own
//!   second hop: absent from the map is refused, not defaulted.
//! - **An anonymous caller at an impersonating source** - a request with no verified subject has no
//!   key to look up, and there is nothing to fall back to.
//!
//! And one refusal it deliberately does NOT make at request time: an impersonating source with an
//! empty declaration. [`DeclaredPrincipals::parse`] refuses that, so it is a startup failure - the
//! whole point being that a deployment which cannot execute as anybody must not boot clean and then
//! refuse every question.
//!
//! # The limit, beside the claim
//!
//! This broker decides WHETHER this caller may be served at a source and presents the caller's own
//! verified assertion for it. It does not decide WHO the caller becomes at the data system - the
//! declared pool resolves that, so the accounts named in `impersonate`'s VALUES are read by nothing
//! (see [`DeclaredPrincipals::names`]). And nothing here proves Google accepted the assertion: that
//! is leg 2, it needs a hosted run, and `docs/where-identity-is-proven.md` records the venue as
//! `wired`.

use std::collections::BTreeMap;

use sutura_domain::identity::{
    CredentialBroker, CredentialsDoNotCoverThePlan, Expiry, LegCredentials, Minted, Presented, PrincipalName, RequestContext,
    SourceSet, SubjectKey,
};
use sutura_domain::model::SourceName;
use sutura_domain::source::SharedIdentityDeclared;

/// Why a declared impersonation map is not one a source can be served under.
///
/// One variant, and an enum for the reason every other error in this crate is one: a second reason
/// has somewhere to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NoDeclaredPrincipals {
    /// The declaration named no subject at all.
    #[error(
        "a source declared `impersonation-at-source` names no subject to execute as, so no caller \
         could ever be served there - declare the subject to service-account map, or declare the \
         source `shared-service-user` with an acknowledgement"
    )]
    Empty,
}

/// The subjects one source may be asked as, and the account each of them resolves to.
///
/// **A parsed type and not a bare map, because the empty map is the interesting value.** An
/// impersonating source with no declared subject can serve nobody: every request would be refused,
/// while the boot log said the source opened. That is the exact defect this whole change exists to
/// remove, so the emptiness is refused at the boundary that can turn it into a startup failure
/// rather than documented at the one that cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredPrincipals(BTreeMap<SubjectKey, PrincipalName>);

impl DeclaredPrincipals {
    /// Parses one source's declared subject-to-principal map.
    ///
    /// **Keyed on the FULL verified subject and never on the masked
    /// [`SubjectId`](sutura_domain::identity::SubjectId)**, for the reason `sutura_config`'s own
    /// declaration gives: which declared account a caller may become is an authorization decision,
    /// and a masked key hands a declared subject's account to every undeclared caller that shares
    /// its mask.
    ///
    /// # Errors
    ///
    /// [`NoDeclaredPrincipals::Empty`] for a declaration naming nobody.
    pub fn parse(declared: BTreeMap<SubjectKey, PrincipalName>) -> Result<Self, NoDeclaredPrincipals> {
        if declared.is_empty() {
            return Err(NoDeclaredPrincipals::Empty);
        }
        Ok(Self(declared))
    }

    /// Does this source's declaration name this subject at all?
    ///
    /// `false` is not a fallback - it is the answer for every caller a deployment did not name, and
    /// [`DeclaredPrincipalBroker::mint`] turns it into a refusal.
    ///
    /// **The KEY is what is read, and the declared account beside it is NOT SENT ANYWHERE TODAY.**
    /// With workload-identity federation the pool resolves a subject to its own principal, so there
    /// is no per-subject account for this adapter to name - the value would become a
    /// `service_account_impersonation_url` on the credential document, which is a follow-up rather
    /// than something built here. Recorded as a dead declared value rather than left for a reader
    /// to discover: `sources.<alias>.workload_identity.impersonate`'s keys decide authorization and
    /// its values decide nothing.
    #[inline]
    #[must_use]
    pub fn names(&self, subject: &SubjectKey) -> bool {
        self.0.contains_key(subject)
    }

    /// How many subjects this source declares. Never zero.
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.0.len()
    }
}

/// A defect in this broker itself, which no configuration reaches.
///
/// Stated rather than unwrapped for `sutura_config::StaticCredentialsUnusable`'s reason: the one
/// thing minting can fail on is a credential set that does not cover the sources it was asked
/// about, and this broker builds its map from that same set. `unwrap_used` is denied and a panic
/// here would be process death under `panic = "abort"` for a case a type already describes.
#[derive(Debug, thiserror::Error)]
pub enum DeclaredPrincipalsUnusable {
    /// The credentials built here did not cover the sources they were asked about.
    #[error("the declared principals did not cover the sources they were minted for")]
    Coverage {
        #[source]
        cause: CredentialsDoNotCoverThePlan,
    },
}

/// Presents the principal a source declared for the asking subject, and the operator's witness for a
/// shared one.
///
/// **Both maps, because one plan may read one of each and a broker is per answer rather than per
/// source** - the same reason [`crate::WorkloadIdentityBroker`] holds two.
#[derive(Debug, Clone, Default)]
pub struct DeclaredPrincipalBroker {
    shared: BTreeMap<SourceName, SharedIdentityDeclared>,
    impersonating: BTreeMap<SourceName, DeclaredPrincipals>,
}

impl DeclaredPrincipalBroker {
    /// A broker that can serve nothing yet.
    ///
    /// Every source is added explicitly, so a source nobody declared is one this broker refuses -
    /// there is no arm that widens an unknown source into the deployment's own identity.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Declares one shared source and the operator's acknowledgement for it.
    #[must_use]
    pub fn shared(mut self, at: SourceName, declared: SharedIdentityDeclared) -> Self {
        drop(self.shared.insert(at, declared));
        self
    }

    /// Declares one impersonating source and the subjects it may be asked as.
    #[must_use]
    pub fn impersonating(mut self, at: SourceName, declared: DeclaredPrincipals) -> Self {
        drop(self.impersonating.insert(at, declared));
        self
    }

    /// How many sources this broker can mint for at all.
    ///
    /// Read by this crate's own suite, where the assertion that matters is that a source declared
    /// impersonating with no entry is ABSENT rather than mapped to something.
    #[must_use]
    pub fn count(&self) -> usize {
        self.shared.len().saturating_add(self.impersonating.len())
    }
}

impl CredentialBroker for DeclaredPrincipalBroker {
    type Error = DeclaredPrincipalsUnusable;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        // **Two passes, and the order is the property** - `sutura_config::StaticCredentialBroker`
        // carries the argument: a plan whose SECOND source is unmintable must be refused before the
        // first one's credential is built, so "refused before anything was minted" is true of what
        // happened rather than only of what escaped.
        for source in sources.iter() {
            if !self.shared.contains_key(source) && !self.impersonating.contains_key(source) {
                return Ok(Minted::Refused { source: source.clone() });
            }
        }
        // The FULL verified subject, which is the only key on which two distinct callers stay
        // distinct. `None` for a request with no verified subject at all - an impersonating source
        // then has nobody to be, and the refusal below is what it gets.
        let asked_by = context.chain().subject();
        let key = asked_by.key();
        // The document leg 1 verified. A broker that federates REQUIRES one - a subject with no
        // assertion has nothing for Google's token service to verify, and answering as the process
        // is the fallback this whole path exists to remove.
        let assertion = context.assertion();
        // **The assertion's OWN expiry, and a broker that federates has no other.** An earlier round
        // minted `NothingExpires` here, which was honest while this broker presented a principal's
        // name - a name does not age - and became a check that always answers yes the moment it
        // started presenting credential material. `BoundToTheRequest::still_usable_at` reads this.
        let assertion_expires = context.assertion_expires();
        let mut presented = BTreeMap::new();
        // One per source, folded by `Expiry::earliest` below: a shared leg runs on a credential this
        // broker did not mint and does not age with the caller, so giving a shared-only plan the
        // caller's expiry would refuse answers for a lifetime that does not apply to them.
        let mut deadlines = Vec::new();
        for source in sources.iter() {
            if let Some(declared) = self.shared.get(source) {
                drop(presented.insert(
                    source.clone(),
                    Presented::SharedServiceUser {
                        declared: declared.clone(),
                    },
                ));
                deadlines.push(Expiry::NothingExpires);
                continue;
            }
            // Unreachable: the pass above established that every source has one of the two halves.
            // Answered rather than unwrapped, because `unwrap_used` is denied.
            let Some(principals) = self.impersonating.get(source) else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            // **The authorization decision, and both ways out of it are refusals.** A caller with no
            // verified subject, and a verified caller this source does not name, are told the same
            // thing: this source cannot be asked as you. Neither is widened, and the refusal names
            // the SOURCE and never the subject - `Minted::Refused` carries one field for that reason.
            if !key.is_some_and(|key| principals.names(key)) {
                return Ok(Minted::Refused { source: source.clone() });
            }
            // **The asking subject's OWN assertion, and that is the whole of leg 2.** The transport
            // puts it behind a workload-identity credential document, so Google's token service
            // verifies it and the source executes as whatever principal the pool resolves the
            // subject to. What this broker decides is only WHETHER this caller may be served here;
            // WHO they become is the pool's, which is why nothing per-subject is minted.
            let Some(assertion) = assertion else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            // A context carrying material with no expiry is not a shape `RequestContext` can build -
            // they are one field - so this arm cannot be reached with an assertion and no bound.
            // Refused rather than defaulted anyway, because the alternative default is forever.
            let Some(expires) = assertion_expires else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            drop(presented.insert(
                source.clone(),
                Presented::SubjectToken {
                    material: assertion.clone(),
                },
            ));
            deadlines.push(expires);
        }
        // **The earliest of what was presented, which is what `Expiry::earliest` exists for.** A
        // federated leg is valid for exactly as long as the caller's own assertion is; a shared leg
        // carries no lifetime this broker knows, so it contributes the fold's identity rather than
        // a guess. A plan reading one of each is bounded by the assertion, which is the
        // conservative direction and the correct one - the federated leg is the one that stops
        // working.
        //
        // **The limit beside it**: this is not a FLOOR. `docs/adr/0008` part 6 asks for a refusal
        // when a credential would age out mid-answer; what is here is the bound
        // `BoundToTheRequest::still_usable_at` compares against, so an assertion valid at the start of
        // a long answer and expired at the end is refused by Google rather than here.
        LegCredentials::minted(asked_by.clone(), Expiry::earliest(deadlines), sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|cause| DeclaredPrincipalsUnusable::Coverage { cause })
    }
}

#[cfg(test)]
mod tests;
