//! The combination checks on what a source hands its data system's token service in place of the
//! caller: the declared delegation hops, and the acknowledgement that a `direct` deployment declares
//! none.

use crate::inbound::InboundIdentity;
use crate::settings::NotFitToServe;
use crate::sources::SourceRegistry;

/// See [`NotFitToServe::DelegationWithoutDirectInbound`],
/// [`NotFitToServe::InboundTokenAtPoolUnacknowledged`] and
/// [`NotFitToServe::InboundTokenAtPoolWithoutEffect`].
pub(super) fn refusals(inbound: Option<&InboundIdentity>, sources: &SourceRegistry) -> Vec<NotFitToServe> {
    let direct = matches!(inbound, Some(InboundIdentity::Direct { .. }));
    let mut refusals = Vec::new();
    for (alias, source) in sources.each() {
        let hops = source.workload_identity().map(|wif| !wif.delegation().is_empty());
        let delegates = hops == Some(true)
            || matches!(
                *source.placement(),
                crate::SourcePlacement::Oracle { ref delegation, .. }
                    | crate::SourcePlacement::Cube { ref delegation, .. } if !delegation.is_empty()
            );
        if delegates && !direct {
            refusals.push(NotFitToServe::DelegationWithoutDirectInbound {
                alias: alias.clone(),
                key: if hops == Some(true) {
                    "workload_identity.delegation"
                } else {
                    "delegation"
                },
            });
        }
        let hopless_direct = direct && hops == Some(false);
        match (hopless_direct, source.inbound_token_at_pool()) {
            (true, None) => refusals.push(NotFitToServe::InboundTokenAtPoolUnacknowledged { alias: alias.clone() }),
            (false, Some(_)) => refusals.push(NotFitToServe::InboundTokenAtPoolWithoutEffect { alias: alias.clone() }),
            (true, Some(_)) | (false, None) => {}
        }
    }
    refusals
}
