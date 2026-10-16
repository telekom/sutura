//! The Cube half of this composition root: one declared source becomes an open `Warehouse`.
//!
//! **`oracle`'s shape, for its reasons**: the dispatcher stays in the composition root, both
//! `open_cube` definitions live here because the compiler picks between them at every call site, and
//! the per-source BUILD lives once in `crate::cube`, shared with `crate::sources`' own root.

/// Opens one Cube adapter per declared source, under the declared token.
///
/// **Nothing is attached and nothing is registered** - the cubes live in the deployment. Everything
/// that can fail before a listener is bound happens in [`crate::cube::build`].
#[cfg(feature = "cube")]
pub(crate) fn open_cube(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::CubeSource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = crate::cube::build(source, configured, outbound)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns. Written as a fallback for
    // the reason `open_files` gives - the workspace denies `unwrap` and `expect`.
    engines
        .map(super::OpenedSources::Cube)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The service a cube-only deployment answers through: the declared-delegation broker, because an
/// `impersonation-at-source` source sends each caller's exchanged token as the bearer.
#[cfg(feature = "cube")]
pub(super) fn served(
    catalogs: &crate::catalog::OpenedCatalogs,
    engines: sutura_app::Warehouses<super::CubeSource>,
    settings: &sutura_config::Settings,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<super::Serving, String> {
    let broker = super::broker::build_broker(settings.sources(), outbound)?;
    super::started(catalogs, engines, broker, None, settings)
}

/// The refusal for a build that did not link the Cube adapter - `open_oracle`'s twin shape.
#[cfg(not(feature = "cube"))]
pub(crate) fn open_cube(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: cube`, and this binary was built without the `cube` feature - so it \
         links no Cube adapter. Build `sutura-cli` with `--features cube`, or declare a `files` source"
    ))
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "cube")]
    use crate::serve::ENGINE_SOURCE;
    use crate::serve::open_engine;
    #[cfg(feature = "cube")]
    use crate::serve::tests::entry;
    use crate::serve::tests::{bundle_over, default_timeout, one_worker, refusal, registry};

    /// One `cube` entry whose token file is not there, so a refusal naming that key is proof the
    /// composition reached the credential step - and dialled nothing.
    fn cube_entry(alias: &str) -> String {
        format!(
            "  {alias}:\n    kind: \"cube\"\n    endpoint: \"https://cube.example.com\"\n    \
             token_file: \"/nonexistent/sutura-test-cube-token\"\n    posture: \"shared-service-user\"\n"
        )
    }

    /// **Refused at startup and naming the source and the feature, not skipped**: a default build
    /// links no Cube adapter. Compiled by `cargo xtask check-default-features` and RUN by
    /// `cargo xtask check-default-feature-tests`; `--all-features` makes this cfg false.
    #[test]
    #[cfg(not(feature = "cube"))]
    fn a_cube_source_is_refused_by_a_build_that_did_not_link_the_adapter() {
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&cube_entry("warehouse")),
                one_worker(),
                default_timeout(),
                None,
            ),
            "a kind this binary linked no adapter for must not start",
        );
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("--features cube"),
            "the refusal must say what to build: {error}"
        );
    }

    #[test]
    #[cfg(feature = "cube")]
    fn a_cube_source_reaches_the_token_file_the_deployment_declared() {
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&cube_entry("warehouse")),
                one_worker(),
                default_timeout(),
                None,
            ),
            "the declared token file is not there, so this deployment does not start",
        );
        assert!(error.contains("warehouse.token_file"), "{error}");
    }

    /// The mixed registry's `cube` group: the `files` half opens, then the Cube half is refused by
    /// its own credential read rather than by the kind or the mix.
    #[test]
    #[cfg(feature = "cube")]
    fn a_catalog_spanning_files_and_cube_reaches_the_cube_credential() {
        let both = format!(
            "{}{}",
            entry(ENGINE_SOURCE, "shared-service-user", ""),
            cube_entry("warehouse")
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
            "a mixed catalog opens each kind and reaches the cube arm's own credential refusal",
        );
        assert!(error.contains("warehouse.token_file"), "{error}");
    }

    /// An impersonating `cube` entry exchanging each caller's token through one hop, with neither its
    /// token file nor its client secret file there.
    #[cfg(feature = "cube")]
    const IMPERSONATING: &str = "  warehouse:\n    kind: \"cube\"\n    endpoint: \"https://cube.example.com\"\n    \
                                 token_file: \"/nonexistent/sutura-test-cube-token\"\n    \
                                 posture: \"impersonation-at-source\"\n    delegation:\n      - token_endpoint: \
                                 \"https://idp.example.com/token\"\n        client_id: \"sutura\"\n        \
                                 client_secret_file: \"/nonexistent/sutura-test-cube-client-secret\"\n        \
                                 audience: \"https://cube.example.com\"\n";

    /// **An impersonating source passes the posture cross-check** and stops at its own token file -
    /// before anything is dialled.
    #[test]
    #[cfg(feature = "cube")]
    fn an_impersonating_cube_source_reaches_the_token_file_the_deployment_declared() {
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &crate::serve::tests::direct_registry(IMPERSONATING),
                one_worker(),
                default_timeout(),
                None,
            ),
            "the declared token file is not there, so this deployment does not start",
        );
        assert!(error.contains("warehouse.token_file"), "{error}");
    }

    /// **A cube-only deployment serves an impersonating source through its delegation**, not as the
    /// shared identity: the arm's service reads the declared client secret, so a file that is not
    /// there refuses naming that key. The token file is there, so the Cube arm opens.
    #[test]
    #[cfg(feature = "cube")]
    fn a_cube_only_deployment_serves_an_impersonating_source_through_its_delegation() {
        use sutura_config::{CatalogKind, CatalogSettings, Catalogs};
        use sutura_domain::model::SourceName;
        use sutura_domain::pinned::DefinitionVersion;

        let directory = std::env::temp_dir().join(format!("sutura-cli-cube-served-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
        let token = directory.join("token");
        std::fs::write(&token, "cube-token-5d2b81").expect("the token file writes");
        let settings = crate::serve::tests::direct_settings(&IMPERSONATING.replace(
            "/nonexistent/sutura-test-cube-token",
            token.to_str().expect("a scratch path is UTF-8"),
        ));
        let opened = open_engine(
            &bundle_over(&[("customers", "warehouse", "dim_customer")]),
            settings.sources(),
            one_worker(),
            default_timeout(),
            None,
        );
        let _ignored = std::fs::remove_dir_all(&directory);
        let Ok(crate::serve::OpenedSources::Cube(engines)) = opened else {
            panic!("a cube-only registry opens the Cube arm");
        };
        let catalog = CatalogSettings::parse(
            SourceName::parse("structure").expect("a test name is a name"),
            CatalogKind::Markdown,
            std::path::PathBuf::from("/nowhere/catalog"),
            std::path::PathBuf::from("/nowhere/data"),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        )
        .expect("a directory and a version are a settings");
        let catalogs = crate::catalog::open_catalog(&Catalogs::parse(vec![catalog]).expect("one name is a registry"), None)
            .expect("a markdown catalog opens");
        let error = super::served(&catalogs, engines, &settings, None)
            .map(drop)
            .expect_err("the declared client secret file is not there");
        assert!(
            error.contains("sources.warehouse.delegation[0].client_secret_file"),
            "{error}"
        );
    }

    /// **The broker composes the source's delegation**: it reads the declared client secret, so a
    /// file that is not there refuses naming that key.
    #[test]
    #[cfg(feature = "cube")]
    fn an_impersonating_cube_sources_delegation_reaches_its_client_secret_file() {
        let error = crate::serve::broker::build_broker(&crate::serve::tests::direct_registry(IMPERSONATING), None)
            .map(drop)
            .expect_err("the declared client secret file is not there");
        assert!(
            error.contains("sources.warehouse.delegation[0].client_secret_file"),
            "{error}"
        );
    }
}
