#![forbid(unsafe_code)]

//! The dial, against loopback listeners rather than an Oracle: what the declared anchors admit, and
//! how long a connect that is never answered may take.

#[cfg(test)]
mod dial {
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use sutura_conformance::corpus;
    use sutura_domain::warehouse::ResultBudget;
    use sutura_exec_oracle::{Channel, Dial, OracleError, OracleWarehouse};

    fn connect(port: u16, channel: Channel<'_>, deadline: Duration) -> Result<OracleWarehouse, OracleError> {
        let dial = Dial::new("127.0.0.1", port, "FREEPDB1", channel).within(deadline);
        let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024).expect("a test budget is positive"));
        OracleWarehouse::connect(
            corpus::source(),
            corpus::posture(),
            dial,
            "sutura",
            "not-a-real-password",
            budget,
        )
    }

    /// A leaf for `127.0.0.1`, self-signed, so the PEM of the leaf is the anchor that admits it.
    fn issue() -> (rcgen::Certificate, rcgen::KeyPair) {
        let params = rcgen::CertificateParams::new([String::from("127.0.0.1")]).expect("an IP name parameterizes");
        let key = rcgen::KeyPair::generate().expect("a key pair generates");
        let certificate = params.self_signed(&key).expect("a self-signed leaf signs");
        (certificate, key)
    }

    /// A TLS server on loopback presenting `certificate` to ONE client, reporting whether the
    /// handshake completed.
    fn tls_server(certificate: &rcgen::Certificate, key: &rcgen::KeyPair) -> (u16, mpsc::Receiver<bool>) {
        let chain: Vec<CertificateDer<'static>> = vec![certificate.der().clone()];
        let key = PrivateKeyDer::try_from(key.serialize_der()).expect("a generated key is a private key");
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("the default protocol versions are safe")
                .with_no_client_auth()
                .with_single_cert(chain, key)
                .expect("a generated chain and its own key are a pair"),
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port binds");
        let port = listener.local_addr().expect("it has an address").port();
        let (told, handshake) = mpsc::channel();
        drop(std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let _ignored = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let Ok(mut server) = rustls::ServerConnection::new(config) else {
                return;
            };
            let mut completed = true;
            while server.is_handshaking() {
                if server.complete_io(&mut stream).is_err() {
                    completed = false;
                    break;
                }
            }
            let _ignored = told.send(completed);
        }));
        (port, handshake)
    }

    /// **The declared anchor admits the server, and an undeclared one does not.** The server's leaf is
    /// issued by no public authority, so the first case completes only if the declared PEM is what the
    /// driver verified against, and the second shows a valid certificate under any other anchor is
    /// refused. Neither connection gets past the handshake to a login: the fake speaks no TNS.
    #[test]
    fn only_the_declared_anchor_admits_a_tls_listener() {
        let (certificate, key) = issue();
        let (port, handshake) = tls_server(&certificate, &key);
        let declared = certificate.pem();
        let _closed = connect(port, Channel::Verified { anchors_pem: &declared }, Duration::from_secs(5));
        assert_eq!(
            handshake.recv_timeout(Duration::from_secs(10)),
            Ok(true),
            "the declared anchor did not admit the server it issued"
        );

        let (port, handshake) = tls_server(&certificate, &key);
        let (other, _other_key) = issue();
        let undeclared = other.pem();
        let _refused = connect(
            port,
            Channel::Verified {
                anchors_pem: &undeclared,
            },
            Duration::from_secs(5),
        )
        .expect_err("a server no declared anchor issued is refused");
        assert_eq!(
            handshake.recv_timeout(Duration::from_secs(10)),
            Ok(false),
            "a server no declared anchor issued completed the handshake"
        );
    }

    #[test]
    fn declared_anchors_with_no_certificate_are_refused_typed() {
        let error = connect(
            1,
            Channel::Verified {
                anchors_pem: "no certificate here",
            },
            Duration::from_secs(1),
        )
        .expect_err("a bundle with no certificate is refused");
        assert!(matches!(error, OracleError::TrustAnchors { .. }), "{error:?}");
    }

    /// **A connect that is never answered is refused within the deadline.** A listener whose accept
    /// queue is full drops further connection requests, so the client's connect waits on the
    /// operating system's own retry schedule - a minute or more - unless the dial is bounded.
    #[test]
    fn a_connect_nobody_answers_is_refused_within_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port binds");
        rustix::net::listen(&listener, 0).expect("the backlog shrinks");
        let address = listener.local_addr().expect("it has an address");
        let mut queued = Vec::new();
        while let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_millis(200)) {
            queued.push(stream);
            assert!(queued.len() < 256, "the accept queue never filled");
        }

        let (told, finished) = mpsc::channel();
        let port = address.port();
        drop(std::thread::spawn(move || {
            let started = Instant::now();
            let refused = connect(port, Channel::Plaintext, Duration::from_secs(1)).is_err();
            let _ignored = told.send((refused, started.elapsed()));
        }));
        let (refused, elapsed) = finished
            .recv_timeout(Duration::from_secs(20))
            .expect("the dial returned within twenty seconds");
        assert!(refused, "a connect nobody answered opened a warehouse");
        assert!(
            elapsed < Duration::from_secs(5),
            "the dial took {elapsed:?} against a one-second deadline"
        );
        drop(listener);
    }
}
