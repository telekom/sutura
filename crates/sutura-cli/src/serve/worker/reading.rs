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
    // A file, not an overlay: a catalog only an overlay declares is not the worker's own.
    let directory = std::env::temp_dir().join(format!("sutura-cli-reading-worker-opens-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a scratch configuration directory is creatable");
    std::fs::write(directory.join("base.yaml"), overlay).expect("a scratch configuration is writable");
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_directory(directory.clone()),
    )
    .expect("a reading worker with a declared catalog and source loads");
    let opened = super::open_reading_sources(&settings);
    let _ignored = (
        std::fs::remove_file(&certificate),
        std::fs::remove_file(&key),
        std::fs::remove_dir_all(&directory),
    );
    opened.expect("a reading worker opens its declared catalog and source at boot");
}

/// A reading worker whose catalog reads none of the sources it declares is refused at boot, and
/// names neither the catalog's source nor its credential.
#[test]
fn a_reading_worker_whose_catalog_reads_none_of_its_sources_is_refused_at_boot() {
    let (certificate, key) = written("none-read");
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player");
    let overlay = format!(
        "server:\n  role: \"reading-worker\"\n  tls_certificate: \"{certificate}\"\n  tls_key: \"{key}\"\n  \
         tls_node_authority: \"{certificate}\"\nsecurity:\n  tls_termination: \"in-process\"\n  identity: \
         \"single-user\"\n  single_user_because: \"the test deployment reads its own fixture files\"\ncatalogs:\n  \
         - name: \"model\"\n    kind: \"markdown\"\n    dir: \"{catalog}\"\n    data_dir: \"{data}\"\n    version: \
         \"test\"\nsources:\n  elsewhere:\n    kind: \"files\"\n    data_dir: \"{data}\"\n    posture: \
         \"shared-service-user\"\n",
        certificate = certificate.display(),
        key = key.display(),
        catalog = example.join("catalog").display(),
        data = example.join("data").display(),
    );
    let directory = std::env::temp_dir().join(format!("sutura-cli-reading-worker-none-read-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a scratch configuration directory is creatable");
    std::fs::write(directory.join("base.yaml"), overlay).expect("a scratch configuration is writable");
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_directory(directory.clone()),
    )
    .expect("a reading worker declaring a source its catalog does not read loads");
    let opened = super::open_reading_sources(&settings);
    let _ignored = (
        std::fs::remove_file(&certificate),
        std::fs::remove_file(&key),
        std::fs::remove_dir_all(&directory),
    );
    let Err(refused) = opened else {
        panic!("a reading worker whose catalog reads none of its declared sources opened them");
    };
    assert!(
        refused.contains("reads none of the sources it declares") && !refused.contains("local"),
        "{refused}"
    );
}
