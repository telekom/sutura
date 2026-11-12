//! The answers of Foundry's ontology v2 metadata API, decoded into the fields this adapter maps.
//!
//! Two operations: list the object types (`GET /api/v2/ontologies/{ontology}/objectTypes`), and
//! list one object type's outgoing link types
//! (`GET /api/v2/ontologies/{ontology}/objectTypes/{objectType}/outgoingLinkTypes`). Both answer a
//! page: `data` and, while more follow, `nextPageToken`.
//!
//! Unknown fields are ignored rather than refused, for `sutura-catalog-cube`'s reason: a release
//! that adds a display field changes nothing this adapter reads. Every field it does read is
//! required in the shape Foundry's API reference documents, except where that reference marks it
//! optional. A link's `cardinality` is kept as the word Foundry sent, so a word this adapter does
//! not know is refused by name at load rather than failing the whole decode.

use std::collections::BTreeMap;

/// One page of a list answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    data: Vec<T>,
    #[serde(default)]
    next_page_token: Option<String>,
}

impl<T> Page<T> {
    #[must_use]
    pub const fn len(&self) -> usize {
        self.data.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The token of the next page; `None` on the last. An empty token reads as the last page.
    #[must_use]
    pub fn next_page_token(&self) -> Option<&str> {
        self.next_page_token.as_deref().filter(|token| !token.is_empty())
    }

    #[must_use]
    pub fn into_data(self) -> Vec<T> {
        self.data
    }
}

/// One object type (`ObjectTypeV2`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectType {
    api_name: String,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
    primary_key: String,
    properties: BTreeMap<String, Property>,
}

impl ObjectType {
    #[must_use]
    pub fn api_name(&self) -> &str {
        &self.api_name
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    #[must_use]
    pub fn primary_key(&self) -> &str {
        &self.primary_key
    }

    /// Every property, keyed by its API name, in name order.
    #[must_use]
    pub const fn properties(&self) -> &BTreeMap<String, Property> {
        &self.properties
    }
}

/// One property of an object type (`PropertyV2`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    data_type: DataType,
}

impl Property {
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    /// The type word Foundry names the property's type with (`string`, `long`, `timestamp`, …).
    #[must_use]
    pub fn data_type(&self) -> &str {
        &self.data_type.kind
    }
}

/// A property's type; only its discriminating word is read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct DataType {
    #[serde(rename = "type")]
    kind: String,
}

/// One side of a link type, as seen from the object type it leaves (`LinkTypeSideV2`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkTypeSide {
    api_name: String,
    object_type_api_name: String,
    cardinality: String,
    #[serde(default)]
    foreign_key_property_api_name: Option<String>,
}

impl LinkTypeSide {
    #[must_use]
    pub fn api_name(&self) -> &str {
        &self.api_name
    }

    /// The object type this side points at.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.object_type_api_name
    }

    /// `ONE` or `MANY` as Foundry wrote it.
    #[must_use]
    pub fn cardinality(&self) -> &str {
        &self.cardinality
    }

    /// The property of the leaving object type that holds the target's primary key, when the link
    /// is backed by one.
    #[must_use]
    pub fn foreign_key(&self) -> Option<&str> {
        self.foreign_key_property_api_name.as_deref()
    }
}

/// One whole read: every object type, and each one's outgoing link types keyed by its API name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ontology {
    object_types: Vec<ObjectType>,
    outgoing: BTreeMap<String, Vec<LinkTypeSide>>,
}

impl Ontology {
    #[must_use]
    pub const fn new(object_types: Vec<ObjectType>, outgoing: BTreeMap<String, Vec<LinkTypeSide>>) -> Self {
        Self { object_types, outgoing }
    }

    #[must_use]
    pub fn object_types(&self) -> &[ObjectType] {
        &self.object_types
    }

    /// The outgoing link types of the object type named `api_name`; none when it has none.
    #[must_use]
    pub fn outgoing(&self, api_name: &str) -> &[LinkTypeSide] {
        self.outgoing.get(api_name).map_or(&[], Vec::as_slice)
    }
}
