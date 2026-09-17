//! The rotation half of `security.outbound.transport_anchors` (`github.com/telekom/sutura#125` item
//! 3): cells proving that a REPLACED trust bundle is adopted by the next handshake, and that a
//! malformed one is refused with the old bundle still in use.
//!
//! **A separate file from `tls.rs` because it is a brand-new mechanism, not a second pin on the
//! existing one.** `tls.rs` proves `secured`/`load_anchors` together over a loopback server; this
//! file proves the [`sutura_tls::Rotating`] SWAP, which shares `tls.rs`'s fake-TLS helpers (all
//! `pub(super)` here, since nothing here ships). Keeping the cells in their own file is also what
//! lets `xtask test-causality` hold the file - a brand-new constructor has no base version to be red
//! against, so these cells are carried as direct unit tests of the mechanism rather than measured
//! against a stale base.
//!
//! Each cell drives `poll_once` directly rather than waiting on [`sutura_tls::POLL_INTERVAL`] - the
//! same "assert that rotation works, not that a timer fires" shape `sutura-tls`'s own suite uses.

use crate::wire::WireAgent;

use super::bounds;
use super::tls::{Scratch, dial, issue, listener, serve_one, server_config};

/// A rotating [`WireAgent`] over a declared bundle, with the poll handle a composition root would
/// drive on [`sutura_tls::POLL_INTERVAL`] - the seam these cells exercise a handshake at a time.
fn rotating(
    bounds: crate::wire::JobBounds,
    scratch: &Scratch,
    name: &str,
    root: &rcgen::Certificate,
) -> (WireAgent, sutura_tls::Rotator<ureq::Agent>) {
    let bundle = scratch.bundle(name, root);
    let (agent, rotator) = WireAgent::rotating_agent(bounds, Some(sutura_tls::Anchors::Bundle(bundle)))
        .expect("the freshly written bundle builds a rotating handle");
    (
        WireAgent::rotating(bounds, agent),
        rotator.expect("a declared bundle returns a poll handle"),
    )
}

#[test]
fn a_replaced_bundle_is_adopted_by_the_next_handshake() {
    // The red/green heart of `github.com/telekom/sutura#125` item 3: replace the declared bundle on
    // disk, rotate, and the next request trusts the NEW CA - and no longer the old one. Without the
    // `Rotating` swap this cell's last assertion fails, because the rebuilt request would still
    // trust ca-a.
    let scratch = Scratch::new("rotate-adopt");
    let ca_a = issue();
    let ca_b = issue();
    let (agent, mut rotator) = rotating(bounds(), &scratch, "root", &ca_a.certificate);

    // Before the swap, a chain signed by ca-a is trusted.
    let (listener_a, port_a) = listener();
    let server_a = serve_one(listener_a, server_config(&ca_a));
    dial(&agent, port_a).expect("ca-a is trusted before the bundle is replaced");
    server_a.join().expect("the ca-a server thread does not panic");

    // Replace the bundle bytes with ca-b and let the rotator look.
    std::fs::write(scratch.path("root"), ca_b.certificate.pem()).expect("the bundle rewrites with ca-b");
    assert_eq!(rotator.poll_once(), sutura_tls::Outcome::Rotated);

    // The NEXT handshake trusts ca-b (adopted the new bundle)...
    let (listener_b, port_b) = listener();
    let server_b = serve_one(listener_b, server_config(&ca_b));
    dial(&agent, port_b).expect("ca-b is adopted and trusted after the swap");
    server_b.join().expect("the ca-b server thread does not panic");

    // ...and refuses the OLD issuer: the swap really happened, it did not just add a root.
    let (listener_c, port_c) = listener();
    let server_c = serve_one(listener_c, server_config(&ca_a));
    let refused = dial(&agent, port_c);
    server_c.join().expect("the ca-a server thread does not panic");
    refused.expect_err("after the swap the previous CA is no longer trusted");
}

#[test]
fn a_malformed_replacement_keeps_the_old_material_and_still_trusts_only_the_old_ca() {
    // The refuse half has no handshake to fail visibly (a per-request agent keeps serving the old
    // one), so the claim is that the NEXT handshake still succeeds under the OLD CA after a
    // malformed replacement is rejected - `sutura-tls`'s own suite pins the exactly-one audit line.
    let scratch = Scratch::new("rotate-malformed");
    let ca_a = issue();
    let (agent, mut rotator) = rotating(bounds(), &scratch, "root", &ca_a.certificate);

    std::fs::write(scratch.path("root"), b"this is not a certificate").expect("prose overwrites the bundle");
    assert_eq!(rotator.poll_once(), sutura_tls::Outcome::Rejected);

    let (listener, port) = listener();
    let server = serve_one(listener, server_config(&ca_a));
    dial(&agent, port).expect("the old material is still in use, so the old CA is still trusted");
    server.join().expect("the fake server thread does not panic");
}
