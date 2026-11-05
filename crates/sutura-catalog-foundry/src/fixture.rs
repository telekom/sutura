//! The recorded answers and the fake reader that serves them.
//!
//! Written from Foundry's public API reference, not recorded from a tenant: one page of
//! `objectTypes` and one page of `outgoingLinkTypes` per object type, every field in the documented
//! shape, every resource identifier synthetic. Four object types - `Aircraft`, `Airport`,
//! `Employee`, `Flight` - with three foreign-key links, their reverse sides and one many-to-many.

use std::collections::BTreeMap;

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use crate::document::{LinkTypeSide, ObjectType, Ontology, Page};
use crate::{FoundryCatalog, FoundryError, OntologyReader};

/// The recorded `objectTypes` page.
pub const OBJECT_TYPES: &str = include_str!("fixture/object-types.json");

/// The recorded `outgoingLinkTypes` page of each object type, by its API name.
pub const OUTGOING: &[(&str, &str)] = &[
    ("Aircraft", include_str!("fixture/outgoing/Aircraft.json")),
    ("Airport", include_str!("fixture/outgoing/Airport.json")),
    ("Employee", include_str!("fixture/outgoing/Employee.json")),
    ("Flight", include_str!("fixture/outgoing/Flight.json")),
];

/// The fake [`OntologyReader`]: it decodes the recorded pages on every read, through the same
/// [`Page`] decode a live answer takes.
#[derive(Debug, Clone, Copy)]
pub struct FixtureReader;

impl OntologyReader for FixtureReader {
    type Error = FoundryError;

    fn read(&self) -> Result<Ontology, Self::Error> {
        let decode = |cause| FoundryError::Read(Box::new(cause));
        let object_types = serde_json::from_str::<Page<ObjectType>>(OBJECT_TYPES).map_err(decode)?;
        let mut outgoing = BTreeMap::new();
        for (api_name, page) in OUTGOING {
            let sides = serde_json::from_str::<Page<LinkTypeSide>>(page).map_err(decode)?;
            outgoing.insert(String::from(*api_name), sides.into_data());
        }
        Ok(Ontology::new(object_types.into_data(), outgoing))
    }
}

/// A [`FoundryCatalog`] over the recorded answers, every object type on `name`. The conformance
/// registry opens it.
#[must_use]
pub fn over_fixture_source(name: SourceName, version: DefinitionVersion) -> FoundryCatalog<FixtureReader> {
    FoundryCatalog::new(name.clone(), version, name, FixtureReader)
}
