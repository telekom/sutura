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

use std::time::Instant;

use arrow_buffer::Buffer;
use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture, Secret};
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::arrow::{Accumulating, ResultBudget, UnannouncedBatch};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, Warehouse};
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

#[cfg(feature = "http")]
pub mod http;

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
}

type TransportResponse<E> = Result<Vec<u8>, E>;
type WarehouseResult<E, T> = Result<T, FoundryError<E>>;

/// Why Foundry did not answer a plan.
#[derive(Debug, thiserror::Error)]
pub enum FoundryError<E>
where
    E: std::error::Error + 'static,
{
    #[error("the Foundry endpoint did not answer")]
    Endpoint {
        #[source]
        cause: E,
    },
    #[error("the plan could not be rendered as Foundry Ontology SQL")]
    Render {
        #[source]
        cause: GenerateError,
    },
    #[error("the source credential does not agree with the declared posture")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
    #[error("the Foundry Arrow result could not be decoded")]
    Arrow {
        #[source]
        cause: arrow_schema::ArrowError,
    },
    #[error("the Foundry answer deadline was spent while decoding Arrow")]
    DeadlineSpent,
    #[error("the Foundry Arrow result did not match its announced schema")]
    Unannounced {
        #[source]
        cause: UnannouncedBatch,
    },
    #[error("a Foundry result cell could not be read as a domain value")]
    Unreadable {
        #[source]
        cause: sutura_domain::warehouse::UnreadableCell,
    },
    #[error("the Foundry source identifier is not the source selected by this plan")]
    WrongSource,
    #[error("the credential broker presented {presented}, which this shared Foundry source cannot send")]
    Undeliverable { presented: &'static str },
    #[error("the Foundry endpoint does not support this plan shape")]
    UnsupportedPlan,
}

/// A connection to one Foundry ontology.
#[derive(Debug)]
pub struct FoundryWarehouse<T> {
    source: SourceName,
    posture: SourcePosture,
    ontology: OntologyIdentifier,
    token: Secret,
    transport: T,
    result_budget: ResultBudget,
}

impl<T> FoundryWarehouse<T> {
    /// Builds a connection over a fixed deployment token and the process's result budget.
    #[must_use]
    pub const fn new(
        source: SourceName,
        posture: SourcePosture,
        ontology: OntologyIdentifier,
        token: Secret,
        transport: T,
        result_budget: ResultBudget,
    ) -> Self {
        Self {
            source,
            posture,
            ontology,
            token,
            transport,
            result_budget,
        }
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
    fn delivered(&self, presented: &Presented) -> WarehouseResult<T::Error, &Secret> {
        if !matches!(presented, Presented::SharedServiceUser { .. }) {
            return Err(FoundryError::Undeliverable {
                presented: presented.as_str(),
            });
        }
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| FoundryError::PresentedDisagreesWithPosture { cause })?;
        Ok(&self.token)
    }

    fn request(&self, executable: Executable<'_>, dry_run: bool) -> WarehouseResult<T::Error, SqlRequest> {
        let row_limit = executable
            .row_limit()
            .map_or(FOUNDRY_MAX_ROW_LIMIT, |rows| rows.min(FOUNDRY_MAX_ROW_LIMIT));
        let generated = match executable {
            Executable::Query(plan) => sutura_sql::generate(plan, Dialect::Foundry),
            Executable::Leg(plan) => sutura_sql::generate_leg(plan, Dialect::Foundry),
            Executable::Stacked(plan) => sutura_sql::generate_stacked(plan, Dialect::Foundry),
        }
        .map_err(|cause| FoundryError::Render { cause })?;
        if generated.source() != &self.source {
            return Err(FoundryError::WrongSource);
        }
        Ok(SqlRequest::from_query(&generated, &self.ontology, row_limit, dry_run))
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

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        let token = self.delivered(presented)?;
        let request = self.request(executable, true)?;
        self.transport
            .execute(&request, token, deadline)
            .map_err(|cause| FoundryError::Endpoint { cause })?;
        Ok(PreFlight::Accepted { estimated_bytes: None })
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        let token = self.delivered(presented)?;
        let request = self.request(executable, false)?;
        let bytes = self
            .transport
            .execute(&request, token, deadline)
            .map_err(|cause| FoundryError::Endpoint { cause })?;
        self.decode(bytes, accepted_rows(executable), deadline)
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let request = self.request(Executable::Query(plan.plan()), false)?;
        let deadline = boot_deadline();
        let bytes = self
            .transport
            .execute(&request, &self.token, deadline)
            .map_err(|cause| FoundryError::Endpoint { cause })?;
        self.decode(bytes, accepted_rows(Executable::Query(plan.plan())), deadline)?
            .to_rows()
            .map(AnchorRows::of)
            .map_err(|cause| FoundryError::Unreadable { cause })
    }

    fn type_dialect(&self) -> Option<TypeDialect> {
        Some(TypeDialect::Foundry)
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, FoundryError::Endpoint { cause } if self.transport.source_refused(cause))
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
            | FoundryError::UnsupportedPlan => false,
        }
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(error, FoundryError::DeadlineSpent)
            || matches!(error, FoundryError::Endpoint { cause } if self.transport.deadline_exceeded(cause))
    }
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
