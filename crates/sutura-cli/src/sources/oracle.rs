//! The Oracle half of the command root: one declared listener becomes an open `Warehouse`.
//!
//! **`clickhouse.rs`'s shape, as thin and for its reason**: the per-source BUILD lives once in
//! `crate::oracle`, shared with `crate::serve`'s own root. What is here is this root's own two
//! things - the registry it wraps the engine in, and the refusal for a build that linked no adapter.

use sutura_domain::model::SourceName;

use crate::sources::Opened;
#[cfg(feature = "oracle")]
use crate::sources::OpenedWith;

/// Opens one Oracle source, under the identity the deployment declared.
///
/// **Nothing is attached** - the tables live in the database - so [`OpenedWith::attached`] is `None`.
///
/// # Errors
///
/// Everything [`crate::oracle::build`] refuses.
#[cfg(feature = "oracle")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
) -> Result<Opened, String> {
    let engine = crate::oracle::build(source, configured)?;
    Ok(Opened::Oracle(OpenedWith {
        engines: sutura_app::Warehouses::of(engine),
        attached: None,
        broker: sutura_config::StaticCredentialBroker::from_registry(registry),
    }))
}

/// The refusal for a build that linked no Oracle adapter - `clickhouse.rs`'s twin shape.
#[cfg(not(feature = "oracle"))]
pub(super) fn open(
    source: &SourceName,
    _configured: &sutura_config::ConfiguredSource,
    _registry: &sutura_config::SourceRegistry,
) -> Result<Opened, String> {
    Err(format!(
        "`sources.{source}` is `kind: oracle`, and this binary was built without the `oracle` \
         feature - so it links no Oracle adapter. Build `sutura-cli` with `--features oracle`, or \
         declare a `files` source"
    ))
}

#[cfg(test)]
mod tests {
    use crate::sources::{bundle_naming, declaring, open_engine, runtime, timeout};

    /// One `sources:` entry for an Oracle database, with every key that kind is opened with. The
    /// password file is a path that is not there, so a refusal naming it is proof the composition
    /// reached the credential step - and dialled nothing.
    fn declaring_oracle() -> sutura_config::SourceRegistry {
        declaring(
            "warehouse",
            "    kind: oracle\n    host: \"127.0.0.1\"\n    port: 1521\n    service_name: \"FREEPDB1\"\n    \
             user: \"sutura\"\n    password_file: \"/nonexistent/sutura-cli-test-oracle-pass\"\n    \
             transport_mode: \"plaintext\"\n",
            "shared-service-user",
        )
    }

    /// A command-line data directory selects nothing on a database, and is refused rather than
    /// dropped - on every build, because the check runs before the kind's own `open`.
    #[test]
    fn a_data_directory_offered_to_an_oracle_source_is_refused_as_an_argument_that_selects_nothing() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_oracle(),
            runtime(),
            timeout(),
            Some(std::path::Path::new("/srv/sutura/data")),
            None,
        )
        .map(|_| ())
        .expect_err("a data directory for a database source is refused");
        assert!(error.contains("an Oracle database"), "{error}");
        assert!(
            error.contains("/srv/sutura/data"),
            "the refusal must name the argument: {error}"
        );
    }

    /// The refusal half of a default build - compiled by `cargo xtask check-default-features` and
    /// EXECUTED by `cargo xtask check-default-feature-tests`; `--all-features` makes it false.
    #[test]
    #[cfg(not(feature = "oracle"))]
    fn a_kind_this_build_did_not_link_is_refused_by_name_and_says_what_to_build() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_oracle(),
            runtime(),
            timeout(),
            None,
            None,
        )
        .map(|_| ())
        .expect_err("a kind this binary linked no adapter for must not open");
        assert!(error.contains("kind: oracle"), "the kind is not named: {error}");
        assert!(
            error.contains("--features oracle"),
            "the refusal must say what to build rather than only what is missing: {error}"
        );
    }

    /// The other half: the kind DISPATCHED to this adapter, the declared posture was accepted
    /// against the adapter's OWN `IMPERSONATION`, and the composition read the password file the
    /// settings tree named - the furthest a test with no listener can reach.
    #[test]
    #[cfg(feature = "oracle")]
    fn a_declared_oracle_source_reaches_the_password_file_the_deployment_declared() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_oracle(),
            runtime(),
            timeout(),
            None,
            None,
        )
        .map(|_| ())
        .expect_err("the declared password file is not there, so this command does not answer");
        assert!(error.contains("password_file"), "the refusal must name the key: {error}");
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    }
}
