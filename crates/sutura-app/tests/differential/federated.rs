//! **The whole two-source path, executed:** the real splitter, two real `DuckDB` executions and the
//! combiner, compared against the same corpus answered by one `DataFusion`.
//!
//! `tests/differential.rs` compares the engine against a data source for a MONO question, and
//! `sutura_app::federated`'s own tests drive `answer_federated` over fakes that return literal rows.
//! The leg goldens render hand-built plans. **Nothing joined the splitter, two executions and the
//! combiner**, so every arithmetic claim about a two-source answer rested on rows a test wrote down.
//! `telekom/sutura#325`'s F5 is that gap, and closed #72 deferred it without an owner.
//!
//! # The instrument, and why it is one line wide
//!
//! One corpus, derived from `examples/single-player` into cargo's own target temp directory, and
//! **two catalogs over it that differ in exactly one line** - whether the `customers` model sits on
//! the data system the metric's own model does. On the one-source catalog every question is a
//! whole-answer plan; on the two-source catalog every question that reaches a customer attribute is
//! split. The DATA both sides read is the same directory, so a disagreement cannot be a fixture.
//! [`the_two_catalogs_differ_in_one_document`] holds the width of the difference, because an
//! instrument whose two sides drifted apart would report a corpus edit as a federation defect.
//!
//! **The shared corpus is derived rather than edited**, and that is a scope decision: a
//! second-source topology is one deployment's, not something a single-source quickstart can say.
//! [`CATALOG_CASES`] and [`DATA_CASES`] carry each derivation with its reason.
//!
//! # What is compared, and what the comparison is
//!
//! `sutura_domain::warehouse::agreement` - the one typed policy `tests/differential.rs` and the
//! `BigQuery` acceptance leg also call. Content first (a multiset, per-variant, tolerance on
//! `Value::Real` alone), then order, because a plan that emits `ORDER BY` claims an order and
//! `telekom/sutura#325`'s F6 was the combiner ranking a null group FIRST where the mono path puts it
//! LAST. **Every disagreement is collected and reported together**, never the first one and out:
//! review measured that F6's single un-fix reddens ten of the twelve compared cases, so a
//! fail-fast comparator would have described one probe as one case's worth of coverage.
//!
//! Each answer's `Provenance` is read too, so *the answer records which identity each leg ran as*
//! is measured here rather than stated - see [`recorded_identities`], which is also what makes the
//! limit below a measurement.
//!
//! # What this does NOT establish
//!
//! **No published artifact can run either side of it.** Both legs execute on `DuckDB`, which is a
//! development dependency and the only adapter here declaring `Warehouse::EXECUTES_LEGS`; a shipped
//! binary refuses every two-source question as `FederationNotExecutable` before minting anything.
//! So what is measured is the implemented federation path, not a deployment's answer - and both
//! legs record `SharedServiceUser`, so neither ran as the asker and nothing here says two subjects
//! get different rows.

use std::path::{Path, PathBuf};

use sutura_app::Validated;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{PinnedDefinitions, Provenance, SemanticCatalog as _};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_domain::warehouse::{RowSet, Value};
use sutura_semantic::{Compiled, compile};

use crate::adapters::{a_caller, posture, shared_credential, source, version};
// `#[path]` for the reason `tests/golden.rs` gives, one level down: a bare `mod corpus;` in a
// submodule of a test target resolves beside the target root, not beside this file.
#[path = "federated/corpus.rs"]
mod corpus;

use corpus::{LOOKUP_SOURCE, derived, derived_question, every_question, lookup_source};

/// An amount of working set no question in this corpus comes near, so only a defect refuses.
///
/// A gibibyte, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` - a literal for the
/// reason `adapters::DataSystemUnderTest for DataFusionWarehouse` gives.
const BUDGET: u64 = 1 << 30;

// ------------------------------------------------------------------------- opening the sides ---

/// One bundle, loaded through the real markdown adapter over a derived catalog.
fn bundle(catalog: &Path) -> PinnedDefinitions {
    sutura_catalog_local::LocalCatalog::new(source(), catalog.to_path_buf(), version())
        .load()
        .unwrap_or_else(|e| panic!("the derived catalog at {} does not load: {e}", catalog.display()))
}

/// The one-source side: the ENGINE, over every table the bundle names.
///
/// The bundle and the registry come back together because a `Validated` bundle is only obtainable
/// by re-executing the anchors against the registry that will answer with it - which is what
/// makes "this side reproduces its own certified numbers" a precondition of the comparison rather
/// than a separate test.
fn one_source(pinned: PinnedDefinitions) -> Side<sutura_exec_datafusion::DataFusionWarehouse> {
    let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
    let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
        source(),
        posture(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("an in-process engine starts");
    for (table, csv) in tables_on(&source(), &pinned) {
        engine
            .attach_csv(&table, &csv)
            .unwrap_or_else(|e| panic!("the engine could not attach {}: {e}", csv.display()));
    }
    let warehouses = sutura_app::Warehouses::of(engine);
    let bundle = sutura_app::verify_and_validate(pinned, &warehouses).expect("the anchors hold on one source");
    Side { bundle, warehouses }
}

/// The two-source side: one `DuckDB` per source, each holding only its own tables.
///
/// Two databases rather than one with everything attached, and that is the point of the leg
/// split: neither statement CAN reach the other side's table, so a join across them has to
/// happen above the port or not at all.
fn two_sources(pinned: PinnedDefinitions) -> Side<sutura_exec_duckdb::DuckDbWarehouse> {
    let warehouses = sutura_app::Warehouses::of(duckdb_on(&source(), &pinned))
        .and(duckdb_on(&lookup_source(), &pinned))
        .expect("two sources, one registry");
    let bundle = sutura_app::verify_and_validate(pinned, &warehouses).expect("the anchors hold on two sources");
    Side { bundle, warehouses }
}

/// One side of the differential: a bundle whose anchors it reproduced, and what answers it.
struct Side<W> {
    bundle: Validated<PinnedDefinitions>,
    warehouses: sutura_app::Warehouses<W>,
}

fn duckdb_on(name: &SourceName, pinned: &PinnedDefinitions) -> sutura_exec_duckdb::DuckDbWarehouse {
    let warehouse = sutura_exec_duckdb::DuckDbWarehouse::in_memory(name.clone(), posture()).expect("an in-memory database opens");
    let attached = tables_on(name, pinned);
    assert!(!attached.is_empty(), "no model in the derived bundle sits on {name}");
    for (table, csv) in attached {
        warehouse
            .attach_csv(&table, &csv)
            .unwrap_or_else(|e| panic!("duckdb could not attach {}: {e}", csv.display()));
    }
    warehouse
}

/// Every table on one data system, and the CSV behind it.
fn tables_on(name: &SourceName, pinned: &PinnedDefinitions) -> Vec<(sutura_domain::model::TableName, PathBuf)> {
    pinned
        .definitions()
        .models()
        .values()
        .filter(|model| model.source() == name)
        .map(|model| {
            let table = model.table_name().clone();
            let csv = derived().data.join(format!("{table}.csv"));
            (table, csv)
        })
        .collect()
}

/// One answer, computed through the whole service path.
fn answered<W>(side: &Side<W>, query: &Query, name: &str) -> Result<ToolOutcome, String>
where
    W: sutura_domain::warehouse::Warehouse,
{
    match sutura_app::answer(
        &side.bundle,
        query,
        &a_caller(),
        &shared_credential(),
        &side.warehouses,
        BUDGET,
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
#[test]
fn a_two_source_answer_is_the_same_answer_as_one_source() {
    let derived = derived();
    let one = one_source(bundle(&derived.one_source));
    let two = two_sources(bundle(&derived.two_source));

    let mut reached: Vec<(String, Reached)> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    for (name, query) in every_question() {
        let Split::Yes(federated) = split_or_not(&name, &query, one.bundle.get(), two.bundle.get()) else {
            continue;
        };
        let from_one = answered(&one, &query, &name);
        let from_two = answered(&two, &query, &name);
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
                (here, there) => {
                    found.push(format!(
                        "{name}: one side answered and the other did not\n  one source: {here:?}\n  two sources: {there:?}"
                    ));
                    Reached::Diverged
                }
            },
            Federated::Refused(reason) => {
                assert!(
                    matches!(from_one, Ok(ToolOutcome::Answer { .. })),
                    "{name}: the one-source deployment must answer what the two-source one refuses, not {from_one:?}"
                );
                assert!(
                    matches!(reason, RefusalReason::MeasureDoesNotFederate { .. }),
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
        "{} of {} two-source question(s) disagreed with their one-source answer:\n\n{}",
        found.len(),
        reached.len(),
        found.join("\n\n")
    );
    for &(case, wanted) in MUST_BE_REACHED {
        let found = reached.iter().find(|&(name, _)| name == case);
        match found {
            Some(&(_, got)) => assert!(
                got == wanted,
                "{case} reached {got:?} rather than {wanted:?}, so what F5 asks that case to cover \
                 is no longer covered by it"
            ),
            None => panic!("{case} is no longer a two-source question, so this file no longer covers it"),
        }
    }
    // Every arm of the match above, reached by something. An arm no question reaches is an arm that
    // could be replaced by a panic and stay green.
    for wanted in [Reached::Agreed, Reached::FailedTogether, Reached::RefusedAsUnfederatable] {
        assert!(
            reached.iter().any(|&(_, got)| got == wanted),
            "no two-source question reached {wanted:?}, so that arm proved nothing"
        );
    }
}

/// What one two-source question did, as the thing [`MUST_BE_REACHED`] pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reached {
    /// Both topologies answered, and the answers agree in content and in order.
    Agreed,
    /// Both topologies failed, which `zero_denominator: fails` is the one supported way to.
    FailedTogether,
    /// The two-source topology refused a measure it cannot re-aggregate.
    RefusedAsUnfederatable,
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
    // Six buckets and two keys, which is where a key-then-bucket ordering could disagree.
    ("subscription-months-by-region-and-term", Reached::Agreed),
    // The whole reduction table above the legs.
    ("two-source-an-average-decomposed-above-the-legs", Reached::Agreed),
    ("two-source-a-maximum-re-taken-above-the-legs", Reached::Agreed),
    ("two-source-a-minimum-re-taken-above-the-legs", Reached::Agreed),
    // A zero denominator in one subgroup, both ways round.
    ("two-source-a-zero-denominator-in-one-subgroup", Reached::Agreed),
    ("two-source-a-zero-denominator-that-fails", Reached::FailedTogether),
    // A distinct value spanning join keys, which is separate feature work rather than a defect.
    (
        "two-source-a-distinct-value-spanning-join-keys",
        Reached::RefusedAsUnfederatable,
    ),
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

/// **A zero denominator in ONE subgroup, and the neighbours it must not reach.**
///
/// The differential above proves the two sides AGREE on this answer; without this, the agreement
/// could be over an answer with no null in it at all. The divide happens above both legs - a guard
/// applied inside one would be a wrong number rather than a refusal - so what is asserted is that
/// the group whose denominator summed to zero is null while the group beside it is a figure.
#[test]
fn a_subgroup_with_no_denominator_is_null_and_its_neighbours_are_not() {
    let two = two_sources(bundle(&derived().two_source));
    let name = "two-source-a-zero-denominator-in-one-subgroup";
    let query = derived_question(name);
    let outcome = answered(&two, &query, name).unwrap_or_else(|e| panic!("{e}"));
    let ToolOutcome::Answer { ref rows, .. } = outcome else {
        panic!("{name}: a supported two-source question is answered, not {outcome:?}");
    };
    let measure = rows.columns().len().saturating_sub(1);
    let cells: Vec<&Value> = rows.rows().iter().filter_map(|row| row.get(measure)).collect();
    assert!(
        cells.iter().any(|cell| matches!(**cell, Value::Null)),
        "{name}: no subgroup had a zero denominator, so the guard above the legs never ran: {rows:?}"
    );
    assert!(
        cells.iter().any(|cell| !matches!(**cell, Value::Null)),
        "{name}: every subgroup was null, so this says nothing about a zero reaching its neighbours: {rows:?}"
    );
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
    let here = compile(query, one).unwrap_or_else(|e| panic!("{name} does not compile on one source: {e}"));
    let there = compile(query, two).unwrap_or_else(|e| panic!("{name} does not compile on two sources: {e}"));
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

/// **Which registered data systems can run a leg, expanded over the registry itself.**
///
/// This file's two-source side names `DuckDB` twice because it is the only registered adapter
/// declaring [`Warehouse::EXECUTES_LEGS`], and an entry that cannot run a leg cannot be either half
/// of a federated answer. A cell rather than a sentence, so registering a second leg-executing
/// adapter REDDENS here and the diff that enrols it arrives beside the registration.
/// `sutura-conformance`'s binding holds the per-adapter agreement between the tag and the constant;
/// what this holds is the COUNT.
///
/// [`Warehouse::EXECUTES_LEGS`]: sutura_domain::warehouse::Warehouse::EXECUTES_LEGS
macro_rules! leg_capability {
    ($name:ident, $adapter:ty) => {
        mod $name {
            use crate::adapters::DataSystemUnderTest;
            use sutura_domain::warehouse::Warehouse;

            #[test]
            fn whether_it_can_run_a_leg_is_what_this_differential_can_use_it_for() {
                let name = <$adapter as DataSystemUnderTest>::NAME;
                assert_eq!(
                    <$adapter as Warehouse>::EXECUTES_LEGS,
                    name == "duckdb",
                    "{name} changed its leg capability; \
                     crates/sutura-app/tests/differential/federated.rs is where a second \
                     leg-executing adapter gets enrolled in the two-source differential"
                );
            }
        }
    };
}

crate::adapters::registered!(data_systems: leg_capability);
