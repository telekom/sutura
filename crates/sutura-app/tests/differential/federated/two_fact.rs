//! **The two-fact ratio, executed** (`telekom/sutura#780`): one cross-model ratio answered with both
//! fact legs and the lookup on each registered data system, compared against two engines.
//!
//! A two-fact plan links its facts through a dimension on a SECOND data system, and the one-source
//! bundle refuses the question for that reason - so unlike the rest of this file there is no
//! one-source answer to compare against, and [`the_two_fact_ratio_is_the_figure_its_rows_add_up_to`]
//! measures that refusal rather than stating it. The reference is two engines instead, and their
//! answer is held to figures worked out by hand from the derived CSVs: the combiner is shared by
//! every side, so only a literal catches a defect in it.
//!
//! The per-data-system half is [`disagreement`], asserted over the whole registry by
//! `tests/differential.rs`'s
//! `leg_executing_data_systems_agree_with_the_engines_and_a_legless_one_is_refused`.
//!
//! **What this does NOT establish.** Every leg runs under one operating-system identity, as in the
//! rest of `federated.rs`. Both facts sit on one source, because a calendar declared once per source
//! is not built. And a registered data system whose tier is not up here is skipped, not compared.

use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_domain::warehouse::{Real, RowSet, Value};
use sutura_semantic::{Compiled, compile};

use super::corpus::{TWO_FACT_QUESTION, as_a_cube, derived, lookup_source};
use super::harness::{Side, answered, bundle, opened_on, two_engines};
use crate::adapters::{DataSystemUnderTest, source};

const NAME: &str = "two-fact-tickets-per-subscription";

fn question() -> Query {
    serde_norway::from_str(TWO_FACT_QUESTION).expect("the two-fact question is a question")
}

/// The rows of one answer, or the reason there were none.
fn rows_of<W>(side: &Side<W>) -> RowSet
where
    W: sutura_domain::warehouse::Warehouse + Sync,
    W::Error: Send,
{
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    match answered(side, &question(), NAME, &combiner).unwrap_or_else(|e| panic!("{e}")) {
        ToolOutcome::Answer { rows, .. } => rows,
        refused @ ToolOutcome::Refusal { .. } => panic!("{NAME}: a two-fact question is answered, not {refused:?}"),
    }
}

/// One answer group, worked out by hand: its region, its month, and tickets over subscription-months.
struct Group {
    region: Option<&'static str>,
    month: &'static str,
    /// `None` where the denominator is zero, which `yields_null` answers as null.
    ratio: Option<f64>,
}

/// Read off the derived CSVs by hand: subscription-months per region are `fct_subscription_monthly`'s
/// own rows, tickets are `corpus::DATA_CASES`'s. The null region is customer 41's one June
/// subscription and customer 42's May tickets - neither has a `dim_customer` row - so May's null
/// group is 5/0 and June's is 0/1: the empty-set rule from both sides.
const BY_HAND: &[Group] = &[
    group(Some("central"), "2026-05-01", Some(0.0)),                   // 0 / 13
    group(Some("east"), "2026-05-01", Some(0.3)),                      // 3 / 10
    group(Some("north"), "2026-05-01", Some(0.25)),                    // 4 / 16
    group(Some("south"), "2026-05-01", Some(0.0)),                     // 0 / 7
    group(Some("west"), "2026-05-01", Some(0.0)),                      // 0 / 16
    group(None, "2026-05-01", None),                                   // 5 / 0
    group(Some("central"), "2026-06-01", Some(0.0)),                   // 0 / 13
    group(Some("east"), "2026-06-01", Some(0.0)),                      // 0 / 10
    group(Some("north"), "2026-06-01", Some(0.25)),                    // 4 / 16
    group(Some("south"), "2026-06-01", Some(0.285_714_285_714_285_7)), // 2 / 7
    group(Some("west"), "2026-06-01", Some(0.4)),                      // 6 / 15
    group(None, "2026-06-01", Some(0.0)),                              // 0 / 1
];

const fn group(region: Option<&'static str>, month: &'static str, ratio: Option<f64>) -> Group {
    Group { region, month, ratio }
}

/// [`BY_HAND`] as the answer the engines are held to.
fn expected() -> RowSet {
    let rows = BY_HAND
        .iter()
        .map(|g| {
            vec![
                g.region.map_or(Value::Null, |r| Value::Text(r.into())),
                Value::Text(g.month.into()),
                g.ratio.map_or(Value::Null, |r| {
                    Value::Real(Real::parse(r).expect("a hand-worked ratio is finite"))
                }),
            ]
        })
        .collect();
    RowSet::new(
        vec![
            String::from("region"),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            String::from("tickets_per_subscription"),
        ],
        rows,
    )
    .expect("the hand-worked answer is rectangular")
}

/// **The certified number, from a literal no code under test produced.**
///
/// Both topologies first: the one-source bundle refuses the question for want of a dimension on a
/// second data system, and the two-source bundle plans it with a second fact leg - so the answer
/// below is a two-fact answer and not a one-fact plan that happens to agree.
#[test]
fn the_two_fact_ratio_is_the_figure_its_rows_add_up_to() {
    let derived = derived();
    let query = question();
    let one = compile(
        &query,
        &ScopedView::everything(&bundle(&derived.one_source)),
        RowCeiling::DEFAULT,
    )
    .expect("the question compiles on one source");
    assert!(
        matches!(
            one,
            Compiled::Refused {
                reason: RefusalReason::CrossModelRatioWithoutSharedDimension { .. }
            }
        ),
        "with every model on one data system there is no link to join two facts on, so this is refused: {one:?}"
    );
    let two = bundle(&derived.two_source);
    let Compiled::Federated { plan } =
        compile(&query, &ScopedView::everything(&two), RowCeiling::DEFAULT).expect("the question compiles on two sources")
    else {
        panic!("{NAME}: a two-fact question over the two-source catalog federates");
    };
    assert!(
        plan.second_fact().is_some(),
        "{NAME}: the plan reads the second fact in a leg of its own"
    );

    let answer = rows_of(&two_engines(two));
    if let Err(disagreement) = agree_on_content(&expected(), &answer, RealTolerance::DIFFERENTIAL) {
        panic!("{NAME}: two engines did not answer the hand-worked figures - {disagreement}\n{answer:?}");
    }
}

/// **A cube's measure answers the same figures** (`telekom/sutura#1148`): the cube expands at load
/// into the metric above, so the second fact leg and the combiner never see a cube.
#[test]
fn a_cube_measure_answers_the_two_fact_ratio_its_metric_does() {
    let answer = rows_of(&two_engines(bundle(&as_a_cube().two_source)));
    if let Err(disagreement) = agree_on_content(&expected(), &answer, RealTolerance::DIFFERENTIAL) {
        panic!("{NAME}: the cube's measure did not answer the hand-worked figures - {disagreement}\n{answer:?}");
    }
}

/// One registered data system holding every leg, on both sources, against two engines: `None` where
/// it answers what they do, or is skipped, and otherwise what it did instead.
///
/// A finding rather than a panic, so `tests/differential.rs`'s one cell makes the assertion. A data
/// system that declares no leg execution must be refused by name before anything runs -
/// `ClickHouse` - and one whose tier is not up here is skipped.
pub(crate) fn disagreement<W>() -> Option<String>
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    if !W::available() {
        return None;
    }
    let derived = derived();
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    if !W::EXECUTES_LEGS {
        // Refused before anything is minted or run, so it is opened with NO table: the refusal
        // cannot depend on data it never reads, and the bundle is the engines'.
        let side = Side {
            bundle: two_engines(bundle(&derived.two_source)).bundle,
            warehouses: sutura_app::Warehouses::of(W::open_on(source(), Vec::new()))
                .and(W::open_on(lookup_source(), Vec::new()))
                .expect("two sources, one registry"),
        };
        return match answered(&side, &question(), NAME, &combiner) {
            Ok(ToolOutcome::Refusal {
                reason: RefusalReason::FederationNotExecutable,
            }) => None,
            other => Some(format!(
                "{NAME}: {} runs no leg, so it is refused as not executable, not {other:?}",
                W::NAME
            )),
        };
    }
    let pinned = bundle(&derived.two_source);
    let warehouses = sutura_app::Warehouses::of(opened_on::<W>(&derived.data, &source(), &pinned))
        .and(opened_on(&derived.data, &lookup_source(), &pinned))
        .expect("two sources, one registry");
    let side = Side {
        bundle: sutura_app::verify_and_validate(pinned, &warehouses).expect("the anchors and the declarations hold"),
        warehouses,
    };
    let reference = rows_of(&two_engines(bundle(&derived.two_source)));
    let other = match answered(&side, &question(), NAME, &combiner) {
        Ok(ToolOutcome::Answer { rows, .. }) => rows,
        other => return Some(format!("{NAME}: {} gave no answer: {other:?}", W::NAME)),
    };
    agree_on_content(&reference, &other, RealTolerance::DIFFERENTIAL)
        .map_err(|d| format!("{NAME}: two engines and {} returned different rows - {d}", W::NAME))
        .and_then(|()| {
            agree_on_order(&reference, &other, RealTolerance::DIFFERENTIAL).map_err(|d| {
                format!(
                    "{NAME}: two engines and {} returned the same rows in different orders - {d}",
                    W::NAME
                )
            })
        })
        .err()
}
