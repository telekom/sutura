//! A `reading-worker`'s own boot: [`super::open_reading_sources`] over a real declared catalog and
//! source, the way `super::run` calls it before serving.

use std::path::Path;

fn written(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let issued = rcgen::generate_simple_self_signed([String::from("reading-worker.example.com")]).expect("a pair generates");
    let at =
        |part: &str| std::env::temp_dir().join(format!("sutura-cli-reading-worker-{name}-{part}-{}.pem", std::process::id()));
    let (certificate, key) = (at("certificate"), at("key"));
    std::fs::write(&certificate, issued.cert.pem()).expect("a scratch certificate is writable");
    std::fs::write(&key, issued.signing_key.serialize_pem()).expect("a scratch key is writable");
    (certificate, key)
}

/// A reading worker opens the catalog and the source it declares, through the exact constructors a
/// serving node's own boot uses.
#[test]
fn a_reading_worker_opens_its_declared_catalog_and_source() {
    let (certificate, key) = written("opens");
    let path = |path: &Path| path.display().to_string();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player");
    let overlay = format!(
        "server:\n  role: \"reading-worker\"\n  tls_certificate: \"{certificate}\"\n  tls_key: \"{key}\"\n  \
         tls_node_authority: \"{certificate}\"\nsecurity:\n  tls_termination: \"in-process\"\n  identity: \
         \"single-user\"\n  single_user_because: \"the test deployment reads its own fixture files\"\ncatalogs:\n  \
         - name: \"model\"\n    kind: \"markdown\"\n    dir: \"{catalog}\"\n    data_dir: \"{data}\"\n    version: \
         \"test\"\nsources:\n  local:\n    kind: \"files\"\n    data_dir: \"{data}\"\n    posture: \
         \"shared-service-user\"\n",
        certificate = path(&certificate),
        key = path(&key),
        catalog = example.join("catalog").display(),
        data = example.join("data").display(),
    );
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("a reading worker with a declared catalog and source loads");
    let opened = super::open_reading_sources(&settings);
    let _ignored = (std::fs::remove_file(&certificate), std::fs::remove_file(&key));
    opened.expect("a reading worker opens its declared catalog and source at boot");
}
