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
//!   credential source is a real addition to a workspace that cross-compiles to musl and gates
//!   licences exactly, and it arrives in exactly one place: [`crate::wire`], behind the crate's
//!   default-off `wire` feature. `docs/adr/0018` prices it. The sentence that kept this seam empty for
//!   a release - *nothing in this repository can verify a network client* - is now half spent: nothing
//!   in CI can, and a developer's own project has. Three tests passed against a real dataset on
//!   2026-08-30, over one hand-built `SUM` rather than the corpus.
//!
//! **What is deliberately NOT here: a method that takes a string.** The request carries a statement
//! this crate rendered from a plan, and there is no entry point a caller could hand SQL to.

use std::collections::BTreeSet;

use sutura_domain::identity::Secret;
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
    subject_bearer: Option<&'job Secret>,
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
        subject_bearer: Option<&'job Secret>,
    ) -> Self {
        Self {
            statement,
            params,
            billing_project,
            default_dataset,
            subject_bearer,
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

    /// The asking subject's own credential, where the leg carried one.
    ///
    /// **This is the half that makes a `BigQuery` source execute as the asker.** A
    /// [`Presented::SubjectToken`] carries the credential a broker minted for the asking subject - an
    /// exchanged Google access token scoped to that subject - and the transport sends it as its bearer
    /// for THIS job, so the endpoint evaluates the statement under whoever the token says. `None` for
    /// the shared posture, whose leg runs under the identity the transport itself already holds.
    #[inline]
    #[must_use]
    pub const fn subject_bearer(&self) -> Option<&Secret> {
        self.subject_bearer
    }

    /// Which parameter form the values are to be sent as.
    ///
    /// **An associated constant rather than a method, because it does not depend on the request.**
    /// Every request this adapter builds is positional, so a `&self` method would have taken a
    /// receiver it never read; a transport writes `JobRequest::PARAMETER_MODE` and gets the same
    /// answer. If a request ever needs to choose, this becomes a method and every call site has to be
    /// revisited - which is the right amount of friction for that change.
    pub const PARAMETER_MODE: ParameterMode = ParameterMode::Positional;

    /// `useLegacySql` is `false`, written rather than taken, because the endpoint's default is the
    /// wrong one.
    ///
    /// Everything this crate renders is `GoogleSQL` - backticks, `DATE_TRUNC(col, MONTH)`, and `?`
    /// parameters, which the endpoint describes as *"`GoogleSQL` only"*. The request's `useLegacySql`
    /// flag **defaults to `true`**, so a transport that writes [`Self::PARAMETER_MODE`] and does not
    /// remember this sends a legacy-SQL request. A transport writes `JobRequest::USE_LEGACY_SQL` and
    /// gets the same answer - the flag is a decision this adapter makes about the dialect, not one per
    /// request.
    pub const USE_LEGACY_SQL: bool = false;

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
///
/// **`Ord` is derived so the pre-flight can group by it**, and the ordering it derives is the inner
/// string's: `parse` neither trims into a different value nor folds case, so the wrapper compares
/// exactly as the text it holds does and there is no invariant for the derive to disagree with.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectId(String);

/// The dataset unqualified table names resolve in, as this adapter holds it.
///
/// `Ord` for the reason [`ProjectId`]'s is derived, plus one of its own: this type PRESERVES case, so
/// the derived ordering and the derived equality are the case-sensitive comparison a dataset id
/// really wants.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DatasetId(String);

/// One dataset, addressed the way a metadata read needs it: the project it lives in and its own id.
///
/// **A named pair rather than two arguments**, for the reason [`ProjectId`] is a wrapper at all: a
/// call taking two ids in the wrong order compiles and is wrong, and here the two are the same
/// shape. It is also the grouping key the pre-flight uses, which is what the derived `Ord` is for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DatasetAddress {
    project: ProjectId,
    dataset: DatasetId,
}

/// Every table one dataset holds, by the id it knows each under.
///
/// A name rather than the type, because `Result<BTreeSet<String>, _>` is over the `type_complexity`
/// threshold this workspace tightened - the same reason [`crate::BigQueryWarehouse`]'s `Mapped`
/// exists - and because *table ids* is what the set means where `BTreeSet<String>` is not.
pub type HeldTables = BTreeSet<String>;

impl DatasetAddress {
    /// Addresses a dataset.
    #[must_use]
    pub const fn of(project: ProjectId, dataset: DatasetId) -> Self {
        Self { project, dataset }
    }

    /// The project the dataset lives in, which is also the one the metadata read is attributed to.
    #[inline]
    #[must_use]
    pub const fn project(&self) -> &ProjectId {
        &self.project
    }

    /// The dataset's own id.
    #[inline]
    #[must_use]
    pub const fn dataset(&self) -> &DatasetId {
        &self.dataset
    }
}

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

impl FieldType {
    /// Decodes a type name the endpoint sends, into the closed vocabulary this adapter maps.
    ///
    /// A query response spells the types the legacy way - `INTEGER`/`FLOAT`/`BOOLEAN` - while the
    /// variants here are named after their modern spellings. The transport that reads an answer's
    /// schema calls this, so which spellings become `Int64` is decided HERE, where the value mapping
    /// lives, and not in the unbuilt transport. A name nobody maps becomes [`Self::Unmapped`] under
    /// the endpoint's own spelling, so an answer is refused NAMING it rather than answered as null.
    #[must_use]
    pub fn parse(name: &str) -> Self {
        match name {
            "INT64" | "INTEGER" => Self::Int64,
            "FLOAT64" | "FLOAT" => Self::Float64,
            "BOOL" | "BOOLEAN" => Self::Bool,
            "NUMERIC" | "BIGNUMERIC" => Self::Numeric,
            "STRING" => Self::String,
            "DATE" => Self::Date,
            other => Self::Unmapped(String::from(other)),
        }
    }
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

/// A job's result: what the columns are, the rows under them, and how many the job produced.
///
/// **The count is part of the result, and that is what makes a partial answer not a result.** The
/// endpoint's `jobs.query` answers one page - "as many results as can be contained within the
/// maximum permitted reply size" - and `totalRows` "can be more than the number of rows in this
/// single page". A first page, or an incomplete job's empty `rows`, is *under the cap, not
/// truncated*, and this adapter's `rows` refuses a delivered count that does not equal what the
/// endpoint reported as total - see [`super::BigQueryError::Incomplete`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRows {
    fields: Vec<Field>,
    rows: Vec<Vec<Cell>>,
    total_rows: usize,
}

impl JobRows {
    /// Assembles a result.
    ///
    /// `total_rows` is what the endpoint reported as `totalRows`, which is present only when a job is
    /// complete - so an incomplete job has no value to fill it with, and the transport has to error.
    #[must_use]
    pub const fn of(fields: Vec<Field>, rows: Vec<Vec<Cell>>, total_rows: usize) -> Self {
        Self {
            fields,
            rows,
            total_rows,
        }
    }

    /// The columns, in the order the statement projected them.
    #[inline]
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The rows on this page.
    #[inline]
    #[must_use]
    pub fn rows(&self) -> &[Vec<Cell>] {
        &self.rows
    }

    /// What the endpoint said the job's total is, which a delivered page is compared against.
    #[inline]
    #[must_use]
    pub const fn total_rows(&self) -> usize {
        self.total_rows
    }
}

/// A `BigQuery` endpoint, as narrow as this adapter's needs.
///
/// Two methods PUT A QUESTION TO THE ENDPOINT, because the port above it has two questions with
/// different costs: running a job reads data and is billed, and validating one does neither. The
/// endpoint really does distinguish them - its request body carries a dry-run flag, and a dry run
/// uses no slots and is not charged - which is what makes `Warehouse::dry_run` able to answer
/// `PreFlight::Accepted` honestly here rather than inheriting the port's `NotAsked` default.
///
/// **Three more members are not that, and the count is spelled out because it has been wrong
/// twice.** `result_did_not_fit` asks the implementor about a failure it already has and sends
/// nothing; `list_tables` sends a metadata read rather than a statement, which is what makes it
/// cheap enough for a boot check; and `apply`, behind the `fixtures` feature, is the second
/// statement-issuing method - present only in a build that loads fixtures, so no deployment can
/// reach it.
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

    /// Every table one dataset holds, by the id the dataset knows it under.
    ///
    /// **The one member of this trait that issues no statement and reads no rows**, which is what
    /// makes it cheap enough to run at boot: it is a metadata read over a whole dataset, so a bundle
    /// naming forty models costs one call rather than forty. `crate::BigQueryWarehouse::preflight` is
    /// the only caller, and `sutura_domain::warehouse::Warehouse::preflight` is the port above it that
    /// says why a boot check exists at all.
    ///
    /// **Required, with no default, and the two defaults available are the reason.** An empty set
    /// would report every table in the bundle as absent and refuse a correct deployment; a set that
    /// claimed to hold everything asked for would be the lie the port above forbids. A transport that
    /// cannot list has to say so as an `Err`, which is the outcome the port keeps separate from *this
    /// table is absent* precisely so an operator is not sent to fix the wrong thing.
    ///
    /// **It takes a [`DatasetAddress`] rather than reading one off a [`JobRequest`]**, because there
    /// is no job: a bundle whose models name a second dataset is one call per dataset, and a request
    /// carries exactly one default dataset. The project is the one the dataset lives in, which for an
    /// unqualified model is the source's billing project and for a qualified one is whatever the path
    /// names.
    ///
    /// # Errors
    ///
    /// Whatever the implementor's own failure is: a credential with no permission to list, a dataset
    /// that is not there, an endpoint that did not answer. What it must NOT do is report any of those
    /// as an empty listing.
    fn list_tables(&self, at: &DatasetAddress) -> Result<HeldTables, Self::Error>;

    /// Was this failure the endpoint declining to return the whole result at once?
    ///
    /// **The `Warehouse::result_did_not_fit` question, one port further down, and it has to be asked
    /// here for the same reason it is asked there.** `Self::Error` is the implementor's own type, so
    /// `BigQueryWarehouse` - which holds the failure as `BigQueryError::Endpoint` - cannot read it.
    /// The domain port names what it needs of an adapter; this names what the adapter needs of its
    /// transport.
    ///
    /// `true` becomes `RefusalReason::ResultTooLarge` carrying `ResultBound::Volume` above the domain
    /// port, so the answer a caller gets is *too much data, ask a narrower question* and not the
    /// `503` a dead endpoint produces. `jobs.query` returns one page - as many rows as fit the maximum
    /// permitted reply size - so a result INSIDE the row cap can still be over that, and a retry
    /// returns the same page.
    ///
    /// **A predicate rather than a conversion, and a `bool` rather than a byte count**, both for the
    /// reasons the domain port states: the refusal vocabulary is the domain's, and the reply-size cap
    /// is the service's own number, reported in neither the answer nor the refusal - so there is
    /// nothing honest to put in a numeric field.
    ///
    /// Defaulted to `false`, which is the answer a fake gives unless a test is about this bound.
    fn result_did_not_fit(&self, _error: &Self::Error) -> bool {
        false
    }

    /// Runs a statement that produces no result set.
    ///
    /// **A method of its own rather than a use of [`Self::run`], and it is behind the `fixtures`
    /// feature so a build that serves questions cannot issue one.** This trait's header says why it
    /// puts two questions to the endpoint, each with its own cost; this is a third - *put these rows
    /// in this table* - that only the corpus acceptance leg asks, and nothing a deployment links can.
    ///
    /// **Why it cannot be [`Self::run`].** `run`'s contract is a COMPLETE result set: the wire refuses
    /// an answer whose `totalRows` is absent or does not equal the delivered count, which is the
    /// mechanism that stops a first page reading as a whole answer. A `CREATE OR REPLACE TABLE` job
    /// has no result set for that check to be about, and what its answer document carries is not
    /// something this repository has verified - so routing a `CREATE` through `run` would be relying
    /// on a response shape nobody here has measured, in the one place a failure is a silently
    /// half-loaded fixture. This method requires the job to have COMPLETED and requires nothing else.
    ///
    /// It still takes a [`JobRequest`], whose constructor is `pub(crate)`, so this is not the string
    /// entry point the module header says does not exist: the statement is rendered by
    /// [`crate::importer`] from a committed CSV whose every cell was parsed first.
    #[cfg(feature = "fixtures")]
    fn apply(&self, request: &JobRequest<'_>) -> Result<(), Self::Error>;
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

    #[test]
    fn a_type_name_the_endpoint_sends_decodes_to_the_vocabulary_this_adapter_maps() {
        // A query response spells the legacy names; the closed vocabulary is named after the modern
        // forms. Decoding belongs HERE so a transport written from the variant names cannot map
        // `INTEGER` to `Unmapped` and hand a live answer a type nobody mapped.
        use super::FieldType;
        for (wire, expected) in [
            ("INTEGER", FieldType::Int64),
            ("INT64", FieldType::Int64),
            ("FLOAT", FieldType::Float64),
            ("FLOAT64", FieldType::Float64),
            ("BOOLEAN", FieldType::Bool),
            ("BOOL", FieldType::Bool),
            ("NUMERIC", FieldType::Numeric),
            ("BIGNUMERIC", FieldType::Numeric),
            ("STRING", FieldType::String),
            ("DATE", FieldType::Date),
        ] {
            assert_eq!(FieldType::parse(wire), expected, "{wire}");
        }
        // The limit the crate documentation names: a time column is a time the endpoint has and this
        // adapter does not, so it stays named rather than becoming a column that answers.
        assert_eq!(FieldType::parse("TIMESTAMP"), FieldType::Unmapped(String::from("TIMESTAMP")));
    }
}
