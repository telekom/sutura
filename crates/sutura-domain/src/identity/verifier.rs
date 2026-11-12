//! The configured verifier that admitted a signed caller.

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
    issuer: String,
    mode: VerifierMode,
}

impl AdmittingVerifier {
    #[must_use]
    pub fn of(issuer: String, mode: VerifierMode) -> Self {
        Self {
            declaration: std::sync::Arc::new(VerifierDeclaration { issuer, mode }),
        }
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
