//! What a data system decides, expanded over every registered one.
//!
//! The bundle comes from `adapters::ReferenceCatalog` throughout: which catalog read it does not
//! change what a data system does with the plan, and the catalog axis is what makes that true.

use sutura_app::{answer, verify_anchors};
use sutura_domain::pinned::AnchorCheck;
use sutura_domain::plan::Executable;
use sutura_domain::query::ToolOutcome;
use sutura_domain::warehouse::RowSet;
use sutura_semantic::{Compiled, compile};

use crate::adapters::{DataSystemUnderTest, ReferenceCatalog, load, open, questions, read_question, stem};

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
    let pinned = load::<ReferenceCatalog>();
    let warehouse: W = open(&pinned);
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
    let pinned = load::<ReferenceCatalog>();
    let warehouse: W = open(&pinned);
    let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");
    for path in questions() {
        let asked = read_question(&path);
        let name = stem(&path);
        let answered = answer(&validated, &asked, &warehouse);
        settings(W::NAME).bind(|| match answered {
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
    let pinned = load::<ReferenceCatalog>();
    let warehouse: W = open(&pinned);
    for path in questions() {
        let asked = read_question(&path);
        let compiled = compile(&asked, &pinned).expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        warehouse
            .dry_run(Executable::Query(plan))
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
    let pinned = load::<ReferenceCatalog>();
    let warehouse: W = open(&pinned);
    let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");
    let total_of = |file: &str| -> f64 {
        let outcome =
            answer(&validated, &question(file), &warehouse).unwrap_or_else(|e| panic!("{file} failed on {}: {e}", W::NAME));
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
fn fails_a_zero_denominator_that_declares_it_fails<W>()
where
    W: DataSystemUnderTest,
{
    let pinned = load::<ReferenceCatalog>();
    let warehouse: W = open(&pinned);
    let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");

    // January has seventy subscription-months and none of them terminated, so there is a group to
    // answer for and the denominator is nevertheless zero.
    let january = question("revenue-per-churned-subscription-january.yaml");
    let error = answer(&validated, &january, &warehouse).expect_err("a zero denominator under `fails` must not answer");
    let rendered = chain(&error);
    assert!(
        rendered.contains("column revenue_per_churned_subscription"),
        "{} does not say which column it could not carry:\n{rendered}",
        W::NAME
    );
    assert!(
        rendered.contains("is not a finite number"),
        "{} failed for some other reason:\n{rendered}",
        W::NAME
    );

    // And June, where three subscriptions terminated, still answers a figure - so this is not a
    // test that would pass with the metric refused outright.
    let outcome = answer(
        &validated,
        &question("revenue-per-churned-subscription-june.yaml"),
        &warehouse,
    )
    .expect("a non-zero denominator answers");
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
        }
    };
}

crate::adapters::registered!(data_systems: cell);
