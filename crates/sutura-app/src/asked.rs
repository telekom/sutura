//! Everything one call was established to be, on either transport - one value, not two.
//!
//! # Why one value rather than two independently-inserted ones
//!
//! `crate::surface::Surface::answer` needs *who is asking* (`RequestContext`) to reach
//! `sutura_domain::identity::CredentialBroker::mint`, and it needs *what this caller may invoke*
//! (`Permitted`) to reach the capability gate. Both are established by the same verification, at the
//! same instant, from the same header - so a transport that inserted them as two separate values
//! would have two places for a request in flight to carry caller A's context beside caller B's
//! grant, if the two insertions were ever reordered or one forgotten. One value with one constructor
//! makes that pairing a type rather than a convention two call sites happen to keep straight.
//!
//! # A caller may not state its own identity
//!
//! No `Deserialize`, for the same reason `sutura_http::inbound::VerifiedCaller` and
//! `sutura_domain::identity::RequestContext` have none: there is no code that could turn
//! caller-supplied bytes into one of these.
//!
//! ```compile_fail
//! // A transport that tried to read one off the wire does not compile.
//! let asked: sutura_app::Asked = serde_json::from_str(r#"{"subject":"someone"}"#).expect("no");
//! drop(asked);
//! ```
//!
//! The compiling twin, so the failure above cannot be passing for a typo - what a caller of
//! [`Asked::established`] can do with one is read the two halves back:
//!
//! ```
//! use sutura_app::{Asked, Permitted};
//! use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};
//!
//! let asked = Asked::established(
//!     RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself)),
//!     Permitted::every_capability(),
//! );
//! assert!(asked.permitted().includes(sutura_app::Capability::DescribeCatalog));
//! ```

use sutura_domain::identity::RequestContext;

use crate::capability::Permitted;

/// Everything one call was established to be: who is asking, and what it may invoke.
///
/// **One constructor, [`Asked::established`], and it takes the two halves already produced by a
/// verification - it performs no verification of its own.** `sutura_http::capability::establish_asked`
/// is the one place that calls it today, handing over what leg 1 already derived - a
/// `VerifiedCaller`'s chain and scopes, or the deployment's own when there is none. The agent
/// surface's own call arrives with PR2 of `telekom/sutura#378`; until then this type adds no third
/// way to decide either half.
///
/// `Clone` because a transport may need to hand the same value to a blocking-pool closure that
/// outlives the request extension it was read from - `sutura_runtime::spawn_carrying_span` is the
/// reason `RequestContext` is already `Clone`, and `Permitted` clones a `BTreeSet` of at most three
/// elements today.
#[derive(Debug, Clone)]
pub struct Asked {
    context: RequestContext,
    permitted: Permitted,
}

impl Asked {
    /// The only way to one of these: hand over what a transport's own verification already produced.
    #[inline]
    #[must_use]
    pub const fn established(context: RequestContext, permitted: Permitted) -> Self {
        Self { context, permitted }
    }

    /// Who this call is attributed to, and the credential it presented, if any.
    #[inline]
    #[must_use]
    pub const fn context(&self) -> &RequestContext {
        &self.context
    }

    /// What this caller may invoke.
    #[inline]
    #[must_use]
    pub const fn permitted(&self) -> &Permitted {
        &self.permitted
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::{PrincipalChain, RequestContext, Secret, Subject};

    use super::Asked;
    use crate::capability::{Capability, Permitted};

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "reading the assertion back is the point of the check: it is the other half `established` must not drop"
    )]
    fn established_returns_exactly_the_two_halves_handed_to_it() {
        // `Subject::Verified`, not `Subject::TheDeploymentItself` - the value a mis-wired
        // `establish_asked` would substitute (ADR 0023's own named trap) is a chain with no
        // assertion for the deployment's own identity, which this fixture cannot be mistaken for.
        let subject = Subject::verified("someone@example.com").expect("a test subject is a subject");
        let context = RequestContext::with_assertion(
            PrincipalChain::of(subject),
            Secret::new("the-assertion-a-transport-verified"),
            // Far future: this cell is about what reaches a broker, not about when it stops.
            4_102_444_800,
        );
        let permitted = Permitted::granted_by([Capability::DescribeCatalog.scope()]);
        let asked = Asked::established(context.clone(), permitted.clone());
        assert_eq!(asked.context().chain(), context.chain());
        assert_eq!(
            asked.context().assertion().map(Secret::expose_secret),
            Some("the-assertion-a-transport-verified")
        );
        assert_eq!(asked.permitted(), &permitted);
    }
}
