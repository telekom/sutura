//! `server.nodes` through `Settings::load`: declared without a node authority it is refused, and an
//! address that is not `https` is refused naming it.
//!
//! The cells read the refusal only through what the base tree already exports - its variant and
//! `Display` - so they build there and fail on its behaviour.

use crate::settings::{Environment, Settings, SettingsError, Sources};

const PAIR: &str = "server:\n  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \"/tls/key.pem\"\n";

fn refusal(overlay: String) -> String {
    let error =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect_err("these nodes are refused");
    let SettingsError::TlsMaterial { ref cause } = *error.reason() else {
        panic!("expected a TLS material refusal, got {error:?}");
    };
    cause.to_string()
}

#[test]
fn nodes_declared_without_a_node_authority_are_refused() {
    let nodes = "  nodes:\n    own: \"https://a.example:8443\"\n    others: [\"https://b.example:8443\"]\n";
    for overlay in [format!("{PAIR}{nodes}"), format!("server:\n{nodes}")] {
        assert_eq!(refusal(overlay), "server.nodes is set and server.tls_node_authority is not");
    }
}

#[test]
fn a_node_address_that_is_not_https_is_refused_naming_it() {
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    \
         others: [\"http://b.example:8443\"]\n"
    );
    assert_eq!(
        refusal(overlay),
        "`http://b.example:8443` in server.nodes is not an https URL with a host"
    );
}
