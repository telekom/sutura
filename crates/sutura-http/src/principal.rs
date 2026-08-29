//! What this transport establishes about who is asking, in one place.
//!
//! **The answer today is: the deployment, and nothing about the caller.** The bearer gate compares a
//! configured secret, so presenting it proves the caller holds something an operator distributed -
//! it authenticates the *deployment*. There is no token validation against an issuer, no claims, and
//! no `CredentialBroker`, so there is no verified caller identity for a chain to name.
//!
//! That is a fact about the front door rather than a gap in this module, and the reason it lives in a
//! function of its own is that it is the only place a *wrong* answer could be introduced. A handler
//! that read a subject out of the request body would compile, pass every existing test, and turn
//! this deployment into a confused deputy - so the construction of a chain is one function, with the
//! reasoning beside it, and no handler assembles one itself.
//!
//! # Why not the client address, or the correlation id
//!
//! Both are available here and neither is an identity. A peer address identifies a socket - a
//! gateway's, usually - and recording it as a principal would put a network location in the field a
//! reader takes to mean "who". A correlation id identifies a *request*, not an asker, and it is
//! already on the span. `sutura_domain::identity::TaskId` is the field a unit of work belongs in, and
//! a task spans calls: minting one per request would make every call its own task, which is a value
//! that looks like an answer and means nothing.
//!
//! # What changes here when a caller can prove who it is
//!
//! This function, and nothing else. It returns `Subject::TheDeploymentItself` today; it will return
//! `Subject::Verified` built from a validated token's subject claim, and an `ActorChain` where the
//! token says an agent acted. Every reader downstream already handles both cases, because the domain
//! makes them handle both.

use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};

/// The request context for one call, derived from what this transport established.
///
/// Takes no argument, deliberately: there is nothing about the request that may contribute to it. A
/// parameter here would be the first place a caller-supplied value could arrive, and the signature is
/// what makes its absence checkable rather than a comment asking the next author not to add one.
pub(crate) const fn established() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::Subject;

    use super::established;

    #[test]
    fn the_chain_this_transport_establishes_names_the_deployment_and_nothing_else() {
        // Not a placeholder: the bearer gate proves a caller holds a configured secret, so the
        // deployment is the honest answer and the type is what says so. Both tail positions are
        // absent, and the assertions say which rather than checking one and implying the other.
        let context = established();
        let chain = context.chain();
        assert_eq!(*chain.subject(), Subject::TheDeploymentItself);
        assert_eq!(chain.subject().established(), "deployment");
        assert!(chain.actors().is_none(), "this transport establishes no actor");
        assert!(context.task().is_none(), "this transport names no task");
    }
}
