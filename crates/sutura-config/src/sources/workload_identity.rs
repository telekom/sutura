//! The Workload Identity Federation setup one `impersonation-at-source` source declares.
//!
//! **This process performs no exchange with the pool.** `audience` names the pool a subject's own
//! assertion is federated against, and the federating is Google's token service's, driven by the
//! driver from the `external_account` document `sutura_exec_bigquery`'s ADBC transport builds. The
//! one exchange this process can run is a declared
//! [`crate::sources::workload_identity::DelegationDeclared`], at the caller's own identity provider,
//! before that. A source that
//! executes as the asking subject has to say *which* pool receives that assertion, and that is the
//! source's declaration rather than this process's guess. See `docs/adr/0008` and `docs/adr/0018`'s
//! sixth amendment.
//!
//! **What each declared value actually reaches.** `audience` is sent. There is no `scope` key:
//! the credential document has no `scopes` member, and the driver's own scope option selects the
//! DELETED principal-switch mechanism rather than this one, so a declared scope would reach
//! nothing and is refused as an unknown key. There is no `impersonate` key either: the federated
//! token IS the caller's own pool principal, and the data system's grants on that principal decide
//! what the caller may read (`docs/adr/0032`'s option 3).
//! `expected_issuer`/`expected_audience` are REFUSED at boot by
//! `sutura_cli::serve::broker` - the RFC 8693 hop that checked them is deleted
//! (`docs/adr/0034`, both amendments), and a declaration nothing reads is a control that reads as
//! being in place.
//!
//! The two newtypes are declared here, in the settings tree that owns the value, and the adapter
//! that sends `audience` holds its own copy - the same reason [`BillingProject`] is checked both
//! here and in the transport that interpolates it: an adapter may not depend on the settings tree,
//! so the format is checked where it is declared AND where it is sent.

/// The pool a subject's own assertion is federated against: a workload identity provider
/// resource.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WifAudience(String);

/// The Workload Identity Federation setup a `impersonation-at-source` source needs.
///
/// **No subject map, and no account to become.** Every verified caller's own assertion is
/// federated to the pool, and the token Google's token service returns is that caller's own
/// `principal://.../subject/<sub>`. Which caller may read what is the data system's grants on that
/// principal; a caller with no grant is refused by the data system, never answered as anybody else.
///
/// **`expected_issuer` and `expected_audience` are refused at boot.** They named what the pool
/// itself trusts, so that a document leg 1 accepts could not be one the pool declines -
/// telekom/sutura#817's seam. The mechanism that compared them was the RFC 8693 hop and its claim
/// check, both deleted (`docs/adr/0034`, both amendments), so `sutura_cli::serve::broker` refuses a
/// source declaring either, naming both keys. They stay parsed and refusable rather than dropped so
/// that a deployment which once declared them fails loudly; whether the settings tree should keep
/// them at all is an owner decision `docs/adr/0034` does not take.
///
/// **Both are still `Option` and a PAIR**, refused unless both or neither are present - a lone
/// value would be half a comparison even once something compares them again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadIdentityConfig {
    audience: WifAudience,
    /// The issuer the pool trusts - the `iss` a subject token must carry for Google STS to accept
    /// it. See the type's own doc for why `None` is a value.
    expected_issuer: Option<crate::IssuerUrl>,
    /// The audience the pool's provider accepts - the STS audience a subject token must carry.
    expected_audience: Option<WifAudience>,
    delegation: Option<DelegationDeclared>,
}

/// The delegation exchange a `direct` deployment runs before the pool will accept its caller.
///
/// `docs/adr/0014`'s fourth amendment: the caller's inbound token is exchanged at
/// `token_endpoint` for one whose `aud` is `audience`, the pool provider's client ID.
///
/// **Held as written and parsed by the crate that sends it**, at boot, by `sutura_cli`'s
/// `build_broker` - the endpoint, client ID and audience each go into a request only that adapter
/// builds, so its parse is the one that decides whether they can be sent, and a refusal there is
/// still a startup failure naming the key. One check runs here instead: an `@` in the endpoint is
/// refused at load, because the startup log prints this tree first. The secret is not here at all: only the path to it,
/// which must be absolute like every other secret file a source names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationDeclared {
    token_endpoint: String,
    client_id: String,
    client_secret_file: std::path::PathBuf,
    audience: String,
}

impl DelegationDeclared {
    pub(crate) fn of(
        alias: &sutura_domain::model::SourceName,
        raw: &crate::raw::RawDelegation,
    ) -> Result<Self, super::InvalidSourceRegistry> {
        if raw.token_endpoint.contains('@') {
            return Err(super::InvalidSourceRegistry::CredentialsInUrl {
                alias: alias.clone(),
                key: "workload_identity.delegation.token_endpoint",
            });
        }
        Ok(Self {
            token_endpoint: raw.token_endpoint.clone(),
            client_id: raw.client_id.clone(),
            client_secret_file: super::parse_absolute(
                alias,
                "workload_identity.delegation.client_secret_file",
                &raw.client_secret_file,
            )?,
            audience: raw.audience.clone(),
        })
    }

    /// The identity provider's token endpoint. **Not tied to the inbound issuer:** the operator
    /// chooses the host, and each caller's token is sent to it.
    #[inline]
    #[must_use]
    pub fn token_endpoint(&self) -> &str {
        &self.token_endpoint
    }

    #[inline]
    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    #[inline]
    #[must_use]
    pub fn client_secret_file(&self) -> &std::path::Path {
        &self.client_secret_file
    }

    /// The pool provider's client ID the exchanged token must carry.
    #[inline]
    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }
}

impl WorkloadIdentityConfig {
    /// Parses a declared audience.
    pub fn parse(audience: impl AsRef<str>) -> Result<Self, InvalidWorkloadIdentity> {
        Self::parse_with_expectations(audience, None, None)
    }

    /// Parses a declaration that also names what the pool trusts - telekom/sutura#817's seam.
    ///
    /// The two extra values are what make `expected_issuer()`/`expected_audience()` readable by the
    /// broker. Both are optional and parsed with the same rules as the values they must match on
    /// the inbound side: the issuer as an absolute `https` URI (the [`crate::IssuerUrl`] parse) and
    /// the audience as a provider resource (the [`WifAudience`] parse).
    pub fn parse_with_expectations(
        audience: impl AsRef<str>,
        expected_issuer: Option<&str>,
        expected_audience: Option<&str>,
    ) -> Result<Self, InvalidWorkloadIdentity> {
        // **The twin-root link is a PAIR, telekom/sutura#817 - both or neither.** A lone value is
        // half a comparison, so it would be a declaration nothing could enforce even if something
        // compared them - the exact gap #817 exists to close. Refused here, at the boundary that
        // can. Nothing compares them TODAY: the RFC 8693 hop and its claim check are deleted and
        // `sutura_cli::serve::broker` refuses either key outright, so what this arm decides is only
        // which error an operator gets.
        let (expected_issuer, expected_audience) = match (expected_issuer, expected_audience) {
            (None, None) => (None, None),
            (Some(issuer), Some(audience)) => (
                Some(crate::IssuerUrl::parse(issuer).map_err(|cause| InvalidWorkloadIdentity::ExpectedIssuer { cause })?),
                Some(
                    WifAudience::parse(audience)
                        .map_err(|cause| InvalidWorkloadIdentity::ExpectedAudience { cause: Box::new(cause) })?,
                ),
            ),
            _ => return Err(InvalidWorkloadIdentity::PartialExpectation),
        };
        Ok(Self {
            audience: WifAudience::parse(audience.as_ref())?,
            expected_issuer,
            expected_audience,
            delegation: None,
        })
    }

    /// The same declaration, with the delegation exchange its callers' tokens go through.
    #[must_use]
    pub fn with_delegation(mut self, delegation: Option<DelegationDeclared>) -> Self {
        self.delegation = delegation;
        self
    }

    /// The delegation exchange, if declared. `Some` requires `security.inbound.mode: direct` -
    /// `crate::NotFitToServe::DelegationWithoutDirectInbound`.
    #[inline]
    #[must_use]
    pub const fn delegation(&self) -> Option<&DelegationDeclared> {
        self.delegation.as_ref()
    }

    /// The provider audience.
    #[inline]
    #[must_use]
    pub const fn audience(&self) -> &WifAudience {
        &self.audience
    }

    /// The issuer the pool trusts, if the declaration named one. `Some` is refused at boot -
    /// nothing compares it; see the type's own doc.
    #[inline]
    #[must_use]
    pub const fn expected_issuer(&self) -> Option<&crate::IssuerUrl> {
        self.expected_issuer.as_ref()
    }

    /// The audience the pool accepts, if the declaration named one. `Some` is refused at boot,
    /// for [`WorkloadIdentityConfig::expected_issuer`]'s reason.
    #[inline]
    #[must_use]
    pub const fn expected_audience(&self) -> Option<&WifAudience> {
        self.expected_audience.as_ref()
    }
}

impl WifAudience {
    /// The longest an audience may be. The endpoint documents up to 256 characters.
    const MOST: usize = 256;

    /// Parses an audience.
    ///
    /// The accepted set is the printable ASCII a workload identity provider resource is built from -
    /// letters, digits and `/ : . - _` - so a value that would escape the STS request body cannot
    /// exist here. Bounded in length, because it is a foreign string heading for a request and a log.
    pub fn parse(raw: &str) -> Result<Self, InvalidWorkloadIdentity> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(InvalidWorkloadIdentity::Empty { what: "audience" });
        }
        if trimmed.chars().count() > Self::MOST {
            return Err(InvalidWorkloadIdentity::TooLong {
                what: "audience",
                found: trimmed.chars().count(),
                most: Self::MOST,
            });
        }
        if let Some(at) = trimmed
            .char_indices()
            .find_map(|(at, c)| (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | ':' | '.' | '-' | '_')).then_some(at))
        {
            return Err(InvalidWorkloadIdentity::Character { what: "audience", at });
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The audience, for building a request.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a declared workload-identity value is not usable.
///
/// **The position is carried and the value is not**, for the reason every refusal about
/// operator-written text carries it: an audience is a foreign string heading for a
/// request, and it does not belong in a log.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidWorkloadIdentity {
    /// Nothing was written, or only whitespace was.
    #[error("the {what} is empty")]
    Empty { what: &'static str },
    /// Longer than the endpoint's ceiling.
    #[error("the {what} is {found} characters, and the ceiling is {most}")]
    TooLong { what: &'static str, found: usize, most: usize },
    /// A character outside the accepted set.
    #[error("the character at position {at} in the {what} is not allowed")]
    Character { what: &'static str, at: usize },
    /// A declared `expected_issuer` is not a usable `https` issuer.
    #[error("`sources.<alias>.workload_identity.expected_issuer` is not usable: {cause}")]
    ExpectedIssuer {
        #[source]
        cause: crate::InvalidInboundValue,
    },
    /// A declared `expected_audience` is not a usable provider audience.
    #[error("`sources.<alias>.workload_identity.expected_audience` is not usable: {cause}")]
    ExpectedAudience {
        #[source]
        cause: Box<Self>,
    },
    /// A declaration named ONE of the pool's two expectations. The twin-root link is a PAIR
    /// (`telekom/sutura#817`) - both or neither - because the boot comparison and the mint claim
    /// check both act only when both are present. A lone value is a documented expectation nothing
    /// enforces, which is exactly the gap #817 exists to close, so it is refused here at the
    /// boundary that can see the declaration whole.
    #[error(
        "`sources.<alias>.workload_identity` declares only one of `expected_issuer` and \
         `expected_audience` - the twin-root link needs both or neither, so a lone value is \
         refused rather than silently unenforced"
    )]
    PartialExpectation,
}

#[cfg(test)]
mod tests {
    use super::{InvalidWorkloadIdentity, WifAudience, WorkloadIdentityConfig};

    #[test]
    fn a_declared_workload_identity_parses_its_audience() {
        let id = WorkloadIdentityConfig::parse(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
        )
        .expect("a real-shaped declaration parses");
        assert!(id.audience().as_str().starts_with("//iam.googleapis.com/"));
    }

    #[test]
    fn an_audience_that_could_escape_a_request_is_refused_without_being_printed() {
        let err = WifAudience::parse("pool provider")
            .expect_err("a character outside the accepted set is refused")
            .to_string();
        assert!(!err.contains("provider"), "{err}");
    }

    #[test]
    fn declared_pool_expectations_parse_and_are_read_back() {
        // telekom/sutura#817's config seam: a source may declare the issuer and STS audience its
        // pool trusts. Absent keeps the bare exchange; present, they surface for the broker to link
        // to leg 1 at boot and to check the subject token against at mint.
        let id = WorkloadIdentityConfig::parse_with_expectations(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            Some("https://accounts.google.com"),
            Some("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/sutura/providers/oidc"),
        )
        .expect("a declared pool issuer and audience parse");
        assert_eq!(
            id.expected_issuer().map(crate::IssuerUrl::as_str),
            Some("https://accounts.google.com")
        );
        assert_eq!(
            id.expected_audience().map(super::WifAudience::as_str),
            Some("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/sutura/providers/oidc")
        );
    }

    #[test]
    fn a_declared_pool_issuer_that_is_not_https_is_refused() {
        // The pool issuer is an absolute `https` URI, parsed with the inbound `IssuerUrl` rules. A
        // value that is not one is refused at declaration, never accepted and then surprising.
        let err = WorkloadIdentityConfig::parse_with_expectations(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            Some("not-an-https-url"),
            Some("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/sutura/providers/oidc"),
        )
        .expect_err("a pool issuer that is not an absolute https URI is refused");
        assert!(matches!(err, super::InvalidWorkloadIdentity::ExpectedIssuer { .. }));
    }
    #[test]
    fn a_declared_lone_expectation_is_refused() {
        // A declaration that names ONLY ONE of the pool's two expectations is refused at parse,
        // not silently half-enforced: both the boot comparison and the mint claim check act only
        // when both halves are present (telekom/sutura#817).
        let err = WorkloadIdentityConfig::parse_with_expectations(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            Some("https://accounts.google.com"),
            None,
        )
        .expect_err("a lone expected_issuer without expected_audience is refused");
        assert!(matches!(err, super::InvalidWorkloadIdentity::PartialExpectation));
    }

    #[test]
    fn a_blank_audience_is_refused_as_empty_naming_the_audience() {
        assert_eq!(
            WifAudience::parse("  "),
            Err(InvalidWorkloadIdentity::Empty { what: "audience" })
        );
    }

    #[test]
    fn an_audience_one_past_its_bound_is_refused_as_too_long() {
        let over = "a".repeat(WifAudience::MOST.saturating_add(1));
        assert_eq!(
            WifAudience::parse(&over),
            Err(InvalidWorkloadIdentity::TooLong {
                what: "audience",
                found: over.len(),
                most: WifAudience::MOST,
            })
        );
        // The bound itself, not just one past it - the endpoint's own doc says up to
        // `WifAudience::MOST` characters is accepted, and `>` rather than `>=` is what makes that
        // true.
        assert!(
            WifAudience::parse(&"a".repeat(WifAudience::MOST)).is_ok(),
            "exactly the bound is an audience this deployment may present"
        );
    }
}
