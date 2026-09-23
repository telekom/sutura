//! The Oracle half of this composition root: one declared source becomes an open `Warehouse`.
//!
//! **`clickhouse`'s shape, for its reasons**: the dispatcher stays in the composition root, both
//! `open_oracle` definitions live here because the compiler picks between them at the one call site,
//! and the per-source BUILD lives once in `crate::oracle`, shared with `crate::sources`' own root.

/// Opens one Oracle adapter per declared source, under the declared credential.
///
/// **Nothing is attached and nothing is registered** - the tables live in the database. Everything
/// that can fail before a listener is bound happens in [`crate::oracle::build`]: the posture
/// cross-check, the password file, and the connection itself.
#[cfg(feature = "oracle")]
pub(crate) fn open_oracle(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::OracleSource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = crate::oracle::build(source, configured)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns. Written as a fallback for
    // the reason `open_files` gives - the workspace denies `unwrap` and `expect`.
    engines
        .map(super::OpenedSources::Oracle)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the Oracle adapter - `open_clickhouse`'s twin shape.
#[cfg(not(feature = "oracle"))]
pub(crate) fn open_oracle(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: oracle`, and this binary was built without the `oracle` feature - \
         so it links no Oracle adapter. Build `sutura-cli` with `--features oracle`, or declare a \
         `files` source"
    ))
}

/// `sutura serve`'s own `oracle` cells: the refusal for a build that did not link the adapter, the
/// posture refusal, and the furthest a fixture with no server reaches - the declared password file,
/// alone and beside a `files` source.
#[cfg(test)]
mod tests {
    #[cfg(feature = "oracle")]
    use crate::serve::ENGINE_SOURCE;
    use crate::serve::open_engine;
    use crate::serve::tests::{bundle_over, default_timeout, one_worker, refusal, registry};
    #[cfg(feature = "oracle")]
    use crate::serve::tests::{entry, wif};

    /// One `oracle` entry whose password file is not there, so a refusal naming that key is proof
    /// the composition reached the credential step - and dialled nothing.
    fn oracle_entry(alias: &str, posture: &str, extra: &str) -> String {
        format!(
            "  {alias}:\n    kind: \"oracle\"\n    host: \"127.0.0.1\"\n    port: 1521\n    service_name: \"FREEPDB1\"\n    \
             user: \"sutura\"\n    password_file: \"/nonexistent/sutura-test-oracle-password\"\n    \
             transport_mode: \"plaintext\"\n    posture: \"{posture}\"\n{extra}"
        )
    }

    /// **Refused at startup and naming the source and the feature, not skipped**: a default build
    /// links no Oracle adapter. Compiled by `cargo xtask check-default-features` and RUN by
    /// `cargo xtask check-default-feature-tests`; `--all-features` makes this cfg false.
    #[test]
    #[cfg(not(feature = "oracle"))]
    fn an_oracle_source_is_refused_by_a_build_that_did_not_link_the_adapter() {
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&oracle_entry("warehouse", "shared-service-user", "")),
                one_worker(),
                default_timeout(),
                None,
            ),
            "a kind this binary linked no adapter for must not start",
        );
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("--features oracle"),
            "the refusal must say what to build: {error}"
        );
    }

    #[test]
    #[cfg(feature = "oracle")]
    fn an_oracle_source_reaches_the_password_file_the_deployment_declared() {
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&oracle_entry("warehouse", "shared-service-user", "")),
                one_worker(),
                default_timeout(),
                None,
            ),
            "the declared password file is not there, so this deployment does not start",
        );
        assert!(error.contains("warehouse.password_file"), "{error}");
    }

    /// **The refusal `github.com/telekom/sutura#127` names as having no cell.** `OracleWarehouse::
    /// IMPERSONATION` is `NoPlaceForASubject`, so an `impersonation-at-source` entry fails
    /// `SourcePosture::deliverable_by` - before the password file is read and before anything is
    /// dialled - and the refusal carries `crate::oracle`'s own sentence saying no build delivers it.
    #[test]
    #[cfg(feature = "oracle")]
    fn an_oracle_source_declared_to_impersonate_does_not_boot() {
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&oracle_entry("warehouse", "impersonation-at-source", wif())),
                one_worker(),
                default_timeout(),
                None,
            ),
            "an impersonating posture with nowhere for a subject's credential to arrive must not start",
        );
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("no per-subject path in this repository yet"),
            "the refusal must say no build delivers it, not that another build can: {error}"
        );
        assert!(
            !error.contains("--features oracle"),
            "this build DID link the adapter: {error}"
        );
        assert!(
            !error.contains("password_file"),
            "the capability cross-check fires before the password file is read: {error}"
        );
    }

    /// The mixed registry's `oracle` group: the `files` half opens, then the Oracle half is refused
    /// by its own credential read rather than by the kind or the mix.
    #[test]
    #[cfg(feature = "oracle")]
    fn a_catalog_spanning_files_and_oracle_reaches_the_oracle_credential() {
        let both = format!(
            "{}{}",
            entry(ENGINE_SOURCE, "shared-service-user", ""),
            oracle_entry("warehouse", "shared-service-user", "")
        );
        let error = refusal(
            open_engine(
                &bundle_over(&[
                    ("customers", ENGINE_SOURCE, "dim_customer"),
                    ("products", "warehouse", "dim_product"),
                ]),
                &registry(&both),
                one_worker(),
                default_timeout(),
                None,
            ),
            "a mixed catalog opens each kind and reaches the oracle arm's own credential refusal",
        );
        assert!(error.contains("warehouse.password_file"), "{error}");
    }
}
