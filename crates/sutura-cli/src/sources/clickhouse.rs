//! The `ClickHouse` half of the command root: one declared HTTP endpoint becomes an open
//! `Warehouse`.
//!
//! **Thinner than `bigquery.rs` and `postgres.rs`, and deliberately so**: the per-source BUILD
//! lives once in `crate::clickhouse`, shared with `crate::serve`'s own root, so the posture
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
/// The `impersonation-at-source` posture, which this command attaches no broker for; then everything
/// [`crate::clickhouse::build`] refuses: a placement the dispatcher should have sent elsewhere, a
/// source with no declared identity, an unreadable or empty password file, and unusable TLS material.
#[cfg(feature = "clickhouse")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
    working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<Opened, String> {
    // `sutura serve` attaches the declared-map broker; this command attaches only the static one,
    // which mints nothing for an impersonating source. Refused before anything is dialled.
    if matches!(
        configured.posture(),
        Some(sutura_domain::source::SourcePosture::ImpersonationAtSource)
    ) {
        return Err(format!(
            "`sources.{source}` is `impersonation-at-source`, and the `sutura` command attaches no \
             broker that names the user a subject executes as - refusing rather than reading every \
             row as this process; no fallback. `sutura serve` is the root that attaches one"
        ));
    }
    let engine = crate::clickhouse::build(source, configured, working_set)?;
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
    _working_set: sutura_exec_datafusion::WorkingSet,
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

    /// A command-line data directory selects nothing on a database, and is refused rather than
    /// dropped - on every build, because the check runs before the kind's own `open`.
    #[test]
    fn a_data_directory_offered_to_a_clickhouse_source_is_refused_as_an_argument_that_selects_nothing() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_clickhouse("shared-service-user", ""),
            runtime(),
            timeout(),
            Some(std::path::Path::new("/srv/sutura/data")),
            None,
        )
        .map(|_| ())
        .expect_err("a data directory for a database source is refused");
        assert!(error.contains("a database has none"), "{error}");
        assert!(
            error.contains("/srv/sutura/data"),
            "the refusal must name the argument: {error}"
        );
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

    /// **The refusal, with its own cell.** This command mints through the static broker, which has
    /// nothing for an impersonating source, so the entry is refused before the password file is read
    /// or anything is dialled. Neutralise the posture check and the password-file refusal appears
    /// instead.
    #[test]
    #[cfg(feature = "clickhouse")]
    fn a_declared_clickhouse_source_configured_to_impersonate_refuses_at_composition() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_clickhouse("impersonation-at-source", "    impersonate:\n      subject-a: analyst_a\n"),
            runtime(),
            timeout(),
            None,
            None,
        )
        .map(|_| ())
        .expect_err("this command attaches no broker for an impersonating source");
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("no fallback"),
            "the refusal must say there is no fallback: {error}"
        );
        assert!(
            error.contains("`sutura serve`"),
            "the refusal must name the root that serves it: {error}"
        );
        assert!(
            !error.contains("password_file"),
            "the posture check fires before the password file is read: {error}"
        );
    }
}
