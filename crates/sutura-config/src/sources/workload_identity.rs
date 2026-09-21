//! The Workload Identity Federation setup one `impersonation-at-source` source declares.
//!
//! **This process performs no exchange.** `audience` names the pool a subject's own assertion is
//! federated against, and the federating is Google's token service's, driven by the driver from the
//! `external_account` document `sutura_exec_bigquery`'s ADBC transport builds. A source that
//! executes as the asking subject has to say *which* pool receives that assertion, and that is the
//! source's declaration rather than this process's guess. See `docs/adr/0008` and `docs/adr/0018`'s
//! sixth amendment.
//!
//! **What each declared value actually reaches, because three of the four reach nothing.**
//! `audience` is sent. `scope` is parsed and sent nowhere: the credential document has no `scopes`
//! member, and the driver's own scope option means service-account impersonation, which this
//! transport does not do. `impersonate`'s KEYS decide which subjects a source may be served for and
//! its VALUES are read by nothing, because the pool resolves each subject to its own principal.
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

/// The OAuth scope an operator declares for the federated credential. Sent by nothing - see
/// [`WorkloadIdentityConfig::scope`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WifScope(String);

/// The Workload Identity Federation setup a `impersonation-at-source` source needs.
///
/// **`impersonate` decides WHO MAY BE SERVED, and nothing else.** A subject present as a key is the
/// only caller a source may be asked as; a caller absent from it is refused by
/// `sutura_exec_bigquery::DeclaredPrincipalBroker` before any network call rather than answered as
/// the process. **The declared account beside each key is read by nothing**: the credential document
/// this feeds carries no `service_account_impersonation_url`, so the federated credential IS the
/// pool principal and the pool resolves each subject to its own account. An empty map is refused
/// where it can become a startup failure, by `DeclaredPrincipals::parse`.
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
    scope: WifScope,
    impersonate: std::collections::BTreeMap<sutura_domain::identity::SubjectKey, WorkloadIdentitySa>,
    /// The issuer the pool trusts - the `iss` a subject token must carry for Google STS to accept
    /// it. See the type's own doc for why `None` is a value.
    expected_issuer: Option<crate::IssuerUrl>,
    /// The audience the pool's provider accepts - the STS audience a subject token must carry.
    expected_audience: Option<WifAudience>,
}

impl WorkloadIdentityConfig {
    /// Parses a declared audience, scope and impersonation map together.
    ///
    /// `impersonate` is read as raw strings rather than already-parsed types, for the reason
    /// `RawSource` carries every field as one: the settings tree speaks in strings, and parsing
    /// happens once, here.
    ///
    /// The map keys on [`sutura_domain::identity::SubjectKey`], the FULL verified subject - never on
    /// the masked [`SubjectId`](sutura_domain::identity::SubjectId) a record renders. Keying an
    /// authorization decision on the mask would hand every undeclared caller sharing a declared
    /// subject's mask that subject's declared service account; the full value is the only key on
    /// which two distinct subjects stay distinct. Two declared keys are refused if they compare
    /// equal, and a declared key that is empty or whitespace-only is refused as unusable - the same
    /// parse that guards every principal identifier.
    pub fn parse(
        audience: impl AsRef<str>,
        scope: impl AsRef<str>,
        impersonate: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, InvalidWorkloadIdentity> {
        Self::parse_with_expectations(audience, scope, impersonate, None, None)
    }

    /// Parses a declaration that also names what the pool trusts - telekom/sutura#817's seam.
    ///
    /// The two extra values are what make `expected_issuer()`/`expected_audience()` readable by the
    /// broker. Both are optional and parsed with the same rules as the values they must match on
    /// the inbound side: the issuer as an absolute `https` URI (the [`crate::IssuerUrl`] parse) and
    /// the audience as a provider resource (the [`WifAudience`] parse).
    pub fn parse_with_expectations(
        audience: impl AsRef<str>,
        scope: impl AsRef<str>,
        impersonate: &std::collections::BTreeMap<String, String>,
        expected_issuer: Option<&str>,
        expected_audience: Option<&str>,
    ) -> Result<Self, InvalidWorkloadIdentity> {
        let mut parsed = std::collections::BTreeMap::new();
        for (subject, target) in impersonate {
            let subject = sutura_domain::identity::SubjectKey::parse(subject)
                .map_err(|cause| InvalidWorkloadIdentity::ImpersonationSubject { cause })?;
            let target = WorkloadIdentitySa::parse(target)?;
            if parsed.insert(subject, target).is_some() {
                return Err(InvalidWorkloadIdentity::DuplicateImpersonationSubject);
            }
        }
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
            scope: WifScope::parse(scope.as_ref())?,
            impersonate: parsed,
            expected_issuer,
            expected_audience,
        })
    }

    /// The provider audience.
    #[inline]
    #[must_use]
    pub const fn audience(&self) -> &WifAudience {
        &self.audience
    }

    /// The scope the exchanged credential would carry, and **no transport in this build sends it.**
    ///
    /// Stated here because this is where an operator declares it. The shipped path federates the
    /// asker's own assertion through an `external_account` credential document, and the pinned
    /// driver has nowhere to put a scope: the document shape
    /// (`cloud.google.com/go/auth@v0.23.2`'s `credsfile::ExternalAccountFile`) has no `scopes`
    /// member, and the driver's own `bigquery.impersonate.scopes` option is read as a request for
    /// service-account impersonation, which replaces the federated credential rather than scoping
    /// it. The `BigQuery` client's own default scope applies instead.
    ///
    /// **Declared and unread, not declared and ignored** - the distinction is that this is the
    /// sentence an operator meets, so nobody reads a narrowed scope as a control that is in place.
    /// Removing the key is a settings break and a follow-up; misreporting it is a defect now.
    #[inline]
    #[must_use]
    pub const fn scope(&self) -> &WifScope {
        &self.scope
    }

    /// The declared subject -> service-account map, for the composition root to hand the broker.
    #[inline]
    #[must_use]
    pub const fn impersonate(&self) -> &std::collections::BTreeMap<sutura_domain::identity::SubjectKey, WorkloadIdentitySa> {
        &self.impersonate
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

/// The service account an operator declares beside a subject in `impersonate`.
///
/// **Parsed, and sent nowhere.** No consumer interpolates it: the credential document the shipped
/// transport builds carries no `service_account_impersonation_url`, so the pool resolves each
/// subject to its own principal and this value decides nothing -
/// `sutura_exec_bigquery::DeclaredPrincipals::names` reads the KEY beside it and not this. The
/// format is still checked at declaration, so a value that was always malformed does not become a
/// surprise on the day something reads it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkloadIdentitySa(String);

impl WorkloadIdentitySa {
    /// A service-account email is bounded by RFC 5321's mailbox length.
    const MOST: usize = 254;

    /// Parses a declared impersonation target.
    ///
    /// The accepted set is the printable ASCII a service-account email is built from - letters,
    /// digits and `. - _ @`, exactly one `@` - so a value that would escape a request path cannot
    /// exist here.
    pub fn parse(raw: &str) -> Result<Self, InvalidWorkloadIdentity> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(InvalidWorkloadIdentity::Empty {
                what: "impersonation target",
            });
        }
        if trimmed.chars().count() > Self::MOST {
            return Err(InvalidWorkloadIdentity::TooLong {
                what: "impersonation target",
                found: trimmed.chars().count(),
                most: Self::MOST,
            });
        }
        if trimmed.matches('@').count() != 1 {
            return Err(InvalidWorkloadIdentity::NotAnAccount);
        }
        if let Some(at) = trimmed
            .char_indices()
            .find_map(|(at, c)| (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | '@')).then_some(at))
        {
            return Err(InvalidWorkloadIdentity::Character {
                what: "impersonation target",
                at,
            });
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The account, for building a request.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
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

impl WifScope {
    /// The longest a scope may be. Larger than an audience because a scope is a URL.
    const MOST: usize = 1024;

    /// Parses a scope.
    ///
    /// A scope is a URL (`https://www.googleapis.com/auth/bigquery.readonly`), so it allows the `%`
    /// and letters a URL does rather than the narrower set an audience does. Same bound, same reason:
    /// it belongs in a request and a refusal should never log it raw.
    pub fn parse(raw: &str) -> Result<Self, InvalidWorkloadIdentity> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(InvalidWorkloadIdentity::Empty { what: "scope" });
        }
        if trimmed.chars().count() > Self::MOST {
            return Err(InvalidWorkloadIdentity::TooLong {
                what: "scope",
                found: trimmed.chars().count(),
                most: Self::MOST,
            });
        }
        if let Some(at) = trimmed.char_indices().find_map(|(at, c)| {
            (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | ':' | '.' | '-' | '_' | '%')).then_some(at)
        }) {
            return Err(InvalidWorkloadIdentity::Character { what: "scope", at });
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The scope, for building a request.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a declared workload-identity value is not usable.
///
/// **The position is carried and the value is not**, for the reason every refusal about
/// operator-written text carries it: an audience and a scope are foreign strings heading for a
/// request, and neither belongs in a log.
///
/// **No `Clone`**, for the reason `InvalidSourceRegistry` (`crate::sources`) already gives: its own
/// `ImpersonationSubject` variant's cause is `sutura_domain::identity::InvalidPrincipalId`, which is
/// not `Clone` either - nothing needs to clone a startup refusal.
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
    /// An `impersonate` target has no `@`, or more than one - so it is not an account.
    #[error("an `impersonate` target is not a service-account email")]
    NotAnAccount,
    /// An `impersonate` key is not a usable principal identifier.
    #[error("a declared `impersonate` subject is not a usable identifier")]
    ImpersonationSubject {
        #[source]
        cause: sutura_domain::identity::InvalidPrincipalId,
    },
    /// Two declared `impersonate` keys compare equal - the same subject declared twice, with no way
    /// to tell which service account was meant.
    #[error("two declared `impersonate` subjects are the same subject")]
    DuplicateImpersonationSubject,
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
    use super::{WifAudience, WorkloadIdentityConfig, WorkloadIdentitySa};

    #[test]
    fn a_declared_workload_identity_parses_both_halves() {
        let id = WorkloadIdentityConfig::parse(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            "https://www.googleapis.com/auth/bigquery.readonly",
            &std::collections::BTreeMap::new(),
        )
        .expect("a real-shaped declaration parses");
        assert!(id.audience().as_str().starts_with("//iam.googleapis.com/"));
        assert_eq!(id.scope().as_str(), "https://www.googleapis.com/auth/bigquery.readonly");
        assert!(
            id.impersonate().is_empty(),
            "an entry with no `impersonate` map keeps a bare exchange"
        );
    }

    #[test]
    fn a_declared_impersonation_map_parses_subject_and_target() {
        let mut declared = std::collections::BTreeMap::new();
        drop(declared.insert(
            String::from("principal-a@example.com"),
            String::from("principal-a@acme-analytics.iam.gserviceaccount.com"),
        ));
        let id = WorkloadIdentityConfig::parse(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            "https://www.googleapis.com/auth/bigquery.readonly",
            &declared,
        )
        .expect("a real-shaped impersonation map parses");
        let subject = sutura_domain::identity::SubjectKey::parse("principal-a@example.com").expect("a test subject is a subject");
        assert_eq!(
            id.impersonate().get(&subject).map(WorkloadIdentitySa::as_str),
            Some("principal-a@acme-analytics.iam.gserviceaccount.com")
        );
    }

    #[test]
    fn an_impersonation_target_with_no_at_sign_is_refused() {
        let mut declared = std::collections::BTreeMap::new();
        drop(declared.insert(String::from("principal-a@example.com"), String::from("not-an-account")));
        let err = WorkloadIdentityConfig::parse(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            "https://www.googleapis.com/auth/bigquery.readonly",
            &declared,
        )
        .expect_err("a target with no `@` is not an account");
        assert!(matches!(err, super::InvalidWorkloadIdentity::NotAnAccount));
    }

    #[test]
    fn an_audience_that_could_escape_a_request_is_refused_without_being_printed() {
        let err = WifAudience::parse("pool provider")
            .expect_err("a character outside the accepted set is refused")
            .to_string();
        assert!(!err.contains("provider"), "{err}");
    }

    #[test]
    fn two_declared_keys_that_compare_equal_after_parse_are_a_refusal_not_a_silent_overwrite() {
        // The map keys on the FULL verified subject, and `parse` trims each declared key the way
        // `parse_principal_id` does - so two RAW keys that differ only by surrounding whitespace
        // ("principal-a@example.com" and " principal-a@example.com ") are distinct entries in the
        // settings map yet compare equal once parsed. That collision is refused rather than silently
        // keeping whichever came last. (Identical raw strings can never reach this code - the
        // settings `BTreeMap` already collapses them.)
        let mut declared = std::collections::BTreeMap::new();
        drop(declared.insert(
            String::from("principal-a@example.com"),
            String::from("principal-a@acme-analytics.iam.gserviceaccount.com"),
        ));
        drop(declared.insert(
            String::from(" principal-a@example.com "),
            String::from("other-sa@acme-analytics.iam.gserviceaccount.com"),
        ));
        let err = WorkloadIdentityConfig::parse(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            "https://www.googleapis.com/auth/bigquery.readonly",
            &declared,
        )
        .expect_err("two declared subjects that compare equal after parse are refused");
        assert!(matches!(err, super::InvalidWorkloadIdentity::DuplicateImpersonationSubject));
    }

    #[test]
    fn a_whitespace_only_declared_key_is_refused() {
        let mut declared = std::collections::BTreeMap::new();
        drop(declared.insert(
            String::from("   "),
            String::from("principal-a@acme-analytics.iam.gserviceaccount.com"),
        ));
        let err = WorkloadIdentityConfig::parse(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            "https://www.googleapis.com/auth/bigquery.readonly",
            &declared,
        )
        .expect_err("a whitespace-only declared subject is not an identifier");
        assert!(matches!(err, super::InvalidWorkloadIdentity::ImpersonationSubject { .. }));
    }

    #[test]
    fn an_over_long_declared_subject_is_refused_as_not_an_identifier() {
        // The IdP-subject shape the map keys on is a `SubjectKey` like any other, and the config
        // boundary refuses an oversized one the same way it refuses an empty or duplicate one:
        // `SubjectKey::parse` bounds the identifier to 256 characters (`parse_principal_id`), and
        // that refusal surfaces here as `ImpersonationSubject` rather than being accepted into the
        // map. Sized past the ceiling exactly so the bound is what is exercised, not a parse-clean
        // long string that happens to be refused for another reason.
        let oversized = format!("{}@idp.example", "a".repeat(300));
        let mut declared = std::collections::BTreeMap::new();
        drop(declared.insert(oversized, String::from("sa-declared@acme-analytics.iam.gserviceaccount.com")));
        let err = WorkloadIdentityConfig::parse(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            "https://www.googleapis.com/auth/bigquery.readonly",
            &declared,
        )
        .expect_err("an over-long declared subject is not a usable identifier");
        assert!(matches!(err, super::InvalidWorkloadIdentity::ImpersonationSubject { .. }));
    }

    #[test]
    fn declared_pool_expectations_parse_and_are_read_back() {
        // telekom/sutura#817's config seam: a source may declare the issuer and STS audience its
        // pool trusts. Absent keeps the bare exchange; present, they surface for the broker to link
        // to leg 1 at boot and to check the subject token against at mint.
        let id = WorkloadIdentityConfig::parse_with_expectations(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            "https://www.googleapis.com/auth/bigquery.readonly",
            &std::collections::BTreeMap::new(),
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
            "https://www.googleapis.com/auth/bigquery.readonly",
            &std::collections::BTreeMap::new(),
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
            "https://www.googleapis.com/auth/bigquery.readonly",
            &std::collections::BTreeMap::new(),
            Some("https://accounts.google.com"),
            None,
        )
        .expect_err("a lone expected_issuer without expected_audience is refused");
        assert!(matches!(err, super::InvalidWorkloadIdentity::PartialExpectation));
    }
}
