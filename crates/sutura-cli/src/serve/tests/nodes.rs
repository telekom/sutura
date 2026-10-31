//! Declared nodes and a worker, on a build that can plan across nodes and on one that cannot.

/// Material naming nodes at `authority`. Nothing here is read until a combiner is built from it.
fn declaring(certificate: &str, key: &str, authority: &str) -> sutura_config::TlsMaterial {
    sutura_config::TlsMaterial::parse(Some(certificate), Some(key), Some(authority))
        .expect("a pair and an authority are TLS material")
        .expect("material was declared")
        .with_nodes("https://node-a.example.com", &[String::from("https://node-b.example.com")])
        .expect("https addresses are nodes")
}

pub(in crate::serve) fn runtime() -> sutura_config::RuntimeSettings {
    sutura_config::Settings::load(&sutura_config::Sources::defaults(sutura_config::Environment::Development))
        .expect("the defaults load")
        .runtime()
}

#[cfg(not(feature = "distributed"))]
#[test]
fn declared_nodes_are_refused_by_a_build_without_the_feature() {
    let tls = declaring("/nonexistent/node.pem", "/nonexistent/node.key", "/nonexistent/authority.pem");
    let Err(refused) = super::super::nodes::combiner(Some(&tls), runtime()) else {
        panic!("declared nodes on a build without the feature planned a combine");
    };
    assert!(
        refused.contains("--features distributed"),
        "the refusal must name the feature that would plan across them: {refused}"
    );
}

/// A node certificate and key, written where `declaring` can name them, the certificate its own
/// authority.
#[cfg(feature = "distributed")]
pub(in crate::serve) fn written(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let issued = rcgen::generate_simple_self_signed([String::from("node-a.example.com")]).expect("a pair generates");
    let at = |part: &str| std::env::temp_dir().join(format!("sutura-cli-{name}-{part}-{}.pem", std::process::id()));
    let (certificate, key) = (at("certificate"), at("key"));
    std::fs::write(&certificate, issued.cert.pem()).expect("a scratch certificate is writable");
    std::fs::write(&key, issued.signing_key.serialize_pem()).expect("a scratch key is writable");
    (certificate, key)
}

#[cfg(feature = "distributed")]
#[test]
fn declared_nodes_serve_this_nodes_worker() {
    let (certificate, key) = written("declared");
    let path = |path: &std::path::Path| path.display().to_string();
    let tls = declaring(&path(&certificate), &path(&key), &path(&certificate));
    let built = super::super::nodes::combiner(Some(&tls), runtime());
    let _ignored = (std::fs::remove_file(&certificate), std::fs::remove_file(&key));
    let (_, worker) = built.expect("a signed node with its authority plans across the declared nodes");
    assert!(worker.is_some(), "declared nodes mounted no worker for the others to reach");
}

#[cfg(feature = "distributed")]
#[test]
fn an_unreadable_node_authority_is_refused_naming_it() {
    let (certificate, key) = written("unreadable");
    let path = |path: &std::path::Path| path.display().to_string();
    let missing = "/nowhere/node-authority.pem";
    let tls = declaring(&path(&certificate), &path(&key), missing);
    let built = super::super::nodes::combiner(Some(&tls), runtime());
    let _ignored = (std::fs::remove_file(&certificate), std::fs::remove_file(&key));
    let Err(refused) = built else {
        panic!("an unreadable node authority planned a combine instead of refusing");
    };
    assert!(
        refused.contains(missing),
        "the refusal must name the authority it could not read: {refused}"
    );
}

#[cfg(not(feature = "distributed"))]
#[test]
fn a_worker_is_refused_by_a_build_without_the_feature() {
    let settings = sutura_config::Settings::load(&sutura_config::Sources::defaults(sutura_config::Environment::Development))
        .expect("the defaults load");
    let Err(refused) = super::super::not_linked::worker_refused(&settings) else {
        panic!("a worker on a build without the feature was not refused");
    };
    assert!(
        refused.contains("--features distributed"),
        "the refusal must name the feature that would serve a worker: {refused}"
    );
}
