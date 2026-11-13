//! The configured verifier that admitted a signed caller.

/// A stable, case-sensitive configuration name; never an issuer URL or a subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VerifierId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidVerifierId {
    #[error("a verifier id is between 1 and 63 ASCII letters, digits, `_` or `-`")]
    Shape,
}

impl VerifierId {
    pub fn parse(raw: &str) -> Result<Self, InvalidVerifierId> {
        if raw.is_empty()
            || raw.len() > 63
            || !raw
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(InvalidVerifierId::Shape);
        }
        Ok(Self(raw.to_owned()))
    }

    /// The implicit name of a legacy single-object declaration.
    #[must_use]
    pub fn primary() -> Self {
        Self(String::from("primary"))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for VerifierId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether admission used the caller's direct credential or a gateway assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifierMode {
    Direct,
    Gateway,
}

impl VerifierMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Gateway => "behind-gateway",
        }
    }
}

/// Admission provenance, copied from configuration after signature verification.
///
/// No wire deserialization. This records admission; it does not prove source impersonation.
/// One immutable declaration is shared across request contexts without copying issuer text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittingVerifier {
    declaration: std::sync::Arc<VerifierDeclaration>,
}

#[derive(Debug, PartialEq, Eq)]
struct VerifierDeclaration {
    id: VerifierId,
    issuer: String,
    mode: VerifierMode,
}

impl AdmittingVerifier {
    #[must_use]
    pub fn of(id: VerifierId, issuer: String, mode: VerifierMode) -> Self {
        Self {
            declaration: std::sync::Arc::new(VerifierDeclaration { id, issuer, mode }),
        }
    }

    #[must_use]
    pub fn id(&self) -> &VerifierId {
        &self.declaration.id
    }

    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.declaration.issuer
    }

    #[must_use]
    pub fn mode(&self) -> VerifierMode {
        self.declaration.mode
    }
}

#[cfg(test)]
mod tests {
    use super::VerifierId;

    #[test]
    fn verifier_names_are_bounded_literal_and_case_sensitive() {
        for raw in ["", " primary", "provider/url", "provider.id", "p{a}"] {
            VerifierId::parse(raw).expect_err("the verifier id is invalid");
        }
        VerifierId::parse(&"a".repeat(64)).expect_err("a verifier id is at most 63 bytes");
        VerifierId::parse(&"a".repeat(63)).expect("the maximum verifier id length is accepted");
        assert_eq!(VerifierId::parse("primary").unwrap(), VerifierId::primary());
        assert_ne!(VerifierId::parse("Staff").unwrap(), VerifierId::parse("staff").unwrap());
    }
}
