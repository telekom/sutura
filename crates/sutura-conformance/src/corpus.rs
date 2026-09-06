//! The corpus the execute packs run: one table, two questions, and the answer written ONCE.
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
//! - **A null in a group key.** Null placement in `ORDER BY` differs per data system and is not
//!   stated by the plan, so a null key would make [`crate::Behaviour::Order`] a claim about the
//!   source's collation. `docs/adr/0012` designed a re-sort for that and the re-sort was superseded
//!   by the two-function comparison - so the packs DO assert order, and that record now says so and
//!   carries the decision this corpus is deferring: a case whose order a source could legitimately
//!   answer differently must be able to opt out of the order behaviour, and there is no field for
//!   that yet. Avoiding the question is weaker than deciding it, which is why it is written here.
//! - **A wide integer or a decimal.** The type-mapping disagreements
//!   [`sutura_domain::warehouse::agreement`]'s header lists are all reachable only past an `i64`,
//!   and nothing here goes near one.
//! - **Files.** `docs/adr/0012`'s *the corpus is files, not code* is unbuilt: a case is a value in
//!   this module, so adding one is still a code change.

#![expect(
    clippy::expect_used,
    clippy::float_arithmetic,
    clippy::panic,
    reason = "every value below is a literal in this file, so one that does not parse is a broken \
              fixture rather than an input to handle - and a `Result` per accessor would put that \
              handling at every call site in every pack. The same argument covers the two \
              filesystem calls in `materialise`: a corpus that cannot be written is a broken \
              environment, and a pack that carried on would assert over an empty table. The \
              float arithmetic is the two expected means, written as the division an adapter \
              performs rather than as a decimal literal transcribed to however many places \
              somebody typed - the arithmetic IS the reference"
)]

use std::path::PathBuf;
use std::sync::OnceLock;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::Presented;
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    LegPlan, LegTerm, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
    QueryPlan, StatementTables,
};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{ParamValue, Real, RowSet, Value};

/// The data system name every plan in this corpus resolves to.
const SOURCE: &str = "conformance";

/// The one table every case reads.
pub const TABLE: &str = "conformance_events";

/// The corpus rows, as the CSV every adapter's fixture materialises.
///
/// **A `&str` and not a committed file, deliberately.** `flake.nix`'s source filter keeps
/// `crates/*/src` wholesale, so a `.csv` beside this module would survive a nix build - but the
/// filter is not the argument. A data system is handed a PATH, so the bytes have to reach a
/// filesystem either way ([`on_disk`]), and rendering them from here is what keeps the rows and the
/// [`Case::expected`] answers below in one file where a reader can check the arithmetic.
///
/// The last row is outside every case's time range on purpose: a corpus whose filters exclude
/// nothing cannot tell an adapter that applied them from one that did not.
const ROWS: &str = "\
day,region,amount_cents
2026-01-01,east,100
2026-01-01,east,250
2026-01-01,north,400
2026-01-02,east,150
2026-01-02,north,600
2026-01-02,north,700
2026-01-03,east,1000
";

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

/// The corpus, as CSV.
#[must_use]
pub const fn csv() -> &'static str {
    ROWS
}

/// The corpus on a filesystem, written once per process, as the path a fixture attaches.
///
/// Written under a per-process temporary name and RENAMED onto the shared one, so two adapters'
/// fixtures running in one process cannot read a half-written file. The bytes are identical either
/// side of the rename, so a reader that saw the old file saw the same corpus.
#[must_use]
pub fn on_disk() -> PathBuf {
    static WRITTEN: OnceLock<PathBuf> = OnceLock::new();
    WRITTEN.get_or_init(materialise).clone()
}

/// Writes [`ROWS`] where an adapter can attach it.
fn materialise() -> PathBuf {
    let dir = std::env::temp_dir().join("sutura-conformance");
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("could not create {}: {e}", dir.display()));
    let staged = dir.join(format!("{TABLE}.{}.csv", std::process::id()));
    let final_path = dir.join(format!("{TABLE}.csv"));
    std::fs::write(&staged, ROWS).unwrap_or_else(|e| panic!("could not write {}: {e}", staged.display()));
    std::fs::rename(&staged, &final_path).unwrap_or_else(|e| panic!("could not rename onto {}: {e}", final_path.display()));
    final_path
}

/// Every question in the corpus.
///
/// A `Vec` rather than a constant, because a [`QueryPlan`] owns its strings and none of these types
/// is `const`-constructible. The count is what [`crate::census`] reports, so a corpus that lost its
/// cases is a failure rather than a fast green.
#[must_use]
pub fn cases() -> Vec<Case> {
    vec![total_by_region_and_day(), mean_by_day()]
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
                String::from("amount_total"),
            )],
            filters: range_filters(),
            params: range_params(),
            range: range(),
        },
        expected: case.expected,
    }
}

/// `SUM` by region and day: the integer class, two keys, and a filter that excludes a row.
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
        String::from("amount_total"),
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
        ],
    );
    Case {
        name: "total-by-region-and-day",
        plan,
        expected,
    }
}

/// `AVG` by day: the approximate class, and the only reason
/// [`sutura_domain::warehouse::agreement::RealTolerance`] is on the call.
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
        String::from("amount_mean"),
        range_filters(),
        range_params(),
        range(),
    );
    let expected = rows(
        &plan,
        vec![
            vec![text("2026-01-01"), real((100.0 + 250.0 + 400.0) / 3.0)],
            vec![text("2026-01-02"), real((150.0 + 600.0 + 700.0) / 3.0)],
        ],
    );
    Case {
        name: "mean-by-day",
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
    PlanBucket::new(String::from("period"), Grain::Day, column("day"))
}

/// The one dimension key in the corpus.
fn region_key() -> PlanKey {
    PlanKey::new(String::from("region"), column("region"))
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
