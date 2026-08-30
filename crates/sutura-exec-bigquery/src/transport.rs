//! The one thing this adapter needs from a `BigQuery` endpoint, as a port.
//!
//! **Why there is a port here at all, when the adapter is already behind one.** `Warehouse` is the
//! domain's port and this crate implements it; this is a second, much narrower one *inside* the
//! adapter, and it buys two things that matter more than the indirection costs:
//!
//! - **Everything this adapter decides becomes testable without a network.** The credential match,
//!   the posture agreement, the leg refusal, the rendering and the whole value mapping are exercised
//!   against a fake that returns rows, which is what *ports get fakes, not mocked HTTP* asks for.
//! - **The dependency decision is isolated to one implementor.** An outbound HTTP stack plus a
//!   credential library is a large addition to a workspace that cross-compiles to musl and gates
//!   licences exactly, and it belongs in the change that can first verify it against a real endpoint.
//!   Nothing in this repository can do that - see the crate documentation.
//!
//! **What is deliberately NOT here: a method that takes a string.** The request carries a statement
//! this crate rendered from a plan, and there is no entry point a caller could hand SQL to.

use sutura_domain::warehouse::ParamValue;

/// How a request writes its bind parameters.
///
/// One variant, and it is a variant rather than an absence because the endpoint's own request body
/// carries this as a field with two values: a query may use positional parameters or named ones and
/// **not both**, so a transport has to state which it is sending rather than infer it from the text.
///
/// Positional is the one this adapter uses, and the decision is recorded on
/// `sutura_sql::Dialect::placeholder_style`: a rendered statement carries `?` and a
/// [`sutura_sql::GeneratedQuery`] carries an ORDERED list of values with no names, because a
/// parameter's identity in a plan IS its position. Named parameters would need a name invented per
/// parameter, with nothing in the domain to invent it from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterMode {
    /// `?` in the statement, an ordered array of values carrying no names beside it.
    Positional,
}

/// One query job, as this adapter asks for it.
///
/// Borrowed rather than owned throughout: it is built per call, handed to one transport, and dropped.
/// A clone here would copy the statement for no reason.
#[derive(Debug)]
pub struct JobRequest<'job> {
    statement: &'job str,
    params: &'job [ParamValue],
    billing_project: &'job ProjectId,
    default_dataset: &'job DatasetId,
}

impl<'job> JobRequest<'job> {
    /// Assembles a request.
    ///
    /// `pub(crate)` so the only thing that can build one is this adapter, from a plan it rendered
    /// itself. A public constructor would be the string entry point the module header says does not
    /// exist: a caller could pass any statement and any parameters.
    pub(crate) const fn new(
        statement: &'job str,
        params: &'job [ParamValue],
        billing_project: &'job ProjectId,
        default_dataset: &'job DatasetId,
    ) -> Self {
        Self {
            statement,
            params,
            billing_project,
            default_dataset,
        }
    }

    /// The statement, with its values still absent from it.
    #[inline]
    #[must_use]
    pub const fn statement(&self) -> &str {
        self.statement
    }

    /// The values, in the order the statement's placeholders take them.
    ///
    /// **The order is the contract**, because [`ParameterMode::Positional`] means the endpoint pairs
    /// the nth value with the nth `?`. A transport that reordered this would send a different query.
    #[inline]
    #[must_use]
    pub const fn params(&self) -> &[ParamValue] {
        self.params
    }

    /// Which parameter form the values are to be sent as.
    ///
    /// **An associated constant rather than a method, because it does not depend on the request.**
    /// Every request this adapter builds is positional, so a `&self` method would have taken a
    /// receiver it never read; a transport writes `JobRequest::PARAMETER_MODE` and gets the same
    /// answer. If a request ever needs to choose, this becomes a method and every call site has to be
    /// revisited - which is the right amount of friction for that change.
    pub const PARAMETER_MODE: ParameterMode = ParameterMode::Positional;

    /// The project this job is billed to.
    #[inline]
    #[must_use]
    pub const fn billing_project(&self) -> &ProjectId {
        self.billing_project
    }

    /// The dataset the statement's unqualified table names resolve in.
    ///
    /// **This is why the generated statement needs no qualifying**, and it is the reason the fourth
    /// dialect changed nothing about how a table is rendered: the endpoint's request carries a default
    /// dataset beside the SQL, so a bare backticked table name resolves there. The generator emits the
    /// same shape it emits for every other dialect.
    #[inline]
    #[must_use]
    pub const fn default_dataset(&self) -> &DatasetId {
        self.default_dataset
    }
}

/// The project a job is billed to, as this adapter holds it.
///
/// **A wrapper for the reason `sutura_exec_datafusion::WorkingSet` is one:** a project id and a
/// dataset id are both text, and a call taking two `&str` in the wrong order compiles and is wrong.
///
/// **Where the format is checked, and why it is checked twice.** `sutura_config::BillingProject`
/// refuses an unusable value when the settings tree is READ, so a deployment fails at startup rather
/// than on its first question - that is a diagnostic job. This type refuses it again because THIS is
/// the crate whose transport interpolates it into a request path, and a check belongs where the risk
/// is. The two are not one copy of one rule: an adapter may not depend on the settings tree, so
/// sharing the type would be an adapter reaching into another adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectId(String);

/// The dataset unqualified table names resolve in, as this adapter holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetId(String);

/// Why a resource name this adapter was handed is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnusableResourceName {
    /// Nothing was written, or only whitespace was.
    #[error("a {what} cannot be empty")]
    Empty { what: &'static str },
    /// A character that could leave the part of a request this value is written into.
    ///
    /// **The position is carried and the value is not.** A project id is one of the things this
    /// repository does not print, so a refusal says where the problem is rather than quoting it.
    #[error("the character at position {at} is not allowed in a {what}")]
    Character { what: &'static str, at: usize },
}

impl ProjectId {
    /// Parses a project id.
    ///
    /// The accepted set is `[a-z0-9-]`, which is what keeps the value inside one URL path segment: no
    /// `/`, no `?`, no `#`, no `%`, no whitespace, nothing non-ASCII. Length is NOT bounded here and is
    /// bounded where the value is declared - this crate's job is that the value cannot escape a
    /// request, and a too-short id is a diagnostic the settings tree already gives.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnusableResourceName> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(UnusableResourceName::Empty { what: "project id" });
        }
        for (at, character) in trimmed.chars().enumerate() {
            if !matches!(character, 'a'..='z' | '0'..='9' | '-') {
                return Err(UnusableResourceName::Character { what: "project id", at });
            }
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The id, for building a request.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl DatasetId {
    /// Parses a dataset id.
    ///
    /// `[A-Za-z0-9_]`, case PRESERVED - a dataset id is case-sensitive, so folding it here would turn
    /// a working declaration into a dataset that does not exist.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnusableResourceName> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(UnusableResourceName::Empty { what: "dataset id" });
        }
        for (at, character) in trimmed.chars().enumerate() {
            if !matches!(character, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_') {
                return Err(UnusableResourceName::Character { what: "dataset id", at });
            }
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The id, for building a request.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the endpoint said a column is.
///
/// **A closed set plus one named escape**, rather than a passthrough of every type the endpoint can
/// return. Each variant here is a claim that this adapter maps that type to a domain value and has a
/// test saying so; [`Self::Unmapped`] carries the endpoint's own spelling so a type nobody mapped
/// produces an error NAMING it rather than a null.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    /// A 64-bit integer.
    Int64,
    /// A double. Mapped through `Real`, which refuses a non-finite value.
    Float64,
    /// An exact decimal - `NUMERIC` or `BIGNUMERIC`. Mapped to TEXT rather than to a double, so an
    /// exact total stays exact; `sutura-exec-duckdb` maps its own `Decimal` the same way and for the
    /// same sentence.
    Numeric,
    /// A boolean.
    Bool,
    /// Text.
    String,
    /// A calendar date, as ISO text.
    Date,
    /// A type this adapter does not map, under the name the endpoint used for it.
    Unmapped(String),
}

/// One column, as the endpoint described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    name: String,
    kind: FieldType,
}

impl Field {
    /// Names one column.
    #[must_use]
    pub const fn of(name: String, kind: FieldType) -> Self {
        Self { name, kind }
    }

    /// The label a result column carries.
    #[inline]
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the endpoint said this column is.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> &FieldType {
        &self.kind
    }
}

/// One cell, as the endpoint sent it.
///
/// **Text or nothing, and that is the endpoint's shape rather than a simplification.** A value in a
/// query response is a JSON string whatever its declared type is - an integer arrives as `"250"` - so
/// the mapping from text to a typed domain value is this adapter's work, and [`Field::kind`] is what
/// decides it. Modelling it as already-typed here would move that work into the transport, where the
/// fake and the real implementor would each have to do it and could disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// JSON `null`.
    Null,
    /// A value, as the endpoint spelled it.
    Text(String),
}

/// A job's result: what the columns are, and the rows under them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRows {
    fields: Vec<Field>,
    rows: Vec<Vec<Cell>>,
}

impl JobRows {
    /// Assembles a result.
    #[must_use]
    pub const fn of(fields: Vec<Field>, rows: Vec<Vec<Cell>>) -> Self {
        Self { fields, rows }
    }

    /// The columns, in the order the statement projected them.
    #[inline]
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The rows.
    #[inline]
    #[must_use]
    pub fn rows(&self) -> &[Vec<Cell>] {
        &self.rows
    }
}

/// A `BigQuery` endpoint, as narrow as this adapter's needs.
///
/// Two methods, because the port above it has two questions with different costs: running a job reads
/// data and is billed, and validating one does neither. The endpoint really does distinguish them -
/// its request body carries a dry-run flag, and a dry run uses no slots and is not charged - which is
/// what makes `Warehouse::dry_run` able to answer `PreFlight::Accepted` honestly here rather than
/// inheriting the port's `NotAsked` default.
pub trait JobTransport {
    /// Why the endpoint could not answer. The adapter wraps it and never lets it reach a caller of
    /// the domain port raw.
    type Error: core::error::Error + Send + Sync + 'static;

    /// Runs a job and returns its rows.
    fn run(&self, request: &JobRequest<'_>) -> Result<JobRows, Self::Error>;

    /// Validates a job without reading data.
    ///
    /// Returns nothing on success: what a caller may conclude is *the endpoint accepted this*, and a
    /// dry run's byte estimate is not something any decision above here reads.
    fn validate(&self, request: &JobRequest<'_>) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::{DatasetId, ProjectId, UnusableResourceName};

    #[test]
    fn a_project_id_that_could_leave_a_url_path_segment_is_refused() {
        // The reason this type re-checks a value the settings tree already refused: THIS crate is the
        // one whose transport writes it into a request path.
        for hostile in [
            "acme/../other",
            "acme?alt=json",
            "acme#f",
            "acme%2f",
            "a b",
            "ACME",
            "acm\u{00e9}",
        ] {
            assert!(ProjectId::parse(hostile).is_err(), "{hostile:?} was accepted as a project id");
        }
        assert_eq!(
            ProjectId::parse("acme-analytics").expect("a plain id parses").as_str(),
            "acme-analytics"
        );
    }

    #[test]
    fn a_refusal_names_a_position_and_not_the_value() {
        let err = ProjectId::parse("acme/one").expect_err("a slash is refused");
        assert!(!err.to_string().contains("acme"), "{err}");
        assert_eq!(
            err,
            UnusableResourceName::Character {
                what: "project id",
                at: 4
            }
        );
    }

    #[test]
    fn a_dataset_id_keeps_its_case_and_refuses_a_hyphen() {
        // Case-sensitive, so folding would name a dataset that does not exist. A hyphen is legal in a
        // project id and not in a dataset id, which is why the two are not one type.
        assert_eq!(
            DatasetId::parse("Analytics_Prod").expect("mixed case parses").as_str(),
            "Analytics_Prod"
        );
        assert!(matches!(
            DatasetId::parse("analytics-prod"),
            Err(UnusableResourceName::Character { .. })
        ));
        assert_eq!(
            ProjectId::parse("analytics-prod")
                .expect("a hyphen IS in a project id")
                .as_str(),
            "analytics-prod"
        );
    }
}
