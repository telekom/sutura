//! The token-exchange setup one `impersonation-at-source` source declares.
//!
//! **This is the tape a subject's own credential is exchanged against** - RFC 8693 handed to a
//! Workload Identity Federation provider. A source that executes as the asking subject has to say
//! *which* provider receives the subject's token and *what the exchanged credential may do*, and
//! both are that source's declaration rather than this process's guess. See `docs/adr/0008` and the
//! issue that wired the adapter that presents one.
//!
//! The two newtypes are declared here, in the settings tree that owns the value, and the broker that
//! performs the exchange holds its own copies in the adapter that links it - the same reason
//! [`BillingProject`] is checked both here and in the transport that interpolates
//! it: an adapter may not depend on the settings tree, so the format is checked where it is declared
//! AND where it is sent.

/// The audience a subject token is exchanged for: a workload identity provider resource.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WifAudience(String);

/// The OAuth scope the exchanged credential is minted for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WifScope(String);

/// The token-exchange setup a `impersonation-at-source` source needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadIdentityConfig {
    audience: WifAudience,
    scope: WifScope,
}

impl WorkloadIdentityConfig {
    /// Parses a declared audience and scope together, since neither is usable alone.
    pub fn parse(audience: impl AsRef<str>, scope: impl AsRef<str>) -> Result<Self, InvalidWorkloadIdentity> {
        Ok(Self {
            audience: WifAudience::parse(audience.as_ref())?,
            scope: WifScope::parse(scope.as_ref())?,
        })
    }

    /// The provider audience.
    #[inline]
    #[must_use]
    pub const fn audience(&self) -> &WifAudience {
        &self.audience
    }

    /// The scope the exchanged credential carries.
    #[inline]
    #[must_use]
    pub const fn scope(&self) -> &WifScope {
        &self.scope
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
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
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
}

#[cfg(test)]
mod tests {
    use super::{WifAudience, WorkloadIdentityConfig};

    #[test]
    fn a_declared_workload_identity_parses_both_halves() {
        let id = WorkloadIdentityConfig::parse(
            "//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso",
            "https://www.googleapis.com/auth/bigquery.readonly",
        )
        .expect("a real-shaped declaration parses");
        assert!(id.audience().as_str().starts_with("//iam.googleapis.com/"));
        assert_eq!(id.scope().as_str(), "https://www.googleapis.com/auth/bigquery.readonly");
    }

    #[test]
    fn an_audience_that_could_escape_a_request_is_refused_without_being_printed() {
        let err = WifAudience::parse("pool provider")
            .expect_err("a character outside the accepted set is refused")
            .to_string();
        assert!(!err.contains("provider"), "{err}");
    }
}
