//! The chain this transport establishes, which is the deployment and nothing more.
//!
//! A near-twin of `sutura_http`'s module of the same name, and **deliberately not shared with it**:
//! `engineering/rust`'s *dependencies point inward* says an adapter never calls another adapter, and that one is
//! `pub(crate)` for exactly that reason. What the two have in common is a fact about today rather
//! than code worth extracting - neither transport establishes a caller identity, so both name the
//! deployment. When one of them learns leg 1 from
//! [how a caller proves who it is](../../../docs/adr/0014-how-a-caller-proves-who-it-is.md), they
//! stop agreeing and a shared helper would have had to be unpicked.

use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};

/// The context every question over this transport is asked in.
///
/// `Subject::TheDeploymentItself` rather than a `Verified` subject with some placeholder id: this
/// transport authenticates nobody, and a `Verified` id that happened to read as a deployment would
/// be indistinguishable from the honest case. Both tail positions are absent, so an agent acting
/// for a human is not something this transport can currently claim - and when it can, this is the
/// one function that changes.
pub(crate) const fn established() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::Subject;

    use super::established;

    #[test]
    fn the_chain_this_transport_establishes_names_the_deployment_and_nothing_else() {
        let context = established();
        let chain = context.chain();
        assert_eq!(*chain.subject(), Subject::TheDeploymentItself);
        assert_eq!(chain.subject().established(), "deployment");
        assert!(
            chain.actors().is_none(),
            "this transport establishes no actor, so it must not claim one"
        );
        assert!(context.task().is_none(), "this transport names no task");
    }
}
