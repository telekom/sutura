//! `security.outbound.client_certificate`/`client_key` (`github.com/telekom/sutura#911`): cells
//! proving a declared deployment-wide client identity is actually PRESENTED at the handshake, and
//! that a rotation which cannot load one leaves the last good identity in use.
//!
//! **The limit stated once, here, rather than at every cell.** These prove PRESENTATION: a mutual
//! peer this test builds demands and checks a certificate, and the declared identity is what it
//! sees. They do not prove that any SHIPPED source verifies one - `HOST` takes a bearer token, not
//! a client certificate, and no shipped source is configured to demand one either. See
//! `wire.rs`'s own module header for that same sentence at the claim.
//!
//! Reuses `tls.rs`'s fixtures (a real loopback listener, a real `rustls::ServerConnection`) the
//! same way `rotation.rs` does, plus [`super::tls::server_config_requiring_client_auth`] for the
//! one thing neither of those needed: a server that REQUIRES a client certificate.

use crate::wire::WireAgent;

use super::bounds;
use super::tls::{Issued, Scratch, dial, issue, listener, serve_one, server_config_requiring_client_auth};

/// A rotating [`WireAgent`] declaring both the server's trust anchor and a client identity, plus
/// the certificate path a cell can corrupt in place to exercise the fail-closed rotation.
fn rotating_with_identity(
    scratch: &Scratch,
    server_root: &rcgen::Certificate,
    client: &Issued,
) -> (WireAgent, sutura_tls::Rotator<ureq::Agent>, std::path::PathBuf) {
    let anchors_bundle = scratch.bundle("anchor", server_root);
    let (certificate, key) = scratch.identity("client", client);
    let declared = sutura_tls::Declared::new(
        sutura_tls::Anchors::Bundle(anchors_bundle),
        Some(sutura_tls::Identity::new(certificate.clone(), key)),
    );
    let (agent, rotator) = WireAgent::rotating_agent(bounds(), Some(declared))
        .expect("a declared anchor and client identity build a rotating handle");
    (
        WireAgent::rotating(bounds(), agent),
        rotator.expect("a declared anchor returns a poll handle"),
        certificate,
    )
}

#[test]
fn a_declared_identity_lets_a_mutual_peer_accept_the_handshake() {
    // The red/green heart of #911: without this change `rotating_agent` never reads
    // `client_certificate`/`client_key` at all, so the agent it builds presents nothing and this
    // mutual-only peer refuses every dial - this cell fails against base.
    let scratch = Scratch::new("identity-present");
    let server = issue();
    let client = issue();
    let (agent, _rotator, _certificate) = rotating_with_identity(&scratch, &server.certificate, &client);

    let (listener, port) = listener();
    let mutual = serve_one(listener, server_config_requiring_client_auth(&server, &client.certificate));
    let answered = dial(&agent, port);
    mutual.join().expect("the fake server thread does not panic");
    answered.expect("a declared client identity is presented and the mutual peer accepts it");
}

#[test]
fn no_declared_identity_is_refused_by_a_mutual_peer() {
    // The control for the cell above: this same mutual peer, dialed by an agent with NO declared
    // identity, must be refused - otherwise the positive cell could be passing because the peer
    // accepts anything, not because the identity was presented.
    let scratch = Scratch::new("identity-absent");
    let server = issue();
    let client_root = issue();
    let anchors_bundle = scratch.bundle("anchor", &server.certificate);
    let declared = sutura_tls::Declared::new(sutura_tls::Anchors::Bundle(anchors_bundle), None);
    let (agent, _rotator) =
        WireAgent::rotating_agent(bounds(), Some(declared)).expect("a declared anchor builds a rotating handle");
    let agent = WireAgent::rotating(bounds(), agent);

    let (listener, port) = listener();
    let mutual = serve_one(
        listener,
        server_config_requiring_client_auth(&server, &client_root.certificate),
    );
    let answered = dial(&agent, port);
    mutual.join().expect("the fake server thread does not panic");
    answered.expect_err("a mutual peer must refuse a client presenting no certificate");
}

#[test]
fn a_malformed_identity_replacement_keeps_the_old_identity_presented() {
    // The limit stated in #911's brief: "a rotation that fails validation must leave the last good
    // material in use" - the SAME rule `rotation.rs` pins for the anchors, now pinned for the
    // identity half `poll_once` also refuses to adopt.
    let scratch = Scratch::new("identity-malformed");
    let server = issue();
    let client = issue();
    let (agent, mut rotator, certificate) = rotating_with_identity(&scratch, &server.certificate, &client);

    // Before the corruption: the declared identity is presented and the mutual peer accepts it.
    let (listener_a, port_a) = listener();
    let mutual_a = serve_one(listener_a, server_config_requiring_client_auth(&server, &client.certificate));
    dial(&agent, port_a).expect("the declared identity is presented and accepted");
    mutual_a.join().expect("the fake server thread does not panic");

    // Overwrite the certificate FILE with prose - a malformed replacement, exactly as
    // `rotation.rs`'s anchor cell does for the trust bundle.
    std::fs::write(&certificate, b"this is not a certificate").expect("prose overwrites the certificate file");
    assert_eq!(rotator.poll_once(), sutura_tls::Outcome::Rejected);

    // The NEXT handshake still presents the OLD identity: the refused rotation left the last good
    // material in use, so the same mutual peer still accepts it.
    let (listener_b, port_b) = listener();
    let mutual_b = serve_one(listener_b, server_config_requiring_client_auth(&server, &client.certificate));
    dial(&agent, port_b).expect("the old identity is still presented after the rejected rotation");
    mutual_b.join().expect("the fake server thread does not panic");
}
