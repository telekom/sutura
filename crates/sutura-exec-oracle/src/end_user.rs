//! Each question's own session under the asker's end-user security context.
//!
//! Logged in as the declared user, carrying the asker's token, closed when the question ends.
//!
//! **No session carries two askers.** A context is attached only to a session opened for one
//! question and dropped with it, so no context outlives its question and no clear has to run for
//! the next asker to be safe. The boot connection never carries an asker's context.
//!
//! **The token is the last delegation hop's, in both of the context's fields**: the end-user token
//! and the database-access token. No data role and no attribute is sent.
//!
//! ## Limits
//!
//! - **One login per question.** Each question pays a TCP connect, a TLS handshake and a password
//!   login before its statement, under the adapter's one lock.
//! - **The token is copied twice into plain `String`s** for the driver's builder, which keeps the
//!   encoded context as obfuscated bytes; nothing here zeroes either copy.
//! - **What the database does with the context is its own**: whether it accepts the token, which data
//!   grants apply, and that a table without `USE DATA GRANTS ONLY` is read with the declared user's
//!   own grants.

use sutura_domain::identity::Secret;

use crate::{DriverError, OracleError, connect_err};

/// The declared dial WITH the declared user, where each question's own session is opened.
#[derive(Clone)]
pub(crate) struct ContextSessions(oracledb::Config);

impl core::fmt::Debug for ContextSessions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ContextSessions").finish_non_exhaustive()
    }
}

impl ContextSessions {
    /// Sessions logged in with `config`, the declared user's.
    pub(crate) const fn new(config: oracledb::Config) -> Self {
        Self(config)
    }

    /// Opens one session as the declared user, carrying `token` as its end-user security context.
    ///
    /// Dropping it closes the session.
    ///
    /// # Errors
    ///
    /// The dial's own errors, as [`crate::TokenSessions::open`] answers them, and
    /// [`OracleError::EndUserContext`] or [`OracleError::EndUserContextUnsupported`] for a context
    /// the driver did not attach. None carries the token.
    pub(crate) fn open(&self, token: &Secret) -> Result<oracledb::Connection, OracleError> {
        #[expect(
            clippy::disallowed_methods,
            reason = "`attaching` asked the packet-trace refusal through `dialled`"
        )]
        let session = oracledb::connect(self.0.clone()).map_err(connect_err)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the asker's token is the context this session carries, so it is handed to the driver \
                      here; nothing on this path logs or formats it"
        )]
        let context = of(String::from(token.expose_secret()))?;
        session.set_end_user_security_context(context).map_err(refused)?;
        Ok(session)
    }
}

/// Whether `connection` can carry an end-user security context, asked once at boot.
///
/// Through the driver's own check: a context set and cleared before any round trip, so nothing is
/// sent.
///
/// # Errors
///
/// [`OracleError::EndUserContextUnsupported`] when the server does not offer end-user security
/// contexts.
pub(crate) fn offered(connection: &oracledb::Connection) -> Result<(), OracleError> {
    connection
        .set_end_user_security_context(of(String::from("capability-probe"))?)
        .map_err(refused)?;
    connection
        .clear_end_user_security_context()
        .map_err(|cause| OracleError::EndUserContext { cause: cause.into() })
}

/// The context that carries `token` as both the end-user token and the database-access token.
fn of(token: String) -> Result<oracledb::EndUserSecurityContext, OracleError> {
    oracledb::EndUserSecurityContext::new(oracledb::EndUserIdentity::Token(token.clone()), token)
        .map_err(|cause| OracleError::EndUserContext { cause: cause.into() })
}

/// The driver's refusal to attach a context, with a server that offers none as its own variant.
fn refused(cause: oracledb::Error) -> OracleError {
    let cause = DriverError::from(cause);
    if matches!(cause.0.kind(), oracledb::ErrorKind::UnsupportedDeepDataSecurityFeature) {
        OracleError::EndUserContextUnsupported { cause }
    } else {
        OracleError::EndUserContext { cause }
    }
}
