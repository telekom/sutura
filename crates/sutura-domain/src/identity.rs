//! Who a request runs as, and the credential material that proves it.
//!
//! Named for the concept rather than for the mechanism it currently uses. `redact` was the
//! earlier name, and it described one property of one type - so the module could not hold
//! the principal chain, the request context or the `CredentialBroker` port that belong beside
//! it, and every one of those would have arrived somewhere else.
//!
//! All three are here now. `principal` holds the chain a call is attributed to and the request
//! context that carries it; `credential` holds what one answer executes with and the
//! [`CredentialBroker`] port that mints it. The port arrived with its first implementor, which is
//! `sutura_config::StaticCredentialBroker` - the static-credential broker a single-user deployment
//! already needs, rather than a fake standing in for one.
//!
//! **Nothing in [`principal`] is `Serialize` or `Deserialize`, and [`Secret`] is neither either.**
//! That is one property rather than two coincidences: an identity is derived from what a transport
//! established, and a type that could be read off the wire is a caller stating its own. The
//! credential material and the chain are the two things in this workspace where being unable to
//! parse the value from a request body is the control.
//!
//! The redaction is the point of [`Secret`], so it has a test. A secret that reaches a log
//! through `{:?}` is not recoverable once shipped, and every structured-logging call site is
//! a chance for it - so the type, not the call site, is where this is fixed.

use std::fmt;

// Private, with the types re-exported below, so there is exactly ONE public path to each of them.
// `pub mod` would have given two - `identity::principal::PrincipalChain` and
// `identity::PrincipalChain` - and a second path to a type is a second name for it in every doc
// comment that mentions it.
mod credential;
mod principal;

pub use crate::identity::credential::{
    Agreed, BoundToTheRequest, CredentialBroker, CredentialsDoNotCoverThePlan, CredentialsDoNotFitTheRequest, Expiry,
    LegCredentials, Minted, Presented, PresentedDisagreesWithPosture, PrincipalName, SourceSet,
};
pub use crate::identity::principal::{
    Actor, ActorChain, ActorsInOrder, Attribution, InvalidPrincipalId, PrincipalChain, RequestContext, Subject, SubjectId, TaskId,
};

/// An opaque secret. `Debug` prints a placeholder; the value is reachable only by an
/// explicit, greppable call to [`Secret::expose`].
///
/// Deliberately NOT `PartialEq`/`Eq`. A derived comparison on credential material is a
/// byte-wise one that returns early on the first difference, which is a timing oracle at
/// whatever call site adds it later - and the call site is where it would be invisible.
/// Nothing here needs to compare secrets; when something does, it arrives with a
/// constant-time implementation and a name that says so, not with a derive. Until then the
/// absence of the impl is the enforcement: `a == b` on a `Secret` does not compile.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Infallible on purpose: every string is a valid secret. There is no invariant here
    /// beyond opacity, and a constructor that returned `Result` would be inventing one.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Named to be conspicuous in review and in a grep. Prefer passing `Secret` around.
    #[inline]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(REDACTED)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("REDACTED")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one test that must exist before there is anything to redact.
    #[test]
    fn debug_does_not_leak_the_secret() {
        let s = Secret::new("hunter2-do-not-log-me");
        assert!(!format!("{s:?}").contains("hunter2"), "Debug leaked: {s:?}");
        assert!(!format!("{s}").contains("hunter2"), "Display leaked: {s}");
        assert_eq!(format!("{s:?}"), "Secret(REDACTED)");
    }

    /// Redaction must not make the value unusable - otherwise people avoid the type.
    #[test]
    fn expose_returns_the_value() {
        assert_eq!(Secret::new("v").expose(), "v");
    }

    /// A struct that merely CONTAINS a Secret must not leak it via derived Debug either.
    #[test]
    fn nested_debug_does_not_leak() {
        #[derive(Debug)]
        struct Config {
            #[expect(dead_code, reason = "read only through the derived Debug impl, which is what this asserts")]
            token: Secret,
        }
        let c = Config {
            token: Secret::new("hunter2"),
        };
        assert!(!format!("{c:?}").contains("hunter2"), "nested Debug leaked: {c:?}");
    }
}
