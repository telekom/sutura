#![forbid(unsafe_code)]
//! Which requests [`sutura_http_client::agent`] lets a proxy carry: `https://` to a host that is not
//! an IP loopback literal, and nothing else.
//!
//! No request here is answered. Each one times out, and the cell reads which listener it dialled
//! from that listener's accept queue - a connection the kernel completed whether or not anything
//! answers it.

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
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
