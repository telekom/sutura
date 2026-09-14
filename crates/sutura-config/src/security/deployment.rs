//! The deployment identity declaration - who a query runs as when one is declared.
//!
//! A file of its own for the same mechanical reason its neighbours are: `security.rs` is at
//! the 1000-line cap `cargo xtask max-lines` enforces, and the deployment-identity group is a
//! self-contained declaration re-exported from [`crate::security`].

use sutura_domain::source::{AcknowledgementReason, InvalidOperatorText};

/// # The variant names are not the configured words, and that is deliberate
///
/// A deployment writes `single-user` or `multi-user` - [`Self::as_str`] and [`Self::NAMES`] own those
/// spellings, and they are the vocabulary
/// [a credential per leg](https://github.com/telekom/sutura/blob/main/docs/adr/0008-a-credential-per-leg-for-the-calling-subject.md)
/// 5a names. The variants are named for the *property each mode decides* instead, because
/// `SingleUser`/`MultiUser` share a postfix and `clippy::enum_variant_names` is denied - and the names
/// that survived that say more: what changes between the two is whether credentials are static
/// configuration or a subject arrives per request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentIdentity {
    /// Static credentials, one user, one host - the `single-user` mode. Carries the operator's own
    /// reason, so the mode is unreachable by leaving a key out.
    StaticCredentials { declared: AcknowledgementReason },
    /// A subject per request, established by the transport - the `multi-user` mode.
    ///
    /// **Nothing establishes one today** - the bearer gate authenticates the deployment - so this mode
    /// is currently a statement of intent whose only mechanical effect is that every shared source has
    /// to be acknowledged on its own entry. That is the honest description and it is worth having: the
    /// acknowledgements are what a deployment needs in place *before* a subject arrives, not after.
    SubjectPerRequest,
}

/// The configured value did not name a deployment mode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not name a deployment mode - one of: {}", DeploymentIdentity::NAMES.join(", "))]
pub struct UnknownDeploymentIdentity {
    found: String,
}

impl DeploymentIdentity {
    /// The key the mode is written under.
    pub const KEY: &'static str = "security.identity";
    /// The key the single-user reason is written under.
    pub const REASON_KEY: &'static str = "security.single_user_because";
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["single-user", "multi-user"];

    /// Reads the declared mode and, for single-user, the operator's reason.
    ///
    /// The reason is **required** for single-user and **refused** for multi-user, which is the same
    /// rule `server.tls_certificate` gets: a value nothing reads is a control that appears to be in
    /// place. Both halves are returned as one typed error rather than checked later, because the mode
    /// and its witness are one declaration.
    pub fn parse(word: &str, reason: Option<&str>) -> Result<Self, InvalidDeploymentIdentity> {
        let reason = reason.map(str::trim).filter(|value| !value.is_empty());
        match word.trim() {
            "single-user" => {
                let Some(text) = reason else {
                    return Err(InvalidDeploymentIdentity::SingleUserWithoutAReason);
                };
                let declared = AcknowledgementReason::written_under(Self::REASON_KEY, text)
                    .map_err(|cause| InvalidDeploymentIdentity::Reason { cause })?;
                Ok(Self::StaticCredentials { declared })
            }
            "multi-user" => {
                if reason.is_some() {
                    return Err(InvalidDeploymentIdentity::ReasonWithoutSingleUser);
                }
                Ok(Self::SubjectPerRequest)
            }
            other => Err(InvalidDeploymentIdentity::Unknown {
                cause: UnknownDeploymentIdentity {
                    found: String::from(other),
                },
            }),
        }
    }

    /// The spelling, for the startup log.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match *self {
            Self::StaticCredentials { .. } => "single-user",
            Self::SubjectPerRequest => "multi-user",
        }
    }

    /// The reason a shared source may borrow as its acknowledgement, if this mode supplies one.
    ///
    /// `Some` for single-user only, and an exhaustive match rather than an `is_single_user()` boolean:
    /// what the mode contributes is the *witness*, so returning the value is what a caller needs and a
    /// boolean would leave every caller to work out where the witness comes from.
    #[inline]
    #[must_use]
    pub const fn shared_witness(&self) -> Option<&AcknowledgementReason> {
        match *self {
            Self::StaticCredentials { ref declared } => Some(declared),
            Self::SubjectPerRequest => None,
        }
    }

    /// Does a shared source need an acknowledgement on its own entry under this mode?
    #[inline]
    #[must_use]
    pub const fn needs_per_source_acknowledgement(&self) -> bool {
        matches!(*self, Self::SubjectPerRequest)
    }
}

/// Why a deployment mode declaration is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDeploymentIdentity {
    #[error("`{}` does not name a deployment mode", DeploymentIdentity::KEY)]
    Unknown {
        #[source]
        cause: UnknownDeploymentIdentity,
    },
    /// Single-user mode with no reason written.
    ///
    /// The reason is what makes the mode a declaration rather than a word: a single-user deployment
    /// serves every source under one identity, and the operator's own sentence for why is what a
    /// reviewer reads and what a shared source borrows as its acknowledgement.
    #[error(
        "`{}` is `single-user` and `{}` is not set. Single-user means every source is read under one \
         static credential, which is correct when that credential is the one user's own - write why, \
         because it is the sentence a reviewer needs and the one a shared source borrows",
        DeploymentIdentity::KEY,
        DeploymentIdentity::REASON_KEY
    )]
    SingleUserWithoutAReason,
    /// A single-user reason on a multi-user deployment, where nothing would read it.
    #[error(
        "`{}` is set and `{}` is `multi-user`, so nothing would read it - a shared source in \
         multi-user mode is acknowledged on its own entry. Remove it, or declare `single-user`",
        DeploymentIdentity::REASON_KEY,
        DeploymentIdentity::KEY
    )]
    ReasonWithoutSingleUser,
    #[error("`{}` is not usable as a reason", DeploymentIdentity::REASON_KEY)]
    Reason {
        #[source]
        cause: InvalidOperatorText,
    },
}
