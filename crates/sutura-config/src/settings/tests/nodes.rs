//! `server.nodes` through `Settings::load`: declared without a node authority it is refused, an
//! address that is not `https` is refused naming it, a leg size above which a question goes
//! across rides with the nodes, never as zero, and so does a bounded wait for other nodes, and the
//! family of address a peer declared by a host is dialled at.
//!
//! The refusal cells read the refusal only through what the base tree already exports - its
//! variant and `Display` - so they build there and fail on its behaviour.

use core::num::NonZeroU64;

use crate::server::TlsMaterial;
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

const PAIR: &str = "server:\n  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \"/tls/key.pem\"\n";

fn refusal(overlay: String) -> String {
    let error =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect_err("these nodes are refused");
    let SettingsError::TlsMaterial { ref cause } = *error.reason() else {
        panic!("expected a TLS material refusal, got {error:?}");
    };
    cause.to_string()
}

/// `#828`: a peer entry under `others` is now a map naming its `url`, `role` and the source
/// aliases it reads - never a bare string.
const WORKER_PEER: &str = "    others:\n      - url: \"https://b.example:8443\"\n        role: \"worker\"\n";

#[test]
fn nodes_declared_without_a_node_authority_are_refused() {
    let nodes = format!("  nodes:\n    own: \"https://a.example:8443\"\n{WORKER_PEER}");
    for overlay in [format!("{PAIR}{nodes}"), format!("server:\n{nodes}")] {
        assert_eq!(refusal(overlay), "server.nodes is set and server.tls_node_authority is not");
    }
}

#[test]
fn a_node_address_that_is_not_https_is_refused_naming_it() {
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    \
         others:\n      - url: \"http://b.example:8443\"\n        role: \"worker\"\n"
    );
    assert_eq!(
        refusal(overlay),
        "`http://b.example:8443` in server.nodes is not an https URL with a host"
    );
}

#[test]
fn a_leg_size_declared_with_the_nodes_rides_with_them_and_zero_is_refused() {
    let nodes = |above: u64| {
        Sources::defaults(Environment::Development).with_overlay(format!(
            "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n\
             {WORKER_PEER}    across_above_bytes: {above}\n\
             security:\n  tls_termination: \"in-process\"\n"
        ))
    };
    let settings = Settings::load(&nodes(4096)).expect("a declared leg size loads");
    let declared = settings
        .server()
        .tls()
        .and_then(TlsMaterial::nodes)
        .expect("the nodes are declared");
    assert_eq!(declared.across_above().map(NonZeroU64::get), Some(4096));
    let error = Settings::load(&nodes(0)).expect_err("a leg size of zero is refused");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
}

#[test]
fn a_wait_for_workers_declared_with_the_nodes_loads() {
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n\
         {WORKER_PEER}    wait_for_workers_seconds: 5\n\
         security:\n  tls_termination: \"in-process\"\n"
    );
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("a declared wait loads");
    assert!(
        settings.server().tls().and_then(TlsMaterial::nodes).is_some(),
        "the nodes are declared"
    );
}

/// `#828`: a declared peer's source alias this node's own `sources:` does not declare is a posture
/// refusal, not a parse-time one - `TlsMaterial::with_nodes` cannot see `sources:` to check it.
#[test]
fn a_declared_node_reading_an_undeclared_source_is_not_fit_to_serve() {
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    \
         others:\n      - url: \"https://b.example:8443\"\n        role: \"reading-worker\"\n        \
         sources:\n          - \"missing\"\nsecurity:\n  tls_termination: \"in-process\"\n"
    );
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("a peer reading a source this node never declares is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal naming the undeclared source, got {error:?}");
    };
    assert_eq!(
        refusals
            .iter()
            .filter(|&refusal| *refusal != NotFitToServe::InProcessTlsNotCompiledIn)
            .cloned()
            .collect::<Vec<_>>(),
        vec![NotFitToServe::DeclaredNodeReadsUndeclaredSource {
            url: String::from("https://b.example:8443/"),
            alias: String::from("missing"),
        }]
    );
}

/// `server.role` is a key a served tree may name, beside the nodes a worker serves.
#[test]
fn a_served_tree_naming_its_role_loads() {
    let sources = Sources::defaults(Environment::Development).with_overlay("server:\n  role: \"serving\"\n");
    Settings::load(&sources).expect("a tree naming the serving role loads");
}

/// A serving node, its source `local`, and one peer: a `reading-worker` reading `local`, or a
/// `worker` reading nothing - with `security` after it.
fn peered(reading: bool, security: &str) -> String {
    let peer = if reading {
        "      - url: \"https://b.example:8443\"\n        role: \"reading-worker\"\n        sources:\n          - \"local\"\n"
    } else {
        "      - url: \"https://b.example:8443\"\n        role: \"worker\"\n"
    };
    format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    others:\n{peer}\
         sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/local\"\n    posture: \"shared-service-user\"\n    \
         acknowledged_because: \"a directory of CSVs this deployment owns\"\n\
         security:\n  tls_termination: \"in-process\"\n{security}"
    )
}

/// What `overlay` is refused with, or nothing when it loads.
fn refused_with(overlay: String) -> Vec<NotFitToServe> {
    match Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)) {
        Ok(_) => Vec::new(),
        Err(error) => match *error.reason() {
            SettingsError::NotFitToServe { ref refusals } => refusals.clone(),
            ref other => panic!("expected the deployment to load or be refused at its posture, got {other:?}"),
        },
    }
}

fn reading_without_inbound() -> NotFitToServe {
    NotFitToServe::ReadingNodeWithoutInboundIdentity {
        url: String::from("https://b.example:8443/"),
        alias: String::from("local"),
    }
}

/// `#828`: a lookup leg read on a reading node is granted for a verified caller, so a deployment that
/// verifies none and declares one is refused before it serves, naming the remedy.
#[test]
fn a_reading_node_on_a_deployment_that_verifies_no_caller_is_not_fit_to_serve() {
    let refusals = refused_with(peered(true, ""));
    assert!(refusals.contains(&reading_without_inbound()), "{refusals:?}");
    let rendered = reading_without_inbound().to_string();
    assert!(rendered.contains("Declare security.inbound"), "{rendered}");
}

#[test]
fn a_reading_node_on_a_deployment_that_verifies_its_callers_is_not_refused_for_it() {
    let refusals = refused_with(peered(true, super::inbound::DIRECT_INBOUND));
    assert!(!refusals.contains(&reading_without_inbound()), "{refusals:?}");
}

#[test]
fn declared_nodes_with_no_reading_node_and_no_inbound_identity_still_serve() {
    let refusals = refused_with(peered(false, ""));
    assert!(
        !refusals
            .iter()
            .any(|refusal| matches!(refusal, NotFitToServe::ReadingNodeWithoutInboundIdentity { .. })),
        "{refusals:?}"
    );
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n\
         {WORKER_PEER}security:\n  tls_termination: \"in-process\"\n"
    );
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect("nodes with no reading node and no inbound identity still load");
}

/// `server.nodes` over `others`, each a worker, with `extra` appended under `nodes`.
fn nodes_over(others: &[&str], extra: &str) -> String {
    let peers = others
        .iter()
        .map(|url| format!("      - url: \"{url}\"\n        role: \"worker\"\n"))
        .collect::<Vec<_>>()
        .concat();
    format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    others:\n{peers}{extra}\
         security:\n  tls_termination: \"in-process\"\n"
    )
}

/// The nodes `sources` declare, as `Debug` renders them: the family is read off the one public view
/// the base tree already has, so these cells build there and fail on its refusal of the key.
fn declared_nodes(sources: &Sources) -> String {
    let settings = Settings::load(sources).expect("these nodes load");
    format!(
        "{:?}",
        settings
            .server()
            .tls()
            .and_then(TlsMaterial::nodes)
            .expect("the nodes are declared")
    )
}

fn loaded(overlay: String) -> String {
    declared_nodes(&Sources::defaults(Environment::Development).with_overlay(overlay))
}

#[test]
fn each_address_family_loads_and_rides_with_the_nodes() {
    for (written, read) in [("auto", "Auto"), ("ipv4", "Ipv4"), ("ipv6", "Ipv6")] {
        let rendered = loaded(nodes_over(
            &["https://b.example:8443"],
            &format!("    address_family: \"{written}\"\n"),
        ));
        assert!(rendered.contains(&format!("address_family: {read}")), "{written}: {rendered}");
    }
}

#[test]
fn a_node_with_no_address_family_dials_either() {
    let rendered = loaded(nodes_over(&["https://b.example:8443"], ""));
    assert!(rendered.contains("address_family: Auto"), "{rendered}");
}

#[test]
fn the_address_family_can_be_set_by_a_variable() {
    let sources = Sources::defaults(Environment::Development)
        .with_overlay(nodes_over(&["https://b.example:8443"], ""))
        .with_variables(super::variables(&[("SUTURA__SERVER__NODES__ADDRESS_FAMILY", "ipv6")]));
    assert!(declared_nodes(&sources).contains("address_family: Ipv6"));
}

#[test]
fn an_unknown_address_family_is_refused_naming_the_value_and_the_choices() {
    let overlay = nodes_over(&["https://b.example:8443"], "    address_family: \"ipv5\"\n");
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("a family that is not auto, ipv4 or ipv6 is refused");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("`ipv5` is not an address family - one of: auto, ipv4, ipv6")
            && rendered.contains("server.nodes.address_family"),
        "{rendered}"
    );
}

/// The refusal as `Display` renders it, through the variant the base tree already has.
fn declared_node_refusal(overlay: String) -> String {
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("a peer of the other family is refused");
    let SettingsError::InvalidDeclaredNode { ref cause } = *error.reason() else {
        panic!("expected a refused peer, got {error:?}");
    };
    cause.to_string()
}

#[test]
fn a_peer_given_as_an_ip_literal_of_the_other_family_is_refused_at_load() {
    for (family, url) in [
        ("ipv6", "https://192.0.2.7:8443"),
        ("ipv4", "https://[2001:db8::7]:8443"),
        ("ipv6", "https://192.0.2.7:8443#192.0.2.7"),
        ("ipv4", "https://b.example:8443#2001:db8::7"),
    ] {
        let overlay = nodes_over(&[url], &format!("    address_family: \"{family}\"\n"));
        let written = url.split_once('#').map_or(url, |(origin, _)| origin);
        let refused = declared_node_refusal(overlay);
        assert!(
            refused.contains(&format!("`{written}")) && refused.contains(&format!("address_family `{family}`")),
            "{family} {url}: {refused}"
        );
    }
}

#[test]
fn a_peer_of_the_chosen_family_and_a_host_name_load_under_each_family() {
    for (family, read, peers) in [
        ("auto", "Auto", ["https://192.0.2.7:8443", "https://[2001:db8::7]:8443"]),
        ("ipv4", "Ipv4", ["https://192.0.2.7:8443", "https://b.example:8443"]),
        ("ipv6", "Ipv6", ["https://[2001:db8::7]:8443", "https://b.example:8443"]),
    ] {
        let rendered = loaded(nodes_over(&peers, &format!("    address_family: \"{family}\"\n")));
        assert!(rendered.contains(&format!("address_family: {read}")), "{family}: {rendered}");
    }
}
