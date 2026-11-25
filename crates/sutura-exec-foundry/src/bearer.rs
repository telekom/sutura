//! Whose bearer a Foundry request carries: the deployment's, or each caller's own.

use sutura_domain::identity::Secret;
use sutura_domain::warehouse::deadline::Deadline;

/// Whose token a request carries. No variant holds both a caller's and a deployment's for a
/// question, so a per-caller source has no deployment token to fall back to.
#[derive(Debug)]
pub(crate) enum Bearer {
    /// The deployment's own token, for a shared source: every question and every anchor.
    Deployment(Box<dyn DeploymentToken>),
    /// Each caller's own token, as the broker presented it; `boot` runs anchors only.
    EachCaller { boot: Option<BootIdentity> },
}

/// The deployment's own Foundry bearer, per request: a fixed token, or a grant minted and
/// refreshed inside the request's deadline.
pub trait DeploymentToken: core::fmt::Debug + Send + Sync {
    /// The bearer for one request.
    ///
    /// # Errors
    ///
    /// [`TokenFailed`] when none could be minted inside `deadline`.
    fn bearer(&self, deadline: Deadline) -> Result<Secret, TokenFailed>;
}

/// A fixed token, read once at boot.
impl DeploymentToken for Secret {
    fn bearer(&self, _deadline: Deadline) -> Result<Secret, TokenFailed> {
        Ok(self.clone())
    }
}

/// Why no deployment token could be presented. Carries no token.
#[derive(Debug, thiserror::Error)]
pub enum TokenFailed {
    #[error("the request deadline was spent before the deployment's token was minted")]
    DeadlineSpent,
    /// The one client-credentials grant's own refusal.
    #[cfg(feature = "http")]
    #[error("the deployment's token could not be minted")]
    Mint(#[source] sutura_http_client::token::TokenUnavailable),
}

/// An impersonating source's startup-only identity: what its anchors run as.
///
/// **Only [`sutura_domain::warehouse::Warehouse::verify_anchor`] reads it.** It is held inside [`Bearer::EachCaller`], whose
/// per-question arm matches the caller's token alone, and it has no accessor a question could reach.
#[derive(Debug)]
pub struct BootIdentity(Box<dyn DeploymentToken>);

impl BootIdentity {
    pub(crate) fn new(token: Box<dyn DeploymentToken>) -> Self {
        Self(token)
    }

    pub(crate) fn token(&self) -> &dyn DeploymentToken {
        self.0.as_ref()
    }
}
