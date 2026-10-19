//! What a request carries besides the question, and the OAuth client its token names.

use core::fmt;

use super::{InvalidPrincipalId, PrincipalChain, TaskId, parse_principal_id};

/// The OAuth client a verified token names - in the usual agent flow, the agent.
///
/// **Recorded, never authorized on.** There is no `PartialEq`, `Eq`, `Hash`, `Ord` or `as_str`, so
/// no grant, map key or comparison can read it; a sink renders it through `Display`:
///
/// ```compile_fail
/// use sutura_domain::identity::OAuthClient;
///
/// let client = OAuthClient::parse("agent-app").expect("a client id");
/// assert!(client == OAuthClient::parse("agent-app").expect("a client id"));
/// ```
///
/// The compiling twin, so the failure above is the missing `==` and not a typo - and the limit: a
/// rendering is a `String`, which review alone keeps out of a decision.
///
/// ```
/// use sutura_domain::identity::OAuthClient;
///
/// let client = OAuthClient::parse("agent-app").expect("a client id");
/// assert_eq!(client.to_string(), "agent-app");
/// ```
///
/// Not masked, unlike [`super::Actor`]: it names an application registration, not a person. It goes
/// through the same parse, so a control or invisible character cannot reach an audit line.
#[derive(Clone)]
pub struct OAuthClient(String);

impl OAuthClient {
    /// Parses a client identifier with the principal-identifier rules.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidPrincipalId> {
        parse_principal_id(raw.as_ref()).map(Self)
    }
}

impl fmt::Display for OAuthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for OAuthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What one client claim of a verified token held.
#[derive(Debug, Clone)]
pub enum ClientClaim {
    /// A client identifier.
    Named(OAuthClient),
    /// Present, but not a string or not a client identifier. Rendered `unreadable`, so a record tells
    /// it apart from a token that named no client; a client literally named `unreadable` renders the
    /// same. A JSON `null` is read as no claim, not as this.
    Unreadable,
}

impl ClientClaim {
    /// Reads one claim value; `None` is a value that is not a string.
    #[must_use]
    pub fn read(raw: Option<&str>) -> Self {
        raw.and_then(|written| OAuthClient::parse(written).ok())
            .map_or(Self::Unreadable, Self::Named)
    }
}

impl fmt::Display for ClientClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Named(ref client) => client.fmt(f),
            Self::Unreadable => f.write_str("unreadable"),
        }
    }
}

/// The OAuth client a verified token names, as its `azp` and RFC 9068 `client_id` claims each held it.
///
/// **Both are kept**, so two claims that disagree are both on the record rather than one chosen.
/// Audit only: nothing grants on it. Leg 1's own `azp` check reads the token, not this: with
/// `extra_scope_claims` set, a token whose `azp` is absent or names the API is refused.
#[derive(Debug, Clone, Default)]
pub struct ClientClaims {
    azp: Option<ClientClaim>,
    client_id: Option<ClientClaim>,
}

impl ClientClaims {
    /// The two claims as the token held them; `None` is a claim the token did not carry.
    #[must_use]
    pub const fn of(azp: Option<ClientClaim>, client_id: Option<ClientClaim>) -> Self {
        Self { azp, client_id }
    }

    /// What `azp` held.
    #[inline]
    pub const fn azp(&self) -> Option<&ClientClaim> {
        self.azp.as_ref()
    }

    /// What `client_id` held.
    #[inline]
    pub const fn client_id(&self) -> Option<&ClientClaim> {
        self.client_id.as_ref()
    }
}

/// What a request carries besides the question.
///
/// Two fields: who the call is attributed to, and - where the transport established one - the
/// caller's own credential assertion. The second is what lets a [`crate::identity::CredentialBroker`]
/// that exchanges a token have the caller's token to exchange: `docs/adr/0008` part 2 sketched a
/// `Caller { subject, assertion }` for exactly this, and `docs/adr/0014`'s open Decision 3 is why the
/// assertion field was absent until a broker existed that performs an exchange. It is an `Option`
/// because the two shapes that reach this value are genuinely different: a deployment's own identity,
/// which has no credential, and a verified caller, whose transport retained the token it verified.
/// `None` is "there is no per-caller credential to exchange", not an oversight.
///
/// Everything a question is answered *for* rather than *about* belongs here, so the credential an
/// execution leg will need and the deadline it will carry have a place to arrive that is not a
/// widened [`crate::query::Query`]. The tool surface stays a question and nothing else.
///
/// **Here rather than in `sutura-app`.** The application is where a context enters the service, but
/// the domain is what names it: a `CredentialBroker` port declared here takes the caller, and a port
/// in the interior cannot speak in a type owned by an adapter or by the service above it.
///
/// Not `Deserialize`, for the reason the chain is not: a request context assembled from the request
/// body is the confused deputy this whole module refuses.
///
/// **Not `PartialEq`/`Eq`, and the `Secret` is why.** This type now holds credential material, and
/// [`crate::identity::Secret`] implements no comparison, because `==` on credential material is a
/// timing oracle; a derived equality would have compared the two secrets byte-wise. `Debug` and
/// `Clone` survive because `Secret` implements both (redacting and value-preserving respectively).
#[derive(Debug, Clone)]
pub struct RequestContext {
    chain: PrincipalChain,
    /// The caller's own credential and when it stops being one, as ONE field.
    ///
    /// **A pair rather than two `Option`s, because the two must not be able to disagree.** A broker
    /// that presents the assertion has to mint an [`crate::identity::Expiry`] for it, and separate
    /// fields would allow "material with no expiry" - which is what
    /// [`crate::identity::Expiry::NothingExpires`] means, and which is false of every assertion leg
    /// 1 verifies. `exp` is in the transport's own `required_spec_claims`, so a verified caller
    /// always has one - and [`Self::with_assertion`] takes the instant rather than an `Expiry`, so
    /// the forever variant cannot be written into this pair at all.
    assertion: Option<(crate::identity::Secret, crate::identity::Expiry)>,
    /// What a deployment mapped this caller's verified group claim onto - `docs/adr/0028`. Empty
    /// for [`Subject::TheDeploymentItself`](crate::identity::Subject::TheDeploymentItself) and for every constructor below that does not call
    /// [`Self::granting`]; nothing here interprets emptiness as "the whole catalog" - that reading
    /// belongs to whoever chooses [`crate::catalog::GrantedAudiences`] vs a [`crate::pinned::view::ScopedView`]
    /// built from [`Subject::TheDeploymentItself`](crate::identity::Subject::TheDeploymentItself), and it must keep the two distinct.
    audiences: crate::catalog::GrantedAudiences,
    /// The OAuth client the caller's token named, for the audit record - see [`ClientClaims`].
    client: ClientClaims,
    /// The address the identity provider verified for this caller, if it did.
    address: Option<crate::identity::VerifiedAddress>,
    /// The caller's verified group claim, which a source's billing map reads.
    groups: crate::identity::CallerGroups,
    /// The account the caller named on the request to be billed, if it named one.
    named_payer: Option<crate::identity::BillingAccount>,
}

impl RequestContext {
    /// A context carrying only the chain, for the caller that has no credential to present - the
    /// deployment's own identity, and every non-verified surface that reaches this value.
    #[inline]
    pub fn of(chain: PrincipalChain) -> Self {
        Self {
            chain,
            assertion: None,
            audiences: crate::catalog::GrantedAudiences::none(),
            client: ClientClaims::of(None, None),
            address: None,
            groups: crate::identity::CallerGroups::default(),
            named_payer: None,
        }
    }

    /// The same context, with the caller's own credential assertion the transport retained and the
    /// moment that assertion stops being one.
    ///
    /// **This is the port change `docs/adr/0008` part 2 asked for and `docs/adr/0014` gated.** A
    /// broker that performs an exchange could not read the caller's token from the [`RequestContext`]
    /// it was handed, because nothing named what the caller presented - only who. This constructor is
    /// the thing that carries it, and it appears next to the first broker that exchanges rather than
    /// before it.
    ///
    /// **The expiry is an instant and not an [`crate::identity::Expiry`]**, which is the whole
    /// mechanism: `Expiry` has a
    /// [`NothingExpires`](crate::identity::Expiry::NothingExpires) variant, and *credential material
    /// that never expires* is not a state a verified assertion can be in - `exp` is in the
    /// transport's own required claims. Taking the seconds and wrapping them here means a caller
    /// CANNOT pass the forever variant, so
    /// [`crate::identity::BoundToTheRequest::still_usable_at`] cannot be handed a leg it will always
    /// answer yes for. The parameter did not exist while every broker either exchanged the assertion
    /// for something with its own lifetime or presented a principal's NAME, which does not age.
    #[inline]
    #[must_use]
    pub fn with_assertion(chain: PrincipalChain, assertion: crate::identity::Secret, not_after_unix_seconds: u64) -> Self {
        Self {
            chain,
            assertion: Some((
                assertion,
                crate::identity::Expiry::At {
                    unix_seconds: not_after_unix_seconds,
                },
            )),
            audiences: crate::catalog::GrantedAudiences::none(),
            client: ClientClaims::of(None, None),
            address: None,
            groups: crate::identity::CallerGroups::default(),
            named_payer: None,
        }
    }

    /// The same context, carrying the audiences a deployment's mapping granted this caller -
    /// `docs/adr/0028`.
    #[inline]
    #[must_use]
    pub fn granting(mut self, audiences: crate::catalog::GrantedAudiences) -> Self {
        self.audiences = audiences;
        self
    }

    /// The same context, carrying the address the transport read from a verified claim.
    #[inline]
    #[must_use]
    pub fn addressed(mut self, address: Option<crate::identity::VerifiedAddress>) -> Self {
        self.address = address;
        self
    }

    /// The same context, carrying the caller's verified group claim.
    #[inline]
    #[must_use]
    pub fn grouped(mut self, groups: crate::identity::CallerGroups) -> Self {
        self.groups = groups;
        self
    }

    /// The same context, carrying the account the caller named to be billed.
    #[inline]
    #[must_use]
    pub fn naming(mut self, payer: Option<crate::identity::BillingAccount>) -> Self {
        self.named_payer = payer;
        self
    }

    #[inline]
    pub const fn groups(&self) -> &crate::identity::CallerGroups {
        &self.groups
    }

    #[inline]
    pub const fn named_payer(&self) -> Option<&crate::identity::BillingAccount> {
        self.named_payer.as_ref()
    }

    /// The caller's verified address: what a `domain_delegation` source runs the question as.
    #[inline]
    pub const fn address(&self) -> Option<&crate::identity::VerifiedAddress> {
        self.address.as_ref()
    }

    /// What this caller was granted, for [`crate::pinned::view::ScopedView::granted_by`].
    #[inline]
    pub const fn audiences(&self) -> &crate::catalog::GrantedAudiences {
        &self.audiences
    }

    #[inline]
    pub const fn chain(&self) -> &PrincipalChain {
        &self.chain
    }

    /// The caller's own credential assertion, where the transport established and retained one.
    ///
    /// `None` for the deployment's own identity and for every surface with no credential to verify.
    /// A broker that only mints from configuration returns a credential without reading it; one that
    /// performs an exchange refuses a request whose caller presented nothing to exchange, as
    /// [`crate::query::RefusalReason::CredentialUnavailable`].
    #[inline]
    pub fn assertion(&self) -> Option<&crate::identity::Secret> {
        self.assertion.as_ref().map(|(material, _)| material)
    }

    /// When the assertion above stops being one, where there is an assertion at all.
    ///
    /// `None` is "there is no caller credential here", which is a different answer from "it never
    /// expires" - and the reason this is not an [`crate::identity::Expiry`] with a
    /// [`NothingExpires`](crate::identity::Expiry::NothingExpires) fallback. A broker that reads
    /// this and finds `None` on a path that needs the caller's own material has to refuse; folding
    /// the two answers together would hand it a credential that outlives every clock.
    #[inline]
    pub fn assertion_expires(&self) -> Option<crate::identity::Expiry> {
        self.assertion.as_ref().map(|&(_, expires)| expires)
    }

    /// The task this request belongs to, if one was named.
    ///
    /// Delegates to the chain rather than holding a second copy: the task is the chain's third
    /// position, and two places to read it from is two places for them to disagree.
    #[inline]
    pub const fn task(&self) -> Option<&TaskId> {
        self.chain.task()
    }

    /// The same context, naming the OAuth client the caller's token was issued to.
    #[inline]
    #[must_use]
    pub fn naming_client(mut self, client: ClientClaims) -> Self {
        self.client = client;
        self
    }

    /// The OAuth client the caller's token named, for the audit record.
    #[inline]
    pub const fn client(&self) -> &ClientClaims {
        &self.client
    }
}
