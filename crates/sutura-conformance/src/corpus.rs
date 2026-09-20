//! The corpus the execute packs run: one table, eight questions, and the answer written ONCE.
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
//! # The cases are files, not code
//!
//! A case is a tracked data file under `corpus/cases/`, embedded at compile time by
//! [`include_str!`] and parsed by the typed loader in `case_files`. Adding a case is a data edit - a
//! new `.case` file plus one `include_str!` line - and no Rust function in this module changes.
//! That is the half of `docs/adr/0012`'s *the corpus is files, not code* that was unbuilt when the
//! cases were values in this module. The fixture table (`corpus/conformance_events.csv`) set the
//! precedent: a tracked data file read at compile time, served from the file rather than from a
//! copied constant.
//!
//! # What this corpus does NOT contain, stated so nobody reads it as the whole suite
//!
//! - **The three cases `docs/adr/0012` names** - a filter on a remote dimension over an orphan key,
//!   a ratio whose denominator is zero for one subgroup, and a `CountDistinct` spanning two join
//!   keys. Each needs a second table and a federated plan; none is here.
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
//! statement of the placement, for `DuckDB`, Postgres and the `datafusion` engine: their own
//! default is already nulls-last, so deleting `ordered_nulls_last`'s `nulls_first: Some(false)`
//! leaves `DuckDB`'s and Postgres's rendered SQL **byte-identical** (`datafusion` renders none to
//! compare) and all three still order nulls last on their own. `crates/sutura-sql`'s
//! `every_order_by_states_nulls_last` is what holds our statement of it for those three, and it
//! holds the AST rather than an answer.
//!
//! **`BigQuery` is the one bound adapter this does not hold for, and binding it changed nothing
//! about that.** `GoogleSQL`'s own default is nulls-FIRST - the opposite direction - which is why
//! `ordered_nulls_last`'s explicit clause is the only dialect text it actually changes. But
//! `BigQuery`'s binding (`crates/sutura-exec-bigquery/tests/conformance.rs`,
//! `telekom/sutura#710`) runs over a CANNED transport that answers every case from the corpus's
//! own [`Case::expected`] rows rather than a live endpoint - see that file's own header - so this
//! behaviour still proves nothing about a real `GoogleSQL` engine's own ordering, in either
//! direction, for this adapter.
//!
//! **What it DOES buy, which is two things and neither is that one.** Through
//! [`crate::Behaviour::Order`] it pins the three REAL engines' own default null ordering - most
//! usefully `datafusion`'s, whose `sort_by` supplies `nulls_first: false` from a default that a
//! version bump could change with no diff of ours - so it is a **dependency regression detector**
//! for those three. Through [`crate::Behaviour::Content`] it is a claim about OUR code: a null key
//! must be a GROUP and not a row a join or a filter dropped, which is the failure class
//! `crates/sutura-app/tests/golden/data_systems.rs` names for a fact key.
//!
//! # Collation: the opt-out this corpus used to lack, and what it does and does not decide
//!
//! Every key above is chosen so no bound engine's own collation can disagree with byte order -
//! `docs/adr/0012` names this as the corpus's deliberate limit. `total-by-collation-sensitive-key-and-day`
//! is the one case that does not have that property on purpose, and [`Case::order_is_asserted`] is
//! what lets it exist without lying: `Behaviour::Order` skips exactly this case, so a source
//! whose locale answers `"apple"` before `"Banana"` is not reported as a defect for disagreeing
//! with a byte order this corpus never claimed. `Behaviour::Content` still runs over it - which
//! four totals came back is ours to assert regardless of anybody's locale.
//!
//! **What this does NOT decide.** It is an opt-out from one assertion, not a statement of which
//! collation is correct - there is no such statement anywhere in this crate, and none is added
//! here. And it is not evidence of a defect found: `DuckDB`, Postgres and the engine all default
//! to byte order today, so this case would be currently green with the assertion left in too, for
//! those three; the field exists so a future adapter's own default does not need to become one
//! before this corpus can say why it is not a fault. `BigQuery` is bound and answers this case
//! too, but `Behaviour::Order` already skips it by `order_is_asserted`, so nothing here has
//! measured what a real `GoogleSQL` collation would do with it.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "every value below is a literal in this file or in a tracked data file, so one that \
              does not parse is a broken fixture rather than an input to handle - and a `Result` \
              per accessor would put that handling at every call site in every pack. The same \
              argument covers the two filesystem calls in `materialise`: a corpus that cannot be \
              written is a broken environment, and a pack that carried on would assert over an \
              empty table"
)]

mod case_files;

use std::path::PathBuf;
use std::sync::LazyLock;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::Presented;
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanPredicate, PlanTerm, PredicateOrigin,
    QueryPlan, ResultLabel, StatementTables,
};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{ParamValue, RowSet};

/// The data system name every plan in this corpus resolves to.
const SOURCE: &str = "conformance";

/// The one table every case reads.
pub const TABLE: &str = "conformance_events";

/// One question, and the answer to it.
///
/// Private fields with accessors, which is what a library crate here owes: a caller cannot assemble
/// a `Case` whose expected rows belong to a different plan.
#[derive(Debug)]
pub struct Case {
    name: String,
    plan: QueryPlan,
    expected: RowSet,
    order_is_asserted: bool,
}

impl Case {
    /// The name a failure reports.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
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

    /// Whether [`crate::Behaviour::Order`] may compare this case's row order at all.
    ///
    /// **`true` for every case but one.** This corpus states no collation - every other case's
    /// group key is either not text or is chosen so byte order and every collation a bound engine
    /// might use agree (see the module header). A case whose key does not have that property would
    /// have a source's own locale decide an order this corpus never claimed, and asserting one
    /// particular order over it would report a legitimate per-source difference as a defect - which
    /// is the reverse of what conformance is for. `false` marks exactly that case; its content is
    /// still asserted, because *which rows* is never a collation question.
    #[inline]
    pub const fn order_is_asserted(&self) -> bool {
        self.order_is_asserted
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

/// The port's deadline every pack executes under.
///
/// A generous budget, opened now: these packs assert content and refusal shapes, not timing, so the
/// only requirement is that it not run out before an adapter answers.
#[must_use]
pub fn deadline() -> Deadline {
    Deadline::opened_at(
        std::time::Instant::now(),
        Budget::parse(std::time::Duration::from_secs(30)).expect("thirty seconds is a budget"),
    )
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
    static WRITTEN: LazyLock<PathBuf> = LazyLock::new(materialise);
    WRITTEN.clone()
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
///
/// The cases are parsed from tracked data files under `corpus/cases/` by the loader in `case_files`.
/// The file list is `FILES`, embedded at compile time by
/// `include_str!`, so a file removed from disk is a compile error and a file removed from the list
/// is caught by `the_loader_reads_every_case_file` in that module's own tests.
#[must_use]
pub fn cases() -> Vec<Case> {
    case_files::load()
}

/// The one leg in the corpus.
#[must_use]
pub fn leg_case() -> LegCase {
    // The same shape as `total-by-region-and-day`, expressed as one source's share of an answer
    // rather than as a whole one - so the expected rows are that case's, unchanged.
    let case = cases()
        .into_iter()
        .find(|c| c.name() == "total-by-region-and-day")
        .expect("the corpus holds the total-by-region-and-day case");
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
            bindings: range_bindings(),
            range: range(),
            top: None,
        },
        expected: case.expected,
    }
}

fn metric(name: &str) -> MetricName {
    MetricName::parse(name).expect("a corpus metric name is a name")
}

fn column(name: &str) -> PlanColumn {
    PlanColumn::new(table(), ColumnName::parse(name).expect("a corpus column name is a name"))
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
/// The two range bounds and the values they bind, as one parsed set.
///
/// One function rather than a filter list beside a parameter list, because the indices and the values
/// are the same fact: `sutura_domain::plan::bindings` is what an adapter does when two helpers
/// disagree.
fn range_bindings() -> PlanBindings {
    let filters = vec![
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
    ];
    PlanBindings::parse(filters, vec![ParamValue::Date(day(1)), ParamValue::Date(day(3))])
        .expect("the corpus range binds its two bounds in placeholder order")
}

#[cfg(test)]
mod tests {
    use super::{TABLE, cases, csv, leg_case, on_disk, range};
    use sutura_domain::plan::Executable;
    use sutura_domain::warehouse::Value;

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
                .any(|cell| matches!(cell, Value::Text(value) if value == &end));
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
