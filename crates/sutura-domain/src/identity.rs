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
//! **[`Secret`] is now a COMPILE ERROR where it used to be a redaction**, and the difference is the
//! whole of `docs/adr/0020`. A hand-written `Display` printing `REDACTED` left `format!("{token}")`
//! and `tracing::info!(%token)` compiling: nothing leaked, and every structured-logging call site
//! stayed a chance for an author to believe they had logged a value. The type is built on
//! `secrecy::SecretString`, which has no `Display` and no `PartialEq`, so those two and `==` do not
//! build at all - pinned by `compile_fail` doctests with compiling twins on [`Secret`] itself. A
//! secret that reaches a log is not recoverable once shipped, so the type, not the call site, is
//! where this is fixed.

use secrecy::{ExposeSecret as _, SecretString};

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

/// An opaque secret that nothing can render.
///
/// The value is reachable only through an explicit, greppable call to [`Secret::expose_secret`]:
/// there is no formatter to reach here, and none on what it wraps.
///
/// # What the compiler refuses, and why that replaced a redaction
///
/// This was `Secret(String)` with a hand-written `Debug` **and** `Display`, both printing
/// `REDACTED`. Nothing ever leaked through them - and the accident stayed *representable*, which is
/// the defect: `format!("{token}")` compiled, `tracing::info!(%token)` compiled, and each produced a
/// line reading `REDACTED` where its author believed a value had been logged. A redaction is a
/// CHECK, and this workspace's rule is to prefer unrepresentable to checked, so the value now lives
/// in a `secrecy::SecretString`, which has no `Display` and no `PartialEq` - and the four accidents
/// below stopped compiling rather than started printing a placeholder. `docs/adr/0020` is the
/// decision, including what the dependency costs and what keeping the hand-written type would have
/// bought instead.
///
/// **It is a newtype over `SecretString` rather than an alias for it, and that is load-bearing in
/// three ways.** A `Deserialize` cannot arrive by FEATURE UNIFICATION: `secrecy`'s `serde` feature
/// gives `SecretBox` one, cargo unions features across a build graph, and a newtype that derives
/// nothing is unaffected by whether some other crate turns it on - so the manifest's
/// `default-features = false` is a supply-chain decision and not the mechanism. `SecretString` also
/// has a `Default`, which is the newtype guide's own counterexample pointed at credentials - a
/// default secret is not a thing - and `From<&str>` plus `From<String>`, two ways in that bypass
/// [`Secret::new`]. The wrapper refuses all three by not restating them.
///
/// ## `Display`, and the `{}` that used to compile
///
/// ```compile_fail
/// let token = sutura_domain::identity::Secret::new("hunter2");
/// // No `Display`, so there is no formatter for `{}` to select.
/// let line = format!("token={token}");
/// drop(line);
/// ```
///
/// The compiling twin, differing by that one formatter - `Debug` is still there, and still cannot
/// print the value:
///
/// ```
/// let token = sutura_domain::identity::Secret::new("hunter2");
/// let line = format!("token={token:?}");
/// assert!(!line.contains("hunter2"), "{line}");
/// assert!(line.contains("REDACTED"), "{line}");
/// ```
///
/// ## A `%` tracing field, which is that same missing `Display`
///
/// `tracing`'s `%` sigil records a field through `tracing::field::display`, whose argument is bound
/// `T: Display` - so the mechanism is the bound below rather than anything about the macro, and this
/// is the honest place to pin it: `sutura-domain` has no `tracing` dependency and may not acquire
/// one, because `cargo xtask check-boundaries` walks this crate's whole resolve graph. The macro
/// itself is pinned where a crate has both, in `sutura_http::inbound`.
///
/// ```compile_fail
/// fn recorded_as_a_display_field(value: impl std::fmt::Display) -> String {
///     value.to_string()
/// }
/// let token = sutura_domain::identity::Secret::new("hunter2");
/// let line = recorded_as_a_display_field(token);
/// drop(line);
/// ```
///
/// The compiling twin, differing by the one call that says out loud what it is doing:
///
/// ```
/// fn recorded_as_a_display_field(value: impl std::fmt::Display) -> String {
///     value.to_string()
/// }
/// let token = sutura_domain::identity::Secret::new("hunter2");
/// assert_eq!(recorded_as_a_display_field(token.expose_secret()), "hunter2");
/// ```
///
/// ## `==`
///
/// Deliberately NOT `PartialEq`/`Eq`, and now inherited rather than merely omitted - `SecretBox` has
/// no `PartialEq` either, so neither the wrapper nor a future `#[derive]` on it can produce one from
/// the inner value. A derived comparison on credential material returns on the first differing byte,
/// which is a timing oracle at whatever call site adds it, and the call site is where it would be
/// invisible. The one comparison this workspace needs is
/// `sutura_config::AccessToken::matches_in_constant_time`, which is named for what it is.
///
/// ```compile_fail
/// let a = sutura_domain::identity::Secret::new("hunter2");
/// let b = sutura_domain::identity::Secret::new("hunter2");
/// assert!(a == b, "no `PartialEq` to call");
/// ```
///
/// The compiling twin, differing by the one thing that makes the comparison visible - and it is
/// still the wrong way to compare a credential, which is the point of it being conspicuous:
///
/// ```
/// let a = sutura_domain::identity::Secret::new("hunter2");
/// let b = sutura_domain::identity::Secret::new("hunter2");
/// assert!(a.expose_secret() == b.expose_secret());
/// ```
///
/// ## `Deserialize`
///
/// There is none, so caller-supplied bytes cannot become credential material - the other half of
/// [`crate::identity::principal`]'s rule that a caller cannot state its own identity.
///
/// ```compile_fail
/// let token: sutura_domain::identity::Secret = serde_json::from_str(r#""hunter2""#).expect("no");
/// drop(token);
/// ```
///
/// The compiling twin, differing by where the value comes from - a constructor a reviewer can see,
/// rather than a deserializer a request reaches:
///
/// ```
/// let token = sutura_domain::identity::Secret::new(String::from("hunter2"));
/// assert_eq!(token.expose_secret(), "hunter2");
/// ```
///
/// # The value is wiped on drop, and the limit is not small
///
/// `SecretBox`'s `Drop` writes zeros over the boxed buffer through `zeroize`'s volatile writes, so
/// the copy THIS TYPE holds does not outlive it in the process image. **It says nothing about copies
/// made before the value arrived.** A settings file read into a `String`, a `serde`-deserialized
/// `Option<String>` on a wire document, and the `String` [`Secret::new`] itself consumes are all
/// ordinary allocations - and `String::into_boxed_str` reallocates whenever capacity exceeds length,
/// which leaves the original buffer freed and unwiped. Shortening that window means the secret being
/// parsed into this type closer to where it is read, not a stronger claim here.
///
/// There is no test for the wipe, deliberately: observing it means reading memory after free, which
/// is undefined behaviour, and `unsafe_code` is `forbid` in this workspace. What is tested is that
/// the type composes - the property above is `zeroize`'s, audited there.
#[derive(Debug, Clone)]
pub struct Secret(SecretString);

impl Secret {
    /// Infallible on purpose: every string is a valid secret. There is no invariant here beyond
    /// opacity, and a constructor that returned `Result` would be inventing one.
    ///
    /// The canonical constructor, and the only one - `From<String>` and `From<&str>` exist on
    /// `SecretString` and are deliberately not restated on the wrapper, so there is one place a
    /// secret comes into existence.
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    /// Named to be conspicuous in review and in a grep, and named after `secrecy`'s own trait
    /// method so the vocabulary is one word rather than two.
    ///
    /// An inherent method rather than an `ExposeSecret` impl: the trait would have to be in scope at
    /// every call site to be callable, which buys nothing here and makes `Secret` substitutable for
    /// the library type in generic code - the opposite of what the newtype is for.
    #[inline]
    pub fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one test that must exist before there is anything to redact.
    ///
    /// A PROPERTY rather than an exact string, and the reason is where the string comes from: the
    /// rendering is `secrecy`'s now, so pinning it byte for byte would fail a harmless upstream
    /// wording change while proving nothing extra. What matters is that the value is absent and that
    /// the reader is told something was withheld. There is no `Display` half to this test any more -
    /// `format!("{s}")` is a compile error, pinned by a `compile_fail` doctest on [`Secret`].
    #[test]
    fn debug_does_not_leak_the_secret() {
        let s = Secret::new("hunter2-do-not-log-me");
        let rendered = format!("{s:?}");
        assert!(!rendered.contains("hunter2"), "Debug leaked: {rendered}");
        assert!(rendered.contains("REDACTED"), "Debug said nothing was withheld: {rendered}");
    }

    /// Redaction must not make the value unusable - otherwise people avoid the type.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the test that the accessor works at all: it is the exposure, not a use of one"
    )]
    fn expose_secret_returns_the_value() {
        assert_eq!(Secret::new("v").expose_secret(), "v");
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

    /// A clone is a second buffer, and it is still opaque.
    ///
    /// Worth its own test because `Clone` on the inner type is hand-written upstream rather than
    /// derived - `SecretBox<str>` cannot derive it - so a clone that returned something renderable
    /// would be an upstream change no other test here would see.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "reading both values IS the assertion that a clone holds what the original held"
    )]
    fn a_clone_is_still_opaque_and_still_holds_the_value() {
        let original = Secret::new("hunter2-do-not-log-me");
        let copy = original.clone();
        assert_eq!(copy.expose_secret(), original.expose_secret());
        assert!(!format!("{copy:?}").contains("hunter2"), "clone leaked: {copy:?}");
    }
}
