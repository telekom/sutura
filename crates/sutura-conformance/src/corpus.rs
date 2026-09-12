//! The corpus the execute packs run: one table, seven questions, and the answer written ONCE.
//!
//! **Written once is the whole property.** Every registered adapter is asked the same plan and
//! compared against the same [`Case::expected`] rows, so *these two data systems answer this
//! question the same way* is a claim about the answer rather than about two hand-written
//! expectations that happen to agree. The comparison itself is
//! [`sutura_domain::warehouse::agreement`]'s, not this module's.
//!
//! # The plan is built from domain types, and that is what keeps this crate portable
//!
//! There is no catalog here and no compiler: a [`QueryPlan`] is a domain value, so the corpus can
//! state one directly. That is the reason a pack can be bound to an adapter that lives in its own
//! crate - the packs depend on the interior and on nothing else, so `sutura-exec-duckdb` can take
//! them as a dev-dependency without acquiring a catalog adapter, `sutura-semantic` or
//! `sutura-app`.
//!
//! # What this corpus does NOT contain, stated so nobody reads it as the whole suite
//!
//! - **The three cases `docs/adr/0012` names** - a filter on a remote dimension over an orphan key,
//!   a ratio whose denominator is zero for one subgroup, and a `CountDistinct` spanning two join
//!   keys. Each needs a second table and a federated plan; none is here.
//! - **Files.** `docs/adr/0012`'s *the corpus is files, not code* is unbuilt: a case is a value in
//!   this module, so adding one is still a code change.
//!
//! # Null placement in a group key: decided, and what the null row does and does NOT detect
//!
//! **`ASC NULLS LAST`, everywhere, and it is not this corpus's choice to make.** `sutura_sql`'s
//! `ordered_nulls_last` states the placement in the AST for every dialect and
//! `sutura-exec-datafusion`, which renders no SQL at all, lands on the same placement because
//! `LogicalPlanBuilder::sort_by` is `Expr::sort(true, false)`.
//! [`sutura_domain::plan::federated`] calls it *the whole of the ordered-result contract*. So the
//! per-case opt-out this module used to say it owed is **not** owed: there is no case here whose
//! null order a conforming source may legitimately answer differently.
//!
//! **What the null row does NOT detect, measured rather than assumed.** It is NOT a check on our
//! statement of the placement. The layer collapses `NULLS LAST` away for every target whose
//! default is already nulls-last, which is all three adapters bound to these packs - so deleting
//! `ordered_nulls_last`'s `nulls_first: Some(false)` leaves `DuckDB`'s and Postgres's rendered SQL
//! **byte-identical**, and the only dialect whose text changes is `BigQuery`, which has no binding
//! here. Their engines then order nulls last on their own. `crates/sutura-sql`'s
//! `every_order_by_states_nulls_last` is what holds our statement of it, and it holds the AST
//! rather than an answer.
//!
//! **What it DOES buy, which is two things and neither is that one.** Through
//! [`crate::Behaviour::Order`] it pins the three engines' own default null ordering - most
//! usefully `datafusion`'s, whose `sort_by` supplies `nulls_first: false` from a default that a
//! version bump could change with no diff of ours - so it is a **dependency regression detector**.
//! Through [`crate::Behaviour::Content`] it is a claim about OUR code: a null key must be a GROUP
//! and not a row a join or a filter dropped, which is the failure class
//! `crates/sutura-app/tests/golden/data_systems.rs` names for a fact key.

#![expect(
    clippy::expect_used,
    clippy::float_arithmetic,
    clippy::panic,
    reason = "every value below is a literal in this file, so one that does not parse is a broken \
              fixture rather than an input to handle - and a `Result` per accessor would put that \
              handling at every call site in every pack. The same argument covers the two \
              filesystem calls in `materialise`: a corpus that cannot be written is a broken \
              environment, and a pack that carried on would assert over an empty table. The \
              float arithmetic is in the expected means and the expected fractional totals, \
              written as the division and the addition an adapter performs rather than as a \
              decimal literal transcribed to however many places somebody typed - the \
              arithmetic IS the reference"
)]

use std::path::PathBuf;
use std::sync::OnceLock;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::Presented;
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    LegPlan, LegTerm, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
    QueryPlan, ResultLabel, StatementTables,
};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{ParamValue, Real, RowSet, Value};

/// The data system name every plan in this corpus resolves to.
const SOURCE: &str = "conformance";

/// The one table every case reads.
pub const TABLE: &str = "conformance_events";

/// One question, and the answer to it.
///
/// Private fields with accessors, which is what a library crate here owes: a caller cannot assemble
/// a `Case` whose expected rows belong to a different plan.
pub struct Case {
    name: &'static str,
    plan: QueryPlan,
    expected: RowSet,
}

impl Case {
    /// The name a failure reports. Static, so a fault carries it without allocating.
    #[inline]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The plan to execute.
    #[inline]
    pub const fn plan(&self) -> &QueryPlan {
        &self.plan
    }

    /// The rows every conforming adapter answers, in the order the plan's `ORDER BY` claims.
    #[inline]
    pub const fn expected(&self) -> &RowSet {
        &self.expected
    }
}

/// One leg, and the answer to it.
///
/// Separate from [`Case`] because a leg is a different shape of executable rather than a different
/// question: it reads the same table over the same range and answers the same rows, which is what
/// makes it a conformance claim - an adapter that executes a leg must reach the answer a whole plan
/// reaches.
pub struct LegCase {
    name: &'static str,
    leg: LegPlan,
    expected: RowSet,
}

impl LegCase {
    #[inline]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    pub const fn leg(&self) -> &LegPlan {
        &self.leg
    }

    #[inline]
    pub const fn expected(&self) -> &RowSet {
        &self.expected
    }
}

/// The data system name every plan here resolves to.
///
/// An adapter opened under another name is handed a plan it does not own, and the packs do not
/// paper over that: it is the fixture's job to open the adapter as this source.
#[must_use]
pub fn source() -> SourceName {
    SourceName::parse(SOURCE).expect("the corpus source name is a name")
}

/// The table every case reads, as the data system knows it.
#[must_use]
pub fn table() -> TableName {
    TableName::parse(TABLE).expect("the corpus table name is a name")
}

/// The posture every adapter in this pack is opened with.
///
/// **Shared, and the packs cannot offer anything else today.** An adapter that CAN carry a
/// per-subject credential needs one minted for a real subject at a real source to be exercised as
/// such, which is leg 2 and is not built - so binding an impersonating adapter to these packs holds
/// it to the shared path only, and the pack says so rather than reporting a green that reads wider.
/// `docs/adr/0012` decides that impersonation gets no negative pack at all, and this is the same
/// boundary from the positive side.
#[must_use]
pub fn posture() -> SourcePosture {
    SourcePosture::SharedServiceUser { declared: declared() }
}

/// What a leg in these packs executes as.
///
/// Built from the same `declared` acknowledgement [`posture`] is, so the credential the pack
/// presents and the posture the fixture opened the adapter with cannot drift apart - which is
/// exactly the disagreement each adapter's exhaustive match on what it received exists to catch.
/// An adapter that compares the two witnesses fails here if a fixture opened it any other way.
#[must_use]
pub fn presented() -> Presented {
    Presented::SharedServiceUser { declared: declared() }
}

/// The one acknowledgement both halves of the identity declaration read.
fn declared() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(
        AcknowledgementReason::parse("the conformance corpus is one CSV read in this process under one identity")
            .expect("the corpus acknowledgement is a reason"),
    )
}

/// The corpus, as CSV, from the committed file under `corpus/`.
///
/// Served from the file rather than from a copied constant so a row edit in
/// `corpus/conformance_events.csv` is a change to the data and not to this module - which is the
/// whole of what "a case is a directory entry, not a function" asks. The bytes are embedded at
/// compile time by [`include_str!`], so this stays `&'static str` and `const`.
///
/// The last row is outside every case's time range on purpose: a corpus whose filters exclude
/// nothing cannot tell an adapter that applied them from one that did not.
#[must_use]
pub const fn csv() -> &'static str {
    include_str!("../corpus/conformance_events.csv")
}

/// The corpus on a filesystem, written once per process, as the path a fixture attaches.
///
/// **Inside THIS WORKTREE, and that is the whole of `telekom/sutura#405`'s first instance.** It
/// used to land on `<temp_dir>/sutura-conformance/<table>.csv` - a purpose and no key - and the
/// argument for the rename below was *the bytes are identical either side*, which is true per TREE
/// and not per machine. Reproduced on 2026-09-07 with two worktrees of this repository, each
/// running its own `on_disk`, one row differing: the `DuckDB` binding failed
/// `total-by-region-and-day` and `total-by-region-and-day-as-a-leg` as content faults naming this
/// corpus's own cases, while the run that overwrote the file was green. The window is wide because
/// `attach_csv` makes a VIEW over `read_csv_auto`, so the file is read at QUERY time; the Postgres
/// binding reads it once per behaviour plus the census at LOAD time.
///
/// A path under the worktree needs no key, because the worktree is the key - the same answer
/// `sutura_dev::scope::Scope::state_dir` gives, and `tests/bound.rs` pins the two spellings
/// together against that type rather than leaving a comment claiming they agree. This crate may not
/// reach `sutura-dev` through a normal dependency (`xtask/src/boundaries/harness.rs`), so the
/// SPELLING is duplicated and the AGREEMENT is mechanical.
///
/// Written under a per-process temporary name and RENAMED onto the shared one, which is still
/// needed and now means something narrower: `nextest` gives each test its own process, so several
/// processes of THIS worktree write this path at once, and a reader must not see a half-written
/// file. Those processes write identical bytes - which is the claim the old path could not make.
#[must_use]
pub fn on_disk() -> PathBuf {
    static WRITTEN: OnceLock<PathBuf> = OnceLock::new();
    WRITTEN.get_or_init(materialise).clone()
}

/// Writes the corpus where an adapter can attach it.
fn materialise() -> PathBuf {
    let dir = state_dir().join(PURPOSE);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("could not create {}: {e}", dir.display()));
    let staged = dir.join(format!("{TABLE}.{}.csv", std::process::id()));
    let final_path = dir.join(format!("{TABLE}.csv"));
    std::fs::write(&staged, csv()).unwrap_or_else(|e| panic!("could not write {}: {e}", staged.display()));
    std::fs::rename(&staged, &final_path).unwrap_or_else(|e| panic!("could not rename onto {}: {e}", final_path.display()));
    final_path
}

/// The directory under [`state_dir`] this crate writes into.
///
/// Named rather than inlined so `tests/bound.rs` can assert the whole path against the type that
/// owns the other half of it.
const PURPOSE: &str = "conformance";

/// The name of this worktree's own state directory, as `sutura_dev::scope` spells it.
///
/// **A second spelling of one value, and the duplication is deliberate rather than overlooked** -
/// the same arrangement `crate::REQUIRE_TIER` is in, for the same reason and with the same
/// mechanism: this crate may reach `sutura-dev` only as a DEV-dependency, so a pack body cannot be
/// written against anything but the interior, and `tests/bound.rs` compares this constant against
/// `sutura_dev::scope::STATE_DIR` so the two cannot drift.
const STATE_DIR: &str = ".sutura-dev";

/// This worktree's own state directory, resolved from this crate's own manifest directory.
///
/// **`CARGO_MANIFEST_DIR` is the only root available here**, and it is enough: cargo bakes it in at
/// compile time, it is inside the worktree being built, and in the nix sandbox it is inside that
/// derivation's own writable copy of the tree - so it is per-worktree wherever this compiles. The
/// walk looks for the two markers `xtask::repo::root` looks for, because a single marker is
/// satisfiable by a directory that is not a checkout of this repository.
///
/// A tree with no root above this crate is a broken fixture rather than a case to handle - the
/// same argument the module's `#![expect]` makes about the two filesystem calls in [`materialise`].
fn state_dir() -> PathBuf {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .ancestors()
        .find(|dir| dir.join("flake.nix").is_file() && dir.join("Cargo.lock").is_file())
        .unwrap_or_else(|| {
            panic!(
                "no checkout root above {} - the corpus has nowhere in this worktree to live",
                manifest.display()
            )
        });
    root.join(STATE_DIR)
}

/// Every question in the corpus.
///
/// A `Vec` rather than a constant, because a [`QueryPlan`] owns its strings and none of these types
/// is `const`-constructible. The count is what [`crate::census`] reports, so a corpus that lost its
/// cases is a failure rather than a fast green.
#[must_use]
pub fn cases() -> Vec<Case> {
    vec![
        total_by_region_and_day(),
        mean_by_day(),
        total_wide_by_day(),
        total_rate_by_day(),
        wide_total_by_day(),
        overflowing_integer_total_by_day(),
        decimal_total_by_day(),
    ]
}

/// The one leg in the corpus.
#[must_use]
pub fn leg_case() -> LegCase {
    // The same shape as `total_by_region_and_day`, expressed as one source's share of an answer
    // rather than as a whole one - so the expected rows are that case's, unchanged.
    let case = total_by_region_and_day();
    LegCase {
        name: "total-by-region-and-day-as-a-leg",
        leg: LegPlan::Fact {
            source: source(),
            metric: metric("amount_total"),
            tables: StatementTables::only(table()),
            bucket: bucket(),
            keys: vec![region_key()],
            terms: vec![LegTerm::new(
                PlanTerm::Aggregate {
                    aggregate: Aggregate::Sum,
                    column: column("amount_cents"),
                },
                // The metric's own name and not a leaf position, because this leg's expected rows
                // ARE the whole-answer case's - the labels have to agree for the comparison to be
                // about the numbers.
                ResultLabel::measure(&metric("amount_total")),
            )],
            filters: range_filters(),
            params: range_params(),
            range: range(),
        },
        expected: case.expected,
    }
}

/// `SUM` by region and day: the integer class, two keys, a filter that excludes a row - and the
/// one NULL group key in the corpus.
///
/// **The null group is expected LAST.** Read the module header for what that detects and what it
/// does not: it is NOT a check on our own statement of `ASC NULLS LAST`, because for all three
/// bound adapters the placement is their engine's own default and deleting our statement of it
/// changes neither their SQL nor their answer. What the order assertion pins is those engines'
/// defaults - `datafusion`'s `sort_by` most of all, since a version bump could change it with no
/// diff of ours - and what the CONTENT assertion pins is ours: a null key is a GROUP rather than a
/// row a join or a filter dropped.
fn total_by_region_and_day() -> Case {
    let plan = QueryPlan::new(
        source(),
        metric("amount_total"),
        StatementTables::only(table()),
        bucket(),
        vec![region_key()],
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("amount_cents"),
            },
        },
        ResultLabel::measure(&metric("amount_total")),
        range_filters(),
        range_params(),
        range(),
    );
    let expected = rows(
        &plan,
        vec![
            vec![text("east"), text("2026-01-01"), Value::Integer(350)],
            vec![text("east"), text("2026-01-02"), Value::Integer(150)],
            vec![text("north"), text("2026-01-01"), Value::Integer(400)],
            vec![text("north"), text("2026-01-02"), Value::Integer(1300)],
            // LAST, because the plan claims `ASC NULLS LAST` - which for all three bound adapters
            // is their own engine's default rather than something our rendering adds. See this
            // function's doc.
            vec![Value::Null, text("2026-01-01"), Value::Integer(50)],
        ],
    );
    Case {
        name: "total-by-region-and-day",
        plan,
        expected,
    }
}

/// `AVG` by day: the approximate class reached through a DIVISION, and the only case that needs
/// [`sutura_domain::warehouse::agreement::RealTolerance`] to agree at all.
///
/// `total_rate_by_day` answers the same class from the data instead, and its totals are exact - so
/// this is still the one case the tolerance is on the call FOR.
///
/// The expected values are written as the division rather than as a decimal literal, so the
/// reference is the `f64` an adapter that summed exactly and divided once must land on - not a
/// transcription of one to however many places somebody typed.
fn mean_by_day() -> Case {
    let plan = QueryPlan::new(
        source(),
        metric("amount_mean"),
        StatementTables::only(table()),
        bucket(),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Avg,
                column: column("amount_cents"),
            },
        },
        ResultLabel::measure(&metric("amount_mean")),
        range_filters(),
        range_params(),
        range(),
    );
    let expected = rows(
        &plan,
        vec![
            // The null-region row is one of the four: a null in a DIMENSION key is absent from the
            // grouping this case does not do, and an adapter that filtered it out instead would
            // answer a mean over three rows.
            vec![text("2026-01-01"), real((100.0 + 250.0 + 400.0 + 50.0) / 4.0)],
            vec![text("2026-01-02"), real((150.0 + 600.0 + 700.0) / 3.0)],
        ],
    );
    Case {
        name: "mean-by-day",
        plan,
        expected,
    }
}

/// `SUM` by day over a column whose total lands exactly on `i64::MAX`: the WIDE integer class.
///
/// **The whole point is which Rust type each adapter arrives through, and that all three arrive at
/// the same cell anyway.** `sum` over a 64-bit integer column is a different type in each of the
/// three bound adapters - `DuckDB` widens to a `HUGEINT`, Postgres to a `NUMERIC`, and the engine
/// reads the shared fixture as `Decimal256` - so before this case the only measure in the corpus was small enough that
/// every one of those arms was interchangeable with a 32-bit read. Each adapter keeps a
/// narrowing arm for its wide type ([`sutura_domain::warehouse::agreement`]'s header lists all
/// three), and nothing exercised one.
///
/// **`i64::MAX` rather than a merely large number, and that is a falsifier rather than a flourish.**
/// `i64::MAX` is `2^63 - 1`, which has 63 significant bits and therefore **no exact `f64`** - it
/// rounds to `2^63`, one higher. So an adapter that routed this total through a 64-bit float on its
/// way to a cell answers a different number here and this case reddens, where a round total like
/// `9_000_000_000_000_000_000` has an exact `f64` and would not notice. That is the arm
/// `sutura-exec-duckdb`'s own `cell` records as having been wrong once.
///
fn total_wide_by_day() -> Case {
    total_by_day(
        "total-wide-by-day",
        "wide_total",
        "wide_cents",
        [Value::Integer(i64::MAX), Value::Integer(14)],
    )
}

/// `SUM` by day over a fractional column: the approximate class, reached from the DATA.
///
/// Distinct from [`mean_by_day`], which is the only other case answering a [`Value::Real`]: that
/// one's real number comes out of an aggregate's DIVISION - and for Postgres out of an explicit
/// cast `sutura_sql` applies to an `AVG` - while this one's comes out of each adapter's type
/// inference over a fractional literal on the load path. So the two fail for different reasons: an
/// adapter that mapped its fixed-point type here, or an inference that answered one, reddens this
/// cell and leaves `mean-by-day` green.
///
/// The expected values are exact binary fractions and their sum is exact in any order, so this
/// case does not lean on [`sutura_domain::warehouse::agreement::RealTolerance`] - it is about which
/// class the cell is, and `mean-by-day` is where the approximation is the subject.
fn total_rate_by_day() -> Case {
    total_by_day(
        "total-rate-by-day",
        "rate_total",
        "rate",
        [real(0.25 + 0.5 + 1.25 + 0.25), real(2.5 + 0.5 + 0.25)],
    )
}

/// `SUM` over a fixed-point column whose values are past `i64`, held exact by every adapter.
fn wide_total_by_day() -> Case {
    total_by_day(
        "wide-total-by-day",
        "wide_amount_total",
        "wide_amount",
        [text("10000000000000000006"), Value::Integer(15)],
    )
}

/// `SUM` over individually signed integers whose total crosses `i64`, without wrapping.
fn overflowing_integer_total_by_day() -> Case {
    total_by_day(
        "overflowing-integer-total-by-day",
        "overflow_amount_total",
        "overflow_amount",
        [text("9223372036854775808"), Value::Integer(6)],
    )
}

/// `SUM` over a decimal column, held exact rather than widened to a binary float.
fn decimal_total_by_day() -> Case {
    total_by_day(
        "decimal-total-by-day",
        "decimal_amount_total",
        "decimal_amount",
        [text("11.50"), text("19.50")],
    )
}

fn total_by_day(case_name: &'static str, metric_name: &str, column_name: &str, totals: [Value; 2]) -> Case {
    let [first, second] = totals;
    let plan = QueryPlan::new(
        source(),
        metric(metric_name),
        StatementTables::only(table()),
        bucket(),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column(column_name),
            },
        },
        ResultLabel::measure(&metric(metric_name)),
        range_filters(),
        range_params(),
        range(),
    );
    let expected = rows(&plan, vec![vec![text("2026-01-01"), first], vec![text("2026-01-02"), second]]);
    Case {
        name: case_name,
        plan,
        expected,
    }
}

/// The expected answer, labelled by the plan itself.
///
/// The labels are NOT written out beside the rows, and that is deliberate: an expectation that
/// spelled them again would be a second statement of what the plan projects, and the two would
/// disagree eventually. [`crate::Behaviour::Labels`] is the assertion that the adapter agrees with
/// the plan about them, and it is a separate one for that reason.
fn rows(plan: &QueryPlan, cells: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(plan.result_labels(), cells).expect("the expected rows are rectangular")
}

fn metric(name: &str) -> MetricName {
    MetricName::parse(name).expect("a corpus metric name is a name")
}

fn column(name: &str) -> PlanColumn {
    PlanColumn::new(table(), ColumnName::parse(name).expect("a corpus column name is a name"))
}

fn text(value: &str) -> Value {
    Value::Text(String::from(value))
}

fn real(value: f64) -> Value {
    Value::Real(Real::parse(value).expect("a corpus mean is finite"))
}

/// The time bucket every case groups by: one day.
fn bucket() -> PlanBucket {
    PlanBucket::new(ResultLabel::bucket(), Grain::Day, column("day"))
}

/// The one dimension key in the corpus.
fn region_key() -> PlanKey {
    PlanKey::new(
        ResultLabel::dimension(&DimensionName::parse("region").expect("a corpus dimension is a dimension")),
        column("region"),
    )
}

/// The window every case reads: two days of the three in the corpus.
fn range() -> TimeRange {
    TimeRange::new(day(1), day(3)).expect("the corpus range is not empty")
}

fn day(of_january: u8) -> Date {
    Date::new(2026, 1, of_january).expect("a January date in 2026 is a date")
}

/// The range, as the predicates a plan carries.
///
/// Definitional, because a bounded range is part of what a metric means rather than something the
/// question asked for - and the adapters read `filters` rather than `range`, so a plan without
/// these is a plan with no `WHERE` at all.
fn range_filters() -> Vec<PlanFilter> {
    vec![
        PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::AtOrAfter {
                column: column("day"),
                param: 0,
            },
        ),
        PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::Before {
                column: column("day"),
                param: 1,
            },
        ),
    ]
}

fn range_params() -> Vec<ParamValue> {
    vec![ParamValue::Date(day(1)), ParamValue::Date(day(3))]
}

#[cfg(test)]
mod tests {
    use super::{TABLE, Value, cases, csv, leg_case, on_disk, range};
    use sutura_domain::plan::Executable;

    /// The corpus is not empty, which is the state every behaviour would be vacuously green over.
    #[test]
    fn the_corpus_holds_cases() {
        assert!(!cases().is_empty());
    }

    /// Each case's expected answer is labelled by its own plan, and is as wide as those labels.
    ///
    /// The one thing a hand-written expectation gets wrong that nothing else would catch: a row of
    /// the wrong width is refused by `RowSet::new`, but a row of the right width in the wrong
    /// COLUMN ORDER is not, and `result_labels` is what says which order that is.
    #[test]
    fn every_expected_answer_is_labelled_by_its_own_plan() {
        for case in cases() {
            assert_eq!(
                case.expected().columns(),
                case.plan().result_labels(),
                "case `{}`",
                case.name()
            );
        }
        let leg = leg_case();
        assert_eq!(leg.expected().columns(), Executable::Leg(leg.leg()).result_labels());
    }

    /// The filters exclude something, and no expected answer mentions it.
    ///
    /// A corpus whose range covers every row cannot tell an adapter that applied the plan's filters
    /// from one that ignored them, and every case here would still be green.
    ///
    /// **Comparing row counts does not say that**, which is what this test did first: aggregation
    /// reduces rows on its own, so *fewer answered rows than data lines* holds over a corpus with no
    /// filter at all. So it reads the DAY instead - one data line is at or after the range's
    /// exclusive end, and no expected answer carries that day.
    #[test]
    fn the_corpus_holds_a_day_every_case_excludes() {
        let end = range().end().to_iso();
        let excluded = csv()
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .filter(|line| line.split(',').next().is_some_and(|day| day >= end.as_str()))
            .count();
        assert!(excluded > 0, "every data line is inside the range, so nothing is excluded");
        for case in cases() {
            let answers_about_it = case
                .expected()
                .rows()
                .iter()
                .flatten()
                .any(|cell| matches!(*cell, Value::Text(ref value) if *value == end));
            assert!(
                !answers_about_it,
                "case `{}` answers about {end}, which its own range excludes",
                case.name()
            );
        }
    }

    /// The corpus reaches a filesystem, which is how every adapter's fixture attaches it.
    #[test]
    fn the_corpus_is_written_where_an_adapter_can_attach_it() {
        let path = on_disk();
        assert_eq!(std::fs::read_to_string(&path).expect("the corpus was written"), csv());
        assert!(path.ends_with(format!("{TABLE}.csv")));
    }
}
