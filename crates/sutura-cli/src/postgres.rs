//! The ONE Postgres composition, shared by both of this crate's composition roots.
//!
//! `crate::oracle`'s shape, for its reason: [`build`] below is the only place a declared `postgres`
//! entry becomes an open `sutura_domain::warehouse::Warehouse`, so the posture cross-check, the
//! secret read, the channel and the driver cannot differ between `sutura serve` and `sutura
//! query`/`mcp`. What stays per root is the feature-off refusal, because the two are different
//! sentences.
//!
//! **Every source is the ADBC adapter**, over the driver this artefact links or the one
//! `SUTURA_POSTGRES_ADBC_DRIVER` names (`sutura_exec_postgres::adbc::PostgresDriver::from_host`).
//! There is no second transport to fall back to and no key that selects one.
//!
//! # The limits, next to the claim
//!
//! - **One declared identity, only.** A Postgres source signs in solely as the deployment's
//!   declared shared service account. OAuth, Kerberos/GSSAPI and per-caller sign-in are not
//!   supported: `AdbcPostgres::IMPERSONATION` is `NoPlaceForASubject`, and `Conninfo` pins
//!   `require_auth` to `password,md5,scram-sha-256,none` and `gssencmode` to `disable`.
//! - **Nothing dials here.** A connection opens per call, so an unreachable server or a refused
//!   login is the boot path's first anchor check, not this function - a source with no anchor
//!   declared first meets its server at the first question.
//! - **`transport_anchors: system` is refused** by name: libpq's `system` store is OpenSSL's
//!   compiled-in default, not the host store sutura reads.
//!
//! The WHOLE module is behind `#[cfg(feature = "postgres")]` at its declaration in `main.rs`, so
//! everything here may name an adapter type unconditionally.

use sutura_config::sources::placement::PostgresDial;
use sutura_config::sources::transport::{SourceTransport, TrustAnchors};
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_postgres::adbc::{AdbcPostgres, Channel, Conninfo, PostgresDriver};
use sutura_exec_postgres::connection::ConnectionTarget;

use crate::commands::render;

/// A `Postgres` source as this binary composes it: one connection string under the deployment's
/// declared identity, secured as the source declares.
pub(crate) type PostgresSource = AdbcPostgres;

/// Builds one Postgres adapter from a declared entry, after checking this build can deliver the
/// source's posture.
///
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; the
/// `impersonation-at-source` posture, which this adapter has nowhere to put; a password file that
/// cannot be read or is empty; a declared channel libpq cannot hold to or material that cannot be
/// read; or no driver to open.
pub(crate) fn build(source: &SourceName, configured: &sutura_config::ConfiguredSource) -> Result<AdbcPostgres, String> {
    // Matched rather than read off accessors every kind would have to have: the dispatcher has
    // already decided this is the Postgres arm, and a second openable kind should arrive as a
    // compile error at this line too.
    let sutura_config::SourcePlacement::Postgres {
        ref dial,
        ref database,
        ref user,
        ref password_file,
        ref transport,
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the Postgres connect step with a placement no Postgres \
             adapter reads, which the dispatcher should have sent elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    // Against this adapter's OWN constant, which is the point of the cross-check being per adapter.
    // `NoPlaceForASubject`, so an `impersonation-at-source` entry FAILS here - before the password
    // file is read.
    identity
        .posture()
        .deliverable_by(AdbcPostgres::IMPERSONATION, source)
        .map_err(|cause| render(&cause))?;
    // Exactly one of these, by construction - `PostgresDial` is the reason there is no third arm
    // here for a state `parse_placement` already refused.
    let (target, port) = match *dial {
        PostgresDial::Tcp { ref host, port } => (ConnectionTarget::Host(host.as_str()), port),
        PostgresDial::UnixSocket { ref directory, port } => (ConnectionTarget::UnixSocket(directory.as_path()), port),
    };
    let password = crate::password_file::read(source, password_file)?;
    let anchors = |declared: &TrustAnchors| match *declared {
        TrustAnchors::System => sutura_tls::Anchors::System,
        TrustAnchors::File(ref path) => sutura_tls::Anchors::Bundle(path.clone()),
    };
    let (declared, identity_pair) = match *transport {
        SourceTransport::Plaintext => (None, None),
        SourceTransport::Verified { anchors: ref declared } => (Some(anchors(declared)), None),
        SourceTransport::Mutual {
            anchors: ref declared,
            ref identity,
        } => (
            Some(anchors(declared)),
            Some(sutura_tls::Identity::new(
                identity.certificate().clone(),
                identity.key().clone(),
            )),
        ),
    };
    let channel = match (declared.as_ref(), identity_pair.as_ref()) {
        (None, _) => Channel::Plaintext,
        (Some(anchors), None) => Channel::Verified(anchors),
        (Some(anchors), Some(pair)) => Channel::Mutual(anchors, pair),
    };
    let conninfo = Conninfo::new(source, target, port, database, user, &password, channel).map_err(|cause| render(&cause))?;
    let driver =
        PostgresDriver::from_host().map_err(|none| format!("`sources.{source}` has no driver to open: {}", render(&none)))?;
    AdbcPostgres::new(source.clone(), identity.posture().clone(), driver, conninfo).map_err(|cause| render(&cause))
}
