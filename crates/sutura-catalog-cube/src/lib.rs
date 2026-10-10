#![forbid(unsafe_code)]
//! A [`SemanticCatalog`] over a Cube metrics serving layer: the definitions Cube's metadata API
//! (`/cubejs-api/v1/meta`) serves, read as a **declaring** source - `github.com/telekom/sutura#1340`.
//!
//! # What a cube becomes
//!
//! Each cube or view is a [`Model`] on the deployment's Cube source: the model and the table are the
//! cube's own name, which is how Cube addresses its members, and each dimension is a column under
//! its short name. The column type is Cube's own type word (`string`, `number`, `time`, `boolean`),
//! because that is all Cube says about it. A cube's description is required, as `Descriptions` is a
//! provided kind; a cube without one is refused by name.
//!
//! # What stays in Cube
//!
//! A measure's computation, a segment's condition and a join's condition are SQL text in Cube's own
//! model, and the plain metadata answer carries none of it. So a measure is decoded and **reported,
//! not defined**: no domain `Metric` is minted, because sutura cannot compute it and never computes
//! a Cube metric itself. The execution path that answers a question over a Cube measure through Cube
//! is not in this crate. A join is Cube's to make when one query names members of two cubes, so no
//! `Relationship` is declared either.

pub mod document;
pub mod fixture;
#[cfg(feature = "http")]
pub mod http;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Column, Definitions, Description, InconsistentDefinitions, InvalidDescription, Model};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{ColumnName, InvalidIdentifier, ModelName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The two halves of a bundle, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);

/// Where the metadata answer a [`CubeCatalog`] decides over comes from.
///
/// The fake seam: [`fixture::FixtureReader`] serves the recorded answer, and [`http::HttpMetaReader`]
/// (behind the `http` feature) asks a running Cube.
pub trait MetaReader {
    type Error: std::error::Error + Send + Sync + 'static;

    fn read(&self) -> Result<document::Meta, Self::Error>;
}

/// Why a metadata answer could not be read as a catalog.
#[derive(Debug, thiserror::Error)]
pub enum CubeError {
    #[error("the reader could not produce a metadata answer")]
    Read(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error("the {kind} {value:?} on {on} is not a name")]
    Identifier {
        kind: &'static str,
        value: String,
        on: String,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("the member {member} is not named <cube>.<member> under the cube {cube}")]
    ForeignMember { cube: String, member: String },
    #[error("the cube {on} carries no description to describe the model")]
    MissingDescription { on: String },
    #[error("the description of {on} is not usable")]
    Description {
        on: String,
        #[source]
        cause: InvalidDescription,
    },
    #[error("the description of dimension {column} on {on} is not usable")]
    ColumnDescription {
        on: String,
        column: ColumnName,
        #[source]
        cause: InvalidDescription,
    },
    #[error("the catalog does not hold together")]
    Inconsistent {
        #[source]
        cause: InconsistentDefinitions,
    },
    #[error("the catalog's knowledge does not hold together")]
    Knowledge {
        #[source]
        cause: InconsistentKnowledge,
    },
    #[error("the definitions could not be hashed")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

/// A catalog read from one Cube deployment's metadata answer.
///
/// `source` is the `sources.<alias>` every cube's model is on: one Cube deployment is one source.
#[derive(Debug, Clone)]
pub struct CubeCatalog<R> {
    name: SourceName,
    version: DefinitionVersion,
    source: SourceName,
    reader: R,
}

impl<R: MetaReader> CubeCatalog<R> {
    pub const fn new(name: SourceName, version: DefinitionVersion, source: SourceName, reader: R) -> Self {
        Self {
            name,
            version,
            source,
            reader,
        }
    }

    fn assemble(&self, meta: &document::Meta) -> Result<Content, CubeError> {
        let models = meta
            .cubes()
            .iter()
            .map(|cube| self.convert_model(cube))
            .collect::<Result<Vec<_>, _>>()?;
        let definitions =
            Definitions::assemble(models, Vec::new(), Vec::new()).map_err(|cause| CubeError::Inconsistent { cause })?;
        let knowledge =
            Knowledge::assemble(&definitions, KnowledgeInput::none()).map_err(|cause| CubeError::Knowledge { cause })?;
        Ok((definitions, knowledge))
    }

    fn convert_model(&self, cube: &document::Cube) -> Result<Model, CubeError> {
        let name = identifier(cube.name(), |raw| ModelName::parse(raw), "model", cube.name())?;
        let table = identifier(cube.name(), |raw| TableName::parse(raw), "table", cube.name())?;
        let mut columns = Vec::with_capacity(cube.dimensions().len());
        let mut primary_key = Vec::new();
        for dimension in cube.dimensions() {
            let short = member(cube, dimension.name())?;
            let column_name = identifier(short, |raw| ColumnName::parse(raw), "dimension", cube.name())?;
            if dimension.primary_key() {
                primary_key.push(column_name.clone());
            }
            let column = Column::from_metadata(column_name, Some(dimension.data_type()), dimension.description(), None).map_err(
                |refusal| {
                    let (column, cause) = refusal.into_parts();
                    CubeError::ColumnDescription {
                        on: cube.name().to_owned(),
                        column,
                        cause,
                    }
                },
            )?;
            columns.push(column);
        }
        let text = cube
            .description()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| CubeError::MissingDescription {
                on: cube.name().to_owned(),
            })?;
        let description = Description::parse(text).map_err(|cause| CubeError::Description {
            on: cube.name().to_owned(),
            cause,
        })?;
        Model::new(name, self.source.clone(), table, columns, description)
            .with_primary_key(primary_key)
            .map_err(|cause| CubeError::Inconsistent { cause })
    }
}

/// The member's short name: what follows `<cube>.` in its full name.
fn member<'name>(cube: &document::Cube, full: &'name str) -> Result<&'name str, CubeError> {
    full.strip_prefix(cube.name())
        .and_then(|rest| rest.strip_prefix('.'))
        .filter(|short| !short.is_empty())
        .ok_or_else(|| CubeError::ForeignMember {
            cube: cube.name().to_owned(),
            member: full.to_owned(),
        })
}

fn identifier<T>(
    raw: &str,
    parse: impl Fn(&str) -> Result<T, InvalidIdentifier>,
    kind: &'static str,
    on: &str,
) -> Result<T, CubeError> {
    parse(raw).map_err(|cause| CubeError::Identifier {
        kind,
        value: raw.to_owned(),
        on: on.to_owned(),
        cause,
    })
}

impl<R: MetaReader> SemanticCatalog for CubeCatalog<R> {
    type Error = CubeError;

    const KIND: CatalogKind = CatalogKind::Declaring;

    /// `Structure` and `Descriptions` always; a dimension's type and description when Cube serves
    /// them. Nothing else: measures are reported, not defined (the crate header says why), and the
    /// knowledge half is empty.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions])
                .and_may_provide([DefinitionKind::ColumnTypes, DefinitionKind::ColumnDescriptions]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let meta = self.reader.read().map_err(|cause| CubeError::Read(Box::new(cause)))?;
        let (definitions, knowledge) = self.assemble(&meta)?;
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| CubeError::Digest { cause })
    }
}

#[cfg(test)]
mod tests;
