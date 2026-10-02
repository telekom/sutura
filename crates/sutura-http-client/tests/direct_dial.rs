#![forbid(unsafe_code)]
//! Which requests [`sutura_http_client::agent`] lets a proxy carry - `https://` to a host that is
//! not loopback, and nothing else - and that it follows no redirect.
//!
//! No request here is answered. Each one times out, and the cell reads which listener it dialled
//! from that listener's accept queue - a connection the kernel completed whether or not anything
//! answers it.

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{SocketAddr, TcpListener};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use sutura_http_client::ReadBounds;

    const UNANSWERED: Duration = Duration::from_millis(500);

    fn listener() -> TcpListener {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        listener.set_nonblocking(true).expect("the listener can be polled");
        listener
    }

    fn url(scheme: &str, listener: &TcpListener) -> String {
        format!(
            "{scheme}://{}/",
            listener.local_addr().expect("a bound listener has an address")
        )
    }

    fn dialled(listener: &TcpListener) -> bool {
        listener.accept().is_ok()
    }

    #[test]
    fn only_tls_to_a_remote_host_may_use_the_proxy_the_agent_or_the_request_carries() {
        let proxy = listener();
        let carried = ureq::Proxy::new(&url("http", &proxy)).expect("a proxy URL parses");
        let agent = sutura_http_client::agent(|config| config.timeout_global(Some(UNANSWERED)).proxy(Some(carried.clone())));

        for scheme in ["http", "https"] {
            let origin = listener();
            drop(agent.get(url(scheme, &origin)).call());
            assert!(dialled(&origin), "{scheme} to loopback was not dialled directly");
            assert!(!dialled(&proxy), "{scheme} to loopback was dialled through the agent's proxy");
        }

        let origin = listener();
        drop(agent.get(url("http", &origin)).config().proxy(Some(carried)).build().call());
        assert!(
            dialled(&origin),
            "a request-level proxy kept a loopback request off its target"
        );
        assert!(!dialled(&proxy), "a request-level proxy carried a loopback request");

        drop(agent.get("http://sutura.invalid/").call());
        assert!(
            !dialled(&proxy),
            "a plaintext request to a remote host was dialled through the proxy"
        );

        drop(agent.get("https://sutura.invalid/").call());
        assert!(
            dialled(&proxy),
            "TLS to a remote host no longer reaches the proxy the agent carries"
        );
    }

    /// Every host in `hosts`, over `https://` to a listening loopback port, is dialled without the
    /// proxy the agent carries.
    fn never_proxied(hosts: &[&str]) {
        let proxy = listener();
        let carried = ureq::Proxy::new(&url("http", &proxy)).expect("a proxy URL parses");
        let agent = sutura_http_client::agent(|config| config.timeout_global(Some(UNANSWERED)).proxy(Some(carried)));
        let origin = listener();
        let port = origin.local_addr().expect("a bound listener has an address").port();
        for host in hosts {
            drop(agent.get(format!("https://{host}:{port}/")).call());
            assert!(!dialled(&proxy), "https://{host} was dialled through the agent's proxy");
        }
    }

    #[test]
    fn localhost_in_any_spelling_is_dialled_directly() {
        never_proxied(&["localhost", "LOCALHOST", "localhost.", "sutura.localhost"]);
    }

    #[test]
    fn a_mapped_or_unspecified_address_is_dialled_directly() {
        never_proxied(&["[::ffff:127.0.0.1]", "[::]", "0.0.0.0"]);
    }

    #[test]
    fn a_loopback_address_in_resolver_shorthand_is_dialled_directly() {
        never_proxied(&["127.1", "2130706433", "0x7f.0.0.1", "0177.0.0.1", "127.0.0.1."]);
    }

    /// A plaintext loopback origin answering one request with a redirect to `target`.
    fn redirecting_to(target: &TcpListener) -> String {
        let origin = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let address = url("http", &origin);
        let location = format!("{}second", url("http", target));
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = origin.accept() {
                drop(stream.read(&mut [0_u8; 4096]));
                drop(write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                ));
            }
        });
        address
    }

    #[test]
    fn the_agent_follows_no_redirect() {
        let target = listener();
        let agent = sutura_http_client::agent(|config| config.timeout_global(Some(UNANSWERED)));
        drop(agent.get(redirecting_to(&target)).call());
        assert!(!dialled(&target), "the agent followed a redirect");
    }

    #[test]
    fn a_request_level_setting_cannot_re_enable_redirects() {
        let target = listener();
        let agent = sutura_http_client::agent(|config| config.timeout_global(Some(UNANSWERED)));
        drop(agent.get(redirecting_to(&target)).config().max_redirects(3).build().call());
        assert!(!dialled(&target), "a request-level setting re-enabled redirects");
    }

    /// A request's head, read up to its blank line.
    fn head(stream: &mut impl std::io::Read) -> String {
        let mut head = Vec::new();
        let mut byte = [0_u8; 1];
        while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|read| read == 1) {
            head.extend_from_slice(&byte);
        }
        String::from_utf8_lossy(&head).into_owned()
    }

    /// A TLS origin for `sutura.invalid` answering one request with a redirect to plaintext
    /// loopback, and the agent that trusts it.
    fn tls_redirector() -> (TcpListener, ureq::tls::TlsConfig) {
        let key = rcgen::KeyPair::generate().expect("a key pair generates");
        let issued = rcgen::CertificateParams::new([String::from("sutura.invalid")])
            .expect("a name parameterizes")
            .self_signed(&key)
            .expect("a self-signed certificate issues");
        let server = std::sync::Arc::new(
            rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("the default protocol versions are safe")
                .with_no_client_auth()
                .with_single_cert(
                    vec![issued.der().clone()],
                    rustls::pki_types::PrivateKeyDer::try_from(key.serialize_der()).expect("a generated key is usable"),
                )
                .expect("a certificate and its own key pair"),
        );
        let origin = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let serving = origin.try_clone().expect("a listener clones");
        std::thread::spawn(move || {
            if let Ok((mut tcp, _)) = serving.accept() {
                let mut connection = rustls::ServerConnection::new(server).expect("a server connection opens");
                let mut tls = rustls::Stream::new(&mut connection, &mut tcp);
                drop(head(&mut tls));
                drop(tls.write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/second\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                ));
                drop(tls.flush());
            }
        });
        let trusted = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::new_with_certs(&[ureq::tls::Certificate::from_der(
                issued.der(),
            )
            .to_owned()]))
            .build();
        (origin, trusted)
    }

    #[test]
    fn a_redirect_inside_a_proxied_tunnel_is_not_followed() {
        let (origin, trusted) = tls_redirector();
        let tunnelled_to = origin.local_addr().expect("a bound listener has an address");
        let proxy = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let carried = ureq::Proxy::new(&url("http", &proxy)).expect("a proxy URL parses");
        let (seen, connects) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Ok((mut client, _)) = proxy.accept() {
                drop(seen.send(head(&mut client)));
                drop(client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n"));
                let upstream = std::net::TcpStream::connect(tunnelled_to).expect("the origin listens");
                let (mut up, mut down) = (
                    upstream.try_clone().expect("a stream clones"),
                    client.try_clone().expect("a stream clones"),
                );
                std::thread::spawn(move || drop(std::io::copy(&mut client, &mut up)));
                std::thread::spawn(move || drop(std::io::copy(&mut &upstream, &mut down)));
            }
        });
        let agent = sutura_http_client::agent(|config| {
            config
                .timeout_global(Some(Duration::from_secs(3)))
                .proxy(Some(carried))
                .tls_config(trusted)
        });
        drop(agent.get("https://sutura.invalid/").config().max_redirects(3).build().call());
        std::thread::sleep(UNANSWERED);
        let connects: Vec<String> = connects.try_iter().collect();
        assert!(
            connects
                .first()
                .is_some_and(|line| line.starts_with("CONNECT sutura.invalid:443")),
            "TLS to a remote host reaches the proxy: {connects:?}"
        );
        assert_eq!(
            connects.len(),
            1,
            "the proxy carried a redirect the agent followed: {connects:?}"
        );
    }

    const NAME: &str = "loopback.sutura.test";

    /// Answers [`NAME`] with `first`, then `later` on every further lookup, counting them; any IP
    /// literal (the proxy's own) answers as itself.
    #[derive(Debug)]
    struct Scripted {
        first: SocketAddr,
        later: SocketAddr,
        lookups: Arc<AtomicUsize>,
    }

    impl ureq::unversioned::resolver::Resolver for Scripted {
        #[expect(clippy::disallowed_types, reason = "test: `ureq`'s own resolver trait names its `Config`")]
        fn resolve(
            &self,
            uri: &ureq::http::Uri,
            _config: &ureq::config::Config,
            _timeout: ureq::unversioned::transport::NextTimeout,
        ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
            let host = uri.host().ok_or(ureq::Error::HostNotFound)?;
            let address = if host == NAME {
                if self.lookups.fetch_add(1, Ordering::SeqCst) == 0 {
                    self.first
                } else {
                    self.later
                }
            } else {
                SocketAddr::new(
                    host.parse().map_err(|_unparsed| ureq::Error::HostNotFound)?,
                    uri.port_u16().ok_or(ureq::Error::HostNotFound)?,
                )
            };
            let mut answer = ureq::unversioned::resolver::ResolvedSocketAddrs::from_fn(|_| address);
            answer.push(address);
            Ok(answer)
        }
    }

    /// One `https://` request to [`NAME`] through an agent carrying a proxy, with [`NAME`]
    /// resolving to `origin` first and to `elsewhere` after; the number of lookups of it.
    fn resolved_to_loopback(proxy: &TcpListener, origin: &TcpListener, elsewhere: &TcpListener) -> usize {
        let address = |listener: &TcpListener| listener.local_addr().expect("a bound listener has an address");
        let lookups = Arc::new(AtomicUsize::new(0));
        let resolver = Scripted {
            first: address(origin),
            later: address(elsewhere),
            lookups: Arc::clone(&lookups),
        };
        let carried = ureq::Proxy::new(&url("http", proxy)).expect("a proxy URL parses");
        let agent = sutura_http_client::agent_resolving_through(
            |config| config.timeout_global(Some(UNANSWERED)).proxy(Some(carried)),
            resolver,
        );
        drop(agent.get(format!("https://{NAME}:{}/", address(origin).port())).call());
        lookups.load(Ordering::SeqCst)
    }

    #[test]
    fn a_name_that_resolves_to_loopback_is_dialled_directly() {
        let (proxy, origin, elsewhere) = (listener(), listener(), listener());
        assert!(
            resolved_to_loopback(&proxy, &origin, &elsewhere) > 0,
            "the name was never resolved"
        );
        assert!(
            !dialled(&proxy),
            "a name resolving to loopback was dialled through the agent's proxy"
        );
        assert!(dialled(&origin), "the loopback address the name resolved to was not dialled");
    }

    #[test]
    fn the_resolution_that_decides_the_route_is_the_one_dialled() {
        let (proxy, origin, elsewhere) = (listener(), listener(), listener());
        let lookups = resolved_to_loopback(&proxy, &origin, &elsewhere);
        assert!(!dialled(&elsewhere), "the dial resolved the name a second time");
        assert_eq!(lookups, 1, "the name was resolved more than once between route and dial");
    }

    #[test]
    fn the_shared_agent_dials_loopback_directly_under_an_environment_proxy() {
        sutura_dev::env_proxy::dialled_directly(
            module_path!(),
            "the_shared_agent_dials_loopback_directly_under_an_environment_proxy",
            || {
                let origin = listener();
                let bounds = ReadBounds::parse(1, 1024).expect("test bounds are nonzero");
                drop(
                    sutura_http_client::fixed(bounds, None)
                        .current()
                        .get(url("http", &origin))
                        .call(),
                );
                assert!(dialled(&origin), "the loopback origin was not dialled");
            },
        );
    }
}
