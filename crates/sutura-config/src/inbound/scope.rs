//! Where a caller's scopes come from, beside a token's own `scope` claim.
//!
//! Both are configuration, never the question: a deployment names the claims it reads in `direct`,
//! or the scopes a gateway's scopeless assertion carries in `behind-gateway`. Neither widens what a
//! token that states its own `scope` is granted.

use std::collections::BTreeSet;

use crate::inbound::InvalidInboundValue;

/// The bounds `sutura-http` puts on a token's scopes, so a grant is one a token could have carried.
const MAX_SCOPE_LENGTH: usize = 128;
const MAX_SCOPES: usize = 64;

const SCOPE_CHARACTERS: &str = "printable ASCII other than space, `\"` and `\\` (RFC 6749 `scope-token`)";

/// Which claims beside `scope` grant scopes in `direct`: Entra's `scp` for a delegated token and
/// `roles` for an app-only one. A closed set, and empty unless the deployment names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExtraScopeClaims {
    scp: bool,
    roles: bool,
}

impl ExtraScopeClaims {
    pub const KEY: &'static str = "security.inbound.extra_scope_claims";
    const ACCEPTED: &'static str = "scp, roles";

    /// Reads `scope` alone - what a deployment that names no extra claim gets.
    #[inline]
    #[must_use]
    pub const fn none() -> Self {
        Self {
            scp: false,
            roles: false,
        }
    }

    /// Reads the written list. Empty is refused: a key written with nothing in it is not absent.
    pub fn parse(written: &[String]) -> Result<Self, InvalidInboundValue> {
        if written.is_empty() {
            return Err(InvalidInboundValue::Empty { key: Self::KEY });
        }
        let mut claims = Self::none();
        for (index, name) in written.iter().enumerate() {
            match name.trim() {
                "scp" => claims.scp = true,
                "roles" => claims.roles = true,
                _ => {
                    return Err(InvalidInboundValue::NotANamedValue {
                        key: Self::KEY,
                        index,
                        accepted: Self::ACCEPTED,
                    });
                }
            }
        }
        Ok(claims)
    }

    #[inline]
    #[must_use]
    pub const fn reads_scp(self) -> bool {
        self.scp
    }

    #[inline]
    #[must_use]
    pub const fn reads_roles(self) -> bool {
        self.roles
    }
}

/// The scopes a `behind-gateway` assertion carries when it has no `scope` claim.
///
/// Every subject the transit issuer signs for this audience without a `scope` gets them, so they are
/// printed at start-up. An assertion that states a `scope`, an empty one included, gets only that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantedScopes(BTreeSet<String>);

impl GrantedScopes {
    pub const KEY: &'static str = "security.inbound.granted_scopes";

    /// Reads the written list: not empty, each value an RFC 6749 `scope-token`, inside the bounds a
    /// token's own scopes are held to. Duplicates collapse, as they do in a token.
    pub fn parse(written: &[String]) -> Result<Self, InvalidInboundValue> {
        let key = Self::KEY;
        if written.is_empty() {
            return Err(InvalidInboundValue::Empty { key });
        }
        let mut granted = BTreeSet::new();
        for scope in written {
            let scope = scope.trim();
            if scope.is_empty() {
                return Err(InvalidInboundValue::Empty { key });
            }
            let found = scope.chars().count();
            if found > MAX_SCOPE_LENGTH {
                return Err(InvalidInboundValue::TooLong {
                    key,
                    found,
                    limit: MAX_SCOPE_LENGTH,
                });
            }
            if let Some(position) = scope
                .chars()
                .position(|character| !character.is_ascii_graphic() || character == '"' || character == '\\')
            {
                return Err(InvalidInboundValue::NotPermitted {
                    key,
                    position,
                    accepted: SCOPE_CHARACTERS,
                });
            }
            let _newly_granted = granted.insert(String::from(scope));
            if granted.len() > MAX_SCOPES {
                return Err(InvalidInboundValue::TooMany { key, limit: MAX_SCOPES });
            }
        }
        Ok(Self(granted))
    }

    #[must_use]
    pub fn contains(&self, scope: &str) -> bool {
        self.0.contains(scope)
    }

    /// Every scope granted, in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &str> + '_ {
        self.0.iter().map(String::as_str)
    }
}

/// Space-separated, the way a `scope` claim is written.
impl core::fmt::Display for GrantedScopes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (index, scope) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            f.write_str(scope)?;
        }
        Ok(())
    }
}
