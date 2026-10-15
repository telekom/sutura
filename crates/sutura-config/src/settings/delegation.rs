//! The combination checks on what a source hands its data system's token service in place of the
//! caller: the declared delegation hops, and the acknowledgement that a `direct` deployment declares
//! none.

use crate::inbound::InboundIdentity;
use crate::settings::NotFitToServe;
use crate::sources::SourceRegistry;

/// See [`NotFitToServe::DelegationWithoutDirectInbound`],
/// [`NotFitToServe::InboundTokenAtPoolUnacknowledged`] and
/// [`NotFitToServe::InboundTokenAtPoolWithoutEffect`].
pub(super) fn hop_refusals(inbound: Option<&InboundIdentity>, sources: &SourceRegistry) -> Vec<NotFitToServe> {
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

#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};
    use crate::sources::workload_identity::InboundTokenAtPool;

    /// A `direct` deployment over one impersonating `workload_identity` source, with `extra` under it.
    fn hopless_overlay(extra: &str) -> Sources {
        Sources::defaults(Environment::Development).with_overlay(format!(
            "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n  inbound:\n    \
             mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
             authorization_server: \"https://issuer.example.com\"\n    key_set_file: \"/etc/sutura/jwks.json\"\n    \
             algorithms: [\"RS256\"]\nsources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    \
             posture: \"impersonation-at-source\"\n    workload_identity:\n      audience: \
             \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n{extra}"
        ))
    }

    const HOP: &str = "      delegation:\n        - token_endpoint: \"https://idp.example.com/token\"\n          \
                       client_id: \"sutura\"\n          client_secret_file: \"/run/secrets/idp-client\"\n          \
                       audience: \"pool-client-id\"\n";

    const ACCEPTED: &str = "    inbound_token_at_pool: \"accepted\"\n";

    fn boot_refusals_over(sources: &Sources) -> Vec<NotFitToServe> {
        let error = Settings::load(sources).expect_err("this deployment does not boot");
        let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
            panic!("expected a boot refusal, got {error:?}");
        };
        refusals.clone()
    }

    #[test]
    fn the_hopless_direct_refusals_are_typed_per_source() {
        let local = SourceName::parse("local").expect("a legal identifier");
        assert_eq!(
            boot_refusals_over(&hopless_overlay("")),
            vec![NotFitToServe::InboundTokenAtPoolUnacknowledged { alias: local.clone() }]
        );
        assert_eq!(
            boot_refusals_over(&hopless_overlay(&format!("{HOP}{ACCEPTED}"))),
            vec![NotFitToServe::InboundTokenAtPoolWithoutEffect { alias: local.clone() }]
        );
        let acknowledged = Settings::load(&hopless_overlay(ACCEPTED)).expect("the acknowledged hopless source boots");
        assert_eq!(
            acknowledged
                .sources()
                .each()
                .find(|&(alias, _)| *alias == local)
                .and_then(|(_, source)| source.inbound_token_at_pool()),
            Some(InboundTokenAtPool::Accepted)
        );
    }
}
