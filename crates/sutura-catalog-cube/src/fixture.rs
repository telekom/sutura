//! The recorded metadata answer and the fake reader that serves it.
//!
//! `fixture/meta.json` is the answer the pinned Cube image in `compose.services.yaml` serves for
//! `examples/cube/model`, recorded from the `cube` profile and pretty-printed. `tests/provisioned.rs`
//! reads the live tier and asserts it still decodes to the same definitions.

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use crate::document::Meta;
use crate::{CubeCatalog, CubeError, MetaReader};

/// The recorded answer, as served.
pub const META: &str = include_str!("fixture/meta.json");

/// The fake [`MetaReader`]: it decodes [`META`] on every read, through the same path a live answer takes.
#[derive(Debug, Clone, Copy)]
pub struct FixtureReader;

impl MetaReader for FixtureReader {
    type Error = CubeError;

    fn read(&self) -> Result<Meta, Self::Error> {
        serde_json::from_str(META).map_err(|cause| CubeError::Read(Box::new(cause)))
    }
}

/// A [`CubeCatalog`] over the recorded answer, every cube on `name`. The golden registry opens it.
#[must_use]
pub fn over_fixture_source(name: SourceName, version: DefinitionVersion) -> CubeCatalog<FixtureReader> {
    CubeCatalog::new(name.clone(), version, name, FixtureReader)
}
