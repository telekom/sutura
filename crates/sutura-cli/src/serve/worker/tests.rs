//! A worker built from its pair and node authority, with no `server.nodes` declared.

#[test]
fn a_worker_builds_from_its_pair_and_node_authority_with_no_nodes_declared() {
    let issued = rcgen::generate_simple_self_signed([String::from("worker.example.com")]).expect("a pair generates");
    let at = |part: &str| std::env::temp_dir().join(format!("sutura-cli-worker-{part}-{}.pem", std::process::id()));
    let (certificate, key) = (at("certificate"), at("key"));
    std::fs::write(&certificate, issued.cert.pem()).expect("a scratch certificate is writable");
    std::fs::write(&key, issued.signing_key.serialize_pem()).expect("a scratch key is writable");
    let path = |path: &std::path::Path| path.display().to_string();
    let tls = sutura_config::TlsMaterial::parse(Some(&path(&certificate)), Some(&path(&key)), Some(&path(&certificate)))
        .expect("a pair and an authority are TLS material")
        .expect("material was declared");
    assert!(tls.nodes().is_none(), "this cell is about a worker with no server.nodes");
    let runtime = sutura_config::Settings::load(&sutura_config::Sources::defaults(sutura_config::Environment::Development))
        .expect("the defaults load")
        .runtime();
    let built = super::mount(&tls, &certificate, runtime);
    let _ignored = (std::fs::remove_file(&certificate), std::fs::remove_file(&key));
    let _mount = built.expect("a signed node with its authority builds the worker it serves");
}

/// A stage whose length prefix declares a mebibyte, sent to a worker whose working set is 2048 bytes
/// by a node the authority signed, over the listener a worker serves on.
#[test]
fn a_message_declaring_more_than_the_working_set_is_refused_before_it_is_read() {
    let issued = rcgen::generate_simple_self_signed([String::from("127.0.0.1")]).expect("a pair generates");
    let (certificate_pem, key_pem) = (issued.cert.pem(), issued.signing_key.serialize_pem());
    let at = |part: &str| std::env::temp_dir().join(format!("sutura-cli-worker-cap-{part}-{}.pem", std::process::id()));
    let (certificate, key) = (at("certificate"), at("key"));
    std::fs::write(&certificate, &certificate_pem).expect("a scratch certificate is writable");
    std::fs::write(&key, &key_pem).expect("a scratch key is writable");
    let path = |path: &std::path::Path| path.display().to_string();
    let tls = sutura_config::TlsMaterial::parse(Some(&path(&certificate)), Some(&path(&key)), Some(&path(&certificate)))
        .expect("a pair and an authority are TLS material")
        .expect("material was declared");
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development)
            .with_overlay("runtime:\n  working_set_max_bytes: 2048\n"),
    )
    .expect("a small working set loads");
    let mount = super::mount(&tls, &certificate, settings.runtime()).expect("the worker builds");
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|free| free.local_addr())
        .expect("a loopback port is free");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime builds");
    let router = runtime
        .block_on(async { sutura_http::worker_router(&settings, &mount) })
        .expect("the worker router builds");
    drop(runtime.spawn(async move { sutura_http::serve_tls(router, address, sutura_runtime::Shutdown::new(), &tls).await }));
    let listening = (0..250).any(|_| {
        std::thread::sleep(core::time::Duration::from_millis(20));
        std::net::TcpStream::connect(address).is_ok()
    });
    assert!(listening, "the worker never listened on {address}");

    let presented = ureq::tls::Certificate::from_pem(certificate_pem.as_bytes()).expect("a generated certificate parses");
    let signing = ureq::tls::PrivateKey::from_pem(key_pem.as_bytes()).expect("a generated key parses");
    let agent = sutura_http_client::agent(|config| {
        config
            .http_status_as_error(false)
            .timeout_global(Some(core::time::Duration::from_secs(10)))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::new_with_certs(std::slice::from_ref(&presented)))
                    .client_cert(Some(ureq::tls::ClientCert::new_with_certs(&[presented], signing)))
                    .build(),
            )
    });
    let answered = agent
        .post(format!("https://{address}/worker.WorkerService/CoordinatorChannel"))
        .header("content-type", "application/grpc")
        .send(&[0_u8, 0, 0x10, 0, 0][..]);
    let _ignored = (std::fs::remove_file(&certificate), std::fs::remove_file(&key));
    let answered = answered.expect("the worker answers a node");
    let header = |name: &str| {
        answered
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(
        header("grpc-status"),
        "11",
        "a mebibyte was read into a 2048-byte worker: {}",
        header("grpc-message")
    );
    runtime.shutdown_background();
}
