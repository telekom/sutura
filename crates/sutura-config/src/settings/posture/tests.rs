//! `server.nodes.wait_for_workers_seconds` through `Settings::load`: a wait below the request
//! timeout rides with the nodes, and one that is not is refused.

use core::num::NonZeroU32;

use crate::server::TlsMaterial;
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

fn waiting(seconds: u32) -> Sources {
    Sources::defaults(Environment::Development).with_overlay(format!(
        "server:\n  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \"/tls/key.pem\"\n  \
         tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    \
         others: [\"https://b.example:8443\"]\n    wait_for_workers_seconds: {seconds}\n\
         security:\n  tls_termination: \"in-process\"\n"
    ))
}

#[test]
fn a_wait_below_the_request_timeout_rides_with_the_nodes_and_one_not_below_it_is_refused() {
    let settings = Settings::load(&waiting(29)).expect("a wait below the timeout loads");
    let declared = settings
        .server()
        .tls()
        .and_then(TlsMaterial::nodes)
        .expect("the nodes are declared");
    assert_eq!(declared.wait_for_workers(), NonZeroU32::new(29));

    // The embedded default timeout is 30 seconds: a wait that long could never answer in time.
    let error = Settings::load(&waiting(30)).expect_err("a wait as long as the timeout is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::WorkersWaitNotBelowDeadline {
            wait_seconds: 30,
            timeout_seconds: 30
        }]
    );
}
