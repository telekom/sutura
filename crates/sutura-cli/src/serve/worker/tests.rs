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
