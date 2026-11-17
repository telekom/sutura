//! Named verifiers and their literal MCP mounts.

use sutura_domain::identity::VerifierId;

use super::{InboundIdentity, TokenRequirement};

/// A literal route path, with no captures, escaping, query or trailing slash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct McpPath(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidMcpPath {
    #[error(
        "an MCP path is an absolute literal path, at most 256 bytes, with nonempty segments of ASCII letters, digits, `-`, `_`, `.`, or `~`; `.` and `..` segments are refused"
    )]
    Shape,
}

impl McpPath {
    pub fn parse(raw: &str) -> Result<Self, InvalidMcpPath> {
        let Some(tail) = raw.strip_prefix('/') else {
            return Err(InvalidMcpPath::Shape);
        };
        if raw.len() > 256
            || tail.split('/').any(|segment| {
                segment.is_empty()
                    || matches!(segment, "." | "..")
                    || !segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
            })
        {
            return Err(InvalidMcpPath::Shape);
        }
        Ok(Self(raw.to_owned()))
    }

    #[must_use]
    pub fn conventional() -> Self {
        Self(String::from("/mcp"))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether either literal subtree contains the other.
    #[must_use]
    pub fn overlaps(&self, other: &str) -> bool {
        let path = self.as_str();
        path == other
            || path.strip_prefix(other).is_some_and(|tail| tail.starts_with('/'))
            || other.strip_prefix(path).is_some_and(|tail| tail.starts_with('/'))
    }
}

impl core::fmt::Display for McpPath {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One complete verifier; the mode retains its own token policy and key source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundVerifier {
    id: VerifierId,
    path: McpPath,
    policy: VerifierPolicy,
    legacy_discovery: bool,
}

impl InboundVerifier {
    #[must_use]
    pub const fn new(id: VerifierId, path: McpPath, identity: InboundIdentity) -> Self {
        Self {
            id,
            path,
            policy: VerifierPolicy::Jwt(identity),
            legacy_discovery: false,
        }
    }

    /// A legacy object retains its resource-derived discovery URL.
    #[must_use]
    pub const fn legacy(id: VerifierId, path: McpPath, identity: InboundIdentity) -> Self {
        Self {
            id,
            path,
            policy: VerifierPolicy::Jwt(identity),
            legacy_discovery: true,
        }
    }

    /// Named-list discovery is bound to this mount. Legacy discovery retains its former URI.
    #[must_use]
    pub const fn metadata_path(&self) -> Option<&McpPath> {
        if self.legacy_discovery { None } else { Some(&self.path) }
    }

    #[must_use]
    pub const fn id(&self) -> &VerifierId {
        &self.id
    }

    #[must_use]
    pub const fn path(&self) -> &McpPath {
        &self.path
    }

    #[must_use]
    pub const fn identity(&self) -> Option<&InboundIdentity> {
        match &self.policy {
            VerifierPolicy::Jwt(identity) => Some(identity),
            VerifierPolicy::Google(_) => None,
        }
    }
    #[must_use]
    pub const fn google(&self) -> Option<&super::google::GoogleTokenInfo> {
        match &self.policy {
            VerifierPolicy::Google(google) => Some(google),
            VerifierPolicy::Jwt(_) => None,
        }
    }
    #[must_use]
    pub const fn with_google(id: VerifierId, path: McpPath, google: super::google::GoogleTokenInfo) -> Self {
        Self {
            id,
            path,
            policy: VerifierPolicy::Google(google),
            legacy_discovery: false,
        }
    }
    #[must_use]
    pub fn mode(&self) -> &'static str {
        self.identity().map_or("direct", InboundIdentity::mode)
    }
    #[must_use]
    pub const fn is_direct(&self) -> bool {
        self.google().is_some() || matches!(self.identity(), Some(InboundIdentity::Direct { .. }))
    }
    #[must_use]
    pub const fn resource(&self) -> Option<&super::ResourceIdentifier> {
        match &self.policy {
            VerifierPolicy::Google(google) => Some(google.resource()),
            VerifierPolicy::Jwt(identity) => identity.requirement().resource(),
        }
    }
    #[must_use]
    pub const fn issuer(&self) -> &super::IssuerUrl {
        match &self.policy {
            VerifierPolicy::Google(google) => google.issuer(),
            VerifierPolicy::Jwt(identity) => identity.requirement().issuer(),
        }
    }
    #[must_use]
    pub const fn audience(&self) -> &super::TokenAudience {
        match &self.policy {
            VerifierPolicy::Google(google) => google.client(),
            VerifierPolicy::Jwt(identity) => identity.requirement().audience(),
        }
    }
    #[must_use]
    pub fn reads_the_authorization_header(&self) -> bool {
        self.identity().is_none_or(InboundIdentity::reads_the_authorization_header)
    }
    #[must_use]
    pub fn who_authenticated(&self) -> &'static str {
        self.identity().map_or(
            "Google verified the caller's access token for the dedicated client",
            InboundIdentity::who_authenticated,
        )
    }

    #[must_use]
    pub fn requirement(&self) -> Option<TokenRequirement<'_>> {
        self.identity().map(InboundIdentity::requirement)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VerifierPolicy {
    Jwt(InboundIdentity),
    Google(super::google::GoogleTokenInfo),
}
