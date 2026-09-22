//! The `ClickHouse` half of the command root: one declared HTTP endpoint becomes an open
//! `Warehouse`.
//!
//! **Thinner than `bigquery.rs` and `postgres.rs`, and deliberately so**: the per-source BUILD
//! lives once in [`crate::clickhouse`], shared with `crate::serve`'s own root, so the posture
//! cross-check, the secret read and the channel resolution cannot differ between the two
//! composition roots the way `bigquery`'s and `postgres`' line-for-line copies can. What is here is
//! this root's own two things - the registry it wraps the engine in, and the refusal for a build
//! that linked no adapter.

use sutura_domain::model::SourceName;

use crate::sources::Opened;
#[cfg(feature = "clickhouse")]
use crate::sources::OpenedWith;

/// Opens one `ClickHouse` source, under the identity and channel the deployment declared.
///
/// **Nothing is attached** - the tables live in the database - so [`OpenedWith::attached`] is `None`
/// here, the same narrowing `postgres.rs` documents.
///
/// # Errors
///
/// Everything [`crate::clickhouse::build`] refuses: a placement the dispatcher should have sent
/// elsewhere, a source with no declared identity, the `impersonation-at-source` posture, an
/// unreadable or empty password file, and unusable TLS material.
#[cfg(feature = "clickhouse")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
) -> Result<Opened, String> {
    let engine = crate::clickhouse::build(source, configured)?;
    Ok(Opened::ClickHouse(OpenedWith {
        engines: sutura_app::Warehouses::of(engine),
        // Nothing to compare: the tables are the database's. See the field's own note, and the caller's.
        attached: None,
        broker: sutura_config::StaticCredentialBroker::from_registry(registry),
    }))
}

/// The refusal for a build that linked no `ClickHouse` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, so the dispatcher in
/// the parent module has exactly one call and the compiler decides which of these it reaches. It
/// names the FEATURE and not just the kind, for the reason `bigquery.rs` gives: the two remedies are
/// in two different files.
#[cfg(not(feature = "clickhouse"))]
pub(super) fn open(
    source: &SourceName,
    _configured: &sutura_config::ConfiguredSource,
    _registry: &sutura_config::SourceRegistry,
) -> Result<Opened, String> {
    Err(format!(
        "`sources.{source}` is `kind: clickhouse`, and this binary was built without the \
         `clickhouse` feature - so it links no ClickHouse adapter. Build `sutura-cli` with \
         `--features clickhouse`, or declare a `files` source"
    ))
}

#[cfg(test)]
mod tests {
    use crate::sources::{bundle_naming, declaring, open_engine, runtime, timeout};

    /// One `sources:` entry for a `ClickHouse` database, with every key that kind is opened with.
    ///
    /// The password file is a path that is not there, so a refusal naming it is proof the
    /// composition reached the credential step - the same lever `postgres.rs`'s own builder uses,
    /// and the furthest a cell with no server can reach.
    fn declaring_clickhouse(posture: &str, extra: &str) -> sutura_config::SourceRegistry {
        declaring(
            "warehouse",
            &format!(
                "    kind: clickhouse\n    host: \"127.0.0.1\"\n    port: 8123\n    user: \"sutura\"\n    \
                 password_file: \"/nonexistent/sutura-cli-test-ch-pass\"\n    \
                 transport_mode: \"plaintext\"\n{extra}"
            ),
            posture,
        )
    }

    /// The refusal half of a default build, exercised the way `just gates` runs the postgres one.
    ///
    /// The `cfg(not(feature = "clickhouse"))` half is compiled by `cargo xtask
    /// check-default-features` and EXECUTED by `cargo xtask check-default-feature-tests`;
    /// everywhere else `--all-features` makes this false. It proves the kind DISPATCHED to the
    /// `ClickHouse` arm's refusal, which fires before any file is read.
    #[test]
    #[cfg(not(feature = "clickhouse"))]
    fn a_kind_this_build_did_not_link_is_refused_by_name_and_says_what_to_build() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_clickhouse("shared-service-user", ""),
            runtime(),
            timeout(),
            None,
            None,
        )
        .map(|_| ())
        .expect_err("a kind this binary linked no adapter for must not open");
        assert!(error.contains("kind: clickhouse"), "the kind is not named: {error}");
        assert!(
            error.contains("--features clickhouse"),
            "the refusal must say what to build rather than only what is missing: {error}"
        );
    }

    /// The other half, and the reason the helper above is not dead under `--all-features`.
    ///
    /// **What it proves, and it is deliberately the furthest a test with no server can reach:** the
    /// kind DISPATCHED to this adapter, the declared posture was accepted against the adapter's OWN
    /// `IMPERSONATION`, and the composition read the password file the settings tree named. A
    /// refusal about the feature, the kind or the posture would mean the composition stopped
    /// earlier.
    #[test]
    #[cfg(feature = "clickhouse")]
    fn a_declared_clickhouse_source_reaches_the_password_file_the_deployment_declared() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_clickhouse("shared-service-user", ""),
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

    /// The `workload_identity` block an `impersonation-at-source` entry must carry to get PAST the
    /// settings parse, so the refusal this cell is about is the composition's and not the tree's.
    #[cfg(feature = "clickhouse")]
    fn wif_for_clickhouse() -> String {
        String::from(
            "    workload_identity:\n      audience: \"//iam.googleapis.com/projects/1/locations/global/\
             workloadIdentityPools/p/providers/sso\"\n      scope: \"https://www.googleapis.com/auth/\
             bigquery.readonly\"\n",
        )
    }

    /// **The refusal this change is careful about, and it has its own cell rather than only a
    /// predicate.** `ClickHouseWarehouse::IMPERSONATION` is `NoPlaceForASubject`, so an
    /// `impersonation-at-source` entry fails `SourcePosture::deliverable_by`'s capability half -
    /// before the password file is read and before anything is dialled, so it is reachable with no
    /// server listening and no secret on disk.
    ///
    /// It also asserts the sentence `crate::clickhouse::IMPERSONATION_DEFERRED` adds, because the
    /// domain refusal's own remedy (*deploy a build whose adapter for that source can impersonate*)
    /// names a build that does not exist for this kind. Neutralise the `deliverable_by` call - or
    /// drop the appended sentence - and this cell fails; `dead_code` would catch neither.
    #[test]
    #[cfg(feature = "clickhouse")]
    fn a_declared_clickhouse_source_configured_to_impersonate_refuses_at_composition() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_clickhouse("impersonation-at-source", &wif_for_clickhouse()),
            runtime(),
            timeout(),
            None,
            None,
        )
        .map(|_| ())
        .expect_err("an impersonating posture with nowhere for a subject's credential to arrive must not open");
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("per-subject credential"),
            "the refusal must say what the adapter cannot do: {error}"
        );
        assert!(
            error.contains("no fallback"),
            "the refusal must say there is no fallback: {error}"
        );
        assert!(
            error.contains("no per-subject path in this repository yet"),
            "the refusal must say the adapter cannot YET deliver it, not that another build can: {error}"
        );
        // NOT the neighbouring arms: the entry is declared, the build DOES link the adapter, and the
        // cross-check fires before the credential step would name the unreadable password file.
        assert!(
            !error.contains("--features clickhouse"),
            "this build DID link the adapter: {error}"
        );
        assert!(
            !error.contains("password_file"),
            "the capability cross-check fires before the password file is read: {error}"
        );
    }
}
