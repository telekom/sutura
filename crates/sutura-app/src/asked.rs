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
/// verification - it performs no verification of its own.** `sutura_http::inbound::gate` (a header
/// becomes a `VerifiedCaller`) and `sutura_mcp` (the process boundary is the boundary) are the two
/// places that call it, each handing over what its own transport already derived. This type adds no
/// third way to decide either half.
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
    use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};

    use super::Asked;
    use crate::capability::{Capability, Permitted};

    #[test]
    fn established_returns_exactly_the_two_halves_handed_to_it() {
        let context = RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself));
        let permitted = Permitted::granted_by([Capability::DescribeCatalog.scope()]);
        let asked = Asked::established(context.clone(), permitted.clone());
        assert_eq!(asked.context().chain(), context.chain());
        assert_eq!(asked.permitted(), &permitted);
    }
}
