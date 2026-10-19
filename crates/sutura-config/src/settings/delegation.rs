//! The refusal of a source that sends the caller's own inbound token to an identity provider - a
//! `workload_identity.delegation`, a `google_login`, or an Oracle or Cube `delegation` - on a
//! deployment that verifies no `direct` caller.

use crate::inbound::InboundIdentity;
use crate::settings::{NotFitToServe, Settings};

/// A declared delegation exchange on a deployment that verifies no `direct` caller - see
/// [`NotFitToServe::DelegationWithoutDirectInbound`].
pub(super) fn refusals(settings: &Settings) -> Vec<NotFitToServe> {
    if matches!(settings.security.inbound(), Some(InboundIdentity::Direct { .. })) {
        return Vec::new();
    }
    settings
        .sources
        .each()
        .filter_map(|(alias, source)| {
            let alias = alias.clone();
            if source.workload_identity().is_some_and(|wif| !wif.delegation().is_empty()) {
                Some(NotFitToServe::DelegationWithoutDirectInbound {
                    alias,
                    key: "workload_identity.delegation",
                })
            } else if matches!(
                source.caller_identity(),
                Some(crate::sources::caller_identity::CallerIdentity::GoogleLogin(_))
            ) {
                Some(NotFitToServe::DelegationWithoutDirectInbound {
                    alias,
                    key: "google_login",
                })
            } else if let crate::SourcePlacement::Oracle { ref delegation, .. }
            | crate::SourcePlacement::Cube { ref delegation, .. } = *source.placement()
                && !delegation.is_empty()
            {
                Some(NotFitToServe::DelegationWithoutDirectInbound {
                    alias,
                    key: "delegation",
                })
            } else {
                None
            }
        })
        .collect()
}
