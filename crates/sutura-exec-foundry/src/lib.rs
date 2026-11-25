#![forbid(unsafe_code)]
//! **Experimental.** A [`Warehouse`] over Foundry Ontology SQL's synchronous Arrow endpoint.
//!
//! The adapter renders the semantic plan, sends it to `executeOntology`, and decodes the returned
//! Arrow IPC stream through the domain's [`Accumulating`] guard. It is intentionally separate from
//! `sutura-catalog-foundry`: a source connection is useful with any catalog that names the same
//! ontology, and adapters may not depend on one another.
//!
//! The HTTP transport is behind the default-off `http` feature. The endpoint is public beta and
//! requires an `OSv2` ontology plus `api:sql-queries-execute` and `api:ontologies-read`. This branch
//! has no tenant venue, so the transport and result path are covered by fakes only; no live-system
//! compatibility claim is made.
//!
//! # Who a question runs as
//!
//! A `shared-service-user` source sends the deployment's own token ([`FoundryWarehouse::new`]): a
//! fixed one, or one a [`DeploymentToken`] mints and refreshes - the composition root hands it the
//! one client-credentials grant every Foundry entry shares. An `impersonation-at-source` source holds no token at all
//! ([`FoundryWarehouse::each_caller`]): each question sends the bearer the broker presented as
//! [`Presented::SubjectToken`] - the caller's own Foundry token, which the identity provider stored
//! when the caller linked their Foundry account - unchanged, so Foundry's own permissions on that
//! user decide what the question reads, and a `401`/`403` is [`Warehouse::source_refused`].
//! **Limits:** built and held by fakes only - no tenant has accepted a brokered token here, and
//! whether such a token carries the scopes this endpoint needs is the tenant's application
//! registration, unverified.
//!
//! **Startup is its own identity, typed apart.** An impersonating source verifies anchors only with
//! a declared [`BootIdentity`] ([`FoundryWarehouse::with_boot_identity`]), which
//! [`Warehouse::verify_anchor`] reads and the per-question path cannot: the request path matches
//! only the caller's token there. Without one, an anchor is [`FoundryError::NoBootIdentity`].

use std::time::Instant;

use arrow_buffer::Buffer;
use sutura_domain::identity::{Presented, Secret};
use sutura_domain::model::{ObjectTypeExclusions, QualifiedTable, SourceName};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::arrow::{Accumulating, ResultBudget, UnannouncedBatch};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::preflight::{AbsentTables, TablesPresent};
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, Warehouse};
use sutura_sql::{Dialect, GeneratedQuery};

mod bearer;
mod directory;
mod error;
#[cfg(feature = "http")]
pub mod http;
mod resolve;

pub use error::FoundryError;

use bearer::Bearer;
pub use bearer::{BootIdentity, DeploymentToken, TokenFailed};
pub use directory::{DIRECTORY_TTL, DirectoryFailed, Listed, ObjectTypeDirectory, ResourceIds};
use directory::{Directory, Whose};

/// The JSON request sent to Foundry's ontology SQL endpoint.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlRequest {
    /// The generated Spark SQL statement. Parameters remain in [`Parameters`] and are never
    /// interpolated into this field.
    query: String,
    /// The ontology API name or RID selected by the source connection.
    #[serde(rename = "ontologyIdentifier")]
    ontology_identifier: String,
    /// Reaching this response boundary is refused because the service may have truncated its answer.
    #[serde(rename = "rowLimit")]
    row_limit: usize,
    /// Positional values for the generated `?` placeholders.
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Parameters>,
    /// Whether this is the endpoint's parse-only mode.
    dry_run: bool,
}

/// The outer Parameters union documented by Foundry's SDK.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Parameters {
    /// The discriminator required by the public API.
    #[serde(rename = "type")]
    kind: &'static str,
    /// Positional parameter values, in placeholder order.
    values: Vec<SqlParameter>,
}

/// A native Foundry SQL parameter value. It is kept as JSON rather than rendered into SQL text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SqlParameter {
    /// A text parameter for a `?` placeholder.
    String { value: String },
    /// A date parameter in ISO form.
    Date { value: String },
}

impl SqlRequest {
    fn from_query(query: &GeneratedQuery, ontology: &OntologyIdentifier, row_limit: usize, dry_run: bool) -> Self {
        Self {
            query: query.sql().to_owned(),
            ontology_identifier: ontology.as_str().to_owned(),
            row_limit,
            parameters: (!query.params().is_empty()).then(|| Parameters {
                kind: "unnamedParameterValues",
                values: query.params().iter().map(SqlParameter::from).collect(),
            }),
            dry_run,
        }
    }
}

impl From<&sutura_domain::warehouse::ParamValue> for SqlParameter {
    fn from(value: &sutura_domain::warehouse::ParamValue) -> Self {
        match value {
            sutura_domain::warehouse::ParamValue::Text(value) => Self::String { value: value.clone() },
            sutura_domain::warehouse::ParamValue::Date(value) => Self::Date { value: value.to_iso() },
        }
    }
}

/// A Foundry ontology API name or resource identifier used by the endpoint's JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyIdentifier(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the Foundry ontology identifier is not one path-safe API name or resource identifier")]
pub struct InvalidOntologyIdentifier;

impl OntologyIdentifier {
    /// Parses the identifier used in the request body. The API name/resource identifier grammar is
    /// shared with the Foundry catalog reader: one ASCII letter or digit first, then only letters,
    /// digits, `.`, `-` or `_`.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidOntologyIdentifier> {
        let raw = raw.as_ref();
        let valid = raw.len() <= 255
            && raw.bytes().next().is_some_and(|first| first.is_ascii_alphanumeric())
            && raw
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
        valid.then(|| Self(String::from(raw))).ok_or(InvalidOntologyIdentifier)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The transport seam: a fake can assert the request without a tenant, while [`http::HttpFoundry`]
/// performs the real bounded request.
pub trait FoundryTransport {
    type Error: std::error::Error + Send + Sync + 'static;

    fn execute(&self, request: &SqlRequest, token: &Secret, deadline: Deadline) -> TransportResponse<Self::Error>;

    fn source_refused(&self, error: &Self::Error) -> bool;

    fn deadline_exceeded(&self, error: &Self::Error) -> bool;

    fn result_did_not_fit(&self, _error: &Self::Error) -> bool {
        false
    }

    /// Whether Foundry found no object type behind a name the statement gave it. Defaulted to
    /// `false`, the answer for a transport that cannot tell.
    fn object_not_found(&self, _error: &Self::Error) -> bool {
        false
    }
}

type TransportResponse<E> = Result<Vec<u8>, E>;
/// A request, the statement it carries, and the API names resolved for it.
type Prepared = (SqlRequest, GeneratedQuery, Vec<String>);
type WarehouseResult<E, T> = Result<T, FoundryError<E>>;
/// Foundry's answer bytes, or why there are none.
type Answered<E> = Result<Vec<u8>, FoundryError<E>>;

/// A connection to one Foundry ontology.
#[derive(Debug)]
pub struct FoundryWarehouse<T> {
    source: SourceName,
    posture: SourcePosture,
    ontology: OntologyIdentifier,
    bearer: Bearer,
    transport: T,
    result_budget: ResultBudget,
    directory: Option<Directory>,
    exclusions: ObjectTypeExclusions,
}

impl<T> FoundryWarehouse<T> {
    /// Builds a connection over a fixed deployment token and the process's result budget.
    #[must_use]
    pub fn new(
        source: SourceName,
        posture: SourcePosture,
        ontology: OntologyIdentifier,
        token: impl DeploymentToken + 'static,
        transport: T,
        result_budget: ResultBudget,
    ) -> Self {
        Self {
            source,
            posture,
            ontology,
            bearer: Bearer::Deployment(Box::new(token)),
            transport,
            result_budget,
            directory: None,
            exclusions: ObjectTypeExclusions::none(),
        }
    }

    /// Builds an `impersonation-at-source` connection: every request carries the caller's own token
    /// the broker presented, and none carries a token of the deployment's.
    #[must_use]
    pub const fn each_caller(
        source: SourceName,
        ontology: OntologyIdentifier,
        transport: T,
        result_budget: ResultBudget,
    ) -> Self {
        Self {
            source,
            posture: SourcePosture::ImpersonationAtSource,
            ontology,
            bearer: Bearer::EachCaller { boot: None },
            transport,
            result_budget,
            directory: None,
            exclusions: ObjectTypeExclusions::none(),
        }
    }

    /// Resolves every model a catalog names by API name to its object type's RID through
    /// `directory`: a question's under its own token, an anchor's under the startup identity, into
    /// two separate maps (`directory` module header). Without one, a statement names the object as
    /// written.
    #[must_use]
    pub fn with_directory(mut self, directory: impl ObjectTypeDirectory + 'static) -> Self {
        self.directory = Some(Directory::new(Box::new(directory)));
        self
    }

    /// Leaves out the object types `exclusions` matches: a model naming one by API name is refused at
    /// boot ([`Warehouse::preflight`]) and at query time, and one naming it by a RID a read listed as
    /// excluded is refused at query time.
    #[must_use]
    pub fn with_exclusions(mut self, exclusions: ObjectTypeExclusions) -> Self {
        self.exclusions = exclusions;
        self
    }

    /// Gives an impersonating source its startup-only identity, so its anchors can be verified. A
    /// shared source is returned unchanged: its anchors already run as its deployment identity.
    #[must_use]
    pub fn with_boot_identity(mut self, boot: impl DeploymentToken + 'static) -> Self {
        if let Bearer::EachCaller { boot: ref mut held } = self.bearer {
            *held = Some(BootIdentity::new(Box::new(boot)));
        }
        self
    }
}

/// Foundry's service-side hard row cap. A response reaching this marker is potentially truncated.
pub const FOUNDRY_MAX_ROW_LIMIT: usize = 10_000;
const MAX_SCHEMA_FIELDS: usize = 1024;
const MAX_SCHEMA_TEXT_BYTES: usize = 64 * 1024;
impl<T> FoundryWarehouse<T>
where
    T: FoundryTransport,
{
    /// The token `presented` sends: the deployment's for a shared leg on a shared source, the
    /// caller's own for a subject's token on a per-caller one - and only where the leg agrees with
    /// this source's declared posture. Every other pairing is refused before any request.
    fn delivered(&self, presented: &Presented, deadline: Deadline) -> WarehouseResult<T::Error, Secret> {
        match (&self.bearer, presented) {
            (Bearer::Deployment(_), Presented::SharedServiceUser { .. })
            | (Bearer::EachCaller { .. }, Presented::SubjectToken { .. }) => {}
            (
                Bearer::Deployment(_),
                Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } | Presented::SubjectSignIn { .. },
            )
            | (
                Bearer::EachCaller { .. },
                Presented::SharedServiceUser { .. } | Presented::SubjectPrincipal { .. } | Presented::SubjectSignIn { .. },
            ) => {
                return Err(FoundryError::Undeliverable {
                    presented: presented.as_str(),
                });
            }
        }
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| FoundryError::PresentedDisagreesWithPosture { cause })?;
        // Agreed: the deployment's token for a shared leg, the caller's own for a subject's.
        match (&self.bearer, presented) {
            (Bearer::EachCaller { .. }, Presented::SubjectToken { material }) => Ok(material.clone()),
            (Bearer::Deployment(token), _) => token.bearer(deadline).map_err(|cause| FoundryError::Token { cause }),
            (
                Bearer::EachCaller { .. },
                Presented::SharedServiceUser { .. } | Presented::SubjectPrincipal { .. } | Presented::SubjectSignIn { .. },
            ) => Err(FoundryError::Undeliverable {
                presented: presented.as_str(),
            }),
        }
    }

    /// The request for `executable`, under `token`, the statement it carries for the answer's
    /// execution record, and the API names resolved for it through `whose` map.
    fn request(
        &self,
        executable: Executable<'_>,
        dry_run: bool,
        token: &Secret,
        deadline: Deadline,
        whose: Whose,
    ) -> WarehouseResult<T::Error, Prepared> {
        let row_limit = executable
            .row_limit()
            .map_or(FOUNDRY_MAX_ROW_LIMIT, |rows| rows.min(FOUNDRY_MAX_ROW_LIMIT));
        let (references, resolved) = resolve::references(
            self.directory.as_ref(),
            &self.exclusions,
            whose,
            &resolve::tables_of(executable),
            token,
            deadline,
        )?;
        let generated = match executable {
            Executable::Query(plan) => sutura_sql::generate_with(plan, Dialect::Foundry, &references),
            Executable::Leg(plan) => sutura_sql::generate_leg_with(plan, Dialect::Foundry, &references),
            Executable::Stacked(plan) => sutura_sql::generate_stacked_with(plan, Dialect::Foundry, &references),
        }
        .map_err(|cause| FoundryError::Render { cause })?;
        if generated.source() != &self.source {
            return Err(FoundryError::WrongSource);
        }
        Ok((
            SqlRequest::from_query(&generated, &self.ontology, row_limit, dry_run),
            generated,
            resolved,
        ))
    }

    /// Sends `request` under `token`. Foundry not finding an object type behind a statement that
    /// named a resolved RID drops the `resolved` names from `whose` map and is
    /// [`FoundryError::StaleObjectType`].
    fn sent(
        &self,
        request: &SqlRequest,
        token: &Secret,
        deadline: Deadline,
        resolved: &[String],
        whose: Whose,
    ) -> Answered<T::Error> {
        self.transport
            .execute(request, token, deadline)
            .map_err(|cause| match self.directory.as_ref() {
                Some(directory) if !resolved.is_empty() && self.transport.object_not_found(&cause) => {
                    directory.forget(whose, resolved);
                    FoundryError::StaleObjectType { cause }
                }
                Some(_) | None => FoundryError::Endpoint { cause },
            })
    }

    /// Asks Foundry to parse, in the endpoint's parse-only mode, a row count over `table` as the
    /// Foundry dialect names it, under the token `presented` delivers: whether the endpoint accepts
    /// that token and resolves that reference. Nothing runs, and no answer is decoded.
    ///
    /// # Errors
    ///
    /// [`FoundryError::Endpoint`] for Foundry's refusal; the refusals [`Warehouse::dry_run`] gives
    /// for a token this source cannot deliver; [`FoundryError::Render`] for a table the dialect
    /// cannot name.
    pub fn parse_object(
        &self,
        table: &QualifiedTable,
        reference: Reference,
        presented: &Presented,
        deadline: Deadline,
    ) -> WarehouseResult<T::Error, ()> {
        let token = self.delivered(presented, deadline)?;
        let by_rid;
        let (table, resolved) = match reference {
            Reference::AsWritten => (table, Vec::new()),
            Reference::Resolved => {
                let (references, resolved) = resolve::references(
                    self.directory.as_ref(),
                    &self.exclusions,
                    Whose::Question,
                    &[table],
                    &token,
                    deadline,
                )?;
                by_rid = references
                    .get(table.name())
                    .cloned()
                    .map_or_else(|| table.clone(), QualifiedTable::resource);
                (&by_rid, resolved)
            }
        };
        let generated = sutura_sql::generate_object_probe(&self.source, table, Dialect::Foundry)
            .map_err(|cause| FoundryError::Render { cause })?;
        let request = SqlRequest::from_query(&generated, &self.ontology, 1, true);
        self.sent(&request, &token, deadline, &resolved, Whose::Question)?;
        Ok(())
    }

    /// Obtains, and drops, a shared source's own bearer - a fixed token as it is, or one mint of its
    /// grant - for an operator's check. `None` for a per-caller source: its startup identity is read
    /// by [`Warehouse::verify_anchor`] alone.
    ///
    /// # Errors
    ///
    /// [`TokenFailed`] inside the `Some` when no bearer could be obtained within `deadline`.
    pub fn deployment_token(&self, deadline: Deadline) -> Option<Result<(), TokenFailed>> {
        match self.bearer {
            Bearer::Deployment(ref token) => Some(token.bearer(deadline).map(drop)),
            Bearer::EachCaller { .. } => None,
        }
    }

    /// Obtains, and drops, an impersonating source's startup identity - one mint of its grant - for
    /// an operator's check. `None` where there is none: a shared source, or one declaring no
    /// `verification_client_credentials`.
    ///
    /// # Errors
    ///
    /// [`TokenFailed`] inside the `Some` when no bearer could be obtained within `deadline`.
    pub fn boot_token(&self, deadline: Deadline) -> Option<Result<(), TokenFailed>> {
        match self.bearer {
            Bearer::EachCaller { boot: Some(ref boot) } => Some(boot.token().bearer(deadline).map(drop)),
            Bearer::Deployment(_) | Bearer::EachCaller { boot: None } => None,
        }
    }

    fn decode(&self, bytes: Vec<u8>, most_rows: usize, deadline: Deadline) -> WarehouseResult<T::Error, ResultBatches> {
        validate_ipc(&bytes, self.result_budget).map_err(|cause| FoundryError::Arrow { cause })?;
        let mut decoder = arrow_ipc::reader::StreamDecoder::new();
        let mut input = Buffer::from_vec(bytes);
        let mut accumulating: Option<Accumulating> = None;
        while !input.is_empty() {
            if deadline.remaining_at(Instant::now()).is_none() {
                return Err(FoundryError::DeadlineSpent);
            }
            if let Some(batch) = decoder.decode(&mut input).map_err(|cause| FoundryError::Arrow { cause })? {
                let schema = decoder.schema().ok_or_else(|| FoundryError::Arrow {
                    cause: arrow_schema::ArrowError::IpcError(String::from("Arrow result has no schema")),
                })?;
                let accumulating =
                    accumulating.get_or_insert_with(|| Accumulating::announcing(schema, most_rows, self.result_budget));
                accumulating
                    .push(batch)
                    .map_err(|cause| FoundryError::Unannounced { cause })?;
            }
        }
        decoder.finish().map_err(|cause| FoundryError::Arrow { cause })?;
        if deadline.remaining_at(Instant::now()).is_none() {
            return Err(FoundryError::DeadlineSpent);
        }
        let Some(accumulating) = accumulating else {
            let schema = decoder.schema().ok_or_else(|| FoundryError::Arrow {
                cause: arrow_schema::ArrowError::IpcError(String::from("Arrow result has no schema")),
            })?;
            return Ok(Accumulating::announcing(schema, most_rows, self.result_budget).finish());
        };
        Ok(accumulating.finish())
    }
}

impl<T> Warehouse for FoundryWarehouse<T>
where
    T: FoundryTransport,
{
    type Error = FoundryError<T::Error>;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        let token = self.delivered(presented, deadline)?;
        let (request, _, resolved) = self.request(executable, true, &token, deadline, Whose::Question)?;
        self.sent(&request, &token, deadline, &resolved, Whose::Question)?;
        Ok(PreFlight::Accepted { estimated_bytes: None })
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        let token = self.delivered(presented, deadline)?;
        let (request, generated, resolved) = self.request(executable, false, &token, deadline, Whose::Question)?;
        let bytes = self.sent(&request, &token, deadline, &resolved, Whose::Question)?;
        // The record every answer carries of what ran: without it the service refuses the answer.
        Ok(self.decode(bytes, accepted_rows(executable), deadline)?.with_execution(
            sutura_domain::warehouse::execution::Execution::sql(
                generated.source().clone(),
                TypeDialect::Foundry,
                generated.sql().to_owned(),
                generated.params(),
            ),
        ))
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        // The one reader of the startup identity: the deployment's own on a shared source, the
        // declared boot identity on an impersonating one, and never a caller's.
        let minter: &dyn DeploymentToken = match self.bearer {
            Bearer::Deployment(ref token) => token.as_ref(),
            Bearer::EachCaller { boot: Some(ref boot) } => boot.token(),
            Bearer::EachCaller { boot: None } => return Err(FoundryError::NoBootIdentity),
        };
        let deadline = boot_deadline();
        let token = minter.bearer(deadline).map_err(|cause| FoundryError::Token { cause })?;
        let (request, _, resolved) = self.request(Executable::Query(plan.plan()), false, &token, deadline, Whose::Boot)?;
        let bytes = self.sent(&request, &token, deadline, &resolved, Whose::Boot)?;
        self.decode(bytes, accepted_rows(Executable::Query(plan.plan())), deadline)?
            .to_rows()
            .map(AnchorRows::of)
            .map_err(|cause| FoundryError::Unreadable { cause })
    }

    fn type_dialect(&self) -> Option<TypeDialect> {
        Some(TypeDialect::Foundry)
    }

    /// The tables named by an API name `exclude_object_types` leaves out, as tables this source
    /// does not hold. Nothing else is asked: a table's existence is Foundry's to answer at query
    /// time, and one written as a RID is checked then.
    fn preflight(&self, tables: &std::collections::BTreeSet<QualifiedTable>) -> Result<TablesPresent, Self::Error> {
        Ok(AbsentTables::parse(resolve::excluded(tables, &self.exclusions))
            .map_or(TablesPresent::NotAsked, TablesPresent::AllBut))
    }

    /// Foundry's `401`/`403`, an object-type list it refused to this token, and every way an object
    /// type a statement needs did not resolve - not listed, listed without a RID, left out, or not
    /// found behind its RID. **One refusal for all of them, deliberately:** a caller cannot tell a
    /// type that does not exist from one they may not see, or from one another caller's read put in
    /// the shared map (`directory` module header). The typed cause stays in the log.
    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, FoundryError::Endpoint { cause } if self.transport.source_refused(cause))
            || matches!(
                error,
                FoundryError::UnknownObjectType { .. }
                    | FoundryError::NoResourceId { .. }
                    | FoundryError::ExcludedObjectType { .. }
                    | FoundryError::StaleObjectType { .. }
                    | FoundryError::Directory {
                        cause: DirectoryFailed::Refused { status: 401 | 403 }
                    }
            )
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        match error {
            FoundryError::Unannounced {
                cause: UnannouncedBatch::OverBound { .. } | UnannouncedBatch::OverBudget { .. },
            }
            | FoundryError::Arrow {
                cause: arrow_schema::ArrowError::MemoryError(_),
            } => true,
            FoundryError::Endpoint { cause } => self.transport.result_did_not_fit(cause),
            FoundryError::Arrow { .. }
            | FoundryError::DeadlineSpent
            | FoundryError::Render { .. }
            | FoundryError::PresentedDisagreesWithPosture { .. }
            | FoundryError::Unannounced { .. }
            | FoundryError::Unreadable { .. }
            | FoundryError::WrongSource
            | FoundryError::Undeliverable { .. }
            | FoundryError::NoBootIdentity
            | FoundryError::Token { .. }
            | FoundryError::UnknownObjectType { .. }
            | FoundryError::NoResourceId { .. }
            | FoundryError::ExcludedObjectType { .. }
            | FoundryError::StaleObjectType { .. }
            | FoundryError::Directory { .. }
            | FoundryError::UnsupportedPlan => false,
        }
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(error, FoundryError::DeadlineSpent)
            || matches!(error, FoundryError::Endpoint { cause } if self.transport.deadline_exceeded(cause))
    }
}

/// How a probed object type is named: as the model writes it, or by the RID the object-type list
/// resolves it to - the form a question is sent with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reference {
    AsWritten,
    Resolved,
}

fn boot_deadline() -> Deadline {
    #[expect(clippy::unreachable, reason = "the literal boot budget is non-zero by construction")]
    let budget = sutura_domain::warehouse::deadline::Budget::parse(core::time::Duration::from_secs(3600))
        .unwrap_or_else(|_| unreachable!("a non-zero literal is a budget"));
    Deadline::opened_at(Instant::now(), budget)
}

fn accepted_rows(executable: Executable<'_>) -> usize {
    executable
        .row_limit()
        .map_or(FOUNDRY_MAX_ROW_LIMIT.saturating_sub(1), |rows| {
            rows.min(FOUNDRY_MAX_ROW_LIMIT).saturating_sub(1)
        })
}

#[expect(
    clippy::little_endian_bytes,
    reason = "Arrow IPC lengths are defined as little-endian on the wire"
)]
const fn ipc_i32(bytes: [u8; 4]) -> i32 {
    i32::from_le_bytes(bytes)
}

/// Checks IPC framing and schema conversion before the decoder allocates metadata or record
/// bodies. Compression is refused until decoded expansion can be charged against the result budget.
fn validate_ipc(bytes: &[u8], budget: ResultBudget) -> Result<(), arrow_schema::ArrowError> {
    let mut offset = 0_usize;
    let mut charged_buffers = 0_usize;
    let mut first = true;
    loop {
        if bytes.len().saturating_sub(offset) < 4 {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "truncated Arrow IPC message length",
            )));
        }
        let mut length_bytes = [0_u8; 4];
        let length = bytes
            .get(offset..offset + 4)
            .ok_or_else(|| arrow_schema::ArrowError::IpcError(String::from("truncated Arrow IPC message length")))?;
        length_bytes.copy_from_slice(length);
        offset += 4;
        if length_bytes == [0, 0, 0, 0] {
            return Ok(());
        }
        if length_bytes == [0xff; 4] {
            if bytes.len().saturating_sub(offset) < 4 {
                return Err(arrow_schema::ArrowError::IpcError(String::from(
                    "truncated Arrow IPC continuation length",
                )));
            }
            let length = bytes
                .get(offset..offset + 4)
                .ok_or_else(|| arrow_schema::ArrowError::IpcError(String::from("truncated Arrow IPC continuation length")))?;
            length_bytes.copy_from_slice(length);
            offset += 4;
            if length_bytes == [0, 0, 0, 0] {
                return Ok(());
            }
        }
        let metadata_len = ipc_i32(length_bytes);
        if metadata_len <= 0 {
            return Err(arrow_schema::ArrowError::ParseError(String::from(
                "invalid Arrow IPC metadata length",
            )));
        }
        let metadata_len = usize::try_from(metadata_len).map_err(|error| {
            arrow_schema::ArrowError::ParseError(format!("Arrow IPC metadata length does not fit in usize: {error}"))
        })?;
        let metadata_end = offset.checked_add(metadata_len).ok_or_else(|| {
            arrow_schema::ArrowError::IpcError(String::from("Arrow IPC metadata length overflows the response"))
        })?;
        if metadata_end > bytes.len() {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "Arrow IPC metadata exceeds the response",
            )));
        }
        let metadata = bytes
            .get(offset..metadata_end)
            .ok_or_else(|| arrow_schema::ArrowError::IpcError(String::from("Arrow IPC metadata exceeds the response")))?;
        let message = arrow_ipc::root_as_message(metadata)
            .map_err(|cause| arrow_schema::ArrowError::ParseError(format!("invalid Arrow IPC message: {cause:?}")))?;
        if first && message.header_type() != arrow_ipc::MessageHeader::Schema {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "Arrow IPC stream has no schema first",
            )));
        }
        first = false;
        if message.header_type() == arrow_ipc::MessageHeader::Schema {
            let schema = message
                .header_as_schema()
                .ok_or_else(|| arrow_schema::ArrowError::ParseError(String::from("Arrow IPC schema header is absent")))?;
            validate_fb_schema(&schema)?;
            let schema = arrow_ipc::convert::try_fb_to_schema(schema)?;
            validate_schema(&schema)?;
        }
        if message.header_type() == arrow_ipc::MessageHeader::RecordBatch {
            let batch = message
                .header_as_record_batch()
                .ok_or_else(|| arrow_schema::ArrowError::IpcError(String::from("Arrow IPC record batch header is absent")))?;
            validate_buffers(&batch, message.bodyLength(), budget, &mut charged_buffers)?;
            if batch.compression().is_some() {
                return Err(arrow_schema::ArrowError::IpcError(String::from(
                    "compressed Foundry Arrow results are unsupported",
                )));
            }
            let length = batch.length();
            if length < 0 || usize::try_from(length).map_or(true, |rows| rows > FOUNDRY_MAX_ROW_LIMIT) {
                return Err(arrow_schema::ArrowError::IpcError(String::from(
                    "Arrow IPC record batch exceeds the Foundry row bound",
                )));
            }
            let nodes = batch
                .nodes()
                .ok_or_else(|| arrow_schema::ArrowError::IpcError(String::from("Arrow IPC record batch has no field nodes")))?;
            if nodes.len() > MAX_SCHEMA_FIELDS {
                return Err(arrow_schema::ArrowError::IpcError(String::from(
                    "Arrow IPC record batch has too many field nodes",
                )));
            }
            for node in nodes {
                let length = node.length();
                let null_count = node.null_count();
                if length < 0
                    || null_count < 0
                    || null_count > length
                    || usize::try_from(length).map_or(true, |rows| rows > FOUNDRY_MAX_ROW_LIMIT)
                {
                    return Err(arrow_schema::ArrowError::IpcError(String::from(
                        "Arrow IPC field node exceeds the Foundry row bound",
                    )));
                }
            }
        } else if message.header_type() == arrow_ipc::MessageHeader::DictionaryBatch {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "dictionary Arrow results are unsupported",
            )));
        } else if !matches!(
            message.header_type(),
            arrow_ipc::MessageHeader::Schema | arrow_ipc::MessageHeader::NONE
        ) {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "this Arrow IPC message kind is unsupported",
            )));
        }
        let body_len = message.bodyLength();
        if body_len < 0 {
            return Err(arrow_schema::ArrowError::ParseError(String::from(
                "negative Arrow IPC body length",
            )));
        }
        let body_len = usize::try_from(body_len).map_err(|error| {
            arrow_schema::ArrowError::ParseError(format!("Arrow IPC body length does not fit in usize: {error}"))
        })?;
        let body_end = metadata_end
            .checked_add(body_len)
            .ok_or_else(|| arrow_schema::ArrowError::IpcError(String::from("Arrow IPC body length overflows the response")))?;
        if body_end > bytes.len() {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "Arrow IPC body exceeds the response",
            )));
        }
        offset = body_end;
    }
}

fn validate_buffers(
    batch: &arrow_ipc::RecordBatch<'_>,
    body_bytes: i64,
    budget: ResultBudget,
    charged: &mut usize,
) -> Result<(), arrow_schema::ArrowError> {
    let buffers = batch
        .buffers()
        .ok_or_else(|| arrow_schema::ArrowError::IpcError(String::from("Arrow IPC record batch has no buffers")))?;
    for buffer in buffers {
        let offset = buffer.offset();
        let length = buffer.length();
        if offset < 0 || length < 0 || offset.checked_add(length).is_none_or(|end| end > body_bytes) {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "Arrow IPC buffer exceeds its body",
            )));
        }
        let length = usize::try_from(length).map_err(|error| {
            arrow_schema::ArrowError::ParseError(format!("Arrow IPC buffer length does not fit in usize: {error}"))
        })?;
        // Count repeated references too: the decoder can copy each unaligned buffer separately.
        *charged = charged.saturating_add(length);
        if *charged > budget.bytes() {
            return Err(arrow_schema::ArrowError::MemoryError(String::from(
                "Foundry IPC buffers exceed the result budget",
            )));
        }
    }
    Ok(())
}

fn validate_fb_schema(schema: &arrow_ipc::Schema) -> Result<(), arrow_schema::ArrowError> {
    let mut fields = 0_usize;
    let mut text_bytes = 0_usize;
    validate_fb_metadata(schema.custom_metadata(), &mut text_bytes)?;
    if let Some(schema_fields) = schema.fields() {
        for field in schema_fields {
            validate_fb_field(&field, &mut fields, &mut text_bytes)?;
        }
    }
    Ok(())
}

fn validate_fb_field(
    field: &arrow_ipc::Field<'_>,
    fields: &mut usize,
    text_bytes: &mut usize,
) -> Result<(), arrow_schema::ArrowError> {
    if field.dictionary().is_some()
        || field.children().is_some_and(|children| !children.is_empty())
        || !matches!(
            field.type_type(),
            arrow_ipc::Type::Int
                | arrow_ipc::Type::FloatingPoint
                | arrow_ipc::Type::Utf8
                | arrow_ipc::Type::LargeUtf8
                | arrow_ipc::Type::Utf8View
                | arrow_ipc::Type::Bool
                | arrow_ipc::Type::Date
                | arrow_ipc::Type::Decimal
        )
    {
        return Err(arrow_schema::ArrowError::IpcError(String::from(
            "unsupported Foundry IPC field type",
        )));
    }
    *fields = fields.saturating_add(1);
    if *fields > MAX_SCHEMA_FIELDS {
        return Err(arrow_schema::ArrowError::IpcError(String::from(
            "Foundry Arrow schema has too many fields",
        )));
    }
    validate_fb_text(field.name(), text_bytes)?;
    validate_fb_metadata(field.custom_metadata(), text_bytes)?;
    Ok(())
}

fn validate_fb_metadata<'a, I>(metadata: Option<I>, text_bytes: &mut usize) -> Result<(), arrow_schema::ArrowError>
where
    I: IntoIterator<Item = arrow_ipc::KeyValue<'a>>,
{
    if let Some(metadata) = metadata {
        for entry in metadata {
            validate_fb_text(entry.key(), text_bytes)?;
            validate_fb_text(entry.value(), text_bytes)?;
        }
    }
    Ok(())
}

fn validate_fb_text(text: Option<&str>, text_bytes: &mut usize) -> Result<(), arrow_schema::ArrowError> {
    *text_bytes = text_bytes.saturating_add(text.map_or(0, str::len));
    if *text_bytes > MAX_SCHEMA_TEXT_BYTES {
        return Err(arrow_schema::ArrowError::IpcError(String::from(
            "Foundry Arrow schema text exceeds its bound",
        )));
    }
    Ok(())
}

fn validate_schema(schema: &arrow_schema::Schema) -> Result<(), arrow_schema::ArrowError> {
    if schema.fields().len() > MAX_SCHEMA_FIELDS || !schema.metadata().is_empty() {
        return Err(arrow_schema::ArrowError::IpcError(String::from(
            "Foundry Arrow schema exceeds the bounded primitive shape",
        )));
    }
    let mut text_bytes = 0_usize;
    for field in schema.fields() {
        text_bytes = text_bytes.saturating_add(field.name().len());
        if text_bytes > MAX_SCHEMA_TEXT_BYTES || !field.metadata().is_empty() {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "Foundry Arrow schema text exceeds its bound",
            )));
        }
        let mapped = matches!(
            field.data_type(),
            arrow_schema::DataType::Int8
                | arrow_schema::DataType::Int16
                | arrow_schema::DataType::Int32
                | arrow_schema::DataType::Int64
                | arrow_schema::DataType::UInt8
                | arrow_schema::DataType::UInt16
                | arrow_schema::DataType::UInt32
                | arrow_schema::DataType::UInt64
                | arrow_schema::DataType::Float64
                | arrow_schema::DataType::Utf8
                | arrow_schema::DataType::LargeUtf8
                | arrow_schema::DataType::Utf8View
                | arrow_schema::DataType::Date32
                | arrow_schema::DataType::Boolean
                | arrow_schema::DataType::Decimal128(..)
                | arrow_schema::DataType::Decimal256(..)
        );
        if !mapped {
            return Err(arrow_schema::ArrowError::IpcError(String::from(
                "Foundry Arrow results must use primitive domain scalar columns",
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod execution_tests;

#[cfg(test)]
mod each_caller_tests;

#[cfg(test)]
mod directory_tests;
