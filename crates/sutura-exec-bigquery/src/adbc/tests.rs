//! The ADBC transport's own guards, asserted WITHOUT a driver - which is the whole design of
//! [`super::AdbcBigQuery::connect`] and the reason this file can exist at all.
//!
//! **`adbc.rs` had no test module before this one**, and a reviewer measured what that cost: `&& false`
//! on either refusal in it left the suite green. Every cell here dies if its guard is neutralised,
//! and the last one is the negative control that stops the others passing for the wrong reason - a
//! path naming no `.so` makes the driver load fail, so a cell asserting a refusal has to show that
//! the refusal arrived INSTEAD of that failure.

use sutura_domain::identity::{PrincipalName, Secret};
use sutura_domain::warehouse::ParamValue;

use super::{AdbcBigQuery, AdbcError, Impersonation, ImpersonationScopes};
use crate::transport::{DatasetId, JobDeadline, JobIdentity, JobRequest, JobTransport as _, ProjectId};

/// A path that names no driver, so a load reached here always fails.
///
/// **That is the point rather than a nuisance:** every guard under test runs before the load, so a
/// cell that shows [`AdbcError::Uncovered`] over this path has shown the guard answered first.
const NO_DRIVER: &str = "/nonexistent/libadbc_driver_bigquery.so";

fn endpoint(impersonation: Impersonation) -> AdbcBigQuery {
    AdbcBigQuery::new(NO_DRIVER, impersonation)
}

fn impersonating() -> Impersonation {
    Impersonation::AtScope(
        ImpersonationScopes::parse("https://www.googleapis.com/auth/cloud-platform").expect("a URL scope is usable"),
    )
}

fn project() -> ProjectId {
    ProjectId::parse("acme-analytics").expect("a test project is a project")
}

fn dataset() -> DatasetId {
    DatasetId::parse("warehouse").expect("a test dataset is a dataset")
}

#[test]
fn a_bearer_is_refused_before_the_driver_is_even_loaded() {
    // The refusal that has to stay, and the ordering that makes it worth having: a loaded driver
    // with no impersonation option is a connection as this deployment, so a request this transport
    // cannot honour must not reach `load_dynamic_from_filename`. `Uncovered` over a path naming no
    // `.so` is how that ordering is observable without a driver.
    let material = Secret::new("an-exchanged-access-token");
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT 1",
        &[],
        &project,
        &dataset,
        JobIdentity::AsBearer(&material),
        JobDeadline::Boot,
    );
    let refused = endpoint(impersonating())
        .run(&request)
        .expect_err("a bearer is not an identity this transport can send");
    assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
    assert!(!refused.to_string().contains("an-exchanged-access-token"), "{refused}");
}

#[test]
fn an_unbound_parameter_is_refused_before_the_driver_is_even_loaded() {
    // The statement carries positional `?` and this transport cannot bind one, so sending it would
    // be a runtime error at the driver over a statement whose values were dropped. Refused in
    // `connect` rather than in `run`, so a second port method cannot reach the driver without it -
    // and, like the cell above, before the load.
    let params = [ParamValue::Text(String::from("north"))];
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT ? AS region",
        &params,
        &project,
        &dataset,
        JobIdentity::Transport,
        JobDeadline::Boot,
    );
    let refused = endpoint(Impersonation::Disabled)
        .run(&request)
        .expect_err("an unbound parameter is not something this transport can send");
    assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
}

#[test]
fn a_declared_principal_this_transport_cannot_name_is_refused_before_the_driver_is_even_loaded() {
    // A domain `PrincipalName` accepts a role, because a data system with `SET ROLE` takes one. This
    // one goes to an impersonation endpoint that takes an address, so the parse that refuses it is
    // this transport's - and it runs before the load too.
    let role = PrincipalName::parse("analyst_role").expect("a role is a domain principal name");
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT 1",
        &[],
        &project,
        &dataset,
        JobIdentity::AsPrincipal(&role),
        JobDeadline::Boot,
    );
    let refused = endpoint(impersonating())
        .run(&request)
        .expect_err("a bare role is not an account this transport can name");
    assert!(matches!(refused, AdbcError::UnusableTarget { .. }), "{refused:?}");
}

#[test]
fn a_request_this_transport_accepts_gets_as_far_as_the_driver_and_fails_there() {
    // **THE NEGATIVE CONTROL for the three cells above.** Without it each of them passes over a
    // transport that refused everything for any reason, because the path names no `.so` either way.
    // A shared leg with no parameters is a request every guard accepts, so what it must fail on is
    // the LOAD - a different variant, reached only after all three guards let it through.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1", &[], &project, &dataset, JobIdentity::Transport, JobDeadline::Boot);
    let failed = endpoint(Impersonation::Disabled)
        .run(&request)
        .expect_err("no driver lives at this path");
    assert!(matches!(failed, AdbcError::Load(_)), "{failed:?}");
}
