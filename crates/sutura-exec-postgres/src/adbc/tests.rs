//! Every step `answer` owns, asserted over a fake connection that records what reached it.
//! The last cell drives `AdbcPostgres::execute` itself over a path naming no driver, which is
//! the negative control: each refusal above it has to arrive INSTEAD of that load failure.

use core::cell::RefCell;
use core::ffi::c_char;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adbc_core::error::{Error as CoreError, Result as AdbcResult, Status};
use adbc_core::options::{InfoCode, ObjectDepth, OptionConnection, OptionStatement, OptionValue};
use adbc_core::{Connection, Optionable, PartitionedResult, Statement};
use arrow_array::{Array as _, ArrayRef, Int8Array, RecordBatch, RecordBatchIterator, RecordBatchReader, StringArray};
use arrow_schema::{Field, Schema};
use sutura_domain::calendar::Date;
use sutura_domain::identity::Secret;
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{ParamValue, ResultBatches, UnannouncedBatch, Value};

use super::session::answer;
use super::{AdbcError, AdbcPostgres, Channel, Conninfo, PostgresDriver, Route};

/// What reached the fake driver, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Sent {
    AutoCommit(String),
    Update(String),
    /// `statement` is the ordinal the connection handed it out under, so a cell can say WHICH
    /// statement a batch was bound to and which one ran.
    Bound {
        statement: usize,
        rows: usize,
        columns: usize,
    },
    Query {
        statement: usize,
        sql: String,
    },
    Rollback,
    Described {
        statement: usize,
        sql: String,
    },
}

type Log = Rc<RefCell<Vec<Sent>>>;

/// What `answer` returned, beside what reached the fake on the way.
type Ran = (Result<ResultBatches, AdbcError>, Vec<Sent>);

/// The connect-time ceiling every cell hands over, `SUTURA_DEV_STATEMENT_TIMEOUT_MS`'s default.
pub(super) const CEILING_MS: u32 = 15_000;

fn not_asked(what: &str) -> CoreError {
    CoreError::with_message_and_status(format!("the fake driver was not expected to {what}"), Status::NotImplemented)
}

pub(super) struct FakeConnection {
    log: Log,
    /// What `execute` answers with: a batch, or `None` for a statement the server refused.
    reply: Option<RecordBatch>,
    /// How many times the stream repeats that batch.
    times: usize,
    /// How many statements this connection has handed out.
    made: usize,
}

impl FakeConnection {
    /// A fresh connection answering `batch`, handing statements out from zero.
    pub(super) fn replying(batch: RecordBatch) -> Self {
        Self::answering(Some(batch))
    }

    /// A fresh connection answering `reply`, or refusing every statement for `None`.
    pub(super) fn answering(reply: Option<RecordBatch>) -> Self {
        Self::streaming(reply, 1)
    }

    /// A fresh connection whose stream answers `reply` `times` times over.
    pub(super) fn streaming(reply: Option<RecordBatch>, times: usize) -> Self {
        Self {
            log: Rc::default(),
            reply,
            times,
            made: 0,
        }
    }

    /// What reached this connection, in order.
    pub(super) fn sent(&self) -> Vec<Sent> {
        self.log.borrow().clone()
    }
}

pub(super) struct FakeStatement {
    log: Log,
    reply: Option<RecordBatch>,
    times: usize,
    sql: String,
    ordinal: usize,
}

impl Optionable for FakeConnection {
    type Option = OptionConnection;

    fn set_option(&mut self, key: Self::Option, value: OptionValue) -> AdbcResult<()> {
        if matches!(key, OptionConnection::AutoCommit) {
            let text = if let OptionValue::String(ref text) = value {
                text.clone()
            } else {
                String::from("<not a string>")
            };
            self.log.borrow_mut().push(Sent::AutoCommit(text));
        }
        Ok(())
    }

    fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
        Err(not_asked("read an option"))
    }

    fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
        Err(not_asked("read an option"))
    }

    fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
        Err(not_asked("read an option"))
    }

    fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
        Err(not_asked("read an option"))
    }
}

impl Connection for FakeConnection {
    type StatementType = FakeStatement;

    fn new_statement(&mut self) -> AdbcResult<Self::StatementType> {
        let ordinal = self.made;
        self.made += 1;
        Ok(FakeStatement {
            log: Rc::clone(&self.log),
            reply: self.reply.clone(),
            times: self.times,
            sql: String::new(),
            ordinal,
        })
    }

    fn cancel(&mut self) -> AdbcResult<()> {
        Err(not_asked("cancel"))
    }

    fn get_info(&self, _codes: Option<HashSet<InfoCode>>) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("describe itself"))
    }

    fn get_objects(
        &self,
        _depth: ObjectDepth,
        _catalog: Option<&str>,
        _db_schema: Option<&str>,
        _table_name: Option<&str>,
        _table_type: Option<Vec<&str>>,
        _column_name: Option<&str>,
    ) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("list objects"))
    }

    fn get_table_schema(
        &self,
        _catalog: Option<&str>,
        _db_schema: Option<&str>,
        _table_name: &str,
    ) -> AdbcResult<arrow_schema::Schema> {
        Err(not_asked("describe a table"))
    }

    fn get_table_types(&self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("list table types"))
    }

    fn get_statistic_names(&self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("list statistics"))
    }

    fn get_statistics(
        &self,
        _catalog: Option<&str>,
        _db_schema: Option<&str>,
        _table_name: Option<&str>,
        _approximate: bool,
    ) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("read statistics"))
    }

    fn commit(&mut self) -> AdbcResult<()> {
        Err(not_asked("commit - nothing on this path commits"))
    }

    fn rollback(&mut self) -> AdbcResult<()> {
        self.log.borrow_mut().push(Sent::Rollback);
        Ok(())
    }

    fn read_partition(&self, _partition: impl AsRef<[u8]>) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        Err(not_asked("read a partition"))
    }
}

impl Optionable for FakeStatement {
    type Option = OptionStatement;

    fn set_option(&mut self, _key: Self::Option, _value: OptionValue) -> AdbcResult<()> {
        Err(not_asked("set a statement option"))
    }

    fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
        Err(not_asked("read an option"))
    }

    fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
        Err(not_asked("read an option"))
    }

    fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
        Err(not_asked("read an option"))
    }

    fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
        Err(not_asked("read an option"))
    }
}

impl Statement for FakeStatement {
    fn bind(&mut self, batch: RecordBatch) -> AdbcResult<()> {
        self.log.borrow_mut().push(Sent::Bound {
            statement: self.ordinal,
            rows: batch.num_rows(),
            columns: batch.num_columns(),
        });
        Ok(())
    }

    fn bind_stream(&mut self, _reader: Box<dyn RecordBatchReader + Send>) -> AdbcResult<()> {
        Err(not_asked("bind a stream"))
    }

    fn execute(&mut self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
        self.log.borrow_mut().push(Sent::Query {
            statement: self.ordinal,
            sql: self.sql.clone(),
        });
        let batch = self
            .reply
            .clone()
            .ok_or_else(|| CoreError::with_message_and_status("the server refused the statement", Status::InvalidArguments))?;
        let schema = batch.schema();
        let stream: Vec<_> = core::iter::repeat_with(|| Ok(batch.clone())).take(self.times).collect();
        Ok(Box::new(RecordBatchIterator::new(stream, schema)))
    }

    fn execute_update(&mut self) -> AdbcResult<Option<i64>> {
        self.log.borrow_mut().push(Sent::Update(self.sql.clone()));
        Ok(None)
    }

    fn execute_schema(&mut self) -> AdbcResult<arrow_schema::Schema> {
        self.log.borrow_mut().push(Sent::Described {
            statement: self.ordinal,
            sql: self.sql.clone(),
        });
        Ok(arrow_schema::Schema::empty())
    }

    fn execute_partitions(&mut self) -> AdbcResult<PartitionedResult> {
        Err(not_asked("partition a result"))
    }

    fn get_parameter_schema(&self) -> AdbcResult<arrow_schema::Schema> {
        Err(not_asked("describe parameters"))
    }

    fn prepare(&mut self) -> AdbcResult<()> {
        Err(not_asked("prepare"))
    }

    fn set_sql_query(&mut self, query: impl AsRef<str>) -> AdbcResult<()> {
        query.as_ref().clone_into(&mut self.sql);
        Ok(())
    }

    fn set_substrait_plan(&mut self, _plan: impl AsRef<[u8]>) -> AdbcResult<()> {
        Err(not_asked("take a substrait plan"))
    }

    fn cancel(&mut self) -> AdbcResult<()> {
        Err(not_asked("cancel"))
    }
}

pub(super) fn one_column(name: &str, column: ArrayRef) -> RecordBatch {
    let schema = Schema::new(vec![Field::new(name, column.data_type().clone(), true)]);
    RecordBatch::try_new(Arc::new(schema), vec![column]).expect("one column is a batch")
}

pub(super) fn open_for(budget: Duration) -> Deadline {
    Deadline::opened_at(Instant::now(), Budget::parse(budget).expect("a positive budget parses"))
}

/// `answer` over a fake that replies with `reply`, and what reached it.
fn run(reply: Option<RecordBatch>, bound: Option<RecordBatch>, deadline: Deadline) -> Ran {
    let log: Log = Rc::default();
    let mut connection = FakeConnection {
        log: Rc::clone(&log),
        reply,
        times: 1,
        made: 0,
    };
    let outcome = answer(&mut connection, "SELECT 1", bound, CEILING_MS, deadline);
    let sent = log.borrow().clone();
    (outcome, sent)
}

pub(super) fn a_number() -> RecordBatch {
    one_column("n", Arc::new(Int8Array::from(vec![1_i8])))
}

#[test]
fn the_deadline_is_set_local_inside_the_drivers_own_transaction_and_always_rolled_back() {
    // The whole mechanism in one sequence. Autocommit OFF is what opens the transaction -
    // without it `SET LOCAL` is a warning and a no-op, and the deadline bounds nothing. The
    // setting precedes the statement it bounds, and the rollback follows: nothing commits.
    // A minute's budget against a 15 s ceiling, so the value is the ceiling exactly.
    let (outcome, sent) = run(Some(a_number()), None, open_for(Duration::from_secs(60)));
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(
        sent,
        vec![
            Sent::AutoCommit(String::from("false")),
            Sent::Update(String::from("SET LOCAL statement_timeout = 15000")),
            Sent::Query {
                statement: 1,
                sql: String::from("SELECT 1")
            },
            Sent::Rollback,
        ]
    );
}

#[test]
fn a_budget_under_the_ceiling_is_what_the_server_is_told() {
    // The other side of the clamp: the request's own budget, not the ceiling, when it is the
    // smaller. Between 4 s and 5 s, because the budget is read on this cell's own clock.
    let (_, sent) = run(Some(a_number()), None, open_for(Duration::from_secs(5)));
    let told = sent
        .iter()
        .find_map(|step| match *step {
            Sent::Update(ref sql) => sql.strip_prefix("SET LOCAL statement_timeout = ").map(str::to_owned),
            _ => None,
        })
        .expect("the timeout was sent");
    let millis: u32 = told.parse().expect("the timeout is a millisecond count");
    assert!((4_000..=5_000).contains(&millis), "{millis}");
}

#[test]
fn a_spent_deadline_sends_nothing_and_reads_as_the_deadline() {
    let opened = Instant::now()
        .checked_sub(Duration::from_secs(60))
        .expect("this host's clock is more than a minute past its epoch");
    let gone = Deadline::opened_at(
        opened,
        Budget::parse(Duration::from_secs(1)).expect("a positive budget parses"),
    );
    let (outcome, sent) = run(Some(a_number()), None, gone);
    let refused = outcome.expect_err("a spent deadline is refused");
    assert!(matches!(refused, AdbcError::DeadlineSpent), "{refused:?}");
    assert!(super::session::deadline_exceeded(&refused));
    assert_eq!(
        sent,
        Vec::<Sent>::new(),
        "nothing may reach the driver once the budget is gone"
    );
}

#[test]
fn a_statement_the_server_refused_is_still_rolled_back_and_is_not_the_deadline() {
    let (outcome, sent) = run(None, None, open_for(Duration::from_secs(60)));
    let refused = outcome.expect_err("the fake refuses the statement");
    assert!(matches!(refused, AdbcError::Adbc(_)), "{refused:?}");
    assert!(
        !super::session::deadline_exceeded(&refused),
        "a refusal with no SQLSTATE is not the deadline"
    );
    assert_eq!(sent.last(), Some(&Sent::Rollback), "{sent:?}");
}

#[test]
fn the_plans_values_are_bound_as_one_row_on_the_statement_that_runs_them() {
    // On the statement that then runs - the second one handed out, after the setting's - and
    // on no other: a batch bound to a throwaway statement leaves the query unbound.
    let day = Date::parse("2026-06-01").expect("a test date is a date");
    let bound = sutura_adbc::parameter_batch(&[ParamValue::Text(String::from("north")), ParamValue::Date(day)])
        .expect("the values are bindable");
    let (outcome, sent) = run(Some(a_number()), bound, open_for(Duration::from_secs(60)));
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(
        sent,
        vec![
            Sent::AutoCommit(String::from("false")),
            Sent::Update(String::from("SET LOCAL statement_timeout = 15000")),
            Sent::Bound {
                statement: 1,
                rows: 1,
                columns: 2
            },
            Sent::Query {
                statement: 1,
                sql: String::from("SELECT 1")
            },
            Sent::Rollback,
        ]
    );
}

pub(super) fn with_sqlstate(code: [u8; 5]) -> AdbcError {
    let mut cause = CoreError::with_message_and_status("the statement failed", Status::Unknown);
    cause.sqlstate = code.map(|byte| c_char::try_from(byte).expect("a SQLSTATE is ASCII"));
    AdbcError::Adbc(cause)
}

#[test]
fn a_statement_cancelled_by_its_timeout_reads_as_the_deadline_and_a_permission_refusal_does_not() {
    assert!(super::session::deadline_exceeded(&with_sqlstate(*b"57014")));
    assert!(!super::session::deadline_exceeded(&with_sqlstate(*b"42501")));
}

#[test]
fn a_scale_zero_numeric_is_an_integer() {
    // The driver hands `NUMERIC` back as `Utf8` whatever the scale, tagged `numeric`; the
    // `numeric` rows reader answers a scale-zero text that parses as an integer.
    let column = Arc::new(StringArray::from(vec!["1"]));
    let field = Field::new("total", column.data_type().clone(), true).with_metadata(HashMap::from([(
        String::from("ADBC:postgresql:typname"),
        String::from("numeric"),
    )]));
    let batch = RecordBatch::try_new(Arc::new(Schema::new(vec![field])), vec![column]).expect("one column is a batch");
    let (outcome, _) = run(Some(batch), None, open_for(Duration::from_secs(60)));
    let batches = outcome.expect("a tagged numeric column is answered");
    let rows = super::numeric::rows(&batches).expect("a numeric cell is read");
    assert_eq!(rows.rows(), [vec![Value::Integer(1)]]);
}

#[test]
fn a_stream_past_the_row_ceiling_is_refused_while_it_is_read() {
    // A literal rather than the constant, so raising the ceiling reddens this cell instead of
    // moving it.
    let (outcome, sent) = run(
        Some(one_column("n", Arc::new(Int8Array::from(vec![0_i8; 1_000_001])))),
        None,
        open_for(Duration::from_secs(60)),
    );
    let refused = outcome.expect_err("a million and one rows are past the ceiling");
    assert!(
        matches!(refused, AdbcError::Unannounced(UnannouncedBatch::OverBound { .. })),
        "{refused:?}"
    );
    assert_eq!(sent.last(), Some(&Sent::Rollback), "{sent:?}");
}

#[test]
fn the_result_byte_budget_is_the_bigquery_transports_quarter_gibibyte() {
    // A constant pinned by a literal, because nothing else here would notice it growing: a
    // stream past a quarter gibibyte is too costly to build in a cell, and `Accumulating`'s own
    // refusal is held in the domain. The row ceiling is held by the cell above instead.
    assert_eq!(super::session::MOST_RESULT_BYTES.get(), 256 * 1024 * 1024);
}

#[test]
fn a_relative_driver_path_is_refused_before_anything_loads_it() {
    let refused = PostgresDriver::parse("lib/libadbc_driver_postgresql.so").expect_err("a relative path is refused");
    assert!(matches!(refused, super::UnusableDriverPath::Relative { .. }), "{refused:?}");
}

#[test]
fn the_linked_route_is_offered_exactly_where_the_linked_driver_initialises() {
    // A source build offers none and has none; a linked build offers the one it can open.
    let offered = PostgresDriver::linked_in();
    assert_eq!(
        offered.is_some(),
        sutura_adbc::linked_postgres_driver().is_ok(),
        "{offered:?}"
    );
    if let Some(linked) = offered {
        linked.probe().expect("the driver this build links initialises");
    }
}

#[test]
fn each_route_reads_as_the_sentence_doctor_prints_and_the_driver_check_matches() {
    // `nix/bigquery-driver-check.sh` passes a musl artefact only on `linked into this binary`.
    assert_eq!(PostgresDriver(Route::Linked).to_string(), "linked into this binary");
    let mounted = PostgresDriver::parse("/opt/lib/libadbc_driver_postgresql.so").expect("an absolute path parses");
    assert_eq!(mounted.to_string(), "mounted at /opt/lib/libadbc_driver_postgresql.so");
}

#[test]
fn a_path_naming_no_driver_fails_at_the_load_and_the_uri_is_never_printed() {
    // THE NEGATIVE CONTROL: the real `execute`, which every guard above lets through, fails on
    // the one thing a fake cannot stand in for. And the connection string, which carries the
    // password in a deployment (a plain marker here), is in neither the `Debug` nor the failure.
    let driver = PostgresDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("an absolute path parses");
    let source = SourceName::parse("pg").expect("a test source is a source");
    let conninfo = Conninfo::new(
        &source,
        crate::connection::ConnectionTarget::Host("127.0.0.1"),
        1,
        "sales",
        "reader",
        &Secret::new("a-connection-string-marker"),
        Channel::Plaintext,
    )
    .expect("plaintext builds");
    let transport = AdbcPostgres::new(
        sutura_conformance::corpus::source(),
        sutura_conformance::corpus::posture(),
        driver,
        conninfo,
    )
    .expect("the default ceiling parses");
    assert!(
        !format!("{transport:?}").contains("connection-string-marker"),
        "{transport:?}"
    );
    let failed = sutura_domain::warehouse::Warehouse::execute_raw(
        &transport,
        &sutura_domain::raw::RawStatement::parse("SELECT 1").expect("a statement"),
        &sutura_conformance::corpus::presented(),
        open_for(Duration::from_secs(60)),
    );
    assert!(
        matches!(
            failed,
            Some(Err(crate::PostgresError::Adbc {
                cause: AdbcError::Load(_)
            }))
        ),
        "{failed:?}"
    );
    assert!(!format!("{failed:?}").contains("connection-string-marker"), "{failed:?}");
}

#[test]
fn a_raw_result_is_cut_to_one_row_past_the_cap() {
    let mut connection = FakeConnection::replying(one_column("n", Arc::new(Int8Array::from(vec![0_i8; 10_005]))));
    let batches = super::session::raw(&mut connection, "SELECT 1", CEILING_MS, open_for(Duration::from_secs(60)))
        .expect("a stream past the raw cap is still read");
    let rows = super::raw_rows(&batches).expect("the raw rows are read");
    assert_eq!(rows.rows().len(), 10_001);
}

#[test]
fn the_port_reads_a_refusal_and_the_deadline_by_their_sqlstate() {
    use sutura_conformance::corpus;
    use sutura_domain::warehouse::Warehouse as _;

    let driver = PostgresDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("an absolute path parses");
    let conninfo = Conninfo::new(
        &corpus::source(),
        crate::connection::ConnectionTarget::Host("127.0.0.1"),
        1,
        "sales",
        "reader",
        &Secret::new("p"),
        Channel::Plaintext,
    )
    .expect("plaintext builds");
    let transport = AdbcPostgres::new(corpus::source(), corpus::posture(), driver, conninfo).expect("the default ceiling parses");
    let failed = |code| crate::PostgresError::Adbc {
        cause: with_sqlstate(code),
    };
    assert!(transport.source_refused(&failed(*b"25006")));
    assert!(!transport.source_refused(&failed(*b"57014")));
    assert!(transport.deadline_exceeded(&failed(*b"57014")));
    assert!(!transport.deadline_exceeded(&failed(*b"42501")));
    // Spent after the connection opened is the one `DeadlineSpent`, not an ADBC variant.
    let spent = crate::PostgresError::from(AdbcError::DeadlineSpent);
    assert!(matches!(spent, crate::PostgresError::DeadlineSpent), "{spent:?}");
    assert!(transport.deadline_exceeded(&spent));
}
