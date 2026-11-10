//! A reading worker reads a `BigQuery` partition with a caller's forwarded token only at its own
//! source of that name, federating at the token's pool, and only while the token's grant lasts.
//!
//! No driver is loaded, and no admission ran, so no cell here reaches a read: a [`Forwarded`] no
//! worker admitted carries an ended grant. That admission records the grant's real end is
//! `sutura-exec-datafusion`'s `served_split` cell's.

use sutura_domain::identity::{Secret, WorkloadPool};
use sutura_domain::model::SourceName;
use sutura_domain::source::SourcePosture;
use sutura_exec_bigquery::adbc::{BytesBilledCeiling, DriverLocation, Impersonation};
use sutura_exec_bigquery::{BigQueryWarehouse, JobBilling};
use sutura_exec_datafusion::{Forwarded, Unread};

use super::super::super::super::OpenedSources;

fn warehouse() -> SourceName {
    SourceName::parse("warehouse").expect("a test source is a source")
}

fn pool_named(pool: &str) -> WorkloadPool {
    WorkloadPool::parse(&format!(
        "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/{pool}/providers/sso"
    ))
    .expect("a test pool is a pool")
}

/// A reader over the example catalog and one `BigQuery` source, `warehouse`, federating at `pool`.
fn reading_at(pool: &str) -> super::super::Opened {
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
    let bigquery = BigQueryWarehouse::over_adbc(
        warehouse(),
        SourcePosture::ImpersonationAtSource,
        JobBilling::CallerRun { caller_may_name: false },
        DriverLocation::parse("/nonexistent/libadbc_driver_bigquery.so").expect("an absolute path"),
        Impersonation::ThroughPool(pool_named(pool)),
        BytesBilledCeiling::parse(1 << 30).expect("a ceiling"),
    );
    super::super::Opened {
        pinned,
        sources: OpenedSources::BigQuery(sutura_app::Warehouses::of(bigquery)),
        budget: settings.server().request_timeout().budget(),
    }
}

fn token_for(pool: &str) -> Forwarded {
    Forwarded::new(Secret::new("a-forwarded-token"), &pool_named(pool))
}

/// A token forwarded for another pool than the one this node's own `warehouse` federates at is
/// refused as an uncertified leg, before its grant's end is read.
#[test]
fn a_token_forwarded_for_another_pool_than_the_workers_own_source_is_refused_as_uncertified() {
    let opened = reading_at("analysts");
    let read = opened.read_forwarded(b"", &warehouse(), b"", token_for("elsewhere"));
    assert!(
        matches!(read, Err(Unread::LegNotCertified)),
        "a token of another pool was not refused as an uncertified leg: {read:?}"
    );
}

/// A token for the source's own pool whose grant has ended is refused naming the source, before
/// the leg is re-certified or the source asked.
#[test]
fn a_token_whose_grant_has_ended_is_refused_before_the_leg_is_certified() {
    let opened = reading_at("analysts");
    let read = opened.read_forwarded(b"", &warehouse(), b"", token_for("analysts"));
    assert!(
        matches!(&read, Err(Unread::Failed(cause)) if cause.to_string().contains("the grant a partition of `warehouse` was admitted under has ended")),
        "a token whose grant ended was not refused as ended: {read:?}"
    );
}
