#![forbid(unsafe_code)]
//! **Experimental.** A [`SemanticCatalog`] over a Palantir Foundry ontology: the object types and
//! link types its v2 metadata API serves, read as a **declaring** source - issue
//! `github.com/telekom/sutura#1727`.
//!
//! Tested against answers recorded from Foundry's public API reference (`src/fixture/`), not
//! against a running Foundry. `docs/what-foundry-can-carry.md` is the field-by-field finding.
//!
//! # What an object type becomes
//!
//! Each object type is a [`Model`] on the catalog's declared source: the model and the table are the
//! object type's API name, each property is a column under its API name with Foundry's own type
//! word, and the object type's primary key is the model's.
//!
//! **A dotted API name is one object, named twice.** The object-type list serves platform-provided
//! types such as `com.palantir.system.Evolution` beside an ontology's own. Its model name - and the
//! table's local name, which columns are qualified by - is the API name with each `.` replaced by
//! `_` (`com_palantir_system_Evolution`); the table keeps the dotted name as an
//! [`ObjectPath`](sutura_domain::model::ObjectPath), which the Foundry dialect renders as one quoted
//! identifier and every other dialect refuses. A dotted name with an empty segment or a segment
//! outside the identifier grammar, or whose mapped name is too long, is refused naming the API name.
//! Two API names that map to one model name are refused naming both: one would shadow the other. A description is required, as
//! `Descriptions` is a provided kind: the object type's own, or else its display name. A column's
//! description is the property's own, or else its display name.
//!
//! # What a link type becomes
//!
//! A link side with cardinality `ONE` that names a foreign-key property is a many-to-one
//! [`Relationship`] from that property to the target's primary key, named
//! `<objectType>_<linkType>`. Nothing else is carried, and nothing else is lost: the `MANY` side of
//! the same link is its reverse, a `ONE` side without a foreign key has it on the other side, and a
//! many-to-many link exposes no join table to join through. A foreign key on a `MANY` side, a
//! cardinality word other than `ONE` or `MANY`, and a link to an object type the read did not list
//! are refused by name.
//!
//! # What stays in Foundry
//!
//! Object-type metadata carries no measure, grain, required filter, value allowlist or anchor -
//! aggregation is a query-time operation in Foundry - so none is declared. The bundle is the same
//! for every caller: the reader presents the deployment's own token, and every caller sees what that
//! token sees, whatever Foundry would show each caller. `caller::CallerObjectTypes` is what a
//! deployment that narrows each caller's VIEW of that bundle lists with instead - the bundle itself
//! does not change.

/// The object types one caller can see, listed under that caller's own token.
#[cfg(feature = "http")]
pub mod caller;
pub mod document;
pub mod fixture;
#[cfg(feature = "http")]
pub mod http;

use std::collections::BTreeMap;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{
    Column, Definitions, Description, InconsistentDefinitions, InvalidDescription, JoinKey, JoinKeys, Model, Relationship,
};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{
    ColumnName, InvalidIdentifier, InvalidQualifiedTable, JoinType, ModelName, ObjectPath, QualifiedTable, RelationshipName,
    SourceName, TableName,
};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The two halves of a bundle, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);

/// The model name an object type becomes, and the table it names.
type Named = (ModelName, QualifiedTable);

/// The longest piece of an answer a refusal quotes.
const SHOWN_CHARS: usize = 80;

/// Where the ontology a [`FoundryCatalog`] decides over comes from.
///
/// The fake seam: [`fixture::FixtureReader`] serves the synthetic answers, and
/// [`http::HttpOntologyReader`] (behind the `http` feature) asks a running Foundry.
pub trait OntologyReader {
    type Error: std::error::Error + Send + Sync + 'static;

    fn read(&self) -> Result<document::Ontology, Self::Error>;
}

/// Why an ontology could not be read as a catalog. A quoted value is cut to 80 characters with
/// control characters removed.
#[derive(Debug, thiserror::Error)]
pub enum FoundryError {
    #[error("the reader could not produce the ontology")]
    Read(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    /// `on` is the object type's API name, and `link` the link type's where the name came from one.
    #[error("the {kind} {value:?} on the object type {on:?}{} is not a name", through(.link.as_deref()))]
    Identifier {
        kind: &'static str,
        value: String,
        on: String,
        link: Option<String>,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("the object type API name {on:?} cannot be mapped to a model name")]
    ObjectTypeName {
        on: String,
        #[source]
        cause: InvalidQualifiedTable,
    },
    #[error("the object types {first:?} and {second:?} both map to the model name {model}, so one would shadow the other")]
    ModelNameCollision {
        first: String,
        second: String,
        model: ModelName,
    },
    #[error("the object type {on} has neither a description nor a display name")]
    MissingDescription { on: String },
    #[error("the description of {on} is not usable")]
    Description {
        on: String,
        #[source]
        cause: InvalidDescription,
    },
    #[error("the description of property {column} on {on} is not usable")]
    ColumnDescription {
        on: String,
        column: ColumnName,
        #[source]
        cause: InvalidDescription,
    },
    #[error("the link type {link} on {on} has the cardinality {cardinality:?}, which is neither ONE nor MANY")]
    UnknownCardinality { on: String, link: String, cardinality: String },
    #[error("the link type {link} on {on} names a foreign key on its MANY side")]
    ForeignKeyOnManySide { on: String, link: String },
    #[error("the link type {link} on {on} points at the object type {target}, which the read did not list")]
    UnlistedTarget { on: String, link: String, target: String },
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

/// A catalog read from one Foundry ontology.
///
/// `source` is the `sources.<alias>` every object type's model is on.
#[derive(Debug, Clone)]
pub struct FoundryCatalog<R> {
    name: SourceName,
    version: DefinitionVersion,
    source: SourceName,
    reader: R,
}

impl<R: OntologyReader> FoundryCatalog<R> {
    pub const fn new(name: SourceName, version: DefinitionVersion, source: SourceName, reader: R) -> Self {
        Self {
            name,
            version,
            source,
            reader,
        }
    }

    fn assemble(&self, ontology: &document::Ontology) -> Result<Content, FoundryError> {
        let mut named: Vec<(&document::ObjectType, Named)> = Vec::with_capacity(ontology.object_types().len());
        let mut owners: BTreeMap<ModelName, &str> = BTreeMap::new();
        for object_type in ontology.object_types() {
            let (model, table) = model_name(object_type.api_name())?;
            if let Some(first) = owners.insert(model.clone(), object_type.api_name()) {
                return Err(FoundryError::ModelNameCollision {
                    first: shown(first),
                    second: shown(object_type.api_name()),
                    model,
                });
            }
            named.push((object_type, (model, table)));
        }
        let targets: BTreeMap<&str, Target<'_>> = named
            .iter()
            .map(|(object_type, (model, _))| {
                (
                    object_type.api_name(),
                    Target {
                        model,
                        key: object_type.primary_key(),
                    },
                )
            })
            .collect();
        let mut models = Vec::with_capacity(named.len());
        let mut relationships = Vec::new();
        for (object_type, (model, table)) in &named {
            let model = self.convert_model(object_type, model.clone(), table.clone())?;
            for side in ontology.outgoing(object_type.api_name()) {
                if let Some(relationship) = relationship(&model, object_type.api_name(), side, &targets)? {
                    relationships.push(relationship);
                }
            }
            models.push(model);
        }
        let definitions =
            Definitions::assemble(models, relationships, Vec::new()).map_err(|cause| FoundryError::Inconsistent { cause })?;
        let knowledge =
            Knowledge::assemble(&definitions, KnowledgeInput::none()).map_err(|cause| FoundryError::Knowledge { cause })?;
        Ok((definitions, knowledge))
    }

    fn convert_model(
        &self,
        object_type: &document::ObjectType,
        name: ModelName,
        table: QualifiedTable,
    ) -> Result<Model, FoundryError> {
        let on = object_type.api_name();
        let mut columns = Vec::with_capacity(object_type.properties().len());
        for (property_name, property) in object_type.properties() {
            let column_name = identifier(property_name, |raw| ColumnName::parse(raw), "property", on, None)?;
            let prose = first_text([property.description(), property.display_name()]);
            let column = Column::from_metadata(column_name, Some(property.data_type()), prose, None).map_err(|refusal| {
                let (column, cause) = refusal.into_parts();
                FoundryError::ColumnDescription {
                    on: shown(on),
                    column,
                    cause,
                }
            })?;
            columns.push(column);
        }
        let primary_key = identifier(
            object_type.primary_key(),
            |raw| ColumnName::parse(raw),
            "primary key",
            on,
            None,
        )?;
        let text = first_text([object_type.description(), Some(object_type.display_name())])
            .ok_or_else(|| FoundryError::MissingDescription { on: shown(on) })?;
        let description = Description::parse(text).map_err(|cause| FoundryError::Description { on: shown(on), cause })?;
        Model::new(name, self.source.clone(), table, columns, description)
            .with_primary_key([primary_key])
            .map_err(|cause| FoundryError::Inconsistent { cause })
    }
}

/// The model name and table an object type's API name becomes - see the crate header.
///
/// # Errors
///
/// [`FoundryError::ObjectTypeName`] naming the API name.
fn model_name(api_name: &str) -> Result<Named, FoundryError> {
    let refused = |cause| FoundryError::ObjectTypeName {
        on: shown(api_name),
        cause,
    };
    let table = object_table(api_name).map_err(refused)?;
    let model = ModelName::parse(table.name().as_str()).map_err(|cause| {
        refused(InvalidQualifiedTable::Table {
            value: shown(api_name),
            cause,
        })
    })?;
    Ok((model, table))
}

/// The table an object type's API name names: a bare table, or a dotted [`ObjectPath`].
///
/// The one parse both the reader, before an API name reaches a request path, and the catalog use.
///
/// # Errors
///
/// [`InvalidQualifiedTable`] for a name that is neither.
pub(crate) fn object_table(api_name: &str) -> Result<QualifiedTable, InvalidQualifiedTable> {
    ObjectPath::parse(api_name)?.map_or_else(
        || {
            TableName::parse(api_name)
                .map(QualifiedTable::from)
                .map_err(|cause| InvalidQualifiedTable::Table {
                    value: shown(api_name),
                    cause,
                })
        },
        QualifiedTable::object,
    )
}

/// What a link side may point at: an object type's model name and its primary key.
struct Target<'read> {
    model: &'read ModelName,
    key: &'read str,
}

/// The relationship one link side declares, or `None` for a side whose join is the other side's or
/// is not expressible (see the crate header). `on` is the origin's API name, which a refusal names.
fn relationship(
    origin: &Model,
    on: &str,
    side: &document::LinkTypeSide,
    targets: &BTreeMap<&str, Target<'_>>,
) -> Result<Option<Relationship>, FoundryError> {
    let link = side.api_name();
    let foreign_key = match (side.cardinality(), side.foreign_key()) {
        ("ONE", Some(foreign_key)) => foreign_key,
        ("ONE" | "MANY", None) => return Ok(None),
        ("MANY", Some(_)) => {
            return Err(FoundryError::ForeignKeyOnManySide {
                on: shown(on),
                link: shown(link),
            });
        }
        (cardinality, _) => {
            return Err(FoundryError::UnknownCardinality {
                on: shown(on),
                link: shown(link),
                cardinality: shown(cardinality),
            });
        }
    };
    let target = targets.get(side.target()).ok_or_else(|| FoundryError::UnlistedTarget {
        on: shown(on),
        link: shown(link),
        target: shown(side.target()),
    })?;
    let via = Some(link);
    let name = identifier(
        &format!("{}_{link}", origin.name()),
        |raw| RelationshipName::parse(raw),
        "link type",
        on,
        via,
    )?;
    let origin_column = identifier(foreign_key, |raw| ColumnName::parse(raw), "foreign key", on, via)?;
    let target_column = identifier(target.key, |raw| ColumnName::parse(raw), "primary key", side.target(), via)?;
    Ok(Some(Relationship::new(
        name,
        origin.name().clone(),
        target.model.clone(),
        JoinType::ManyToOne,
        JoinKeys::single(JoinKey::Equal {
            origin: origin_column,
            target: target_column,
        }),
    )))
}

/// The first of `texts` that holds more than whitespace, trimmed.
fn first_text<'text>(texts: impl IntoIterator<Item = Option<&'text str>>) -> Option<&'text str> {
    texts.into_iter().flatten().map(str::trim).find(|text| !text.is_empty())
}

/// Answer text as a refusal quotes it.
fn shown(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_control()).take(SHOWN_CHARS).collect()
}

/// The tail of a [`FoundryError::Identifier`] that names the link a name came from.
fn through(link: Option<&str>) -> String {
    link.map_or_else(String::new, |link| format!(" (link type {link:?})"))
}

fn identifier<T>(
    raw: &str,
    parse: impl Fn(&str) -> Result<T, InvalidIdentifier>,
    kind: &'static str,
    on: &str,
    link: Option<&str>,
) -> Result<T, FoundryError> {
    parse(raw).map_err(|cause| FoundryError::Identifier {
        kind,
        value: shown(raw),
        on: shown(on),
        link: link.map(shown),
        cause,
    })
}

impl<R: OntologyReader> SemanticCatalog for FoundryCatalog<R> {
    type Error = FoundryError;

    const KIND: CatalogKind = CatalogKind::Declaring;

    /// `Structure` and `Descriptions` always; a property's type and description, and a
    /// foreign-key-backed link, when the ontology serves them. Nothing else: the crate header says
    /// why, and the knowledge half is empty.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions]).and_may_provide([
                DefinitionKind::ColumnTypes,
                DefinitionKind::ColumnDescriptions,
                DefinitionKind::Relationships,
            ]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let ontology = self.reader.read().map_err(|cause| FoundryError::Read(Box::new(cause)))?;
        let (definitions, knowledge) = self.assemble(&ontology)?;
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| FoundryError::Digest { cause })
    }
}

#[cfg(test)]
mod tests;
