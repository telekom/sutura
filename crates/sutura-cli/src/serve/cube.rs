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
