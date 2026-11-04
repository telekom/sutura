//! A reading worker reads only a `PostgreSQL` partition.

use sutura_domain::model::SourceName;
use sutura_exec_datafusion::Unread;

/// A partition of a source that is not `PostgreSQL` is refused as a failed read naming the source,
/// never read and never answered as an uncertified leg. A files source stands in for `BigQuery`,
/// which no offline test opens: every kind but `PostgreSQL` reaches the same arm.
#[test]
fn a_partition_of_a_source_that_is_not_postgres_is_refused_naming_it() {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player");
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"the test deployment reads its own fixture \
         files\"\ncatalogs:\n  - name: \"model\"\n    kind: \"markdown\"\n    dir: \"{catalog}\"\n    data_dir: \
         \"{data}\"\n    version: \"test\"\nsources:\n  local:\n    kind: \"files\"\n    data_dir: \"{data}\"\n    \
         posture: \"shared-service-user\"\n",
        catalog = example.join("catalog").display(),
        data = example.join("data").display(),
    );
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("the overlay loads");
    let catalogs = crate::catalog::open_catalog(settings.catalogs(), None).expect("the example catalog opens");
    let pinned = crate::catalog::load(&catalogs).expect("the example catalog loads");
    let sources = super::super::super::open_engine(
        &pinned,
        settings.sources(),
        settings.runtime(),
        settings.server().request_timeout(),
        None,
    )
    .expect("the example opens its files source");
    let opened = super::Opened {
        pinned,
        sources,
        budget: settings.server().request_timeout().budget(),
    };
    let local = SourceName::parse("local").expect("a source name");
    let read = opened.read(b"", &local, b"");
    assert!(
        matches!(&read, Err(Unread::Failed(cause)) if cause.to_string().contains("does not read source `local`")),
        "a files partition was not refused as a source a reading worker does not read: {read:?}"
    );
}
