//! A reading worker reads no partition of a kind that does not split.

use sutura_domain::model::SourceName;
use sutura_exec_datafusion::Unread;

fn opened() -> super::Opened {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player");
    let inbound = "  inbound:\n    mode: direct\n    resource: https://resource.example.com\n    authorization_server: https://issuer.example.com\n    key_set_file: /keys.json\n    algorithms: [RS256]\n";
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"the test deployment reads its own fixture \
         files\"\n{inbound}catalogs:\n  - name: \"model\"\n    kind: \"markdown\"\n    dir: \"{catalog}\"\n    data_dir: \
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
    super::Opened {
        pinned,
        sources,
        budget: settings.server().request_timeout().budget(),
        admissions: sutura_config::credentials::SourceAdmissions::from_registry(settings.sources()),
    }
}

#[test]
fn a_partition_of_a_source_that_is_not_postgres_is_refused_naming_it() {
    let opened = opened();
    let local = SourceName::parse("local").expect("a source name");
    let read = opened.read(b"", &local, b"", &std::sync::Arc::new(arrow_schema::Schema::empty()));
    assert!(
        matches!(&read, Err(Unread::Failed(cause)) if cause.to_string().contains("does not read source `local`")),
        "a files partition was not refused as a source a reading worker does not read: {read:?}"
    );
}

#[cfg(all(feature = "bigquery", feature = "postgres"))]
mod tokens;

#[cfg(all(feature = "distributed", feature = "bigquery", feature = "postgres"))]
mod native_bigquery;

#[tokio::test]
async fn the_concrete_reader_checks_its_sources_verifier_membership_before_decoding_or_dispatch() {
    use sutura_domain::identity::{Secret, VerifierId, WorkloadPool};
    use sutura_exec_datafusion::{Forwarded, PartitionReader as _};

    let local = SourceName::parse("local").expect("a source");
    let staff = VerifierId::primary();
    let partner = VerifierId::parse("partner").expect("an id");
    let opened = sutura_runtime::spawn_carrying_span(opened)
        .await
        .expect("the file source opens on a blocking task");
    let reader = super::Reader(std::sync::Arc::new(opened));
    let schema = std::sync::Arc::new(arrow_schema::Schema::empty());
    assert!(matches!(
        reader.read(&partner, b"malformed", &local, b"", &schema, None),
        Err(Unread::LegNotCertified)
    ));
    let pool = WorkloadPool::parse("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/q")
        .expect("a pool");
    let forwarded = Forwarded::new(Secret::new("fixture-token"), &pool);
    assert!(matches!(
        reader.read_forwarded(&partner, b"malformed", &local, b"", &schema, forwarded, None),
        Err(Unread::LegNotCertified)
    ));

    let mut admitted = reader
        .read(&staff, b"malformed", &local, b"", &schema, None)
        .expect("the verifier is admitted");
    let next = std::future::poll_fn(|context| admitted.as_mut().poll_next(context))
        .await
        .expect("the source dispatch returns its refusal");
    assert!(matches!(next, Err(ref cause) if cause.to_string().contains("does not read source `local`")));
    let missing = SourceName::parse("missing").expect("a source");
    assert!(matches!(
        reader.read(&staff, b"malformed", &missing, b"", &schema, None),
        Err(Unread::LegNotCertified)
    ));
}
