//! `sources.<alias>.caller_session`.
//!
//! How an `impersonation-at-source` `oracle` source opens each question's own session. Read after the entry parsed, as `inbound_token_at_pool` is, so the kind and the posture it is
//! checked against are the parsed ones and a refused channel is refused first.

use sutura_domain::model::SourceName;
use sutura_domain::source::SourcePosture;

use super::{InvalidSourceRegistry, SourceKind, SourceRegistry};

/// How an impersonating `oracle` source opens each question's own session.
///
/// Both shapes send the token the caller's own token is exchanged for through the source's
/// `delegation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CallerSession {
    /// `own-login`, the default: the token is the session's login, and the database runs the
    /// statement as the user it maps the token to.
    #[default]
    OwnLogin,
    /// `end-user-context`: the session logs in as the declared `user` and carries the token as its
    /// end-user security context, and the database's data grants decide what the caller reads.
    EndUserContext,
}

impl CallerSession {
    /// The key under `sources.<alias>`.
    pub const KEY: &'static str = "caller_session";

    /// `own-login` or `end-user-context`; anything else is refused, not read as absent.
    ///
    /// # Errors
    ///
    /// [`InvalidCallerSession::Unknown`] for any other value.
    pub fn parse(raw: &str) -> Result<Self, InvalidCallerSession> {
        match raw {
            "own-login" => Ok(Self::OwnLogin),
            "end-user-context" => Ok(Self::EndUserContext),
            _ => Err(InvalidCallerSession::Unknown),
        }
    }
}

/// Why a written `caller_session` is refused.
///
/// No variant carries the value: it is operator-written text, and the two accepted words are the
/// whole remedy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidCallerSession {
    /// Neither `own-login` nor `end-user-context`.
    #[error("the only values are `own-login` and `end-user-context`")]
    Unknown,
    /// Written on a source whose caller does not run at the source.
    #[error("it is written on a source that is not `impersonation-at-source` - remove it, or write the posture you meant")]
    NotImpersonating,
}

/// Records on `registry` the `caller_session` written on the entry `written`, after the entry parsed.
pub(crate) fn declare(registry: &mut SourceRegistry, written: &str, value: &str) -> Result<(), InvalidSourceRegistry> {
    let alias = SourceName::parse(written).map_err(|cause| InvalidSourceRegistry::Alias {
        written: String::from(written),
        cause,
    })?;
    let Some(source) = registry.by_alias.get_mut(&alias) else {
        return Ok(());
    };
    if source.kind() != SourceKind::Oracle {
        return Err(InvalidSourceRegistry::KeyNotForKind {
            alias,
            kind: source.kind(),
            key: CallerSession::KEY,
        });
    }
    let invalid = |cause| InvalidSourceRegistry::CallerSession {
        alias: alias.clone(),
        cause,
    };
    let session = CallerSession::parse(value).map_err(invalid)?;
    if !matches!(source.posture(), Some(SourcePosture::ImpersonationAtSource)) {
        return Err(invalid(InvalidCallerSession::NotImpersonating));
    }
    source.caller_session = session;
    Ok(())
}
