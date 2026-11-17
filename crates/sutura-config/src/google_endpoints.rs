//! Google API destinations, parsed before any caller credential can be sent.

use crate::raw::RawGoogleEndpoints;

/// An HTTPS URL under Google's API DNS suffix, without URL credentials, query or fragment.
/// This restricts the destination name; DNS routing and TLS trust remain the operator's boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleApiEndpoint(String);

impl GoogleApiEndpoint {
    fn parse(key: &'static str, raw: &str) -> Result<Self, InvalidGoogleEndpoint> {
        let refused = || InvalidGoogleEndpoint { key, cause: None };
        let url = url::Url::parse(raw).map_err(|cause| InvalidGoogleEndpoint { key, cause: Some(cause) })?;
        if raw.trim() != raw
            || url.scheme() != "https"
            || !url.host_str().is_some_and(|host| host.ends_with(".googleapis.com"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path().contains('@')
        {
            return Err(refused());
        }
        Ok(Self(url.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The key alone is rendered: a mistaken URL may itself contain a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("`google_endpoints.{key}` must be an HTTPS URL under .googleapis.com without credentials, query or fragment")]
pub struct InvalidGoogleEndpoint {
    key: &'static str,
    #[source]
    cause: Option<url::ParseError>,
}

/// Explicit Google destinations shared by serving nodes and reading workers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleEndpoints {
    bigquery: GoogleApiEndpoint,
    bigquery_storage: GoogleApiEndpoint,
    sts: GoogleApiEndpoint,
    iam_credentials: GoogleApiEndpoint,
    oauth_token: GoogleApiEndpoint,
    tokeninfo: GoogleApiEndpoint,
}

impl Default for GoogleEndpoints {
    fn default() -> Self {
        Self {
            bigquery: GoogleApiEndpoint("https://bigquery.googleapis.com/bigquery/v2/".into()),
            bigquery_storage: GoogleApiEndpoint("https://bigquerystorage.googleapis.com/".into()),
            sts: GoogleApiEndpoint("https://sts.googleapis.com/v1/token".into()),
            iam_credentials: GoogleApiEndpoint("https://iamcredentials.googleapis.com/".into()),
            oauth_token: GoogleApiEndpoint("https://oauth2.googleapis.com/token".into()),
            tokeninfo: GoogleApiEndpoint("https://oauth2.googleapis.com/tokeninfo".into()),
        }
    }
}

impl GoogleEndpoints {
    pub(crate) fn parse(raw: &RawGoogleEndpoints) -> Result<Self, InvalidGoogleEndpoint> {
        let mut endpoints = Self::default();
        for (key, written, target) in [
            ("bigquery", &raw.bigquery, &mut endpoints.bigquery),
            ("bigquery_storage", &raw.bigquery_storage, &mut endpoints.bigquery_storage),
            ("sts", &raw.sts, &mut endpoints.sts),
            ("iam_credentials", &raw.iam_credentials, &mut endpoints.iam_credentials),
            ("oauth_token", &raw.oauth_token, &mut endpoints.oauth_token),
            ("tokeninfo", &raw.tokeninfo, &mut endpoints.tokeninfo),
        ] {
            if let Some(written) = written {
                *target = GoogleApiEndpoint::parse(key, written)?;
            }
        }
        for (key, endpoint) in [
            ("bigquery_storage", &endpoints.bigquery_storage),
            ("iam_credentials", &endpoints.iam_credentials),
        ] {
            let url = url::Url::parse(endpoint.as_str()).map_err(|cause| InvalidGoogleEndpoint { key, cause: Some(cause) })?;
            if url.path() != "/" {
                return Err(InvalidGoogleEndpoint { key, cause: None });
            }
        }
        Ok(endpoints)
    }

    #[must_use]
    pub const fn bigquery(&self) -> &GoogleApiEndpoint {
        &self.bigquery
    }
    #[must_use]
    pub const fn bigquery_storage(&self) -> &GoogleApiEndpoint {
        &self.bigquery_storage
    }
    #[must_use]
    pub const fn sts(&self) -> &GoogleApiEndpoint {
        &self.sts
    }
    #[must_use]
    pub const fn iam_credentials(&self) -> &GoogleApiEndpoint {
        &self.iam_credentials
    }
    #[must_use]
    pub const fn oauth_token(&self) -> &GoogleApiEndpoint {
        &self.oauth_token
    }
    #[must_use]
    pub const fn tokeninfo(&self) -> &GoogleApiEndpoint {
        &self.tokeninfo
    }
}
