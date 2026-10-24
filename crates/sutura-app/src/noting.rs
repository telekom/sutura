//! The broker a call is answered through, noting who each leg was delegated as.
//!
//! [`surface::LocalService`](crate::surface::LocalService) writes one audit record per call, and a
//! call that FAILS after minting leaves through an `Err` that carries no grant. So the delegation is
//! noted where it is minted, once, and the record reads it on either side.

use std::sync::OnceLock;

use sutura_domain::identity::{CredentialBroker, DelegatedAs, Minted, RequestContext, SourceSet};

/// A broker that answers as `broker` does and keeps the names of the legs it delegated.
pub(crate) struct NotingBroker<'broker, B> {
    broker: &'broker B,
    delegated: OnceLock<DelegatedAs>,
}

impl<'broker, B> NotingBroker<'broker, B> {
    pub(crate) const fn over(broker: &'broker B) -> Self {
        Self {
            broker,
            delegated: OnceLock::new(),
        }
    }

    /// What the call's one mint delegated; empty when nothing was minted.
    pub(crate) fn delegated(self) -> DelegatedAs {
        self.delegated.into_inner().unwrap_or_default()
    }
}

impl<B: CredentialBroker> CredentialBroker for NotingBroker<'_, B> {
    type Error = B::Error;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let minted = self.broker.mint(context, sources)?;
        // One mint per call (`CredentialBroker::mint`'s own contract), so the first is the call's.
        drop(self.delegated.set(DelegatedAs::minted(&minted)));
        Ok(minted)
    }
}
