//! What a data system decides, expanded over every registered one.
//!
//! The bundle comes from `adapters::ReferenceCatalog` throughout: which catalog read it does not
//! change what a data system does with the plan, and the catalog axis is what makes that true.

use sutura_app::{answer, verify_anchors};
use sutura_domain::pinned::AnchorCheck;
use sutura_domain::plan::Executable;
use sutura_domain::query::ToolOutcome;
use sutura_domain::warehouse::RowSet;
use sutura_domain::warehouse::cardinality::{DeclaredKey, KeyUniqueness};
use sutura_semantic::{Compiled, compile};

use crate::adapters::{DataSystemUnderTest, ReferenceCatalog, data_root, load, open, questions, read_question, stem};

use crate::shared::{chain, question, settings, stable};

/// "Reproduce a number that already exists" is the acceptance criterion that separates this
/// from a demo.
///
/// The numbers are in the catalog documents, the data is in the CSVs, and nothing in between is
/// allowed to change what they add up to. Per data system, because an arithmetic difference
/// between two of them is exactly what a one-sided anchor check cannot see.
fn reproduces_every_declared_anchor<W>()
where
    W: DataSystemUnderTest,
{
    // A network adapter with no provisioned tier to run against is SKIPPED (the notice is already
    // on stderr) rather than failed; the skip-or-fail direction is `SUTURA_DEV_REQUIRE_TIER`.
    if !W::available() {
        return;
    }
    let pinned = load::<ReferenceCatalog>();
    let warehouse = sutura_app::Warehouses::of(open::<W>(&pinned));
    let report = verify_anchors(&pinned, &warehouse);
    settings(W::NAME).bind(|| insta::assert_yaml_snapshot!("anchor_report", &report));
    for (metric, check) in report.checks() {
        assert_eq!(
            *check,
            AnchorCheck::Matched,
            "{metric} did not reproduce its declared number on {}",
            W::NAME
        );
    }
    assert!(
        !report.checks().is_empty(),
        "the example catalog declares no anchor, so this proved nothing"
    );
    // And the same run, through the operation that mints the proof. `verify_anchors` above is
    // what an operator reads; this is the only thing that produces a bundle the service takes.
    drop(sutura_app::verify_and_validate(pinned, &warehouse).expect("a bundle whose anchors all matched is fit to serve"));
}

/// The end of the line: the statement we generated, run, and the rows it returned.
///
/// This is what would catch a change that is valid SQL, plans identically, and returns a
/// different number. Three outcomes, not two. A question may also be one the data system
/// answered and the adapter would not carry: `revenue_per_churned_subscription` declares
/// `zero_denominator: fails`, so its January statement divides by zero, and IEEE float division
/// by zero answers `inf` rather than raising. That value used to be pinned right here as
/// `Real: inf` under the metric's own certified name, and the snapshot read as coverage.
fn runs_the_corpus_and_pins_the_rows<W>()
where
    W: DataSystemUnderTest,
{
    if !W::available() {
        return;
    }
    let pinned = load::<ReferenceCatalog>();
    let warehouse = sutura_app::Warehouses::of(open::<W>(&pinned));
    let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");
    for path in questions() {
        let asked = read_question(&path);
        let name = stem(&path);
        let answered = answer(
            &validated,
            &asked,
            &crate::adapters::a_caller(),
            &crate::adapters::shared_credential(),
            &warehouse,
            1 << 30,
        );
        settings(W::NAME).bind(|| match answered.map(sutura_app::Answered::into_outcome) {
            Ok(ToolOutcome::Refusal { ref reason }) => {
                insta::assert_yaml_snapshot!(format!("{name}__refused"), reason);
            }
            Ok(ToolOutcome::Answer { ref rows, .. }) => {
                insta::assert_snapshot!(format!("{name}__rows"), stable(rows));
            }
            Err(ref error) => {
                insta::assert_snapshot!(format!("{name}__error"), chain(error));
            }
        });
    }
}

/// Asking "would this be accepted" never answers no for a plan that runs.
///
/// The port takes a plan, so what an adapter does to answer that is its own business, and the two
/// registered here answer it differently on purpose. The SQL-rendering one prepares the statement:
/// a round trip buys "this failed before reading anything" instead of "this would have failed", and
/// preparing is far cheaper than running. The engine takes the port's **defaulted** `dry_run` and
/// does nothing, because for an in-process engine checking is not a cheaper question than
/// answering - it is building the logical plan and running the analyzer and the optimizer, which is
/// most of executing it, so a required pre-flight meant planning every question twice.
///
/// **So this cell is real for one entry and vacuous for the other, and that is worth saying rather
/// than leaving to be discovered.** For an adapter that takes the default it cannot fail; what it
/// still holds up is the contract for any adapter that overrides it - a pre-flight that rejects a
/// plan the adapter would then have run is a bug, and this is where it shows. The guarantee the
/// engine gives instead is asserted where it lives, in
/// `a_plan_naming_a_table_that_was_never_attached_is_an_error_and_never_an_empty_answer`:
/// resolution still happens, on the one pass it makes.
fn accepts_every_plan_before_running_it<W>()
where
    W: DataSystemUnderTest,
{
    if !W::available() {
        return;
    }
    let pinned = load::<ReferenceCatalog>();
    // The ADAPTER and not a registry, deliberately: `dry_run` is a port method and this test is about
    // the port. A registry is a lookup, so routing through it here would be asserting the lookup twice
    // and the pre-flight once.
    let warehouse: W = open(&pinned);
    for path in questions() {
        let asked = read_question(&path);
        let compiled = compile(&asked, &pinned).expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        warehouse
            .dry_run(Executable::Query(plan), &crate::adapters::presented())
            .unwrap_or_else(|e| panic!("{} was rejected by {}: {e}", stem(&path), W::NAME));
    }
}

/// The measure column of a result, summed as a real number.
///
/// Rendered and parsed rather than matched on a variant, because two adapters legitimately
/// return different Rust types for the same number - one hands back a wide integer where the
/// other hands back a decimal - and a reconciliation that matched `Value::Integer` would sum
/// zero on the other one and fail for a reason that is not the one it is about.
fn total(rows: &RowSet, label: &str) -> f64 {
    let index = rows
        .column_index(label)
        .unwrap_or_else(|| panic!("no single {label:?} column in {:?}", rows.columns()));
    (0..rows.rows().len())
        .filter_map(|row| rows.cell(row, index))
        .map(|value| {
            value
                .render()
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("{label} came back as {:?}, which is not a number: {e}", value.render()))
        })
        .sum()
}

/// THE BUG THIS EXISTS FOR, and it shipped.
///
/// The generator emitted an INNER join, so every fact row whose dimension row was missing
/// silently vanished from a grouped answer. Subscription 1071 in
/// `data/fct_subscription_monthly.csv` names customer 41 and `data/dim_customer.csv` stops at 40,
/// so with an inner join `recurring_revenue` for June answers 202121 while
/// `recurring_revenue by region` totals 197122. Two numbers, one metric, one period, and nothing
/// raising an error anywhere.
///
/// The catalog's existing guard could not see it. `may_duplicate_rows` refuses a join that
/// would FAN OUT the fact rows; this is the same failure by ELIMINATION, and a cardinality
/// check has nothing to say about it.
///
/// Asserted as a reconciliation rather than against a literal, because that is the property:
/// grouping by a dimension must partition the measure, not filter it. A left join makes the
/// unmatched row group under a null key, so the totals agree. Per data system, because a join
/// is one of the things each of them implements for itself.
///
/// Three groupings and not one, because the corpus reaches two relationships and the last of them
/// reaches both at once: a `LEFT` that was fixed on one join path and missed on another would
/// reconcile here and not there.
fn partitions_the_measure_rather_than_filtering_it<W>()
where
    W: DataSystemUnderTest,
{
    if !W::available() {
        return;
    }
    let pinned = load::<ReferenceCatalog>();
    let warehouse = sutura_app::Warehouses::of(open::<W>(&pinned));
    let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");
    let total_of = |file: &str| -> f64 {
        let outcome = answer(
            &validated,
            &question(file),
            &crate::adapters::a_caller(),
            &crate::adapters::shared_credential(),
            &warehouse,
            1 << 30,
        )
        .unwrap_or_else(|e| panic!("{file} failed on {}: {e}", W::NAME))
        .into_outcome();
        let ToolOutcome::Answer { ref rows, .. } = outcome else {
            panic!("{file} was refused: {outcome:?}");
        };
        total(rows, "recurring_revenue")
    };

    let ungrouped = total_of("recurring-revenue-june.yaml");
    for grouped_by in [
        "recurring-revenue-by-region.yaml",
        "recurring-revenue-by-segment.yaml",
        "recurring-revenue-by-region-and-family.yaml",
    ] {
        assert_eq!(
            format!("{:.12e}", total_of(grouped_by)),
            format!("{ungrouped:.12e}"),
            "{grouped_by} does not reconcile with the ungrouped total on {}; a dimension join \
             is filtering the measure instead of partitioning it",
            W::NAME
        );
    }
    // And the row that makes the test mean something is actually in the data: without an
    // unmatched key every join is a no-op and this reconciles trivially.
    assert_eq!(
        format!("{ungrouped:.12e}"),
        format!("{:.12e}", 202_121.0_f64),
        "the corpus no longer carries a subscription-month whose customer is absent, so this test \
         proves nothing; restore it in examples/single-player/data/fct_subscription_monthly.csv"
    );
}

/// THE BUG THIS EXISTS FOR, stated as the one assertion rather than as a snapshot diff.
///
/// `zero_denominator: fails` said "an empty period is a fault and not a figure" and answered
/// `inf`: both generators cast the numerator to a floating type before dividing, so the
/// division is IEEE float division, which does not raise on a zero denominator. Nothing caught
/// it - no fixture used the word, so no golden, no row snapshot, no anchor and no differential
/// row reached it, and because BOTH adapters produced `inf` the differential test agreed and
/// passed. So it is asserted per data system: an adapter that stopped refusing would otherwise
/// be covered by the other one's refusal.
///
/// Asserted as a `ServiceError` rather than as a `RefusalReason` on purpose, and the
/// distinction is the governance one this crate's own error type makes: a refusal is something
/// a caller asked for and may not have, and this caller asked a question the definition
/// permits. What went wrong is downstream of the plan.
///
/// ## Postgres refuses differently, and that is the point rather than the exception
///
/// The adapter set now splits on HOW `fails` is honored. `DuckDB` and the engine divide with IEEE
/// semantics and so receive `inf`/`NaN`, which the adapter refuses as a non-finite cell -
/// "column X is not a finite number". Postgres, asked the same statement, RAISES `division by
/// zero` at the server (measured: `1.0::float8 / 0.0` is an ERROR there, not `inf`). Both honor
/// the metric's contract - neither answers a number - and the assertion below is deliberately
/// per-adapter rather than one shape for all: the IEEE adapters still have to name the column and
/// the non-finite cell, and Postgres has to carry the server's typed `division by zero`. The
/// alternative - translating Postgres's server error into a non-finite refusal - would fabricate a
/// cause the server never sent.
fn fails_a_zero_denominator_that_declares_it_fails<W>()
where
    W: DataSystemUnderTest,
{
    if !W::available() {
        return;
    }
    let pinned = load::<ReferenceCatalog>();
    let warehouse = sutura_app::Warehouses::of(open::<W>(&pinned));
    let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");

    // January has seventy subscription-months and none of them terminated, so there is a group to
    // answer for and the denominator is nevertheless zero.
    let january = question("revenue-per-churned-subscription-january.yaml");
    let error = answer(
        &validated,
        &january,
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &warehouse,
        1 << 30,
    )
    .expect_err("a zero denominator under `fails` must not answer");
    let rendered = chain(&error);
    // Per-adapter, on how this one HONORS `fails` - a type switch rather than a weakening of the
    // shared assertion, so the ones that can name what they refused still have to.
    match W::NAME {
        // Postgres RAISES `division by zero` at the server where the IEEE adapters hand back a
        // non-finite cell, and a server error names no column; its contract is the server's own
        // refusal, which the adapter carries as the typed DivisionByZero variant.
        "postgres" => assert!(
            rendered.contains("division by zero"),
            "postgres neither refused a non-finite cell nor reported the server's division-by-zero:\n{rendered}"
        ),
        // The engine and DuckDB divide with IEEE semantics and receive a non-finite value, which
        // the adapter refuses NAMING THE COLUMN - the strong assertion kept for the adapters that
        // can make it.
        adapter => {
            let expected = format!("column {}", january.metric());
            assert!(
                rendered.contains(&expected) && rendered.contains("is not a finite number"),
                "{adapter} neither named the column it could not carry nor refused it as non-finite:\n{rendered}"
            );
        }
    }

    // And June, where three subscriptions terminated, still answers a figure - so this is not a
    // test that would pass with the metric refused outright.
    let outcome = answer(
        &validated,
        &question("revenue-per-churned-subscription-june.yaml"),
        &crate::adapters::a_caller(),
        &crate::adapters::shared_credential(),
        &warehouse,
        1 << 30,
    )
    .expect("a non-zero denominator answers")
    .into_outcome();
    let ToolOutcome::Answer { ref rows, .. } = outcome else {
        panic!("June has terminations, so it is an answer, not {outcome:?}");
    };
    // 202121 minor units of recurring revenue over three terminations, which is the June anchor of
    // `recurring_revenue` divided by the June anchor of `subscriptions_churned`.
    assert_eq!(
        format!("{:.12e}", total(rows, "revenue_per_churned_subscription")),
        format!("{:.12e}", 67_373.666_666_666_67_f64),
        "{} answered a different figure for June",
        W::NAME
    );
}

/// **Every data system in this registry COUNTS a declared join key, and counts the right thing.**
///
/// `Warehouse::declared_key` is defaulted to `KeyUniqueness::NotAsked`, so an adapter that never
/// implemented it - or one whose value mapping stopped handing back an integer, which arrives as an
/// `Err` that the boot path reads as *unchecked* - would leave every cardinality declaration on that
/// data system silently unchecked. Nothing else would say so: the corpus satisfies its declarations,
/// so a probe that answered nothing and a probe that answered cleanly leave the same green run.
///
/// **The counts are compared against the fixture CSV**, not merely against each other: a probe that
/// resolved the wrong table, or one over a table nobody loaded, answers `0` over `0` - which *is*
/// unique, vacuously, and would read as a clean check forever.
///
/// **What this axis does not reach is `BigQuery`**, which is not in this registry at all - no
/// published artifact links the crate, and that registry's rule is that a cell which cannot execute
/// reads as coverage. So a dimension model on a dataset is unchecked, stated in
/// `.agents/skills/sutura/invariants` and in `SECURITY.md` rather than implied by a green run here.
fn counts_every_declared_join_key<W>()
where
    W: DataSystemUnderTest,
{
    if !W::available() {
        return;
    }
    let pinned = load::<ReferenceCatalog>();
    let warehouse = open::<W>(&pinned);
    let definitions = pinned.definitions();
    assert!(
        !definitions.relationships().is_empty(),
        "the example catalog declares no relationship, so this proved nothing"
    );
    for relationship in definitions.relationships().values() {
        let key = DeclaredKey::promised_by(relationship, definitions).unwrap_or_else(|e| {
            panic!(
                "{} declares a cardinality that promises no unique key: {e}",
                relationship.name()
            )
        });
        // The registry cell is the second of the two call sites `clippy.toml` permits for a port
        // method that executes with no credential - the boot path holds the other. It is here rather
        // than nowhere because a capability nothing measures is a capability that can vanish.
        #[expect(
            clippy::disallowed_methods,
            reason = "this cell is one of the two permitted callers of a port method that executes with no \
                      credential; it exists to measure that the method still does what the boot path needs"
        )]
        let answered = warehouse
            .declared_key(key)
            .unwrap_or_else(|e| panic!("{} could not count {} on {}: {e}", W::NAME, key.column(), key.table()));
        let KeyUniqueness::Counted(counts) = answered else {
            panic!(
                "{} did not count {}, so every cardinality declaration on it is unchecked and nothing \
                 else in this suite would have said so",
                W::NAME,
                key.column()
            );
        };
        let rows = rows_in_fixture(&key);
        assert_eq!(
            counts.rows(),
            rows,
            "{} counted {} values of {} where the fixture holds {rows}",
            W::NAME,
            counts.rows(),
            key.column()
        );
        assert!(
            counts.is_unique(),
            "{} says {} is not unique in {}, which would make the example corpus unservable: {counts:?}",
            W::NAME,
            key.column(),
            key.table()
        );
    }
}

/// How many data rows the fixture CSV behind a probed table holds.
///
/// The corpus quotes nothing and every row carries a key, so a line count is the number the probe
/// has to reproduce. `adapters::fixture_tables` is not reused here deliberately: what wants checking
/// is that the adapter read the table the KEY names, which means resolving the path from the key.
fn rows_in_fixture(key: &DeclaredKey<'_>) -> u64 {
    let path = data_root().join(format!("{}.csv", key.table().name()));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    text.lines().skip(1).filter(|line| !line.is_empty()).count() as u64
}

/// One cell of the data-system axis.
macro_rules! cell {
    ($name:ident, $adapter:ty) => {
        mod $name {
            #[test]
            fn every_declared_anchor_reproduces_its_number() {
                super::reproduces_every_declared_anchor::<$adapter>();
            }

            #[test]
            fn the_corpus_runs_and_the_rows_are_pinned() {
                super::runs_the_corpus_and_pins_the_rows::<$adapter>();
            }

            #[test]
            fn every_plan_is_accepted_before_it_is_run() {
                super::accepts_every_plan_before_running_it::<$adapter>();
            }

            #[test]
            fn a_dimension_join_does_not_change_the_measure() {
                super::partitions_the_measure_rather_than_filtering_it::<$adapter>();
            }

            #[test]
            fn a_metric_declaring_that_a_zero_denominator_fails_does_fail() {
                super::fails_a_zero_denominator_that_declares_it_fails::<$adapter>();
            }

            #[test]
            fn whether_it_counts_a_declared_join_key_is_what_the_boot_check_can_use_it_for() {
                super::counts_every_declared_join_key::<$adapter>();
            }
        }
    };
}

crate::adapters::registered!(data_systems: cell);
