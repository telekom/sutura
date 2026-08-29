//! What this transport establishes about who is asking, in one place.
//!
//! **Two answers now, and a deployment gets exactly one of them.** Where `security.inbound` is
//! declared, [`crate::inbound`] verifies the caller's own token and this module turns the result into a
//! request context. Where it is not, the answer is the one it has always been: the bearer gate compares
//! a configured secret, so presenting it proves the caller holds something an operator distributed - it
//! authenticates the *deployment*, and [`Subject::TheDeploymentItself`] is the honest value for that.
//!
//! Both live here rather than in a handler for the reason this module has always given: this is the
//! only place a *wrong* answer could be introduced. A handler that read a subject out of the request
//! body would compile, pass every existing test, and turn this deployment into a confused deputy.
//!
//! # The control moved from the arity of a function to a type, and it is not weaker
//!
//! [`established`] takes no argument and never will: there is nothing about a request that may
//! contribute to a chain that names no verified caller. [`of_verified`] takes a
//! [`VerifiedCaller`], which is a different thing from a value read off a request:
//!
//! - its one constructor is `pub(crate)` and is called from exactly one place,
//!   `crate::inbound::token::TokenValidator::verify`, after a signature check;
//! - it implements no `Deserialize`, with a `compile_fail` doctest and a compiling twin;
//! - the only thing that puts one into a request is `crate::inbound::require_verified_caller`, which
//!   runs the verification.
//!
//! So the question a reviewer should ask has changed from *"can a request reach this parameter"* to
//! *"can a request produce this type"*, and the answer to the second is no.
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
//! **A token could name a task and none does.** There is no registered claim for one, `docs/adr/0014`
//! names none, and inventing a vendor claim to read would be this transport deciding what a task is.
//! So the third position stays absent, which is what
//! `sutura_domain::identity::PrincipalChain::task` returning an `Option` is for.

use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};

use crate::inbound::VerifiedCaller;

/// The request context for one call on a deployment that establishes no caller identity.
///
/// Takes no argument, deliberately: there is nothing about the request that may contribute to it. A
/// parameter here would be the first place a caller-supplied value could arrive, and the signature is
/// what makes its absence checkable rather than a comment asking the next author not to add one.
pub(crate) const fn established() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
}

/// The request context for one call whose caller proved who it is.
///
/// **Borrows the chain the verification produced rather than assembling one.** Nothing is added here
/// and nothing is decided here: the subject came out of a `sub` claim, the actors out of an `act`
/// claim, and both were parsed by `sutura_domain::identity` so a control character cannot reach the
/// audit line this context is recorded under. If this function ever grows a second parameter, that is
/// the moment to ask what request data it is letting in.
pub(crate) fn of_verified(caller: &VerifiedCaller) -> RequestContext {
    RequestContext::of(caller.chain().clone())
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::{ActorChain, Attribution, PrincipalChain, Subject, SubjectId};

    use crate::inbound::{Scopes, VerifiedCaller};

    use super::{established, of_verified};

    fn a_person() -> Subject {
        Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        }
    }

    #[test]
    fn a_deployment_that_verifies_nothing_names_the_deployment_and_nothing_else() {
        // The answer for every deployment that declares no `security.inbound`, which is every
        // deployment that shipped before leg 1. Both tail positions are absent, and the assertions say
        // which rather than checking one and implying the other.
        let context = established();
        let chain = context.chain();
        assert_eq!(*chain.subject(), Subject::TheDeploymentItself);
        assert_eq!(chain.subject().established(), "deployment");
        assert!(chain.actors().is_none(), "this path establishes no actor");
        assert!(context.task().is_none(), "this transport names no task");
    }

    #[test]
    fn a_verified_caller_becomes_a_context_naming_the_person_and_the_agent_that_acted() {
        // The half that could not exist before leg 1: an audit record for this call names the person.
        // Asserted through `Attribution` rather than by reading a field, because that is the accessor
        // the domain makes a reader go through.
        let actor = ActorChain::of(sutura_domain::identity::Actor::parse("query_agent").expect("a test actor is an actor"));
        let caller = VerifiedCaller::established(PrincipalChain::of(a_person()).acting(actor), Scopes::none());
        let context = of_verified(&caller);
        assert_eq!(context.chain().subject().established(), "verified");
        let Attribution::ActingFor { subject, actors } = context.chain().attribution() else {
            panic!("an agent acting for a subject is not a bare subject");
        };
        assert_eq!(subject, &a_person());
        assert_eq!(actors.immediate().as_str(), "query_agent");
        // And the two answers are different values, which is the distinction the whole chain exists to
        // carry: nothing about a record from this path can be confused with one from the other.
        assert_ne!(context, established());
        // The task position stays absent even here: no claim names one - see the module documentation.
        assert!(context.task().is_none(), "no token claim names a task");
    }
}
