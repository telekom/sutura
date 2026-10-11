//! The broker a served deployment with an impersonating source answers through: a verified subject
//! in, that subject's own credential (or the declared principal a switching source becomes) out.
//!
//! **A federating source is presented the asker's own verified assertion and nothing beside it**
//! ([`DeclaredPrincipalBroker::federating`]). The `BigQuery` adapter puts it behind a
//! workload-identity credential document, Google's token service exchanges it for a federated token
//! of the pool's `principal://.../subject/<sub>`, and the data system's own grants and row policies
//! on that principal decide what the caller reads (`docs/adr/0032`'s option 3). There is no
//! subject map and no account to become: a caller with no grant is
//! refused by the data system, never answered as anybody else.
//!
//! **Here, beside [`crate::StaticCredentialBroker`], and in no adapter crate**: it reads only the
//! domain's identity port, so every adapter that can deliver `impersonation-at-source` is served
//! through it without linking another adapter.
//!
//! **Why this is the only declared broker.** It used to sit beside an EXCHANGING one, whose two HTTP
//! hops went away with the `wire` transport (`docs/adr/0018`'s eighth amendment). What the ADBC
//! transport needs is narrower: not a token to exchange, but the asking subject's OWN assertion for
//! the driver to federate against the pool this source declares. No socket and no cache - Google's
//! token service performs the exchange - but an EXPIRY, because credential material ages out.
//!
//! **A switching source is the second thing it presents** ([`DeclaredPrincipalBroker::switching`]):
//! a declared subject-to-principal map, and the leg carries the declared principal alone, as
//! [`Presented::SubjectPrincipal`], for a source that authenticates the deployment and switches per
//! statement - `ClickHouse`'s `EXECUTE AS`. No assertion is required there; leg 1's verified subject
//! is the key, and an anonymous or undeclared caller is refused.
//!
//! **An authenticating source is the third** ([`DeclaredPrincipalBroker::authenticating`]): a set
//! of subjects rather than a map, for a source that opens each request's session with the asker's
//! own verified assertion and resolves who that is itself - Oracle's token authentication.
//!
//! # What it refuses, which is the half that matters
//!
//! - **A source it holds no declaration for** - refused as `credential_unavailable`, so a forgotten
//!   attachment cannot read every row as the deployment.
//! - **An anonymous caller, or one with no assertion, at a federating source** - there is nothing
//!   for the pool to verify, and nothing to fall back to.
//! - **A caller a switching or authenticating source's declaration does not name** - refused, never
//!   widened to a bare deployment identity.
//!
//! # The limit, beside the claim
//!
//! What a federated caller may read is decided by IAM grants this repository cannot see: no type,
//! lint, hook or gate here reads a live policy, and the driver's token fetch is lazy, so a pool that
//! refuses the assertion or a principal with no grant surfaces on the FIRST QUESTION by that
//! subject, never at boot.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::identity::{
    CredentialBroker, CredentialsDoNotCoverThePlan, Delegation, DelegationFailed, Expiry, LegCredentials, Minted, Presented,
    PrincipalName, RequestContext, SourceSet, SubjectKey,
};
use sutura_domain::model::SourceName;
use sutura_domain::source::SharedIdentityDeclared;

/// Why a declared subject map or set is not one a source can be served under.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoDeclaredPrincipals {
    /// The declaration named no subject at all.
    #[error(
        "a source declared `impersonation-at-source` names no subject to execute as, so no caller \
         could ever be served there - declare the subjects it may be asked as, or declare the \
         source `shared-service-user` with an acknowledgement"
    )]
    Empty,
}

/// The subjects one switching source may be asked as, and the principal each of them becomes.
///
/// **A parsed type and not a bare map, because the empty map is the interesting value.** A
/// switching source with no declared subject can serve nobody: every request would be refused,
/// while the boot log said the source opened. So the emptiness is refused at the boundary that can
/// turn it into a startup failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredPrincipals(BTreeMap<SubjectKey, PrincipalName>);

impl DeclaredPrincipals {
    /// Parses a declaration for a source that SWITCHES to the declared principal on the
    /// deployment's own connection - [`DeclaredPrincipalBroker::switching`].
    ///
    /// **Keyed on the FULL verified subject and never on the masked
    /// [`SubjectId`](sutura_domain::identity::SubjectId)**: whether a caller may be served at this
    /// source at all is an authorization decision, and a masked key admits every undeclared caller
    /// that shares a declared subject's mask. Only emptiness is refused here: the crate that sends a
    /// value narrows it (`sutura_exec_clickhouse::execute_as::ClickHouseUser` for `EXECUTE AS`).
    ///
    /// # Errors
    ///
    /// [`NoDeclaredPrincipals::Empty`] for a declaration naming nobody.
    pub fn switched(declared: BTreeMap<SubjectKey, PrincipalName>) -> Result<Self, NoDeclaredPrincipals> {
        if declared.is_empty() {
            return Err(NoDeclaredPrincipals::Empty);
        }
        Ok(Self(declared))
    }

    /// The principal this source switches to for this subject, if it declares the subject at all.
    ///
    /// `None` is not a fallback - it is the answer for every caller a deployment did not name, and
    /// [`DeclaredPrincipalBroker::mint`] turns it into a refusal.
    #[inline]
    #[must_use]
    pub fn target(&self, subject: &SubjectKey) -> Option<&PrincipalName> {
        self.0.get(subject)
    }

    /// How many subjects this source declares. Never zero.
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.0.len()
    }
}

/// The subjects one authenticating source may be asked as - [`DeclaredPrincipalBroker::authenticating`].
///
/// A set and not a [`DeclaredPrincipals`], because the source names no principal: the database
/// resolves the asker's own token to a user itself. Never empty, for [`DeclaredPrincipals`]' reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredSubjects(BTreeSet<SubjectKey>);

impl DeclaredSubjects {
    /// Parses one source's declared subjects.
    ///
    /// # Errors
    ///
    /// [`NoDeclaredPrincipals::Empty`] for a declaration naming nobody.
    pub fn parse(declared: BTreeSet<SubjectKey>) -> Result<Self, NoDeclaredPrincipals> {
        if declared.is_empty() {
            return Err(NoDeclaredPrincipals::Empty);
        }
        Ok(Self(declared))
    }

    /// Whether this source may be asked as `subject`.
    #[inline]
    #[must_use]
    pub fn admits(&self, subject: &SubjectKey) -> bool {
        self.0.contains(subject)
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
    /// The delegation exchange a `direct` source needs produced no usable token - the identity provider is a hard
    /// runtime dependency, so this is `503 identity_unavailable` and never an answer as anybody.
    #[error("the delegation exchange for `{source}` failed")]
    Delegation {
        /// The source whose exchange failed.
        source: SourceName,
        #[source]
        cause: DelegationFailed,
    },
}

/// Presents the asking subject's own credential at a federating or authenticating source, the
/// declared principal at a switching one - and the operator's witness for a shared one.
///
/// **The credential is the verified assertion itself**, or, for a federating source declared with
/// a [`Delegation`], the token that delegation returned for that assertion.
///
/// **All four maps, because one plan may read one of each and a broker is per answer rather than
/// per source** - the reason `docs/adr/0008` part 4 gives for a broker being per answer at all.
#[derive(Debug, Clone, Default)]
pub struct DeclaredPrincipalBroker {
    shared: BTreeMap<SourceName, SharedIdentityDeclared>,
    federating: BTreeMap<SourceName, Option<Delegation>>,
    switching: BTreeMap<SourceName, DeclaredPrincipals>,
    authenticating: BTreeMap<SourceName, DeclaredSubjects>,
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

    /// Declares one federating source: every verified caller's own assertion is presented, and the
    /// data system's grants on that caller's federated principal decide what it reads.
    ///
    /// With `delegation`, the caller's inbound token serves leg 1 only (`direct` mode), and what this
    /// source presents is the token `delegation` exchanges it for.
    #[must_use]
    pub fn federating(mut self, at: SourceName, delegation: Option<Delegation>) -> Self {
        drop(self.federating.insert(at, delegation));
        self
    }

    /// Declares one impersonating source that switches to each declared subject's principal on the
    /// deployment's own connection - `ClickHouse`'s `EXECUTE AS`.
    #[must_use]
    pub fn switching(mut self, at: SourceName, declared: DeclaredPrincipals) -> Self {
        drop(self.switching.insert(at, declared));
        self
    }

    /// Declares one impersonating source that opens each request's session with the asker's own
    /// verified assertion, for the subjects it declares. No principal is named: the source resolves
    /// the token itself.
    #[must_use]
    pub fn authenticating(mut self, at: SourceName, declared: DeclaredSubjects) -> Self {
        drop(self.authenticating.insert(at, declared));
        self
    }

    /// How many sources this broker can mint for at all.
    ///
    /// Read by this crate's own suite, where the assertion that matters is that a source declared
    /// impersonating with no entry is ABSENT rather than mapped to something.
    #[must_use]
    pub fn count(&self) -> usize {
        self.shared
            .len()
            .saturating_add(self.federating.len())
            .saturating_add(self.switching.len())
            .saturating_add(self.authenticating.len())
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
            if !self.shared.contains_key(source)
                && !self.federating.contains_key(source)
                && !self.switching.contains_key(source)
                && !self.authenticating.contains_key(source)
            {
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
        // **The assertion's OWN expiry** - and for a delegated source the exchanged token's too,
        // folded below by `Expiry::earlier_of`. An earlier round
        // minted `NothingExpires` here, which was honest while this broker presented a principal's
        // name - a name does not age - and became a check that always answers yes the moment it
        // started presenting credential material. `BoundToTheRequest::still_usable_at` reads this.
        let assertion_expires = context.assertion_expires();
        let mut presented = BTreeMap::new();
        // One per source, folded by `Expiry::earliest` below: a shared leg runs on a credential this
        // broker did not mint and does not age with the caller, so giving a shared-only plan the
        // caller's expiry would refuse answers for a lifetime that does not apply to them.
        let mut deadlines = Vec::new();
        let mut exchanges = Vec::new();
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
            // **The same authorization decision, and the asker's own assertion with no principal
            // beside it**: the source opens this request's session with the token and resolves who
            // it is. No exchange, so nothing here waits on I/O before a later refusal.
            if let Some(subjects) = self.authenticating.get(source) {
                let admitted = key.is_some_and(|key| subjects.admits(key));
                let (true, Some(assertion), Some(expires)) = (admitted, assertion, assertion_expires) else {
                    return Ok(Minted::Refused { source: source.clone() });
                };
                drop(presented.insert(
                    source.clone(),
                    Presented::SubjectToken {
                        material: assertion.clone(),
                    },
                ));
                deadlines.push(expires);
                continue;
            }
            if let Some(principals) = self.switching.get(source) {
                let Some(target) = key.and_then(|key| principals.target(key)) else {
                    return Ok(Minted::Refused { source: source.clone() });
                };
                // A name does not age, so the bound is the caller's own assertion where leg 1 had one.
                drop(presented.insert(source.clone(), Presented::SubjectPrincipal { name: target.clone() }));
                deadlines.push(assertion_expires.unwrap_or(Expiry::NothingExpires));
                continue;
            }
            // Unreachable: the pass above established that every source is declared somewhere.
            // Answered rather than unwrapped, because `unwrap_used` is denied.
            let Some(delegation) = self.federating.get(source) else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            // **The one refusal a federating source makes, and it is the whole of *no fallback*.** A
            // request with no verified subject, or one whose subject carries no assertion, has
            // nothing for Google's token service to verify - and the only alternative to refusing is
            // answering as the deployment. Every other caller is presented its own assertion: which
            // rows that caller reads is the data system's grants on its federated principal.
            let (Some(_), Some(assertion), Some(expires)) = (key, assertion, assertion_expires) else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            exchanges.push((source, delegation.as_ref(), assertion, expires));
        }
        // **Every refusal above is decided before any exchange below**, so a plan refused at its
        // second source sends nothing to the IdP for its first. And no store: each exchange's token
        // lives in this request's credentials alone, so two subjects cannot share one.
        for (source, delegation, assertion, expires) in exchanges {
            let (material, expires) = match delegation {
                None => (assertion.clone(), expires),
                Some(delegation) => {
                    let delegated = delegation
                        .exchange(assertion)
                        .map_err(|cause| DeclaredPrincipalsUnusable::Delegation {
                            source: source.clone(),
                            cause,
                        })?;
                    let until = Expiry::At {
                        unix_seconds: delegated.not_after_unix_seconds(),
                    };
                    (delegated.into_token(), expires.earlier_of(until))
                }
            };
            drop(presented.insert(source.clone(), Presented::SubjectToken { material }));
            deadlines.push(expires);
        }
        // **The earliest of what was presented, which is what `Expiry::earliest` exists for.** A
        // federated leg is valid for as long as the caller's own assertion is, or the earlier of that
        // and its exchanged token for a delegated source; a shared leg
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
