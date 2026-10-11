//! The Cube half of the command root: one declared deployment becomes an open `Warehouse`.
//!
//! **`oracle.rs`'s shape, as thin and for its reason**: the per-source BUILD lives once in
//! `crate::cube`, shared with `crate::serve`'s own root. What is here is this root's own two things -
//! the registry it wraps the adapter in, and the refusal for a build that linked no adapter.

use sutura_domain::model::SourceName;

use crate::sources::Opened;
#[cfg(feature = "cube")]
use crate::sources::OpenedWith;

/// Opens one Cube source, under the identity the deployment declared.
///
/// **Nothing is attached** - the cubes live in the deployment - so [`OpenedWith::attached`] is `None`.
///
/// # Errors
///
/// Everything [`crate::cube::build`] refuses.
#[cfg(feature = "cube")]
pub(super) fn open(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    registry: &sutura_config::SourceRegistry,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<Opened, String> {
    let engine = crate::cube::build(source, configured, outbound)?;
    Ok(Opened::Cube(OpenedWith {
        engines: sutura_app::Warehouses::of(engine),
        attached: None,
        broker: sutura_config::StaticCredentialBroker::from_registry(registry),
    }))
}

/// The refusal for a build that linked no Cube adapter - `oracle.rs`'s twin shape.
#[cfg(not(feature = "cube"))]
pub(super) fn open(
    source: &SourceName,
    _configured: &sutura_config::ConfiguredSource,
    _registry: &sutura_config::SourceRegistry,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<Opened, String> {
    Err(format!(
        "`sources.{source}` is `kind: cube`, and this binary was built without the `cube` feature - \
         so it links no Cube adapter. Build `sutura-cli` with `--features cube`, or declare a `files` \
         source"
    ))
}

#[cfg(test)]
mod tests {
    use crate::sources::{bundle_naming, declaring, open_engine, runtime, timeout};

    /// One `sources:` entry for a Cube deployment at `endpoint` whose token is `token_file`.
    fn declaring_cube(endpoint: &str, token_file: &std::path::Path) -> sutura_config::SourceRegistry {
        declaring(
            "warehouse",
            &format!(
                "    kind: cube\n    endpoint: \"{endpoint}\"\n    token_file: \"{}\"\n",
                token_file.display()
            ),
            "shared-service-user",
        )
    }

    /// A token file that is not there, so a refusal naming it is proof the composition reached the
    /// credential step - and dialled nothing.
    fn absent_token() -> &'static std::path::Path {
        std::path::Path::new("/nonexistent/sutura-cli-test-cube-token")
    }

    /// `contents` written to a scratch token file, the source opened over it, and the file removed.
    #[cfg(feature = "cube")]
    fn opened_over_token(name: &str, endpoint: &str, contents: &str) -> Result<crate::sources::Opened, String> {
        let directory = std::env::temp_dir().join(format!("sutura-cli-cube-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).map_err(|cause| format!("the scratch directory: {cause}"))?;
        let token_file = directory.join("token");
        std::fs::write(&token_file, contents).map_err(|cause| format!("the scratch token file: {cause}"))?;
        let opened = open_engine(
            &bundle_naming("warehouse"),
            &declaring_cube(endpoint, &token_file),
            runtime(),
            timeout(),
            None,
            None,
        );
        let _ignored = std::fs::remove_dir_all(&directory);
        opened
    }

    /// A command-line data directory selects nothing on a deployment, and is refused rather than
    /// dropped - on every build, because the check runs before the kind's own `open`.
    #[test]
    fn a_data_directory_offered_to_a_cube_source_is_refused_as_an_argument_that_selects_nothing() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_cube("https://cube.example.com", absent_token()),
            runtime(),
            timeout(),
            Some(std::path::Path::new("/srv/sutura/data")),
            None,
        )
        .map(|_| ())
        .expect_err("a data directory for a Cube source is refused");
        assert!(error.contains("a Cube deployment"), "{error}");
        assert!(
            error.contains("/srv/sutura/data"),
            "the refusal must name the argument: {error}"
        );
    }

    /// The refusal half of a default build - compiled by `cargo xtask check-default-features` and
    /// EXECUTED by `cargo xtask check-default-feature-tests`; `--all-features` makes it false.
    #[test]
    #[cfg(not(feature = "cube"))]
    fn a_cube_kind_this_build_did_not_link_is_refused_by_name_and_says_what_to_build() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_cube("https://cube.example.com", absent_token()),
            runtime(),
            timeout(),
            None,
            None,
        )
        .map(|_| ())
        .expect_err("a kind this binary linked no adapter for must not open");
        assert!(error.contains("kind: cube"), "the kind is not named: {error}");
        assert!(
            error.contains("--features cube"),
            "the refusal must say what to build rather than only what is missing: {error}"
        );
    }

    /// The kind DISPATCHED to this adapter, the declared posture was accepted against the adapter's
    /// OWN `IMPERSONATION`, and the composition read the token file the settings tree named.
    #[test]
    #[cfg(feature = "cube")]
    fn a_declared_cube_source_reaches_the_token_file_the_deployment_declared() {
        let error = open_engine(
            &bundle_naming("warehouse"),
            &declaring_cube("https://cube.example.com", absent_token()),
            runtime(),
            timeout(),
            None,
            None,
        )
        .map(|_| ())
        .expect_err("the declared token file is not there, so this command does not answer");
        assert!(error.contains("`sources.warehouse.token_file` could not be read"), "{error}");
    }

    /// A blank token file is refused naming the key, never sent as an empty bearer.
    #[test]
    #[cfg(feature = "cube")]
    fn an_empty_cube_token_file_is_refused_naming_the_key() {
        let error = opened_over_token("empty", "https://cube.example.com", " \n")
            .map(|_| ())
            .expect_err("an empty token is not a credential");
        assert!(error.contains("`sources.warehouse.token_file` is empty"), "{error}");
    }

    /// The endpoint is parsed after the token is read, and an unusable one is refused naming its key.
    #[test]
    #[cfg(feature = "cube")]
    fn a_cube_endpoint_the_client_cannot_use_is_refused_naming_its_key() {
        let error = opened_over_token("endpoint", "ftp://cube.example.com", "a-token\n")
            .map(|_| ())
            .expect_err("an endpoint that is not http(s) is not dialled");
        assert!(
            error.contains("`sources.warehouse.endpoint` is not a usable endpoint"),
            "{error}"
        );
    }

    /// A readable token and a usable endpoint open the Cube arm - nothing is dialled, so no
    /// deployment is needed.
    #[test]
    #[cfg(feature = "cube")]
    fn a_cube_source_with_a_readable_token_opens_without_dialling() {
        let opened = opened_over_token("opens", "https://cube.example.com", "a-token\n");
        assert!(
            matches!(opened, Ok(crate::sources::Opened::Cube(_))),
            "a complete Cube source opens as the Cube arm"
        );
    }
}
