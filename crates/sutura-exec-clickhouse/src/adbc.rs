//! The `ClickHouse` ADBC transport.
//!
//! The driver is the vendored, linked-or-mounted archive prepared by #1571. This module keeps the
//! request identity and channel options on one per-call database, then hands the driver's Arrow
//! stream straight to `Accumulating`. The older HTTP transport remains available only to the
//! fixture and conformance targets while those loaders are migrated separately.

use std::sync::Arc;
use std::time::Instant;

use adbc_core::error::{Error as CoreError, Status};
use adbc_core::options::{AdbcVersion, OptionDatabase, OptionStatement};
use adbc_core::{Connection as _, Database as _, Driver as _, Optionable as _, Statement as _};
use adbc_driver_manager::ManagedDriver;
use arrow_array::{Array as _, RecordBatch, RecordBatchReader as _, StringArray, StructArray};
use arrow_schema::{ArrowError, Field, Schema};

use sutura_adbc::DriverLocation;
use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture, Secret};
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::cardinality::{CountsNotRead, DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{Accumulating, AnchorRows, ResultBatches, ResultBudget, UnreadableCell, Warehouse};
use sutura_sql::generate::{generate, generate_key_probe, generate_leg};
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

use crate::deadline;
use crate::execute_as::{ClickHouseUser, NotAClickHouseUser};

const QUERY_TIMEOUT: &str = "clickhouse.client.query_timeout";
const TLS_ROOT_CERTS: &str = "clickhouse.client.tls_root_certs";
const MTLS_CERT_CHAIN: &str = "clickhouse.client.mtls_cert_chain";
const MTLS_PRIVATE_KEY: &str = "clickhouse.client.mtls_private_key";
const JOIN_USE_NULLS: &str = "clickhouse.setting.join_use_nulls";
const JSON_QUOTE_DENORMALS: &str = "clickhouse.setting.output_format_json_quote_denormals";
const JSON_QUOTE_64BIT_INTEGERS: &str = "clickhouse.setting.output_format_json_quote_64bit_integers";
const JSON_QUOTE_DECIMALS: &str = "clickhouse.setting.output_format_json_quote_decimals";
const DECIMAL_TRAILING_ZEROS: &str = "clickhouse.setting.output_format_decimal_trailing_zeros";
const TIMEOUT_OVERFLOW_MODE: &str = "clickhouse.setting.timeout_overflow_mode";
const MAX_EXECUTION_TIME: &str = "clickhouse.setting.max_execution_time";
const MOST_RESULT_ROWS: usize = 1_000_000;

/// TLS material already validated by the composition root, in the PEM shape the driver accepts.
#[derive(Clone)]
pub struct TlsOptions {
    root_certs: Option<String>,
    mtls_cert_chain: Option<String>,
    mtls_private_key: Option<String>,
}

impl core::fmt::Debug for TlsOptions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TlsOptions")
            .field("root_certs", &self.root_certs.as_ref().map(|_| "<pem>"))
            .field("mtls_cert_chain", &self.mtls_cert_chain.as_ref().map(|_| "<pem>"))
            .field("mtls_private_key", &self.mtls_private_key.as_ref().map(|_| "<pem>"))
            .finish()
    }
}

impl TlsOptions {
    /// PEM roots, and optionally the client certificate and key, for one declared channel.
    #[must_use]
    pub const fn new(root_certs: Option<String>, mtls_cert_chain: Option<String>, mtls_private_key: Option<String>) -> Self {
        Self {
            root_certs,
            mtls_cert_chain,
            mtls_private_key,
        }
    }
}

/// Why the `ClickHouse` driver location could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum DriverLocationError {
    /// The mounted path did not meet the absolute-path requirement.
    #[error(transparent)]
    InvalidPath(#[from] sutura_adbc::UnusableDriverPath),
    /// This build carries no linked driver and no mount was declared.
    #[error("no `ClickHouse` ADBC driver is mounted or linked into this build")]
    NoDriver,
    /// The environment variable could not be interpreted as UTF-8.
    #[error("SUTURA_CLICKHOUSE_ADBC_DRIVER is not valid UTF-8")]
    NotUnicode,
}

/// Resolves the mounted driver used by source builds, or the archive carried by a release binary.
pub fn driver_location() -> Result<DriverLocation, DriverLocationError> {
    match std::env::var("SUTURA_CLICKHOUSE_ADBC_DRIVER") {
        Ok(path) => DriverLocation::parse(&path).map_err(DriverLocationError::InvalidPath),
        Err(std::env::VarError::NotPresent) => DriverLocation::clickhouse_linked_in().ok_or(DriverLocationError::NoDriver),
        Err(std::env::VarError::NotUnicode(_)) => Err(DriverLocationError::NotUnicode),
    }
}

#[derive(Debug)]
pub struct DriverMessage {
    text: String,
    status: Status,
}

impl DriverMessage {
    fn of(error: &CoreError) -> Self {
        Self {
            text: error.to_string(),
            status: error.status,
        }
    }

    const fn refused_identity(&self) -> bool {
        matches!(self.status, Status::Unauthorized)
    }

    const fn timed_out(&self) -> bool {
        matches!(self.status, Status::Timeout)
    }
}

impl core::fmt::Display for DriverMessage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.text)
    }
}

/// Why the ADBC `ClickHouse` path could not answer.
#[derive(Debug, thiserror::Error)]
pub enum AdbcError {
    #[error("could not load the ClickHouse ADBC driver: {0}")]
    Load(DriverMessage),
    #[error("ClickHouse ADBC call failed: {0}")]
    Driver(DriverMessage),
    #[error("could not read a ClickHouse ADBC result batch: {0}")]
    Batch(#[source] ArrowError),
    #[error("the ClickHouse ADBC result stream was refused: {0}")]
    Unannounced(#[source] sutura_domain::warehouse::UnannouncedBatch),
    #[error("the query parameters could not be assembled: {0}")]
    Parameters(#[source] ArrowError),
    #[error("the ClickHouse ADBC result could not be read as domain values: {0}")]
    Decode(#[source] UnreadableCell),
    #[error("the request deadline was already spent before the statement was sent")]
    DeadlineSpent,
    #[error("the plan could not be rendered for ClickHouse")]
    Render(#[source] GenerateError),
    #[error("the source was handed a subject credential with no ClickHouse user")]
    NoPlaceForASubject,
    #[error("the principal is not a ClickHouse user this adapter can name")]
    NotAClickHouseUser(#[source] NotAClickHouseUser),
    #[error("a federated leg would run through EXECUTE AS rather than the caller's own credential")]
    LegUnderExecuteAs,
    #[error("the presented credential disagrees with the source posture")]
    PresentedDisagreesWithPosture(#[source] PresentedDisagreesWithPosture),
    #[error("the key probe did not return two counts")]
    KeyCounts(#[source] CountsNotRead),
}

/// A `ClickHouse` warehouse over its ADBC driver.
pub struct AdbcClickHouseWarehouse {
    source: SourceName,
    posture: SourcePosture,
    driver: DriverLocation,
    endpoint: String,
    username: String,
    password: Secret,
    tls: Option<TlsOptions>,
    result_budget: ResultBudget,
}

impl core::fmt::Debug for AdbcClickHouseWarehouse {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AdbcClickHouseWarehouse")
            .field("source", &self.source)
            .field("endpoint", &self.endpoint)
            .field("driver", &self.driver)
            .finish_non_exhaustive()
    }
}

impl AdbcClickHouseWarehouse {
    /// Opens one source over a driver location and its declared channel.
    #[must_use]
    pub const fn new(
        source: SourceName,
        posture: SourcePosture,
        driver: DriverLocation,
        endpoint: String,
        username: String,
        password: Secret,
        tls: Option<TlsOptions>,
        result_budget: ResultBudget,
    ) -> Self {
        Self {
            source,
            posture,
            driver,
            endpoint,
            username,
            password,
            tls,
            result_budget,
        }
    }

    fn deliverable<'p>(&self, presented: &'p Presented) -> Result<RunsAs<'p>, AdbcError> {
        let (named, token) = match *presented {
            Presented::SharedServiceUser { .. } => (None, None),
            Presented::SubjectPrincipal { ref name } => (Some(name), None),
            Presented::SubjectSignIn { ref name, ref material } => (Some(name), Some(material)),
            Presented::SubjectToken { .. } => return Err(AdbcError::NoPlaceForASubject),
        };
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(AdbcError::PresentedDisagreesWithPosture)?;
        let Some(name) = named else {
            return Ok(RunsAs::Deployment);
        };
        let user = ClickHouseUser::parse(name.as_str()).map_err(AdbcError::NotAClickHouseUser)?;
        Ok(match token {
            None => RunsAs::Switched(user),
            Some(token) => RunsAs::SignedIn { user, token },
        })
    }

    fn render(executable: Executable<'_>) -> Result<GeneratedQuery, AdbcError> {
        match executable {
            Executable::Query(plan) => generate(plan, Dialect::ClickHouse),
            Executable::Leg(leg) => generate_leg(leg, Dialect::ClickHouse),
        }
        .map_err(AdbcError::Render)
    }

    fn run(
        &self,
        query: &GeneratedQuery,
        deadline: Deadline,
        most_rows: usize,
        runs_as: RunsAs<'_>,
    ) -> Result<ResultBatches, AdbcError> {
        if deadline::refuse_if_spent(deadline).is_err() {
            return Err(AdbcError::DeadlineSpent);
        }
        let mut driver = load_driver(&self.driver)?;
        let (username, password) = match runs_as {
            RunsAs::Deployment => (self.username.clone(), &self.password),
            RunsAs::Switched(user) => (user.to_string(), &self.password),
            RunsAs::SignedIn { user, token } => (user.to_string(), token),
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "the deployment password's destination is the ADBC driver's connection handshake, which is the one place the value itself is the payload"
        )]
        let mut options = vec![
            (OptionDatabase::Uri, self.endpoint.clone().into()),
            (OptionDatabase::Username, username.into()),
            (OptionDatabase::Password, password.expose_secret().to_owned().into()),
            (OptionDatabase::Other(JOIN_USE_NULLS.to_owned()), "1".into()),
            (OptionDatabase::Other(JSON_QUOTE_DENORMALS.to_owned()), "1".into()),
            (OptionDatabase::Other(JSON_QUOTE_64BIT_INTEGERS.to_owned()), "1".into()),
            (OptionDatabase::Other(JSON_QUOTE_DECIMALS.to_owned()), "1".into()),
            (OptionDatabase::Other(DECIMAL_TRAILING_ZEROS.to_owned()), "1".into()),
            (OptionDatabase::Other(TIMEOUT_OVERFLOW_MODE.to_owned()), "throw".into()),
        ];
        if let Some(tls) = &self.tls {
            if let Some(value) = &tls.root_certs {
                options.push((OptionDatabase::Other(TLS_ROOT_CERTS.to_owned()), value.clone().into()));
            }
            if let Some(value) = &tls.mtls_cert_chain {
                options.push((OptionDatabase::Other(MTLS_CERT_CHAIN.to_owned()), value.clone().into()));
            }
            if let Some(value) = &tls.mtls_private_key {
                options.push((OptionDatabase::Other(MTLS_PRIVATE_KEY.to_owned()), value.clone().into()));
            }
        }
        let database = driver
            .new_database_with_opts(options)
            .map_err(|error| AdbcError::Driver(DriverMessage::of(&error)))?;
        let mut connection = database
            .new_connection()
            .map_err(|error| AdbcError::Driver(DriverMessage::of(&error)))?;
        let mut statement = connection
            .new_statement()
            .map_err(|error| AdbcError::Driver(DriverMessage::of(&error)))?;
        let seconds = deadline::max_execution_time_seconds(deadline, Instant::now()).ok_or(AdbcError::DeadlineSpent)?;
        statement
            .set_option(OptionStatement::Other(QUERY_TIMEOUT.to_owned()), seconds.to_string().into())
            .map_err(|error| AdbcError::Driver(DriverMessage::of(&error)))?;
        statement
            .set_option(
                OptionStatement::Other(MAX_EXECUTION_TIME.to_owned()),
                seconds.to_string().into(),
            )
            .map_err(|error| AdbcError::Driver(DriverMessage::of(&error)))?;
        let sql = rewrite_placeholders(query.sql(), query.params())?;
        statement
            .set_sql_query(sql)
            .map_err(|error| AdbcError::Driver(DriverMessage::of(&error)))?;
        if let Some(batch) = sutura_adbc::parameter_batch(query.params()).map_err(AdbcError::Parameters)? {
            statement
                .bind(batch)
                .map_err(|error| AdbcError::Driver(DriverMessage::of(&error)))?;
        }
        execute_to_completion(&mut statement, most_rows, self.result_budget)
    }
}

enum RunsAs<'a> {
    Deployment,
    Switched(ClickHouseUser),
    SignedIn { user: ClickHouseUser, token: &'a Secret },
}

fn load_driver(location: &DriverLocation) -> Result<ManagedDriver, AdbcError> {
    if let Some(path) = location.mounted() {
        return ManagedDriver::load_dynamic_from_filename(path, Some(b"AdbcClickhouseInit"), AdbcVersion::default())
            .map_err(|error| AdbcError::Load(DriverMessage::of(&error)));
    }
    sutura_adbc::linked_clickhouse_driver().map_err(|error| AdbcError::Load(DriverMessage::of(&error)))
}

fn rewrite_placeholders(statement: &str, params: &[sutura_domain::warehouse::ParamValue]) -> Result<String, AdbcError> {
    let found = statement.matches('?').count();
    if found != params.len() {
        return Err(AdbcError::Driver(DriverMessage {
            text: format!(
                "the rendered statement carried {found} placeholders, and {} parameters were bound",
                params.len()
            ),
            status: Status::InvalidArguments,
        }));
    }
    let mut out = String::with_capacity(statement.len() + params.len() * 12);
    let mut parts = statement.split('?').collect::<Vec<_>>().into_iter();
    let Some(tail) = parts.next_back() else {
        return Ok(out);
    };
    for (index, (part, param)) in parts.zip(params.iter()).enumerate() {
        out.push_str(part);
        out.push_str("{p");
        out.push_str(&index.to_string());
        out.push(':');
        out.push_str(match param {
            sutura_domain::warehouse::ParamValue::Text(_) => "String",
            sutura_domain::warehouse::ParamValue::Date(_) => "Date",
        });
        out.push('}');
    }
    out.push_str(tail);
    Ok(out)
}

fn execute_to_completion<S>(statement: &mut S, most_rows: usize, budget: ResultBudget) -> Result<ResultBatches, AdbcError>
where
    S: adbc_core::Statement,
{
    let reader = statement
        .execute()
        .map_err(|error| AdbcError::Driver(DriverMessage::of(&error)))?;
    let schema = reader.schema();
    let mut accumulating = None;
    for batch in reader {
        let batch = batch.map_err(AdbcError::Batch)?;
        let batch = normalize_batch(&batch)?;
        if accumulating.is_none() {
            accumulating = Some(Accumulating::announcing(batch.schema(), most_rows, budget));
        }
        if let Some(accumulating) = accumulating.as_mut() {
            accumulating.push(batch).map_err(AdbcError::Unannounced)?;
        }
    }
    Ok(accumulating.map_or_else(
        || Accumulating::announcing(schema, most_rows, budget).finish(),
        Accumulating::finish,
    ))
}

/// The `ClickHouse` sum expression is a tuple carrying a type name and three nullable slots. The
/// ADBC driver reports that tuple faithfully as an Arrow `Struct`; the domain reader accepts the
/// scalar total, so select the slot named by the type before the batch enters `Accumulating`.
fn normalize_batch(batch: &RecordBatch) -> Result<RecordBatch, AdbcError> {
    let schema = batch.schema();
    let mut fields = Vec::with_capacity(schema.fields().len());
    let mut columns = Vec::with_capacity(batch.num_columns());
    for (field, column) in schema.fields().iter().zip(batch.columns()) {
        let Some(struct_array) = column.as_any().downcast_ref::<StructArray>() else {
            fields.push(Arc::clone(field));
            columns.push(Arc::clone(column));
            continue;
        };
        if struct_array.num_columns() != 4 || batch.num_rows() == 0 {
            fields.push(Arc::clone(field));
            columns.push(Arc::clone(column));
            continue;
        }
        let Some(type_names) = struct_array.column(0).as_any().downcast_ref::<StringArray>() else {
            return Err(unsupported_struct(field));
        };
        if type_names.is_null(0) {
            return Err(unsupported_struct(field));
        }
        let Some(sum_type) = crate::sum::SumType::parse(type_names.value(0)) else {
            return Err(unsupported_struct(field));
        };
        for row in 1..type_names.len() {
            if !type_names.is_null(row) && type_names.value(row) != type_names.value(0) {
                return Err(unsupported_struct(field));
            }
        }
        let selected = Arc::clone(struct_array.column(sum_type.slot()));
        fields.push(Arc::new(field.as_ref().clone().with_data_type(selected.data_type().clone())));
        columns.push(selected);
    }
    let schema = Arc::new(Schema::new(fields));
    RecordBatch::try_new(schema, columns).map_err(AdbcError::Batch)
}

fn unsupported_struct(field: &Field) -> AdbcError {
    AdbcError::Decode(UnreadableCell::UnsupportedType {
        column: field.name().to_owned(),
        arrow_type: format!("{}", field.data_type()),
    })
}

impl Warehouse for AdbcClickHouseWarehouse {
    type Error = AdbcError;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        let runs_as = self.deliverable(presented)?;
        if matches!((&executable, &runs_as), (Executable::Leg(_), RunsAs::Switched(_))) {
            return Err(AdbcError::LegUnderExecuteAs);
        }
        let most_rows = executable.row_limit().unwrap_or(MOST_RESULT_ROWS);
        self.run(&Self::render(executable)?, deadline, most_rows, runs_as)
    }

    fn type_dialect(&self) -> Option<TypeDialect> {
        Some(TypeDialect::ClickHouse)
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let query = generate(plan.plan(), Dialect::ClickHouse).map_err(AdbcError::Render)?;
        let rows = self.run(&query, boot_deadline(), MOST_RESULT_ROWS, RunsAs::Deployment)?;
        rows.to_rows().map(AnchorRows::of).map_err(AdbcError::Decode)
    }

    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        let query = generate_key_probe(&key, Dialect::ClickHouse).map_err(AdbcError::Render)?;
        let rows = self.run(&query, boot_deadline(), MOST_RESULT_ROWS, RunsAs::Deployment)?;
        let rows = rows.to_rows().map_err(AdbcError::Decode)?;
        KeyUniqueness::read(&rows).map_err(AdbcError::KeyCounts)
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, AdbcError::Driver(message) | AdbcError::Load(message) if message.refused_identity())
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(error, AdbcError::DeadlineSpent) || matches!(error, AdbcError::Driver(message) if message.timed_out())
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(error, AdbcError::Unannounced(error) if matches!(error, sutura_domain::warehouse::UnannouncedBatch::OverBound { .. } | sutura_domain::warehouse::UnannouncedBatch::OverBudget { .. }))
    }
}

fn boot_deadline() -> Deadline {
    #[expect(
        clippy::expect_used,
        reason = "3600 seconds is a literal, non-zero constant - Budget::parse can only fail on zero, so this cannot actually panic"
    )]
    let budget = Budget::parse(core::time::Duration::from_secs(3600)).expect("3600 seconds is a non-zero duration");
    Deadline::opened_at(Instant::now(), budget)
}
