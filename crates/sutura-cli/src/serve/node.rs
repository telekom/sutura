//! The `node` half of this composition root: another sutura node that runs a source's legs.
//!
//! A node source runs only legs of a federated answer, so it opens only inside the mixed registry
//! ([`open_mixed`](crate::serve::kind::open_mixed)); `sutura query` and `sutura mcp` refuse it,
//! having no verified caller whose token a leg could carry. Nothing is dialled at boot: the first
//! leg is the first connection.

use sutura_domain::model::SourceName;

use super::kind::AnyWarehouse;

/// Opens one node client per declared source, trusting `security.outbound.transport_anchors` when
/// declared and the host's store otherwise, and holding at most the working-set ceiling of one leg's
/// answer.
#[cfg(feature = "node")]
pub(super) fn node_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    use sutura_domain::warehouse::Warehouse as _;

    let mut declared = Vec::with_capacity(sources.len());
    for source in sources {
        let configured = super::configured_source(source, registry)?;
        let sutura_config::SourcePlacement::Node { ref endpoint, .. } = *configured.placement() else {
            return Err(format!(
                "`sources.{source}` reached the node step with another kind's placement"
            ));
        };
        let identity = configured
            .identity()
            .ok_or_else(|| format!("`sources.{source}` declares no identity a leg could run under"))?;
        identity
            .posture()
            .deliverable_by(sutura_exec_node::NodeWarehouse::IMPERSONATION, source)
            .map_err(|cause| crate::commands::render(&cause))?;
        let endpoint = sutura_exec_node::NodeEndpoint::parse(endpoint)
            .map_err(|cause| format!("`sources.{source}.endpoint` is not a usable node endpoint: {cause}"))?;
        declared.push((*source, identity.posture().clone(), endpoint));
    }
    let anchors = sutura_tls::load_anchors(outbound.map_or(&sutura_tls::Anchors::System, sutura_tls::Declared::anchors))
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    let budget = sutura_domain::warehouse::arrow::ResultBudget::of_bytes(runtime.working_set().bytes());
    let mut engines: Option<sutura_app::Warehouses<AnyWarehouse>> = None;
    for (source, posture, endpoint) in declared {
        let engine = sutura_exec_node::NodeWarehouse::open(source.clone(), posture, &endpoint, anchors.clone(), budget)
            .map_err(|cause| format!("`sources.{source}` could not be opened as a node source: {cause}"))?;
        let engine = AnyWarehouse::Node(Box::new(engine));
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    engines.ok_or_else(|| String::from("no node source was declared"))
}

/// The refusal for a build that did not link the node client.
#[cfg(not(feature = "node"))]
pub(super) fn node_group(
    sources: &[&SourceName],
    _registry: &sutura_config::SourceRegistry,
    _runtime: sutura_config::RuntimeSettings,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let named = sources.iter().map(|source| source.as_str()).collect::<Vec<&str>>().join(", ");
    Err(format!(
        "[{named}] declares `kind: node`, and this binary was built without the `node` feature - so it \
         links no node client. Build `sutura-cli` with `--features node`, or declare the source here"
    ))
}

/// The hops a `node` source's `delegation` declares, in order - empty for any other placement.
#[cfg(feature = "node")]
pub(super) fn declared_delegation(
    configured: &sutura_config::ConfiguredSource,
) -> &[sutura_config::sources::workload_identity::DelegationDeclared] {
    let sutura_config::SourcePlacement::Node { ref delegation, .. } = *configured.placement() else {
        return &[];
    };
    delegation
}

#[cfg(test)]
mod tests {
    use crate::serve::tests::{bundle_over, default_timeout, direct_registry, entry, one_worker, refusal};
    use crate::serve::{ENGINE_SOURCE, open_engine};

    /// A `files` source beside a `node` source at `endpoint`, on a `direct` deployment.
    fn beside_a_node(endpoint: &str) -> sutura_config::SourceRegistry {
        direct_registry(&format!(
            "{}  sales:\n    kind: \"node\"\n    endpoint: \"{endpoint}\"\n    posture: \"impersonation-at-source\"\n    \
             delegation:\n      - token_endpoint: \"https://idp.example.com/token\"\n        client_id: \"sutura\"\n        \
             client_secret_file: \"/nonexistent/idp-client\"\n        audience: \"https://node-b.example.com\"\n",
            entry(ENGINE_SOURCE, "shared-service-user", "")
        ))
    }

    fn opened(endpoint: &str, outbound: Option<&sutura_tls::Declared>) -> Result<crate::serve::OpenedSources, String> {
        open_engine(
            &bundle_over(&[
                ("customers", ENGINE_SOURCE, "dim_customer"),
                ("products", "sales", "dim_product"),
            ]),
            &beside_a_node(endpoint),
            one_worker(),
            default_timeout(),
            outbound,
        )
    }

    /// Run by `cargo xtask check-default-feature-tests`; `--all-features` makes this cfg false.
    #[test]
    #[cfg(not(feature = "node"))]
    fn a_node_source_is_refused_by_a_build_that_did_not_link_the_client() {
        let error = refusal(opened("https://node-b.example.com", None), "a build without the node client");
        assert!(error.contains("sales") && error.contains("--features node"), "{error}");
    }

    #[test]
    #[cfg(feature = "node")]
    fn a_node_endpoint_the_client_cannot_dial_is_refused_at_boot() {
        let error = refusal(
            opened("https://user@node-b.example.com", None),
            "credentials in a node endpoint",
        );
        assert!(error.contains("`sources.sales.endpoint`"), "{error}");
    }

    /// The source opens inside the mix, dials nothing, and keeps its relay declaration through the
    /// registry's own enum - decision 9 at the other node reads it there.
    #[test]
    #[cfg(feature = "node")]
    fn a_node_source_opens_in_the_mix_and_declares_that_it_relays() {
        use sutura_domain::warehouse::Warehouse as _;

        let issued = rcgen::generate_simple_self_signed([String::from("node-b.example.com")]).expect("a pair generates");
        let bundle = std::env::temp_dir().join(format!("sutura-cli-node-anchor-{}.pem", std::process::id()));
        std::fs::write(&bundle, issued.cert.pem()).expect("a scratch bundle is writable");
        let outbound = sutura_tls::Declared::new(sutura_tls::Anchors::Bundle(bundle.clone()), None);
        let opened = opened("https://node-b.example.com", Some(&outbound));
        let _ignored = std::fs::remove_file(&bundle);
        let Ok(crate::serve::OpenedSources::Mixed(mixed)) = opened else {
            panic!("a files source beside a node source opens as a mix");
        };
        let node = mixed
            .engines
            .get(&sutura_domain::model::SourceName::parse("sales").expect("a name"))
            .expect("the node source is in the registry");
        assert!(node.relays_to_a_node() && node.executes_legs());
        let files = mixed
            .engines
            .get(&sutura_domain::model::SourceName::parse(ENGINE_SOURCE).expect("a name"))
            .expect("the files source is in the registry");
        assert!(!files.relays_to_a_node());
    }
}
