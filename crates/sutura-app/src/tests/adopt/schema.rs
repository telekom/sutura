//! A load and a refresh over a real `DuckDB`: the schema a bundle's tables have at the source is
//! read at the load, pinned with the snapshot, and read again by a refresh.

use super::*;

use sutura_domain::model::QualifiedTable;
use sutura_domain::warehouse::ResultBudget;
use sutura_domain::warehouse::schema::{SchemaDigest, TableSchemas};
use sutura_exec_duckdb::{DuckDbError, DuckDbWarehouse};

/// The served bundle's `orders`, with June 2026 summing to its anchor.
const ORDERS: &str = "order_date,amount_cents\n2026-06-03,100000\n2026-06-17,97122\n";

/// [`ORDERS`] with `amount_cents` read as a `DOUBLE`, still summing to the anchor.
const ORDERS_IN_DOUBLES: &str = "order_date,amount_cents\n2026-06-03,100000.5\n2026-06-17,97121.5\n";

/// Writes `csv` to a file of its own and (re-)attaches it as `orders`. The view reads the file at
/// each statement, so each version keeps its own.
fn attach(warehouse: &DuckDbWarehouse, file: &str, csv: &str) {
    let path = std::env::temp_dir().join(format!("sutura-adopt-{}-{file}.csv", std::process::id()));
    std::fs::write(&path, csv).expect("the temp dir is writable");
    warehouse
        .attach_csv(&TableName::parse("orders").expect("a test table is a table"), &path)
        .expect("a written CSV attaches");
}

/// An in-memory `DuckDB` holding nothing yet.
fn duckdb() -> DuckDbWarehouse {
    let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive"));
    DuckDbWarehouse::in_memory(source(), shared(), budget).expect("an in-memory database opens")
}

fn duckdb_with(file: &str, csv: &str) -> DuckDbWarehouse {
    let warehouse = duckdb();
    attach(&warehouse, file, csv);
    warehouse
}

fn schema_digest_of(outcome: &ToolOutcome) -> &SchemaDigest {
    let ToolOutcome::Answer { ref provenance, .. } = *outcome else {
        panic!("expected an answer, not {outcome:?}");
    };
    provenance.schema_digest()
}

/// A real `DuckDB` the cell keeps a handle to, so it can re-attach a table under the running
/// service. Once `armed`, the next `execute` meets the test thread twice, as in
/// [`RendezvousWarehouse`]; every other call delegates straight through. A clone shares all four.
#[derive(Clone)]
struct SharedDuckDb {
    inner: Arc<DuckDbWarehouse>,
    armed: Arc<AtomicBool>,
    reached: Arc<Barrier>,
    proceed: Arc<Barrier>,
}

impl Warehouse for SharedDuckDb {
    type Error = DuckDbError;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        self.inner.source()
    }

    fn posture(&self) -> &SourcePosture {
        self.inner.posture()
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.inner.dry_run(executable, presented, deadline)
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.reached.wait();
            self.proceed.wait();
        }
        self.inner.execute(executable, presented, deadline)
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test wrapper delegating the boot path's one call to the adapter it wraps"
        )]
        let rows = self.inner.verify_anchor(plan);
        rows
    }

    fn table_schemas(&self, tables: &BTreeSet<QualifiedTable>) -> Result<TableSchemas, Self::Error> {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test wrapper delegating the load's and the refresh's one call to the adapter it wraps"
        )]
        let read = self.inner.table_schemas(tables);
        read
    }
}

/// A served `DuckDB` over [`ORDERS`], and the clone of its [`SharedDuckDb`] the cell drives it by.
fn serving_duckdb() -> (Service<SharedDuckDb>, SharedDuckDb) {
    let shared = SharedDuckDb {
        inner: Arc::new(duckdb_with("served", ORDERS)),
        armed: Arc::new(AtomicBool::new(false)),
        reached: Arc::new(Barrier::new(2)),
        proceed: Arc::new(Barrier::new(2)),
    };
    (serving(shared.clone()), shared)
}

#[test]
fn a_bundle_naming_a_column_or_a_table_the_source_lacks_is_refused_at_the_load() {
    // Catches a load that never reads the source's schema, and one that reads a failed read as a
    // table with no columns. DuckDB's driver answers a table it lacks with `Internal`, not
    // `NotFound`, so that half refuses as an unread schema.
    let lacking_column = duckdb_with("lacking-column", "order_date,amount\n2026-06-03,197122\n");
    let refused = verify_and_validate(served(), &Warehouses::of(lacking_column)).err();
    let Some(NotValidated::ColumnNotAtSource(ref missing)) = refused else {
        panic!("a source lacking a column refuses the bundle's column, not {refused:?}");
    };
    assert_eq!(missing.column().as_str(), "amount_cents");

    let refused = verify_and_validate(served(), &Warehouses::of(duckdb())).err();
    let Some(NotValidated::SchemaNotRead(ref unread)) = refused else {
        panic!("a source lacking the table refuses its schema as unread, not {refused:?}");
    };
    assert_eq!(unread.source(), &source());
}

#[test]
fn a_changed_column_type_under_the_same_bundle_rotates_onto_the_new_schema() {
    // Catches an `adopt` that compares the bundle's digest alone: the bundle is unchanged, the type
    // of a column it names is not, so the next answer must carry another schema digest.
    let (service, shared) = serving_duckdb();
    let before = schema_digest_of(&ask(&service, "revenue")).clone();

    attach(&shared.inner, "in-doubles", ORDERS_IN_DOUBLES);
    let adopted = service
        .adopt(served())
        .expect("the same bundle validates over the new column type");

    assert_eq!(
        adopted,
        Adopted::Rotated {
            previous: served().digest().clone(),
            digest: served().digest().clone(),
        }
    );
    assert_ne!(schema_digest_of(&ask(&service, "revenue")), &before);
}

#[test]
fn a_question_in_flight_keeps_the_schema_digest_it_started_with() {
    // Catches an `answer` that reads the served schema set again after its start: the column type
    // changes and is adopted while this question is inside `execute`.
    let (service, shared) = serving_duckdb();
    let before = schema_digest_of(&ask(&service, "revenue")).clone();
    shared.armed.store(true, Ordering::SeqCst);

    let (in_flight, adopted) = std::thread::scope(|scope| {
        let question = scope.spawn(|| ask(&service, "revenue"));
        shared.reached.wait();
        attach(&shared.inner, "in-doubles", ORDERS_IN_DOUBLES);
        let adopted = service.adopt(served());
        shared.proceed.wait();
        (question.join().expect("the question thread does not panic"), adopted)
    });

    assert!(
        matches!(adopted, Ok(Adopted::Rotated { .. })),
        "the new schema landed mid-question, not {adopted:?}"
    );
    assert_eq!(schema_digest_of(&in_flight), &before);
    assert_ne!(schema_digest_of(&ask(&service, "revenue")), &before);
}
