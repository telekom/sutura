#![forbid(unsafe_code)]

//! The dial, against loopback listeners rather than an Oracle: what the declared anchors admit, how
//! long a connect that is never answered may take, what a listener's answer is refused for, and
//! what a session opened with an asker's token sends.

#[cfg(test)]
mod dial {
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use sutura_conformance::corpus;
    use sutura_domain::identity::Secret;
    use sutura_domain::warehouse::ResultBudget;
    use sutura_exec_oracle::{Channel, Dial, OracleError, OracleWarehouse, TokenSessions};

    /// A token no other byte in a session resembles, short enough to be sent in one piece.
    const CANARY: &str = "canary-token-7c1e9a";

    /// A fake listener's side of a TLS session.
    type Tls = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

    /// The session data pairs a fake listener answers with.
    type Fields<'pair> = [(&'pair str, &'pair str)];

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

    /// A TLS server configuration presenting `certificate`.
    fn server_config(certificate: &rcgen::Certificate, key: &rcgen::KeyPair) -> Arc<rustls::ServerConfig> {
        let chain: Vec<CertificateDer<'static>> = vec![certificate.der().clone()];
        let key = PrivateKeyDer::try_from(key.serialize_der()).expect("a generated key is a private key");
        Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("the default protocol versions are safe")
                .with_no_client_auth()
                .with_single_cert(chain, key)
                .expect("a generated chain and its own key are a pair"),
        )
    }

    /// Opens TLS presenting `certificate` on a connection a fake listener accepted, so the fake
    /// speaks TNS inside it.
    fn tls(certificate: &rcgen::Certificate, key: &rcgen::KeyPair) -> impl FnOnce(TcpStream) -> Option<Tls> + Send + 'static {
        let config = server_config(certificate, key);
        move |mut stream| {
            let mut server = rustls::ServerConnection::new(config).ok()?;
            while server.is_handshaking() {
                let _io = server.complete_io(&mut stream).ok()?;
            }
            Some(rustls::StreamOwned::new(server, stream))
        }
    }

    /// The driver's configuration for a token login to `port` over TLS verified against `pem`.
    fn token_login(port: u16, pem: &str) -> oracledb::Config {
        oracledb::Config::default()
            .set_connect_string(&format!("tcps://127.0.0.1:{port}/FREEPDB1"))
            .expect("a loopback connect string parses")
            .set_trust_anchors_pem(pem)
            .expect("an issued certificate is an anchor")
            .set_external_auth(oracledb::ExternalAuth::AccessToken(String::from(CANARY)))
    }

    /// How often `needle` occurs in `haystack`.
    fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
        haystack.windows(needle.len()).filter(|window| *window == needle).count()
    }

    /// A TLS server on loopback presenting `certificate` to ONE client, reporting whether the
    /// handshake completed.
    fn tls_server(certificate: &rcgen::Certificate, key: &rcgen::KeyPair) -> (u16, mpsc::Receiver<bool>) {
        let config = server_config(certificate, key);
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

    /// The driver's own message for the connect a listener answering with ONE `packet_type` packet
    /// makes: it must come back as a typed refusal, not stop the process.
    fn answered_with(packet_type: u8, body: Vec<u8>) -> String {
        let port = sutura_dev::tns_listener::answering(packet_type, body).expect("the fake listener binds");
        let error = connect(port, Channel::Plaintext, Duration::from_secs(5)).expect_err("the connect is refused");
        assert!(matches!(error, OracleError::Connect { .. }), "{error:?}");
        std::error::Error::source(&error)
            .expect("the refusal carries the driver's cause")
            .to_string()
    }

    #[test]
    fn a_connect_answered_with_an_unknown_packet_type_is_refused_typed() {
        let cause = answered_with(3, Vec::new());
        assert!(cause.contains("unknown packet type 3"), "{cause}");
    }

    #[test]
    fn a_refuse_whose_error_number_is_not_a_number_is_refused_typed() {
        let mut body = vec![0, 0, 0, 9];
        body.extend(b"(ERR=abc)");
        let cause = answered_with(4, body);
        assert!(cause.contains("unexpected error format"), "{cause}");
    }

    /// Protocol version 315 (`0x013B`), the twelve bytes the driver skips, then the flags byte with
    /// the network-authentication-required bit set.
    #[test]
    fn an_accept_that_requires_network_encryption_is_refused_typed() {
        let mut body = vec![0x01, 0x3B];
        body.extend([0; 12]);
        body.push(0x10);
        let cause = answered_with(2, body);
        assert!(cause.contains("native network encryption"), "{cause}");
    }

    /// A connect to `port` inside `catch_unwind`, so a cell asserts on a connect that does not
    /// return in its own body.
    fn connect_catching(port: u16) -> std::thread::Result<Result<(), OracleError>> {
        std::panic::catch_unwind(|| connect(port, Channel::Plaintext, Duration::from_secs(5)).map(drop))
    }

    /// The driver's own message for a connect that must come back as a typed refusal.
    fn driver_cause(returned: Result<(), OracleError>) -> String {
        let error = returned.expect_err("the connect is refused");
        assert!(matches!(error, OracleError::Connect { .. }), "{error:?}");
        std::error::Error::source(&error)
            .expect("the refusal carries the driver's cause")
            .to_string()
    }

    /// A DATA packet whose length says eight bytes: two short of a DATA packet's header.
    #[test]
    fn a_packet_shorter_than_its_header_is_refused_typed() {
        let port = sutura_dev::tns_listener::sending(vec![0, 8, 0, 0, 6, 0, 0, 0]).expect("the fake listener binds");
        let returned = connect_catching(port).expect("the connect returns rather than unwinding");
        let cause = driver_cause(returned);
        assert!(cause.contains("shorter than its header"), "{cause}");
    }

    /// A packet whose length says six bytes: two short of any packet's header.
    #[test]
    fn a_packet_shorter_than_any_header_is_refused_typed() {
        let port = sutura_dev::tns_listener::sending(vec![0, 6, 0, 0, 2, 0]).expect("the fake listener binds");
        let returned = connect_catching(port).expect("the connect returns rather than unwinding");
        let cause = driver_cause(returned);
        assert!(cause.contains("shorter than its header"), "{cause}");
    }

    /// The same short marker with a DATA packet after it: the driver still waits for a RESET, so it
    /// discards that packet rather than answering from it, and the close after it ends the reset.
    #[test]
    fn a_marker_too_short_to_name_its_type_is_not_read_as_a_reset() {
        let port = sutura_dev::tns_listener::marking_then_data().expect("the fake listener binds");
        let returned = connect_catching(port).expect("the connect returns rather than unwinding");
        let cause = driver_cause(returned);
        assert!(cause.contains("unable to recover"), "{cause}");
    }

    /// A BREAK marker makes the driver reset the connection and wait for a RESET marker; the next
    /// marker ends before its type, so it is not one, and the listener then closes.
    #[test]
    fn a_marker_too_short_to_name_its_type_is_refused_typed() {
        let port = sutura_dev::tns_listener::marking().expect("the fake listener binds");
        let returned = connect_catching(port).expect("the connect returns rather than unwinding");
        let cause = driver_cause(returned);
        assert!(cause.contains("unable to recover"), "{cause}");
    }

    /// **A server-sent iteration count above the driver's cap is refused at once.** Deriving a key
    /// over `u32::MAX` PBKDF2 iterations would run for hours. The connect runs on its own
    /// thread, so one still deriving after twenty seconds fails this cell's own assertion.
    #[test]
    fn an_iteration_count_above_the_cap_is_refused_at_once() {
        let session = [("AUTH_PBKDF2_VGEN_COUNT", "4294967295"), ("AUTH_VFR_DATA", "00")];
        let port = sutura_dev::tns_listener::authenticating(&session).expect("the fake listener binds");
        let (told, answered) = mpsc::channel();
        drop(std::thread::spawn(move || {
            let _ignored = told.send(connect(port, Channel::Plaintext, Duration::from_secs(5)).map(drop));
        }));
        let returned = answered
            .recv_timeout(Duration::from_secs(20))
            .expect("the connect returns at once for an iteration count above the cap");
        let cause = driver_cause(returned);
        assert!(cause.contains("more than the 1048576"), "{cause}");
    }

    /// **Each verifier field the database sends is read typed, and a refusal names it.** Each row
    /// sends every field the driver reads before one as a valid value, then leaves that field out
    /// or sends it malformed: not hex, or a session key that is not two cipher blocks.
    #[test]
    fn each_malformed_verifier_field_is_refused_typed_by_its_name() {
        let vgen = ("AUTH_PBKDF2_VGEN_COUNT", "1");
        let data = ("AUTH_VFR_DATA", "00");
        let blocks = "00".repeat(32);
        let key = ("AUTH_SESSKEY", blocks.as_str());
        let sder = ("AUTH_PBKDF2_SDER_COUNT", "1");
        let rows: [(&Fields<'_>, &str); 7] = [
            (&[data], "AUTH_PBKDF2_VGEN_COUNT"),
            (&[vgen], "AUTH_VFR_DATA"),
            (&[vgen, ("AUTH_VFR_DATA", "zz")], "AUTH_VFR_DATA"),
            (&[vgen, data], "AUTH_SESSKEY"),
            (&[vgen, data, ("AUTH_SESSKEY", "00")], "AUTH_SESSKEY"),
            (&[vgen, data, key], "AUTH_PBKDF2_SDER_COUNT"),
            (&[vgen, data, key, sder], "AUTH_PBKDF2_CSK_SALT"),
        ];
        for (session, field) in rows {
            let port = sutura_dev::tns_listener::authenticating(session).expect("the fake listener binds");
            let returned = connect_catching(port).expect("the connect returns rather than unwinding");
            let cause = driver_cause(returned);
            assert!(cause.ends_with(&format!("missing or invalid {field}")), "{field}: {cause}");
        }
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

    /// **A token is never dialled in the clear, a redirect's address included.** The declared
    /// listener speaks TLS and redirects to a plaintext address. The driver follows redirects by
    /// default, and refuses this one before it dials that address.
    #[test]
    fn a_token_login_refuses_a_redirect_to_a_plaintext_address_before_dialling_it() {
        let (certificate, key) = issue();
        let listener =
            sutura_dev::tns_listener::RedirectingListener::start_over(tls(&certificate, &key)).expect("the fake listener binds");
        let refused = oracledb::connect(token_login(listener.port(), &certificate.pem()))
            .map(drop)
            .expect_err("a redirect to a plaintext address is refused");
        assert!(
            matches!(refused.kind(), oracledb::ErrorKind::ExternalAuthRequiresTcps),
            "{refused:?}"
        );
        assert!(
            !listener.target_was_dialled(Duration::from_secs(2)),
            "the plaintext address the redirect named was dialled"
        );
    }

    /// **A token login sends its token once.** The fake offers fast authentication, which carries
    /// the token's own authentication message, and answers it; the driver then sends no second one.
    #[test]
    fn a_token_login_sends_its_token_once() {
        let (certificate, key) = issue();
        let (port, sent) = sutura_dev::tns_listener::authenticating_over(tls(&certificate, &key), &[("AUTH_SESSKEY", "00")])
            .expect("the fake listener binds");
        let _refused = oracledb::connect(token_login(port, &certificate.pem()))
            .map(drop)
            .expect_err("the fake completes no login");
        let sent = sent
            .recv_timeout(Duration::from_secs(10))
            .expect("the fake reports what it read");
        assert_eq!(occurrences(&sent, CANARY.as_bytes()), 1, "the token was sent other than once");
    }

    /// **An asker's session carries their token, and its refusal does not repeat it.** The session
    /// is opened the way an impersonating source opens one for each question; the fake reads its
    /// authentication message and answers it with session data no login can use.
    #[test]
    fn an_askers_session_carries_their_token_and_its_refusal_does_not_repeat_it() {
        let (certificate, key) = issue();
        let (port, sent) = sutura_dev::tns_listener::authenticating_over(tls(&certificate, &key), &[("AUTH_SESSKEY", "00")])
            .expect("the fake listener binds");
        let pem = certificate.pem();
        let dial =
            Dial::new("127.0.0.1", port, "FREEPDB1", Channel::Verified { anchors_pem: &pem }).within(Duration::from_secs(5));
        let sessions = TokenSessions::new(dial).expect("the dial is usable");
        let refused = sessions
            .open(&Secret::new(CANARY))
            .map(drop)
            .expect_err("the fake completes no login");
        let sent = sent
            .recv_timeout(Duration::from_secs(10))
            .expect("the fake reports what it read");
        assert!(occurrences(&sent, b"AUTH_TOKEN") >= 1, "the session sent no token");
        assert!(
            occurrences(&sent, CANARY.as_bytes()) >= 1,
            "the session sent another token than the asker's"
        );
        let mut rendered = vec![format!("{refused:?}")];
        let mut cause: Option<&dyn std::error::Error> = Some(&refused);
        while let Some(error) = cause {
            rendered.push(error.to_string());
            cause = error.source();
        }
        let rendered = rendered.join(" | ");
        assert!(!rendered.contains(CANARY), "the refusal repeats the token: {rendered}");
    }

    /// **With the driver's packet trace switched on, an asker's session is refused before it
    /// dials.** The cell above runs again in a child process with the trace switched on, and fails
    /// at its dial with the refusal rather than reaching its listener.
    #[test]
    fn with_the_packet_trace_switched_on_an_askers_session_is_refused_before_it_dials() {
        let child = std::process::Command::new(std::env::current_exe().expect("the test binary has a path"))
            .args([
                "--exact",
                "dial::an_askers_session_carries_their_token_and_its_refusal_does_not_repeat_it",
            ])
            .env("RSO_DEBUG_PACKETS", "")
            .stdin(std::process::Stdio::null())
            .output()
            .expect("the test binary runs");
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(!child.status.success(), "the session ran with the packet trace on: {printed}");
        assert!(
            printed.contains("PacketTraceOn"),
            "the session failed for another reason: {printed}"
        );
    }
}
