//! The `Postgres` half of the composition root: one declared source becomes an open `Warehouse`.
//!
//! **Everything here is behind `#[cfg(feature = "postgres")]` except the refusal for its absence**,
//! for the reason `bigquery.rs` gives at the same shape: this module is the answer to "which kinds
//! did THIS BUILD link", a question `sutura_config` cannot answer.

use sutura_domain::model::SourceName;

#[cfg(feature = "postgres")]
use crate::commands::render;
use crate::sources::Opened;
#[cfg(feature = "postgres")]
use crate::sources::OpenedWith;

/// A `Postgres` source as this binary composes it: one connection under the declared identity.
///
/// Named once for the reason `bigquery::BigQuerySource` is: it appears in a registry type, a
/// `Warehouse` bound and a constructor's return, and because the three layers ARE the composition.
#[cfg(feature = "postgres")]
pub(crate) type PostgresSource = sutura_exec_postgres::PostgresWarehouse;

/// Opens one `PostgreSQL` source, under the identity and channel the deployment declared.
///
/// **Nothing is attached** - the tables live in the database - so [`OpenedWith::attached`] is `None`
/// here, the same narrowing `bigquery.rs` documents. What opening does is everything that can fail
/// before a question is asked against a real server: the posture cross-check, the TLS resolution
/// (which reads the declared trust anchors and an optional client identity and refuses what a closed
/// type must refuse), and the connection itself - so a source whose channel is misdeclared refuses
/// the question rather than answering over a channel it believes is secured.
///
/// **The composition is `sutura-serve`'s `build_postgres`, line for line, through the same public
/// constructors** - which is what issue 121 asks for by "one composition per adapter, shared by both
/// roots". The TLS reading and the connection live in `sutura-exec-postgres` (`tls::client_config`
/// and `connect_secured`), so a fix to either lands once and both roots get it.
///
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; a
/// posture this adapter cannot deliver; the `impersonation-at-source` posture (this adapter has
/// nowhere for a subject's credential); a password file that cannot be read; or the declared TLS
/// material being unusable.
#[cfg(feature = "postgres")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
) -> Result<Opened, String> {
    use sutura_exec_postgres::PostgresWarehouse;
    use sutura_exec_postgres::connection::{ConnectionTarget, config};
    use sutura_exec_postgres::tls::{TlsAnchors, TlsIdentity, client_config};

    let sutura_config::SourcePlacement::Postgres {
        ref host,
        ref unix_socket,
        port,
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
    // `PostgresWarehouse` declares `NoPlaceForASubject`, so an `impersonation-at-source` entry FAILS
    // the capability half here - and the static broker below serves only the declared identity.
    identity
        .posture()
        .deliverable_by(
            <PostgresWarehouse as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(|cause| render(&cause))?;

    let target = match (host, unix_socket) {
        (Some(host), _) => ConnectionTarget::Host(host),
        (None, Some(socket)) => ConnectionTarget::UnixSocket(socket),
        (None, None) => {
            return Err(format!(
                "`sources.{source}` declares no `host` and no `unix_socket`, so this adapter has \
                 nothing to dial"
            ));
        }
    };
    let config = config(target, port, database, user, password_file)
        .map_err(|cause| format!("`sources.{source}.password_file` could not be read: {cause}"))?;

    let tls = match transport {
        sutura_config::sources::transport::SourceTransport::Plaintext => None,
        sutura_config::sources::transport::SourceTransport::Verified { anchors } => {
            let anchors = match anchors {
                sutura_config::sources::transport::TrustAnchors::System => TlsAnchors::System,
                sutura_config::sources::transport::TrustAnchors::File(path) => TlsAnchors::Bundle(path.clone()),
            };
            Some(client_config(&anchors, None).map_err(|cause| {
                format!(
                    "`sources.{source}` is declared TLS and its material is not usable: {}",
                    render(&cause)
                )
            })?)
        }
        sutura_config::sources::transport::SourceTransport::Mutual { anchors, identity } => {
            let anchors = match anchors {
                sutura_config::sources::transport::TrustAnchors::System => TlsAnchors::System,
                sutura_config::sources::transport::TrustAnchors::File(path) => TlsAnchors::Bundle(path.clone()),
            };
            let tls_identity = TlsIdentity::new(identity.certificate().clone(), identity.key().clone());
            Some(client_config(&anchors, Some(&tls_identity)).map_err(|cause| {
                format!(
                    "`sources.{source}` is declared mTLS and its material is not usable: {}",
                    render(&cause)
                )
            })?)
        }
    };
    let engine = PostgresWarehouse::connect_secured(source.clone(), identity.posture().clone(), &config, tls)
        .map_err(|cause| render(&cause))?;
    Ok(Opened::Postgres(OpenedWith {
        engines: sutura_app::Warehouses::of(engine),
        // Nothing to compare: the tables are the database's. See the field's own note, and the caller's.
        attached: None,
        broker: sutura_config::StaticCredentialBroker::from_registry(registry),
    }))
}

/// The refusal for a build that linked no `Postgres` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, so the dispatcher in
/// the parent module has exactly one call and the compiler decides which of these it reaches. It
/// names the FEATURE and not just the kind, for the reason `bigquery.rs` gives: the two remedies are
/// in two different files.
#[cfg(not(feature = "postgres"))]
pub(super) fn open(
    source: &SourceName,
    _configured: &sutura_config::ConfiguredSource,
    _registry: &sutura_config::SourceRegistry,
) -> Result<Opened, String> {
    Err(format!(
        "`sources.{source}` is `kind: postgres`, and this binary was built without the `postgres` \
         feature - so it links no Postgres adapter. Build `sutura-cli` with `--features postgres`, \
         or declare a `files` source"
    ))
}

#[cfg(test)]
mod tests {
    use crate::sources::{bundle_naming, declaring, open_engine, runtime, timeout};

    /// One `sources:` entry for a `PostgreSQL` database, with every key that kind is opened with.
    ///
    /// Here rather than beside `declaring_bigquery` in the parent, because this is its only caller:
    /// the parent's entry builders are shared with the dataset-argument case, and this one is not.
    /// The password file is a path that is not there, so a refusal naming it is proof the
    /// composition reached the connect layer.
    fn declaring_postgres(posture: &str, extra: &str) -> sutura_config::SourceRegistry {
        declaring(
            "warehouse",
            &format!(
                "    kind: postgres\n    host: \"127.0.0.1\"\n    port: 5432\n    database: \"marts\"\n    \
                 user: \"sutura\"\n    password_file: \"/nonexistent/sutura-cli-test-pg-pass\"\n    \
                 transport_mode: \"plaintext\"\n{extra}"
            ),
            posture,
        )
    }

    /// The refusal half of a default build, exercised the way `just gates` runs the bigquery one.
    ///
    /// The `cfg(not(feature = "postgres"))` half is compiled by the default-feature gates and
    /// executed by `cargo xtask check-default-feature-tests`; everywhere else `--all-features` makes
    /// this false. It proves the kind DISPATCHED to the postgres arm's refusal, which fires before
    /// any file is read.
    #[test]
    #[cfg(not(feature = "postgres"))]
    fn a_kind_this_build_did_not_link_is_refused_by_name_and_says_what_to_build() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_postgres("shared-service-user", ""),
            runtime(),
            timeout(),
            None,
        )
        .map(|_| ())
        .expect_err("a kind this binary linked no adapter for must not open");
        assert!(error.contains("kind: postgres"), "the kind is not named: {error}");
        assert!(
            error.contains("--features postgres"),
            "the refusal must say what to build rather than only what is missing: {error}"
        );
    }

    /// The other half, and the reason the helper above is not dead under `--all-features`: the
    /// `cfg(not(..))` cell is absent there, so a builder only that one reached would be an unused
    /// function under `dead_code = "deny"`. This is also the interesting half.
    ///
    /// **What it proves, and it is deliberately the furthest a test with no server can reach:** the
    /// kind DISPATCHED to this adapter, the declared posture was accepted against the adapter's OWN
    /// `IMPERSONATION`, and the composition read the password file the settings tree named. A refusal
    /// about the feature, the kind or the posture would mean the composition stopped earlier.
    #[test]
    #[cfg(feature = "postgres")]
    fn a_declared_postgres_source_reaches_the_password_file_the_deployment_declared() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_postgres("shared-service-user", ""),
            runtime(),
            timeout(),
            None,
        )
        .map(|_| ())
        .expect_err("the declared password file is not there, so this command does not answer");
        assert!(
            error.contains("password_file"),
            "the refusal must name the key that could not be read: {error}"
        );
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    }
}
