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
//! no map - the leg carries the caller's own verified subject as [`Presented::SubjectPrincipal`],
//! for a source that authenticates the deployment and switches per statement - `ClickHouse`'s
//! `EXECUTE AS`. No assertion is required there; an anonymous caller is refused, and so is one
//! whose verified name IS the deployment's own user at that source. A source each caller signs in
//! at ([`DeclaredPrincipalBroker::signing_in`]) is presented that name beside the token the
//! delegation exchanges their assertion for, as [`Presented::SubjectSignIn`].
//!
//! **An authenticating source is the third** ([`DeclaredPrincipalBroker::authenticating`]): the
//! declared delegation exchanges the asker's assertion before the source opens the session with
//! the returned token and resolves who that is itself - Oracle's token authentication.
//!
//! # What it refuses, which is the half that matters
//!
//! - **A source it holds no declaration for** - refused as `credential_unavailable`, so a forgotten
//!   attachment cannot read every row as the deployment.
//! - **An anonymous caller, or one with no assertion, at a federating or authenticating source** - there is nothing
//!   for the pool to verify, and nothing to fall back to.
//! - **A caller whose verified name is the deployment's own user at a switching source** -
//!   refused: switching to that user would answer as the deployment.
//!
//! # The limit, beside the claim
//!
//! What a federated caller may read is decided by IAM grants this repository cannot see: no type,
//! lint, hook or gate here reads a live policy, and the driver's token fetch is lazy, so a pool that
//! refuses the assertion or a principal with no grant surfaces on the FIRST QUESTION by that
//! subject, never at boot.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::identity::{
    BillTo, CredentialBroker, CredentialsDoNotCoverThePlan, Delegation, DelegationFailed, Expiry, LegCredentials, Minted,
    Presented, PrincipalName, RequestContext, SourceSet, SubjectKey,
};
use sutura_domain::model::SourceName;
use sutura_domain::source::SharedIdentityDeclared;

/// The deployment's own user at a switching source, and the exchange for a caller's sign-in token.
type Switching = (PrincipalName, Option<Delegation>);

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

/// Presents the asking subject's own credential at a federating or authenticating source, their
/// verified name at a switching one - and the operator's witness for a shared one.
///
/// **The credential is the verified assertion itself**, or, for a federating source declared with
/// a [`Delegation`], or any authenticating source, the token that delegation returned for that assertion.
///
/// **All five, because one plan may read one of each and a broker is per answer rather than
/// per source** - the reason `docs/adr/0008` part 4 gives for a broker being per answer at all.
#[derive(Debug, Clone, Default)]
pub struct DeclaredPrincipalBroker {
    shared: BTreeMap<SourceName, SharedIdentityDeclared>,
    federating: BTreeMap<SourceName, Option<Delegation>>,
    /// Each source that runs a caller as their verified name, the deployment's own user there - a
    /// name no caller is run as - and, for a source each caller signs in at, the exchange for the
    /// token they sign in with.
    switching: BTreeMap<SourceName, Switching>,
    authenticating: BTreeMap<SourceName, Delegation>,
    /// Each source that runs a caller as the address their identity provider verified.
    addressing: BTreeSet<SourceName>,
    /// Who pays for a `bigquery` source's caller-run jobs.
    billing: BTreeMap<SourceName, crate::sources::billing::SourceBilling>,
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

    /// Declares one impersonating source that switches, on the deployment's own connection, to the
    /// principal named by each caller's verified subject - `ClickHouse`'s `EXECUTE AS`. `deployment`
    /// is the user that connection authenticates as: a caller whose verified name is that user is
    /// refused, since switching to it would answer as the deployment.
    #[must_use]
    pub fn switching(mut self, at: SourceName, deployment: PrincipalName) -> Self {
        drop(self.switching.insert(at, (deployment, None)));
        self
    }

    /// Declares one impersonating source that each caller signs in at as the principal their
    /// verified subject names, with the token `delegation` exchanges their verified assertion for -
    /// never the assertion itself. A caller whose verified name is `deployment` is refused, as at a
    /// [`Self::switching`] source.
    #[must_use]
    pub fn signing_in(mut self, at: SourceName, deployment: PrincipalName, delegation: Delegation) -> Self {
        drop(self.switching.insert(at, (deployment, Some(delegation))));
        self
    }

    /// Declares one impersonating source that opens each request's session with the token
    /// `delegation` exchanges the asker's verified assertion for. No principal is named and no
    /// caller is listed: the source resolves the token to its own user and decides what it may read.
    #[must_use]
    pub fn authenticating(mut self, at: SourceName, delegation: Delegation) -> Self {
        drop(self.authenticating.insert(at, delegation));
        self
    }

    /// Declares one `domain_delegation` source, presented each caller's verified address as the
    /// principal the source runs as. A caller with no verified address is refused: the deployment
    /// vouches for the address, so there is no other identity to fall back to.
    #[must_use]
    pub fn addressing(mut self, at: SourceName) -> Self {
        let _newly_declared = self.addressing.insert(at);
        self
    }

    /// Bills `at`'s caller-run legs by `billing`.
    #[must_use]
    pub fn billing(mut self, at: SourceName, billing: crate::sources::billing::SourceBilling) -> Self {
        drop(self.billing.insert(at, billing));
        self
    }

    /// The first group of the caller's the source maps, else the payer the caller named.
    ///
    /// A mapped group goes first only where the source lets callers name a payer. Elsewhere a named
    /// payer is passed on, even beside a mapped group, so the source refuses it rather than billing
    /// the group while the caller believes their payer was taken.
    fn bill_to(&self, at: &SourceName, context: &RequestContext) -> BillTo {
        let named = context.named_payer();
        if let Some(account) = self
            .billing
            .get(at)
            .filter(|billing| named.is_none() || billing.caller_may_name())
            .and_then(|billing| billing.for_groups(context.groups()))
        {
            return BillTo::Group(account.clone());
        }
        named.map_or(BillTo::Unstated, |named| BillTo::Named(named.clone()))
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
            .saturating_add(self.addressing.len())
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
                && !self.addressing.contains(source)
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
            if self.addressing.contains(source) {
                let (Some(_), Some(address)) = (key, context.address()) else {
                    return Ok(Minted::Refused { source: source.clone() });
                };
                drop(presented.insert(source.clone(), Presented::SubjectPrincipal { name: address.into() }));
                deadlines.push(assertion_expires.unwrap_or(Expiry::NothingExpires));
                continue;
            }
            // **Every verified caller, and the token their assertion is exchanged for with no
            // principal beside it**: the source opens this request's session with it and resolves
            // who it is. A caller with no assertion has nothing to exchange and is refused. The
            // exchange runs below, after every refusal.
            if let Some(delegation) = self.authenticating.get(source) {
                let (Some(_), Some(assertion), Some(expires)) = (key, assertion, assertion_expires) else {
                    return Ok(Minted::Refused { source: source.clone() });
                };
                exchanges.push((source, None, Some(delegation), assertion, expires));
                continue;
            }
            // **Every verified caller, as the principal their own verified subject names**: the
            // source switches to it per statement. No caller, or one whose name is the deployment's
            // own user here, is refused - switching to that user would answer as the deployment.
            if let Some((deployment, delegation)) = self.switching.get(source) {
                let Some(Ok(name)) = key.map(SubjectKey::principal) else {
                    return Ok(Minted::Refused { source: source.clone() });
                };
                if name == *deployment {
                    return Ok(Minted::Refused { source: source.clone() });
                }
                // A caller who signs in presents the EXCHANGED token beside their name; the exchange
                // runs below, after every refusal.
                if let Some(delegation) = delegation.as_ref() {
                    let (Some(assertion), Some(expires)) = (assertion, assertion_expires) else {
                        return Ok(Minted::Refused { source: source.clone() });
                    };
                    exchanges.push((source, Some(name), Some(delegation), assertion, expires));
                    continue;
                }
                drop(presented.insert(source.clone(), Presented::SubjectPrincipal { name }));
                // A name does not age, so the bound is the caller's own assertion where leg 1 had one.
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
            exchanges.push((source, None, delegation.as_ref(), assertion, expires));
        }
        // **Every refusal above is decided before any exchange below**, so a plan refused at its
        // second source sends nothing to the IdP for its first. And no store: each exchange's token
        // lives in this request's credentials alone, so two subjects cannot share one.
        for (source, name, delegation, assertion, expires) in exchanges {
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
            let leg = match name {
                None => Presented::SubjectToken { material },
                Some(name) => Presented::SubjectSignIn { name, material },
            };
            drop(presented.insert(source.clone(), leg));
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
        let bill_to = sources
            .iter()
            .map(|source| (source.clone(), self.bill_to(source, context)))
            .filter(|(_, bill_to)| *bill_to != BillTo::Unstated)
            .collect();
        LegCredentials::minted(asked_by.clone(), Expiry::earliest(deadlines), sources, presented)
            .map(|credentials| Minted::Granted {
                credentials: credentials.billed(bill_to),
            })
            .map_err(|cause| DeclaredPrincipalsUnusable::Coverage { cause })
    }
}

#[cfg(test)]
mod tests;

/// Beside the broker, because every cell here names `addressing` or a `VerifiedAddress`.
#[cfg(test)]
mod addressing {
    use sutura_domain::identity::{
        Agreed, CredentialBroker as _, Minted, PrincipalChain, RequestContext, Secret, SourceSet, Subject, VerifiedAddress,
    };
    use sutura_domain::model::SourceName;

    use super::{DeclaredPrincipalBroker, Presented};

    fn warehouse() -> SourceName {
        SourceName::parse("warehouse").expect("a test source name is a name")
    }

    fn address(raw: &str) -> VerifiedAddress {
        VerifiedAddress::parse(raw).expect("a test address is an address")
    }

    /// A verified caller carrying its assertion, as leg 1 hands it over.
    fn caller(raw: &str) -> RequestContext {
        RequestContext::with_assertion(
            PrincipalChain::of(Subject::verified(raw).expect("a test subject is a subject")),
            Secret::new(format!("assertion.for.{raw}")),
            4_102_444_800,
        )
    }

    fn addressing() -> DeclaredPrincipalBroker {
        DeclaredPrincipalBroker::empty().addressing(warehouse())
    }

    #[test]
    fn a_domain_delegation_caller_is_presented_as_their_verified_address() {
        let asked = SourceSet::of(warehouse());
        let minted = addressing()
            .mint(
                &caller("idp-subject-1").addressed(Some(address("Analyst.A@example.com"))),
                &asked,
            )
            .expect("a caller with an address is mintable");
        let asked_by = Subject::verified("idp-subject-1").expect("a test subject is a subject");
        let Agreed::Granted { credentials } = minted
            .agreeing_with(&asked_by, &asked, 4_000_000_000)
            .expect("the grant agrees with the request")
        else {
            panic!("expected a granted agreement")
        };
        match credentials.presented_for(&warehouse()).expect("the source was asked for") {
            Presented::SubjectPrincipal { name } => assert_eq!(name.as_str(), "Analyst.A@example.com"),
            other => panic!("expected the verified address, got {other:?}"),
        }
    }

    #[test]
    fn a_domain_delegation_request_with_no_address_is_refused() {
        // The `sub` is an address and the assertion is there: neither is a fallback for the claim. An
        // address with no verified caller behind it is refused too.
        let asked = SourceSet::of(warehouse());
        for context in [
            caller("analyst-a@example.com"),
            RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
                .addressed(Some(address("analyst-a@example.com"))),
        ] {
            let minted = addressing().mint(&context, &asked).expect("a refusal is an answer");
            assert!(
                matches!(minted, Minted::Refused { ref source } if *source == warehouse()),
                "a request with no verified address was minted a leg"
            );
        }
    }
}
