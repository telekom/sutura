//! Which client certificates a listener with a node authority accepts.

use std::path::Path;

use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use sutura_config::TlsMaterial;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use super::super::fixture::{Pair, Scratch, atomically, client_presenting, exchange, generate, get_over_tls, write};
use super::super::{Termination, TlsListener, TlsNotUsable};

/// An authority's certificate, and a node certificate it signed with that node's key.
struct Authority {
    certificate: String,
    node: CertificateDer<'static>,
    key: Vec<u8>,
}

impl Authority {
    fn generate() -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("no names are valid parameters");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let issuer = CertifiedIssuer::self_signed(params, KeyPair::generate().expect("a key generates"))
            .expect("an authority signs itself");
        let key = KeyPair::generate().expect("a key generates");
        let node = CertificateParams::new(vec![String::from("node-a")])
            .expect("a name is valid parameters")
            .signed_by(&key, &issuer)
            .expect("the authority signs a node");
        Self {
            certificate: issuer.pem(),
            node: node.der().clone(),
            key: key.serialize_der(),
        }
    }

    fn presented_to(&self, server: &Pair) -> std::sync::Arc<tokio_rustls::rustls::ClientConfig> {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()));
        client_presenting(&server.certificate, vec![self.node.clone()], key)
    }
}

/// `pair` as the listener's own, and `authority` as the file its node authority is read from.
fn with_authority(directory: &Path, pair: &Pair, authority: &str) -> TlsMaterial {
    let material = write(directory, pair);
    let path = directory.join("authority.pem");
    atomically(&path, authority.as_bytes());
    TlsMaterial::parse(material.certificate().to_str(), material.key().to_str(), path.to_str())
        .expect("a pair and an authority are usable together")
        .expect("a pair is material")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_the_authority_signed_and_a_caller_with_none_get_in_and_another_authoritys_node_does_not() {
    let scratch = Scratch::new("node-authority");
    let pair = generate();
    let ours = Authority::generate();
    let termination = Termination::prepare(&with_authority(scratch.path(), &pair, &ours.certificate))
        .expect("a generated authority is usable material");
    let tcp = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a loopback port is bindable");
    let port = tcp.local_addr().expect("a bound socket has an address").port();
    let listener = TlsListener::wrap(tcp, termination.config()).expect("the listener wraps");
    let router = axum::Router::new().route("/health", axum::routing::get(|| async { "ok" }));
    let serving = tokio::spawn(async move {
        let _outcome = axum::serve(
            axum::serve::ListenerExt::tap_io(listener, |_io| ()),
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });

    let signed = exchange(port, "/health", ours.presented_to(&pair)).await;
    assert!(
        signed.as_deref().is_ok_and(|answer| answer.starts_with("HTTP/1.1 200 OK")),
        "{signed:?}"
    );
    let anonymous = get_over_tls(port, "/health", &pair.certificate).await;
    assert!(anonymous.starts_with("HTTP/1.1 200 OK"), "{anonymous}");
    // Without the authority the listener asks for no certificate, so this node's is never sent and
    // the request is answered.
    let foreign = exchange(port, "/health", Authority::generate().presented_to(&pair)).await;
    assert!(foreign.is_err(), "{foreign:?}");
    serving.abort();
}

#[test]
fn an_authority_file_with_no_usable_certificate_is_refused_before_the_socket_binds() {
    let scratch = Scratch::new("no-authority");
    let pair = generate();
    let prose = Termination::prepare(&with_authority(scratch.path(), &pair, "this is not a certificate\n"))
        .expect_err("prose is not an authority");
    assert!(matches!(prose, TlsNotUsable::NoCertificate { .. }), "{prose:?}");
    let undecodable = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
    let undecodable = Termination::prepare(&with_authority(scratch.path(), &pair, undecodable))
        .expect_err("three zero bytes are not a certificate");
    assert!(matches!(undecodable, TlsNotUsable::NotAnAuthority { .. }), "{undecodable:?}");
    assert!(format!("{undecodable}").contains("authority.pem"), "{undecodable}");
}
