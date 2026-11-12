//! The combination checks on what a source hands its data system's token service in place of the
//! caller: a source that sends the caller's own inbound token to an identity provider - a
//! `workload_identity.delegation`, a `google_login`, or an Oracle, Cube, `ClickHouse` or `node`
//! `delegation` - on a deployment that verifies no `direct` caller, and the acknowledgement that a
//! `direct` deployment declares no hop. These are every [`crate::SourcePlacement`] variant that
//! carries a `delegation` list; a kind added with one more needs a row here too, because this module
//! reads the placement by name rather than through one shared accessor. The own-audience check below
//! does not read `node` through the shared `hops` slice - its hop reaching another node that also
//! verifies it is a different exposure from the others, not the data system receiving this
//! deployment's own token, so it earns its own refusal and message rather than being folded into
//! [`crate::settings::NotFitToServe::DelegationToTheDeploymentsOwnAudience`].

use crate::inbound::{InboundIdentity, InboundVerifier};
use crate::settings::NotFitToServe;
use crate::sources::SourceRegistry;
use crate::sources::caller_identity::CallerIdentity;

/// Reads `workload_identity.delegation`, `google_login`, and the `delegation` list of an Oracle,
/// Cube, `ClickHouse` or `node` placement. See [`NotFitToServe::DelegationWithoutDirectInbound`],
/// [`NotFitToServe::InboundTokenAtPoolUnacknowledged`],
/// [`NotFitToServe::InboundTokenAtPoolWithoutEffect`],
/// [`NotFitToServe::DelegationToTheDeploymentsOwnAudience`] and
/// [`NotFitToServe::NodeDelegationToTheDeploymentsOwnAudience`].
pub(super) fn hop_refusals(inbound: &[InboundVerifier], sources: &SourceRegistry) -> Vec<NotFitToServe> {
    let mut refusals = Vec::new();
    for (alias, source) in sources.each() {
        let own: Vec<_> = inbound
            .iter()
            .filter_map(|verifier| match verifier.identity() {
                InboundIdentity::Direct { resource, .. } => Some(resource),
                InboundIdentity::BehindGateway { .. } => None,
            })
            .collect();
        let direct = inbound.iter().any(|verifier| {
            matches!(verifier.identity(), InboundIdentity::Direct { .. })
                && source.admitted_by().is_some_and(|ids| ids.contains(verifier.id()))
        });
        let hops = source.workload_identity().map(|wif| !wif.delegation().is_empty());
        let delegation = super::admission::delegation_key(source).or_else(|| {
            (matches!(source.caller_identity(), Some(CallerIdentity::GoogleLogin(_)))
                && !source.admitted_by().is_some_and(std::collections::BTreeSet::is_empty))
            .then_some("google_login")
        });
        if let Some(key) = delegation
            && !direct
        {
            refusals.push(NotFitToServe::DelegationWithoutDirectInbound {
                alias: alias.clone(),
                key,
            });
        }
        let hopless_direct = direct && hops == Some(false);
        match (hopless_direct, source.inbound_token_at_pool()) {
            (true, None) => refusals.push(NotFitToServe::InboundTokenAtPoolUnacknowledged { alias: alias.clone() }),
            (false, Some(_)) => refusals.push(NotFitToServe::InboundTokenAtPoolWithoutEffect { alias: alias.clone() }),
            (true, Some(_)) | (false, None) => {}
        }
        let hops = source.workload_identity().map_or_else(
            || match *source.placement() {
                crate::SourcePlacement::Oracle { ref delegation, .. }
                | crate::SourcePlacement::Cube { ref delegation, .. }
                | crate::SourcePlacement::ClickHouse { ref delegation, .. } => delegation.as_slice(),
                _ => &[],
            },
            |wif| wif.delegation(),
        );
        if let crate::SourcePlacement::Node { ref delegation, .. } = *source.placement()
            && let Some(last) = delegation.last()
            && own
                .iter()
                .any(|resource| last.audience() == resource.as_str() || last.audience() == resource.audience().as_str())
        {
            refusals.push(NotFitToServe::NodeDelegationToTheDeploymentsOwnAudience { alias: alias.clone() });
        }
        if let Some(last) = hops.last()
            && own
                .iter()
                .any(|resource| last.audience() == resource.as_str() || last.audience() == resource.audience().as_str())
        {
            refusals.push(NotFitToServe::DelegationToTheDeploymentsOwnAudience { alias: alias.clone() });
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

    #[test]
    fn a_hop_to_the_deployments_own_audience_is_a_typed_refusal() {
        assert_eq!(
            boot_refusals_over(&hopless_overlay(
                &HOP.replace("\"pool-client-id\"", "\"https://sutura.example.com\"")
            )),
            vec![NotFitToServe::DelegationToTheDeploymentsOwnAudience {
                alias: SourceName::parse("local").expect("a legal identifier")
            }]
        );
    }

    /// A `node` source over `inbound`, its last hop asking for `audience`.
    fn node_over(inbound: &str, audience: &str) -> Sources {
        Sources::defaults(Environment::Development).with_overlay(format!(
            "security:\n  identity: \"multi-user\"\n{inbound}sources:\n  sales:\n    \
             kind: \"node\"\n    endpoint: \"https://node-b.example.com\"\n    posture: \"impersonation-at-source\"\n    \
             delegation:\n      - token_endpoint: \"https://idp.example.com/token\"\n        client_id: \"sutura\"\n        \
             client_secret_file: \"/run/secrets/idp-client\"\n        audience: \"{audience}\"\n"
        ))
    }

    const DIRECT: &str = "  inbound:\n    mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
                          authorization_server: \"https://issuer.example.com\"\n    key_set_file: \"/etc/sutura/jwks.json\"\n    \
                          algorithms: [\"RS256\"]\n";

    #[test]
    fn a_node_delegation_needs_a_direct_deployment() {
        assert_eq!(
            boot_refusals_over(&node_over("", "https://node-b.example.com")),
            vec![NotFitToServe::DelegationWithoutDirectInbound {
                alias: SourceName::parse("sales").expect("a legal identifier"),
                key: "delegation",
            }]
        );
        Settings::load(&node_over(DIRECT, "https://node-b.example.com")).expect("a direct deployment serves a node source");
    }

    #[test]
    fn a_node_hop_to_the_deployments_own_audience_is_a_typed_refusal() {
        assert_eq!(
            boot_refusals_over(&node_over(DIRECT, "https://sutura.example.com")),
            vec![NotFitToServe::NodeDelegationToTheDeploymentsOwnAudience {
                alias: SourceName::parse("sales").expect("a legal identifier")
            }]
        );
    }
}
