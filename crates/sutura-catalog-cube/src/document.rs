//! The answer of Cube's metadata API (`GET /cubejs-api/v1/meta`), decoded into the fields this
//! adapter maps.
//!
//! Cube's answer also carries display fields (`title`, `shortTitle`, `formatDescription`,
//! `drillMembers`, `folders`, …) that this adapter does not read. Unknown fields are ignored rather
//! than refused: each Cube release adds display fields, and a refusal there would stop a deployment on
//! an upgrade that changed nothing it reads. Every field it does read is required in the shape the
//! pinned Cube serves (`src/fixture/meta.json` is that answer, recorded).
//!
//! The reader asks for the plain answer, never `?extended=true`: the extended one adds each member's
//! SQL text and each join's SQL condition, and this adapter takes no SQL text out of a serving layer.

/// The whole answer: every cube and view the token may see.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Meta {
    cubes: Vec<Cube>,
}

impl Meta {
    #[must_use]
    pub fn cubes(&self) -> &[Cube] {
        &self.cubes
    }
}

/// Whether an entry of the answer is a cube or a view over cubes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CubeKind {
    Cube,
    View,
}

/// One cube or view.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Cube {
    name: String,
    #[serde(rename = "type")]
    kind: CubeKind,
    #[serde(default)]
    description: Option<String>,
    measures: Vec<Measure>,
    dimensions: Vec<Dimension>,
    segments: Vec<Segment>,
}

impl Cube {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> CubeKind {
        self.kind
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    #[must_use]
    pub fn measures(&self) -> &[Measure] {
        &self.measures
    }

    #[must_use]
    pub fn dimensions(&self) -> &[Dimension] {
        &self.dimensions
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
}

/// A measure, named `<cube>.<measure>`. Its computation stays in Cube.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measure {
    name: String,
    agg_type: String,
    #[serde(default)]
    description: Option<String>,
}

impl Measure {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Cube's own aggregation word (`sum`, `count`, `countDistinct`, `number`, …), as served.
    #[must_use]
    pub fn agg_type(&self) -> &str {
        &self.agg_type
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// A dimension, named `<cube>.<dimension>`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimension {
    name: String,
    #[serde(rename = "type")]
    data_type: String,
    #[serde(default)]
    description: Option<String>,
    primary_key: bool,
}

impl Dimension {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Cube's own type word (`string`, `number`, `time`, `boolean`, …), as served.
    #[must_use]
    pub fn data_type(&self) -> &str {
        &self.data_type
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    #[must_use]
    pub const fn primary_key(&self) -> bool {
        self.primary_key
    }
}

/// A segment: a named filter, named `<cube>.<segment>`. Its condition stays in Cube.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Segment {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

impl Segment {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}
