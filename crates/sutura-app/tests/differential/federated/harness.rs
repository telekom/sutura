//! The harness for `federated.rs` - the bundle loader, the side-openers, the differential, the
//! classifier and the `MUST_BE_REACHED` table - plus the `DuckDB`-side differential test.

use std::path::{Path, PathBuf};

use sutura_app::Validated;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::pinned::{NotValidated, PinnedDefinitions, Provenance, SemanticCatalog as _};
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::RowSet;
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_semantic::{Compiled, compile};

use crate::adapters::{a_caller, deadline, posture, shared_credential, source, version};

use super::corpus::{LOOKUP_SOURCE, derived, every_question, lookup_source};

/// An amount of working set no question in this corpus comes near, so only a defect refuses.
///
/// A gibibyte, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` - a literal for the
/// reason `adapters::DataSystemUnderTest for DataFusionWarehouse` gives.
pub(super) const BUDGET: u64 = 1 << 30;

// ------------------------------------------------------------------------- opening the sides ---

/// A bundle that may not have validated, beside the registry that judged it.
///
/// A named pair rather than an inline tuple, which the complexity threshold in `clippy.toml` catches
/// and is right to: the two halves are *what the boot path decided* and *what it asked*, and a bare
/// two-element tuple says which is which nowhere.
pub(super) type Attempted<W> = (Result<Validated<PinnedDefinitions>, NotValidated>, sutura_app::Warehouses<W>);

/// One bundle, loaded through the real markdown adapter over a derived catalog.
pub(super) fn bundle(catalog: &Path) -> PinnedDefinitions {
    sutura_catalog_local::LocalCatalog::new(source(), catalog.to_path_buf(), version())
        .load()
        .unwrap_or_else(|e| panic!("the derived catalog at {} does not load: {e}", catalog.display()))
}

/// Compile only: these topology witnesses open no execution adapter.
pub(super) fn compiled_dimensions(pinned: &PinnedDefinitions, dimensions: &str) -> Compiled {
    let query = serde_norway::from_str(&format!(
        "metrics: [recurring_revenue]\ngrain: month\nrange: {{ start: 2026-06-01, end: 2026-07-01 }}\n\
         dimensions: [{dimensions}]\n"
    ))
    .expect("the topology question is valid");
    compile(&query, &ScopedView::everything(pinned), RowCeiling::DEFAULT).expect("the derived catalog is consistent")
}

/// The one-source side: the ENGINE, over every table the bundle names.
///
/// The bundle and the registry come back together because a `Validated` bundle is only obtainable
/// by re-executing the anchors against the registry that will answer with it - which is what
/// makes "this side reproduces its own certified numbers" a precondition of the comparison rather
/// than a separate test.
///
/// **That precondition is this file's THIRD-PARTY oracle, and it is why the comparison is not
/// circular.** An anchor's expected figure is a literal declared in the catalog
/// (`metrics/recurring_revenue.md`'s `value: 202121`), not a number recorded from a run - so a
/// defect shared by every side of the comparison still reddens here, before any two answers are
/// compared. Review measured that: a mutation to a shared expression builder fails at this call.
pub(super) fn one_source(pinned: PinnedDefinitions) -> Side<sutura_exec_datafusion::DataFusionWarehouse> {
    let (bundle, warehouses) = validating_on_one_source(&derived().data, pinned);
    Side {
        bundle: bundle.expect("the anchors and the declarations hold on one source"),
        warehouses,
    }
}

/// The one-source registry, and whatever the bundle validated to.
///
/// Split out of [`one_source`] so the violated corpus can read the `Err` this one unwraps. Nothing
/// else differs: the same engine, the same tables, the same one call that mints the proof.
pub(super) fn validating_on_one_source(
    data: &Path,
    pinned: PinnedDefinitions,
) -> Attempted<sutura_exec_datafusion::DataFusionWarehouse> {
    let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
    let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
        source(),
        posture(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("an in-process engine starts");
    for (table, csv) in tables_on(data, &source(), &pinned) {
        engine
            .attach_csv(&table, &csv)
            .unwrap_or_else(|e| panic!("the engine could not attach {}: {e}", csv.display()));
    }
    let warehouses = sutura_app::Warehouses::of(engine);
    let validated = sutura_app::verify_and_validate(pinned, &warehouses);
    (validated, warehouses)
}

/// The two-source side: one `DuckDB` per source, each holding only its own tables.
///
/// Two databases rather than one with everything attached, and that is the point of the leg
/// split: neither statement CAN reach the other side's table, so a join across them has to
/// happen above the port or not at all.
pub(super) fn two_sources(pinned: PinnedDefinitions) -> Side<sutura_exec_duckdb::DuckDbWarehouse> {
    let (bundle, warehouses) = validating_on_two_sources(&derived().data, pinned);
    Side {
        bundle: bundle.expect("the anchors and the declarations hold on two sources"),
        warehouses,
    }
}

/// **The two-source side a release can actually run: one ENGINE per source.**
///
/// The same topology [`two_sources`] builds, with the adapter swapped for the one the shipped
/// binary links. Each engine attaches only its own source's tables, so neither can reach the
/// other's - the isolation is which tables were registered, not which directory they came from,
/// which is exactly what `sutura-cli`'s `open_files` does per declared `files` entry.
pub(super) fn two_engines(pinned: PinnedDefinitions) -> Side<sutura_exec_datafusion::DataFusionWarehouse> {
    let (bundle, warehouses) = validating_on_two_engines(&derived().data, pinned);
    Side {
        bundle: bundle.expect("the anchors and the declarations hold on two engines"),
        warehouses,
    }
}

/// The two-engine registry, and whatever the bundle validated to. [`validating_on_two_sources`]'s
/// twin, split out for its reason: the violated corpus reads the `Err` this one unwraps, and on this
/// topology that refusal is the one a RELEASE would give.
pub(super) fn validating_on_two_engines(
    data: &Path,
    pinned: PinnedDefinitions,
) -> Attempted<sutura_exec_datafusion::DataFusionWarehouse> {
    let warehouses = sutura_app::Warehouses::of(engine_on(data, &source(), &pinned))
        .and(engine_on(data, &lookup_source(), &pinned))
        .expect("two sources, one registry");
    let validated = sutura_app::verify_and_validate(pinned, &warehouses);
    (validated, warehouses)
}

/// One engine holding one source's tables and nothing else.
///
/// [`duckdb_on`]'s twin, and it asserts the same non-empty precondition for the same reason: an
/// engine with no table attached would answer nothing and the failure would name the question.
pub(super) fn engine_on(
    data: &Path,
    name: &SourceName,
    pinned: &PinnedDefinitions,
) -> sutura_exec_datafusion::DataFusionWarehouse {
    let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
    let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
        name.clone(),
        posture(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("an in-process engine starts");
    let attached = tables_on(data, name, pinned);
    assert!(!attached.is_empty(), "no model in the derived bundle sits on {name}");
    for (table, csv) in attached {
        engine
            .attach_csv(&table, &csv)
            .unwrap_or_else(|e| panic!("the engine could not attach {}: {e}", csv.display()));
    }
    engine
}

/// The two-source registry, and whatever the bundle validated to. [`validating_on_one_source`]'s
/// twin, for its reason.
pub(super) fn validating_on_two_sources(
    data: &Path,
    pinned: PinnedDefinitions,
) -> Attempted<sutura_exec_duckdb::DuckDbWarehouse> {
    let warehouses = sutura_app::Warehouses::of(duckdb_on(data, &source(), &pinned))
        .and(duckdb_on(data, &lookup_source(), &pinned))
        .expect("two sources, one registry");
    let validated = sutura_app::verify_and_validate(pinned, &warehouses);
    (validated, warehouses)
}

/// One side of the differential: a bundle whose anchors it reproduced, and what answers it.
pub(super) struct Side<W> {
    pub(super) bundle: Validated<PinnedDefinitions>,
    pub(super) warehouses: sutura_app::Warehouses<W>,
}

pub(super) fn duckdb_on(data: &Path, name: &SourceName, pinned: &PinnedDefinitions) -> sutura_exec_duckdb::DuckDbWarehouse {
    let warehouse = sutura_exec_duckdb::DuckDbWarehouse::in_memory(name.clone(), posture()).expect("an in-memory database opens");
    let attached = tables_on(data, name, pinned);
    assert!(!attached.is_empty(), "no model in the derived bundle sits on {name}");
    for (table, csv) in attached {
        warehouse
            .attach_csv(&table, &csv)
            .unwrap_or_else(|e| panic!("duckdb could not attach {}: {e}", csv.display()));
    }
    warehouse
}

/// Every table on one data system, and the CSV behind it.
///
/// The data directory is a parameter rather than [`derived`]'s, because this file now derives two
/// corpora: the one every question is answered over, and the one whose `many_to_one` is violated.
pub(super) fn tables_on(
    data: &Path,
    name: &SourceName,
    pinned: &PinnedDefinitions,
) -> Vec<(sutura_domain::model::TableName, PathBuf)> {
    pinned
        .definitions()
        .models()
        .values()
        .filter(|model| model.source() == name)
        .map(|model| {
            let table = model.table_name().clone();
            let csv = data.join(format!("{table}.csv"));
            (table, csv)
        })
        .collect()
}

/// One answer, computed through the whole service path.
///
/// The combiner is a parameter rather than built here, because `clippy::unwrap_in_result` is denied
/// in a `Result`-returning body and a combiner's construction is fallible. Its caller holds one for
/// the length of a comparison, which is also the honest shape: a deployment builds one.
pub(super) fn answered<W>(
    side: &Side<W>,
    query: &Query,
    name: &str,
    combiner: &sutura_exec_datafusion::DataFusionCombiner,
) -> Result<ToolOutcome, String>
where
    W: sutura_domain::warehouse::Warehouse + Sync,
    W::Error: Send,
{
    match sutura_app::answer(
        &side.bundle,
        query,
        &a_caller(),
        &shared_credential(),
        &side.warehouses,
        combiner,
        BUDGET,
        deadline(),
        &sutura_app::SpendLedger::no_budget(),
        RowCeiling::DEFAULT,
    ) {
        Ok(answered) => Ok(answered.into_outcome()),
        Err(error) => Err(chain(&error, name)),
    }
}

/// An error and every cause beneath it, as one string.
///
/// `Display` on a `thiserror` enum prints the outermost message and stops, and the outermost one
/// here is true of an outage and of a non-finite cell alike. What tells them apart is one level
/// down.
fn chain(error: &dyn core::error::Error, name: &str) -> String {
    let mut out = format!("{name}: {error}");
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

// ----------------------------------------------------------------------------- the instrument ---

/// **The differential.** Every question the two-source bundle splits, answered both ways.
///
/// The classifier is the two compiles, not a list: a question is a two-source question when the
/// one-source bundle plans a whole answer for it and the two-source bundle does something else.
/// That is what keeps this file from asserting over a hand-maintained set of question names -
/// adding a customer-attribute question to the shared corpus enrols it here.
///
/// **Generic in the two-source side's adapter, and that is what makes the second pass a second
/// MEASUREMENT rather than a second copy of this body.** Both callers compare against the same
/// one-`DataFusion` side and are held to the same [`MUST_BE_REACHED`] table, so a case that stopped
/// being a two-source question is a coverage loss on both passes at once. `topology` names the side
/// under test in every failure, because otherwise a red run says which question and not which
/// adapter held its legs.
pub(super) fn differential<W>(one: &Side<sutura_exec_datafusion::DataFusionWarehouse>, two: &Side<W>, topology: &str)
where
    W: sutura_domain::warehouse::Warehouse + Sync,
    W::Error: Send,
{
    let mut reached: Vec<(String, Reached)> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    for (name, query) in every_question() {
        let Split::Yes(federated) = split_or_not(&name, &query, one.bundle.get(), two.bundle.get()) else {
            continue;
        };
        let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
        let from_one = answered(one, &query, &name, &combiner);
        let from_two = answered(two, &query, &name, &combiner);
        let outcome = match federated {
            Federated::Split => match (from_one, from_two) {
                (
                    Ok(ToolOutcome::Answer {
                        rows: ref here,
                        provenance: ref one_ran_as,
                    }),
                    Ok(ToolOutcome::Answer {
                        rows: ref there,
                        provenance: ref two_ran_as,
                    }),
                ) => {
                    found.extend(disagreements_between(&name, here, there));
                    found.extend(recorded_identities(&name, one_ran_as, two_ran_as));
                    Reached::Agreed
                }
                (Err(ref here), Err(ref there)) => {
                    found.extend(failed_together(&name, here, there));
                    Reached::FailedTogether
                }
                // D19 + A4: the mono path's zero-denominator-fails case still leaves the driver's
                // own "inf is not a finite number" as an `Err` - a gap this PR does not close,
                // named rather than left for the fallback arm below to call a divergence. The
                // federated combiner's own version of the same failure is a `RefusalReason` now,
                // so this is the ONE shape this corpus case is allowed to reach: not agreement, and
                // not failing together, but the federated side correctly refusing what the mono
                // side still errors on.
                (
                    Err(_),
                    Ok(ToolOutcome::Refusal {
                        reason:
                            RefusalReason::FederatedAnswerNotWellFormed {
                                federated: sutura_domain::plan::FederatedAnswerRefusal::NonFinite,
                            },
                    }),
                ) => Reached::FederatedRefusedWhatMonoFailed,
                (here, there) => {
                    found.push(format!(
                        "{name} on {topology}: one side answered and the other did not\n  one source: {here:?}\n  two sources: {there:?}"
                    ));
                    Reached::Diverged
                }
            },
            Federated::Refused(reason) => {
                assert!(
                    matches!(from_one, Ok(ToolOutcome::Answer { .. })),
                    "{name} on {topology}: the one-source deployment must answer what the two-source \
                     one refuses, not {from_one:?}"
                );
                assert!(
                    matches!(
                        reason,
                        RefusalReason::MeasureDoesNotFederate { .. } | RefusalReason::MultiMetricFederationNotExecutable { .. }
                    ),
                    "{name}: a two-source question this corpus refuses must say why, not {reason:?}"
                );
                Reached::RefusedAsUnfederatable
            }
        };
        reached.push((name, outcome));
    }
    // **Every disagreement, then one panic - never the first one and out.** The loop used to panic
    // where the comparison is made, which made "this change reddens exactly one case" a property of
    // the fail-fast rather than a finding about the change: review re-ran the three mutations with
    // the panics turned into prints and F6's null ordering reddened TEN of the twelve compared
    // cases, not one. A regression's REACH is the more useful half of the diagnosis, and a
    // comparator that stops at the first row cannot report it.
    assert!(
        found.is_empty(),
        "on {topology}, {} of {} two-source question(s) disagreed with their one-source answer:\n\n{}",
        found.len(),
        reached.len(),
        found.join("\n\n")
    );
    for &(case, wanted) in MUST_BE_REACHED {
        let found = reached.iter().find(|&(name, _)| name == case);
        match found {
            Some(&(_, got)) => assert!(
                got == wanted,
                "{case} on {topology} reached {got:?} rather than {wanted:?}, so what F5 asks that \
                 case to cover is no longer covered by it"
            ),
            None => panic!("{case} is no longer a two-source question, so this file no longer covers it"),
        }
    }
    // Every arm of the match above, reached by something. An arm no question reaches is an arm that
    // could be replaced by a panic and stay green. `FailedTogether` is deliberately not in this
    // list, for `Diverged`'s own reason below: D19 + A4 moved this corpus's one prior occupant to
    // `FederatedRefusedWhatMonoFailed`, and the arm stays as a defensive shape for a future case
    // that fails on both sides for an unrelated cause.
    for wanted in [
        Reached::Agreed,
        Reached::RefusedAsUnfederatable,
        Reached::FederatedRefusedWhatMonoFailed,
    ] {
        assert!(
            reached.iter().any(|&(_, got)| got == wanted),
            "on {topology}, no two-source question reached {wanted:?}, so that arm proved nothing"
        );
    }
}

/// What one two-source question did, as the thing [`MUST_BE_REACHED`] pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reached {
    /// Both topologies answered, and the answers agree in content and in order.
    Agreed,
    /// Both topologies failed the same way.
    ///
    /// **Not in the arm-coverage loop below**, for the same reason [`Self::Diverged`] is not:
    /// `zero_denominator: fails` was the one corpus case that reached it, and D19 + A4 moved that
    /// case to [`Self::FederatedRefusedWhatMonoFailed`] because the federated side now refuses
    /// rather than erroring. Kept as a defensive shape for a future case that fails on both sides
    /// for an unrelated reason, rather than folding into [`Self::Diverged`] and losing the
    /// distinction between "both sides agree something is wrong" and "the two sides disagree".
    FailedTogether,
    /// The two-source topology refused a measure it cannot re-aggregate.
    RefusedAsUnfederatable,
    /// D19 + A4: the federated combiner refused a non-finite ratio as
    /// `RefusalReason::FederatedAnswerNotWellFormed` where the mono path still leaves the
    /// driver's own `Err` for the same `zero_denominator: fails` case. Not
    /// [`Self::FailedTogether`], because the two sides no longer fail the same way; not
    /// [`Self::Diverged`], because this ONE divergence is the fix this PR makes rather than a
    /// defect it would otherwise report.
    FederatedRefusedWhatMonoFailed,
    /// The two topologies did not agree on whether there is an answer at all.
    ///
    /// Not in [`MUST_BE_REACHED`] and not in the arm-coverage loop, because on a tree where the
    /// federated path is correct nothing reaches it: it is the shape a defect takes, and it is a
    /// variant rather than a panic so that the collected report can carry it beside the others.
    Diverged,
}

/// **The cases `telekom/sutura#325`'s F5 asks for, and which arm each has to reach.**
///
/// A hand-written list, deliberately, and the only one in this file. The classifier above is
/// mechanical so a NEW customer-attribute question enrols itself; this is the other direction - a
/// case that stopped being a two-source question, or that started merely refusing where it used to
/// be compared, is a coverage loss no count would show. Without the classifier the file asserts
/// over a list; without the list it could compare twelve of the wrong questions.
const MUST_BE_REACHED: &[(&str, Reached)] = &[
    // F1: a null fact join key beside an unmatched non-null one, retained under LEFT with its
    // measure intact.
    ("two-source-a-null-key-and-an-orphan-key", Reached::Agreed),
    // F2: a legal dimension named after the remote join target.
    ("two-source-a-dimension-named-like-the-link", Reached::Agreed),
    // F6 and the orphan key: the shared corpus's own unmatched customer, whose null group the mono
    // path orders LAST.
    ("recurring-revenue-by-region", Reached::Agreed),
    // A remote filter, which is what makes the join INNER rather than LEFT.
    ("recurring-revenue-annual-in-north", Reached::Agreed),
    // A remote filter and a remote grouping key together.
    ("recurring-revenue-business-only", Reached::Agreed),
    // A same-source join on the fact leg beside the remote one.
    ("recurring-revenue-by-region-and-family", Reached::Agreed),
    // The same shape with an UNMATCHED row in that same-source join, which is the only case in this
    // corpus that observes the leg's own join kind - see `corpus::DATA_CASES`.
    ("two-source-a-same-source-orphan-beside-a-remote-one", Reached::Agreed),
    // Six buckets and two keys, which is where a key-then-bucket ordering could disagree.
    ("subscription-months-by-region-and-term", Reached::Agreed),
    // The whole reduction table above the legs.
    ("two-source-an-average-decomposed-above-the-legs", Reached::Agreed),
    ("two-source-a-maximum-re-taken-above-the-legs", Reached::Agreed),
    ("two-source-a-minimum-re-taken-above-the-legs", Reached::Agreed),
    // A zero denominator in one subgroup, both ways round.
    ("two-source-a-zero-denominator-in-one-subgroup", Reached::Agreed),
    (
        "two-source-a-zero-denominator-that-fails",
        Reached::FederatedRefusedWhatMonoFailed,
    ),
    // A distinct value spanning join keys, which is separate feature work rather than a defect.
    (
        "two-source-a-distinct-value-spanning-join-keys",
        Reached::RefusedAsUnfederatable,
    ),
    // `github.com/telekom/sutura#777`'s case 2 - a federated `top` ranks above the combine.
    ("two-source-a-case-2-combine-then-rank-top", Reached::Agreed),
];

/// What the two answers disagree about: nothing, the content, or the order.
///
/// Content first and order second, deliberately: the first symptom of a wrong number would otherwise
/// be reported as a sort order. Both are reported when both hold, because they are two claims.
fn disagreements_between(name: &str, one_source: &RowSet, two_sources: &RowSet) -> Vec<String> {
    let mut found = Vec::new();
    if let Err(disagreement) = agree_on_content(one_source, two_sources, RealTolerance::DIFFERENTIAL) {
        found.push(format!(
            "{name}: one source and two sources returned different rows - {disagreement}"
        ));
    }
    if let Err(disagreement) = agree_on_order(one_source, two_sources, RealTolerance::DIFFERENTIAL) {
        found.push(format!(
            "{name}: one source and two sources returned the same rows in different orders, and the \
             plan's ORDER BY claims one - {disagreement}"
        ));
    }
    found
}

/// **What each answer recorded itself as having run as**, which is the guarantee only this file can
/// measure over two real legs.
///
/// `sutura_app::federated` merges the legs' identities through `ExecutedAs::and`, and
/// `.agents/skills/sutura/query-surface/SKILL.md`'s *second execution leg* row states the promise as
/// each leg running under that source's acknowledged shared identity **and the answer recording
/// which**. Until this file existed no two-leg answer came out of a real execution, so the recording
/// had never been read off one - and `into_outcome()` used to discard it here one line before
/// anything could look. What the postures make measured rather than stated is this file's own
/// limit: both legs are `SharedServiceUser`, so neither ran as the asker.
fn recorded_identities(name: &str, one_ran_as: &Provenance, two_ran_as: &Provenance) -> Vec<String> {
    let mut found = Vec::new();
    let expected = posture();
    let legs = |provenance: &Provenance| -> Vec<(String, bool)> {
        provenance
            .executed_as()
            .legs()
            .map(|(source, posture)| (String::from(source.as_str()), *posture == expected))
            .collect()
    };
    let own = String::from(source().as_str());
    let here = legs(one_ran_as);
    if here != vec![(own.clone(), true)] {
        found.push(format!(
            "{name}: a whole answer from one data system records that one leg and the posture it was \
             opened with, not {here:?}"
        ));
    }
    // Source order, because `ExecutedAs` keeps its legs in a `BTreeMap` - so `geo` before `local`
    // is the record's own order rather than the order the legs ran in.
    let there = legs(two_ran_as);
    if there != vec![(String::from(LOOKUP_SOURCE), true), (own, true)] {
        found.push(format!(
            "{name}: a two-source answer records BOTH legs and the posture each source was opened \
             with, not {there:?}"
        ));
    }
    found
}

/// Both sides failed, and this asserts they failed for the SAME reason.
///
/// `zero_denominator: fails` is the one case in this corpus where a supported question has no
/// figure, and the two sides reach it from opposite directions: the one-source side divides in
/// the engine and the port refuses to carry a non-finite cell, while the two-source side divides
/// above the legs and the combiner refuses. Both must name the METRIC, which is what makes each
/// message evidence about this definition rather than about some failure during this question:
/// `chain` prepends the QUESTION's name, so without the metric a failure raised while computing
/// something else would have passed this cell unchanged. Review found that gap.
fn failed_together(name: &str, one_source: &str, two_sources: &str) -> Vec<String> {
    let metric = "revenue_per_churned_subscription";
    let mut found = Vec::new();
    if !(one_source.contains("is not a finite number") && one_source.contains(metric)) {
        found.push(format!(
            "{name}: the one-source side must refuse a non-finite cell for `{metric}`, not:\n{one_source}"
        ));
    }
    if !(two_sources.contains("could not be assembled") && two_sources.contains("finite") && two_sources.contains(metric)) {
        found.push(format!(
            "{name}: the two-source side must fail to assemble a non-finite `{metric}`, not:\n{two_sources}"
        ));
    }
    found
}

enum Federated {
    Split,
    Refused(RefusalReason),
}

enum Split {
    Yes(Federated),
    No,
}

/// Whether this question is a two-source question, decided by compiling it against both bundles.
fn split_or_not(name: &str, query: &Query, one: &PinnedDefinitions, two: &PinnedDefinitions) -> Split {
    let here = compile(query, &ScopedView::everything(one), RowCeiling::DEFAULT)
        .unwrap_or_else(|e| panic!("{name} does not compile on one source: {e}"));
    let there = compile(query, &ScopedView::everything(two), RowCeiling::DEFAULT)
        .unwrap_or_else(|e| panic!("{name} does not compile on two sources: {e}"));
    match (here, there) {
        (Compiled::Planned { .. }, Compiled::Federated { .. }) => Split::Yes(Federated::Split),
        (Compiled::Planned { .. }, Compiled::Refused { reason }) => Split::Yes(Federated::Refused(reason)),
        (Compiled::Refused { reason: ref one_reason }, Compiled::Refused { reason: ref two_reason }) => {
            assert_eq!(
                format!("{one_reason:?}"),
                format!("{two_reason:?}"),
                "{name}: a compile-side refusal must not depend on where a model sits"
            );
            Split::No
        }
        (Compiled::Planned { .. }, Compiled::Planned { .. }) => Split::No,
        (here, there) => panic!("{name}: the one-source bundle did not plan a whole answer\n  one: {here:?}\n  two: {there:?}"),
    }
}

/// **Two `DuckDB` databases hold the legs.** The only executed evidence that the RENDERER's leg path
/// (`sutura_sql::generate_leg`) answers, and the only pass whose rows cross that adapter's own
/// Arrow-to-domain mapping. A development dependency, so what it measures is the implemented
/// federation path rather than a deployment's answer.
#[test]
fn a_two_source_answer_is_the_same_answer_as_one_source() {
    let derived = derived();
    let one = one_source(bundle(&derived.one_source));
    let two = two_sources(bundle(&derived.two_source));
    differential(&one, &two, "two duckdb databases");
}
