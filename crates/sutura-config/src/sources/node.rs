//! The `node` entry's own keys: another sutura node that runs this source's legs.
//!
//! What lives here is this kind's half of `super::entry::parse_placement`, `cube`'s shape. A node
//! source is always `impersonation-at-source` with a `delegation`: each leg reaches the other node
//! as the caller's token exchanged for that node's audience, and never as this deployment
//! (`docs/adr/20261017032054-leg-transport-between-sutura-nodes.md`, decisions 5 and 6).

use super::entry::dialled_source_keys;
use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind};
use crate::sources::placement::{InvalidDelegation, SourcePlacement};
use sutura_domain::model::SourceName;

/// Reads a `kind: node` entry into its placement: the node's `endpoint` and the `delegation` each
/// caller's token is exchanged through.
///
/// # Errors
///
/// A key that belongs to another kind; a missing `endpoint`; and the `delegation` refusals - none
/// declared, one declared on a shared entry, an endpoint that is not `https` - plus a shared entry
/// with no delegation, which no leg could be sent under.
pub(super) fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    super::refuse_foreign_keys(
        alias,
        kind,
        [
            ("data_dir", written(entry.data_dir)),
            ("max_bytes_billed", entry.max_bytes_billed.is_some()),
            ("token_file", written(entry.token_file)),
        ],
    )?;
    super::refuse_foreign_keys(alias, kind, dialled_source_keys(entry, &written))?;
    let endpoint = super::required(alias, kind, "endpoint", entry.endpoint)?;
    let delegation = super::parse_delegation(
        alias,
        kind,
        entry,
        (!endpoint.trim().starts_with("https://")).then_some(InvalidDelegation::PlaintextEndpoint),
    )?;
    if delegation.is_empty() {
        return Err(InvalidSourceRegistry::Delegation {
            alias: alias.clone(),
            cause: InvalidDelegation::NodeNotImpersonating,
        });
    }
    Ok(SourcePlacement::Node {
        endpoint: String::from(endpoint.trim()),
        delegation,
    })
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    use crate::raw::RawDelegation;
    use crate::security::DeploymentIdentity;
    use crate::sources::placement::{InvalidDelegation, SourcePlacement};
    use crate::sources::workload_identity::{DelegationDeclared, DelegationGrant};
    use crate::sources::{InvalidSourceRegistry, RawSourceEntry, SourceKind, SourceRegistry};

    const HOPS: &[RawDelegation] = &[];

    fn hop() -> RawDelegation {
        RawDelegation {
            token_endpoint: String::from("https://idp.example.com/token"),
            client_id: String::from("sutura"),
            client_secret_file: String::from("/etc/sutura/client-secret"),
            audience: String::from("https://node-b.example.com"),
            grant: DelegationGrant::TokenExchange,
        }
    }

    /// An impersonating `node` entry over `delegation`. A test changes one field at a time.
    fn node<'a>(written: &'a str, delegation: &'a [RawDelegation]) -> RawSourceEntry<'a> {
        RawSourceEntry {
            written,
            kind: "node",
            data_dir: None,
            billing_project: None,
            dataset: None,
            max_bytes_billed: None,
            posture: "impersonation-at-source",
            acknowledged_because: None,
            verification_identity: None,
            workload_identity: None,
            host: None,
            unix_socket: None,
            port: None,
            database: None,
            service_name: None,
            endpoint: Some("https://node-b.example.com"),
            token_file: None,
            database_file: None,
            user: None,
            password_file: None,
            transport_mode: None,
            transport_anchors: None,
            client_certificate: None,
            client_key: None,
            impersonate: false,
            delegation: Some(delegation),
            subjects: false,
        }
    }

    fn single_user() -> DeploymentIdentity {
        DeploymentIdentity::parse("single-user", Some("one operator, their own files, their own credentials"))
            .expect("a declared single-user mode parses")
    }

    fn parsed(entry: RawSourceEntry<'_>) -> Result<SourceRegistry, InvalidSourceRegistry> {
        SourceRegistry::parse(&[entry], Some(&single_user()))
    }

    #[test]
    fn a_node_source_declares_its_endpoint_and_its_delegation() {
        let hops = [hop()];
        let registry = parsed(node("sales", &hops)).expect("an impersonating node entry parses");
        let configured = registry
            .get(&SourceName::parse("sales").expect("a name"))
            .expect("the entry is there");
        assert_eq!(configured.kind(), SourceKind::Node);
        let SourcePlacement::Node { endpoint, delegation } = configured.placement() else {
            panic!("expected a node placement, got {:?}", configured.placement());
        };
        assert_eq!(endpoint, "https://node-b.example.com");
        assert_eq!(
            delegation.iter().map(DelegationDeclared::audience).collect::<Vec<_>>(),
            ["https://node-b.example.com"]
        );
    }

    /// The `InvalidDelegation` set the leg-transport ADR reuses for a node, plus the shared entry
    /// with no delegation at all, which no leg could be sent under. The `http://` refusal is
    /// `settings::tests::sources`'s cell, so this file is no census of the enum.
    #[test]
    fn a_node_source_is_refused_unless_it_sends_each_leg_as_its_caller() {
        let hops = [hop()];
        let shared = |delegation| RawSourceEntry {
            posture: "shared-service-user",
            acknowledged_because: Some("one identity for everybody"),
            delegation,
            ..node("sales", &hops)
        };
        for (entry, expected) in [
            (
                RawSourceEntry {
                    delegation: None,
                    ..node("sales", HOPS)
                },
                InvalidDelegation::Missing,
            ),
            (shared(Some(&hops)), InvalidDelegation::NotImpersonating),
            (shared(None), InvalidDelegation::NodeNotImpersonating),
        ] {
            match parsed(entry) {
                Err(InvalidSourceRegistry::Delegation { cause, .. }) => assert_eq!(cause, expected),
                other => panic!("expected {expected:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_key_that_means_nothing_to_a_node_source_is_refused_naming_it() {
        let hops = [hop()];
        for (entry, key) in [
            (
                RawSourceEntry {
                    token_file: Some("/etc/sutura/token"),
                    ..node("sales", &hops)
                },
                "token_file",
            ),
            (
                RawSourceEntry {
                    host: Some("127.0.0.1"),
                    ..node("sales", &hops)
                },
                "host",
            ),
        ] {
            match parsed(entry) {
                Err(InvalidSourceRegistry::KeyNotForKind { key: refused, kind, .. }) => {
                    assert_eq!((refused, kind), (key, SourceKind::Node));
                }
                other => panic!("expected `{key}` to be refused, got {other:?}"),
            }
        }
    }
}
