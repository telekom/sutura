//! Direct verification of opaque Google access tokens; no JWT key source is implied.

use super::{ClientId, GrantedScopes, InvalidInboundValue, IssuerUrl, MaxTokenBytes, ResourceIdentifier, TokenAudience};
use crate::google_endpoints::{GoogleApiEndpoint, InvalidGoogleEndpoint};
use crate::raw::RawInbound;
use crate::sources::address_domains::{AddressDomains, InvalidAddressDomains};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidGoogleVerifier {
    #[error("google_tokeninfo requires `{key}`")]
    Required { key: &'static str },
    #[error("google_tokeninfo refuses `{key}`; it does not validate JWTs or gateway assertions")]
    Inapplicable { key: &'static str },
    #[error("google_tokeninfo is direct only and requires https://accounts.google.com as authorization_server")]
    ModeOrIssuer,
    #[error(transparent)]
    Value(#[from] InvalidInboundValue),
    #[error(transparent)]
    Endpoint(#[from] InvalidGoogleEndpoint),
    #[error(transparent)]
    Domains(#[from] InvalidAddressDomains),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleTokenInfo {
    resource: ResourceIdentifier,
    issuer: IssuerUrl,
    client: TokenAudience,
    hosted_domains: Option<AddressDomains>,
    required_scopes: GrantedScopes,
    endpoint: GoogleApiEndpoint,
}

impl GoogleTokenInfo {
    pub(crate) fn parse(raw: &RawInbound, default_endpoint: &GoogleApiEndpoint) -> Result<Self, InvalidGoogleVerifier> {
        if raw.mode.as_deref() != Some("direct") || raw.authorization_server.as_deref() != Some("https://accounts.google.com") {
            return Err(InvalidGoogleVerifier::ModeOrIssuer);
        }
        for (present, key) in [
            (raw.key_set_file.is_some(), "key_set_file"),
            (raw.key_set_url.is_some(), "key_set_url"),
            (!raw.algorithms.is_empty(), "algorithms"),
            (raw.key_set_max_stale_seconds.is_some(), "key_set_max_stale_seconds"),
            (raw.token_type.is_some() || raw.accept_any_token_type, "token_type"),
            (raw.audience.is_some(), "audience"),
            (raw.extra_scope_claims.is_some(), "extra_scope_claims"),
            (
                raw.transit_header.is_some()
                    || raw.transit_issuer.is_some()
                    || raw.transit_audience.is_some()
                    || raw.transit_token_type.is_some()
                    || raw.transit_max_lifetime_seconds.is_some()
                    || raw.accept_long_transit_lifetime
                    || raw.transit_single_use,
                "transit",
            ),
            (raw.granted_scopes.is_some(), "granted_scopes"),
            (raw.address_claim.is_some() || raw.accept_address_claim, "address_claim"),
        ] {
            if present {
                return Err(InvalidGoogleVerifier::Inapplicable { key });
            }
        }
        let resource = ResourceIdentifier::parse(required(raw.resource.as_deref(), "resource")?)?
            .limiting_token_bytes(raw.max_token_bytes.map_or(Ok(MaxTokenBytes::DEFAULT), MaxTokenBytes::parse)?);
        let client = required(raw.client_id.as_deref(), "client_id")?;
        let _checked = ClientId::parse("security.inbound.client_id", client)?;
        let required_scopes = raw
            .required_scopes
            .as_deref()
            .ok_or(InvalidGoogleVerifier::Required { key: "required_scopes" })?;
        Ok(Self {
            resource,
            issuer: IssuerUrl::parse("https://accounts.google.com")?,
            client: TokenAudience::parse(client)?,
            hosted_domains: raw.hosted_domains.as_deref().map(AddressDomains::parse).transpose()?,
            required_scopes: GrantedScopes::parse(required_scopes)?,
            endpoint: raw
                .tokeninfo_endpoint
                .as_deref()
                .map(|endpoint| GoogleApiEndpoint::parse("tokeninfo", endpoint))
                .transpose()?
                .unwrap_or_else(|| default_endpoint.clone()),
        })
    }
    #[must_use]
    pub const fn max_token_bytes(&self) -> MaxTokenBytes {
        self.resource.max_token_bytes()
    }
    #[must_use]
    pub const fn resource(&self) -> &ResourceIdentifier {
        &self.resource
    }
    #[must_use]
    pub const fn issuer(&self) -> &IssuerUrl {
        &self.issuer
    }
    #[must_use]
    pub const fn client(&self) -> &TokenAudience {
        &self.client
    }
    #[must_use]
    pub const fn required_scopes(&self) -> &GrantedScopes {
        &self.required_scopes
    }
    #[must_use]
    pub const fn endpoint(&self) -> &GoogleApiEndpoint {
        &self.endpoint
    }
    #[must_use]
    pub const fn hosted_domains(&self) -> Option<&AddressDomains> {
        self.hosted_domains.as_ref()
    }
}

fn required<'a>(value: Option<&'a str>, key: &'static str) -> Result<&'a str, InvalidGoogleVerifier> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(InvalidGoogleVerifier::Required { key })
}
