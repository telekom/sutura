//! The broker a served `BigQuery` deployment impersonates through: a verified subject in, that
//! subject's own verified assertion out.
//!
//! **Both halves of the declared map are read, which is what `telekom/sutura#929` F3 changed.** Its
//! KEYS decide WHETHER a caller may be served here; its VALUES name the account that caller's
//! question is to execute as, carried on [`Presented::SubjectToken`] and interpolated by
//! [`crate::adbc`] into the credential document's `service_account_impersonation_url` (see
//! [`DeclaredPrincipals::target`]). The chain is *this subject's own assertion federates to the
//! pool's principal, and that principal then impersonates the declared account* - so the pool
//! principal is the one holding the binding and the caller is never the deployment.
//!
//! **Why this is the only broker this crate carries.** It used to sit beside an EXCHANGING one -
//! `sts::WorkloadIdentityBroker`, which handed the caller's own assertion to a token service and
//! presented what came back as the leg's bearer. Its two HTTP hops went away with the `wire`
//! transport, leaving a 2,571-line tree no composition root could reach and whose every exchange
//! was a test fake, so `docs/adr/0018`'s eighth amendment deleted it. What the ADBC transport needs
//! is narrower and different in kind: not a token to exchange, but the asking subject's OWN
//! assertion for the driver to federate against the pool this source declares. No socket and no
//! cache - Google's token service performs the exchange - but an EXPIRY, because what this presents
//! is credential material and credential material ages out.
//!
//! One broker with one answer to *what does this leg present* is the shape `docs/adr/0008` part 4
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
//! This broker decides WHETHER this caller may be served at a source, presents the caller's own
//! verified assertion for it, and names the account the question is to run as. What it CANNOT check
//! is that the named account is **reachable**: no type, lint, hook or gate in this repository sees a
//! live IAM policy, and there is no boot-time probe to add - the driver's token fetch is lazy,
//! `AdbcBigQuery::probe` opens no connection and reads no credential, and a boot-time mint would
//! need a caller's assertion, which boot does not have. A declared account the pool principal may
//! not impersonate therefore surfaces as `AdbcError::Adbc` on the FIRST QUESTION BY THAT SUBJECT and
//! never at boot. What is refused here is the account's SHAPE ([`DeclaredPrincipals::parse`]), which
//! is a different claim.
//!
//! And nothing here proves Google accepted the assertion or the second hop: that is leg 2, it needs
//! a hosted run, and `docs/where-identity-is-proven.md` records the venue as `wired`.

use std::collections::BTreeMap;

use sutura_domain::identity::{
    CredentialBroker, CredentialsDoNotCoverThePlan, Expiry, LegCredentials, Minted, Presented, PrincipalName, RequestContext,
    SourceSet, SubjectKey,
};
use sutura_domain::model::SourceName;
use sutura_domain::source::SharedIdentityDeclared;

/// Why a declared impersonation map is not one a source can be served under.
///
/// Two variants, and the second one is the reason the enum was one from the start: an empty
/// declaration can serve nobody, and a declared ACCOUNT this transport cannot name in a request is
/// the same class of defect one level down.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoDeclaredPrincipals {
    /// The declaration named no subject at all.
    #[error(
        "a source declared `impersonation-at-source` names no subject to execute as, so no caller \
         could ever be served there - declare the subject to service-account map, or declare the \
         source `shared-service-user` with an acknowledgement"
    )]
    Empty,
    /// A declared target is not a shape this transport can name in an impersonation URL.
    ///
    /// **A parse refusal and not a warning, because the value selects an ACCOUNT.** It is
    /// interpolated into one path segment of `service_account_impersonation_url`, so a value
    /// carrying `/` re-points that segment at a different account - and
    /// `PrincipalName::parse` accepts `/`, since it is the parser every principal identifier in
    /// the domain shares and a role name is not an email. The narrowing belongs to the crate that
    /// SENDS the value, which is this one.
    #[error(
        "`{target}` is declared as an impersonation target and is not a service-account address \
         this adapter can name in a request - at most 254 characters of letters, digits and \
         `. - _`, with exactly one `@`"
    )]
    NotAServiceAccount {
        /// The value as declared. Not a secret - a service-account address is what an operator has
        /// to read back to fix the declaration, and `PrincipalName`'s own doc says why it is not
        /// redacted.
        target: PrincipalName,
    },
}

/// Is this a service-account address this transport can name in an impersonation URL?
///
/// **The send-side half of a value configuration already parsed**, for the reason
/// [`crate::transport::ProjectId`] and [`crate::adbc::WorkloadPool`] are both parsed twice: a check
/// belongs where the risk is, and the risk here is one path segment of a URL that decides which
/// account a question runs as. The accepted set is the printable ASCII a service-account address is
/// built from, so nothing that could close a JSON string, add a URL segment or carry a query can
/// exist in a value that reaches the document.
///
/// Called from [`DeclaredPrincipals::parse`], where a bad declaration is a STARTUP failure, and
/// again from `crate::adbc::identity` before the interpolation, because [`Presented`] is a public
/// port and this broker is not the only thing that can construct one.
pub(crate) fn names_a_service_account(target: &PrincipalName) -> bool {
    /// RFC 5321's mailbox length, which is what `sutura_config` bounds the same value by.
    const MOST: usize = 254;

    let raw = target.as_str();
    !raw.is_empty()
        && raw.chars().count() <= MOST
        && raw.matches('@').count() == 1
        && raw
            .chars()
            .all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | '@'))
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
    /// declaration gives: whether a caller may be served at this source at all is an authorization
    /// decision, and a masked key admits every undeclared caller that shares a declared subject's
    /// mask - which, since the pool resolves whoever is admitted to a principal of its own, is an
    /// undeclared caller reading rows as somebody.
    ///
    /// **And the VALUES are narrowed here**, which is what makes this a parsed type rather than a
    /// non-empty map: what a value of this type now means is *a non-empty map whose accounts this
    /// transport can name in an impersonation URL*. Refused at the boundary that can turn it into a
    /// startup failure, through `sutura_cli`'s `build_broker`.
    ///
    /// # Errors
    ///
    /// [`NoDeclaredPrincipals::Empty`] for a declaration naming nobody, and
    /// [`NoDeclaredPrincipals::NotAServiceAccount`] for a declared account this adapter cannot put
    /// in a request.
    pub fn parse(declared: BTreeMap<SubjectKey, PrincipalName>) -> Result<Self, NoDeclaredPrincipals> {
        if declared.is_empty() {
            return Err(NoDeclaredPrincipals::Empty);
        }
        if let Some(target) = declared.values().find(|target| !names_a_service_account(target)) {
            return Err(NoDeclaredPrincipals::NotAServiceAccount { target: target.clone() });
        }
        Ok(Self(declared))
    }

    /// The account this source is to execute this subject's questions as, if it declares the
    /// subject at all.
    ///
    /// `None` is not a fallback - it is the answer for every caller a deployment did not name, and
    /// [`DeclaredPrincipalBroker::mint`] turns it into a refusal.
    ///
    /// **Both halves of the entry are read, and the VALUE is what this returns.** The key decides
    /// authorization and the account beside it decides which principal the question runs as: it
    /// rides on [`Presented::SubjectToken`] and becomes the credential document's
    /// `service_account_impersonation_url`, so changing a declared account changes which account a
    /// caller's question executes as. A round of this adapter read the key and dropped the value,
    /// which accepted a security-critical setting and then ignored it.
    ///
    /// Returning the account rather than a `bool` is what makes that mechanical: there is no
    /// arrangement of this signature in which the value is unread and the caller still compiles.
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

/// Presents the asking subject's own verified assertion at a source that declares it, beside the
/// account declared for that subject - and the operator's witness for a shared one.
///
/// **The assertion AND the account, because either alone loses the property.** The assertion is
/// what the caller possesses and what the pool verifies; the account is what a deployment declared
/// this caller's questions should run as, and a broker that presented only the assertion ran every
/// declared caller as one pool principal whatever the map said.
///
/// **Both maps, because one plan may read one of each and a broker is per answer rather than per
/// source** - the reason `docs/adr/0008` part 4 gives for a broker being per answer at all.
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
            let Some(target) = key.and_then(|key| principals.target(key)) else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            // **The asking subject's OWN assertion, plus the account declared beside that
            // subject.** The transport puts the assertion behind a workload-identity credential
            // document, so Google's token service verifies it and resolves the subject to the
            // pool's principal - and that principal then impersonates `target`, which is what makes
            // the map's VALUES decide something. Both halves travel because a transport with only
            // the assertion runs every declared caller as one pool principal, and one with only the
            // account has nothing the subject possesses in the chain.
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
                    impersonate: Some(target.clone()),
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
