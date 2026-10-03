//! The `Postgres` half of the composition root: one declared source becomes an open `Warehouse`.
//!
//! **Everything here is behind `#[cfg(feature = "postgres")]` except the refusal for its absence**,
//! for the reason `bigquery.rs` gives at the same shape: this module is the answer to "which kinds
//! did THIS BUILD link", a question `sutura_config` cannot answer.

use sutura_domain::model::SourceName;

use crate::sources::Opened;
#[cfg(feature = "postgres")]
use crate::sources::OpenedWith;

/// A `Postgres` source as this binary composes it - `crate::postgres`'s, named here for the
/// registry type in the parent.
#[cfg(feature = "postgres")]
pub(crate) type PostgresSource = crate::postgres::PostgresSource;

/// Opens one `PostgreSQL` source, under the identity and channel the deployment declared.
///
/// **Nothing is attached** - the tables live in the database - so [`OpenedWith::attached`] is `None`
/// here, the same narrowing `bigquery.rs` documents. The composition is `crate::postgres::build`,
/// shared with `crate::serve`'s root.
///
/// # Errors
///
/// `crate::postgres::build`'s.
#[cfg(feature = "postgres")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
) -> Result<Opened, String> {
    Ok(Opened::Postgres(OpenedWith {
        engines: sutura_app::Warehouses::of(crate::postgres::build(source, configured)?),
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

    /// The `workload_identity` block an `impersonation-at-source` entry must carry.
    ///
    /// Its own helper rather than [`super::wif`], which is gated on `bigquery` - this root's
    /// Postgres cell needs one under `postgres` alone, the same reason `files.rs`'s `wif_for_files`
    /// is not shared either.
    #[cfg(feature = "postgres")]
    fn wif_for_postgres() -> String {
        String::from(
            "    workload_identity:\n      audience: \"//iam.googleapis.com/projects/1/locations/global/\
             workloadIdentityPools/p/providers/sso\"\n      scope: \"https://www.googleapis.com/auth/\
             bigquery.readonly\"\n",
        )
    }

    /// The Postgres half of the cross-check `crate::serve`'s composition root is held to
    /// (`crates/sutura-cli/src/serve/tests.rs`'s `a_postgres_source_configured_to_impersonate_refuses_
    /// at_boot`) - proven here too, because #124's last comment framed the boot refusal over BOTH
    /// composition roots and only `serve`'s had a cell.
    ///
    /// `AdbcPostgres::IMPERSONATION` is `NoPlaceForASubject`, and this root's `open` runs the
    /// check before it reads `password_file` or dials anything, so the refusal is reachable with no
    /// server listening and no password file on disk.
    #[test]
    #[cfg(feature = "postgres")]
    fn a_declared_postgres_source_configured_to_impersonate_refuses_at_boot() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_postgres("impersonation-at-source", &wif_for_postgres()),
            runtime(),
            timeout(),
            None,
            None,
        )
        .map(|_| ())
        .expect_err("an impersonating posture with nowhere for a subject's credential to arrive must not start");
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("per-subject credential"),
            "the refusal must say what the adapter cannot do: {error}"
        );
        assert!(
            error.contains("no fallback"),
            "the refusal must say there is no fallback: {error}"
        );
        // NOT the neighbouring arms: the entry is declared, the build DOES link the adapter, and the
        // check fires before the connection step would name the unreadable password file.
        assert!(
            !error.contains("--features postgres"),
            "this build DID link the adapter: {error}"
        );
        assert!(
            !error.contains("password_file"),
            "the capability cross-check fires before the password file is read: {error}"
        );
    }
}
