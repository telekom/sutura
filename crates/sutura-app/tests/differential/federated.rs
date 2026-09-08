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
//! # Two two-source sides, and neither one is redundant
//!
//! [`a_two_source_answer_is_the_same_answer_as_one_source`] runs the legs on two `DuckDB`
//! databases; [`two_engines_answer_what_one_engine_answers`] runs them on two instances of the
//! ENGINE, which is the adapter a release links. Both compare against the same one-`DataFusion`
//! side through [`differential`], so what differs between them is only which adapter holds the
//! legs.
//!
//! **Why the `DuckDB` side is kept, corrected.** The reason first written here was that with the
//! engine on both sides a bug shared between the leg and whole-plan translations would CANCEL, so
//! only the `DuckDB` pass could catch that class. **That was wrong, and review measured it:** a
//! shared defect reddens the engine pass too, because `verify_and_validate` re-executes each
//! anchor against the registry before any comparison happens and an anchor is a literal DECLARED in
//! the catalog - `value: 202121` in `metrics/recurring_revenue.md` - which no code under test
//! produced. **The anchor, not the second engine, is what catches a bug shared between the two
//! translations.**
//!
//! What the `DuckDB` pass actually adds is a different IMPLEMENTATION rather than a stronger oracle:
//! its legs are rendered SQL through `sutura_sql::generate_leg`, so it is the only executed evidence
//! that the renderer's leg path answers at all, and its rows cross that adapter's own Arrow-to-domain
//! mapping. Deleting it would delete both. The engine pass is the half that says a PUBLISHED
//! artefact answers.
//!
//! # What this does NOT establish
//!
//! **Two sources are not two identities, and no side of this measures leg 2.** Every leg here runs
//! under one operating-system identity: `DataFusionWarehouse::IMPERSONATION` is
//! `NoPlaceForASubject`, both sources are opened `shared-service-user`, and
//! [`recorded_identities`] reads that back off each answer rather than stating it. So this is
//! single-player federation - nothing here says two subjects get different rows.
//!
//! **The `DuckDB` side still runs on a development dependency**, so that pass measures the
//! implemented federation path rather than a deployment's answer. What changed is the engine side:
//! it declares `Warehouse::EXECUTES_LEGS`, and it is non-optional in both shipped binaries.
//!
//! **Neither side is the served deployment.** These sides open adapters in-process from a bundle
//! this file loads; `crates/sutura-serve/tests/served.rs` is the only place a two-source question
//! reaches the composed binary over HTTP.

use std::path::{Path, PathBuf};

use sutura_app::Validated;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{NotValidated, PinnedDefinitions, Provenance, SemanticCatalog as _};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_domain::warehouse::{RowSet, Value};
use sutura_semantic::{Compiled, compile};

use crate::adapters::{a_caller, posture, shared_credential, source, version};
// `#[path]` for the reason `tests/golden.rs` gives, one level down: a bare `mod corpus;` in a
// submodule of a test target resolves beside the target root, not beside this file.
#[path = "federated/corpus.rs"]
mod corpus;

use corpus::{
    A_DUPLICATED_KEY, LOOKUP_SOURCE, NULL_DIMENSION_KEYS, derived, derived_question, every_question, lookup_source, violated,
    with_null_keys,
};

/// An amount of working set no question in this corpus comes near, so only a defect refuses.
///
/// A gibibyte, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` - a literal for the
/// reason `adapters::DataSystemUnderTest for DataFusionWarehouse` gives.
const BUDGET: u64 = 1 << 30;

// ------------------------------------------------------------------------- opening the sides ---

/// A bundle that may not have validated, beside the registry that judged it.
///
/// A named pair rather than an inline tuple, which the complexity threshold in `clippy.toml` catches
/// and is right to: the two halves are *what the boot path decided* and *what it asked*, and a bare
/// two-element tuple says which is which nowhere.
type Attempted<W> = (Result<Validated<PinnedDefinitions>, NotValidated>, sutura_app::Warehouses<W>);

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
///
/// **That precondition is this file's THIRD-PARTY oracle, and it is why the comparison is not
/// circular.** An anchor's expected figure is a literal declared in the catalog
/// (`metrics/recurring_revenue.md`'s `value: 202121`), not a number recorded from a run - so a
/// defect shared by every side of the comparison still reddens here, before any two answers are
/// compared. Review measured that: a mutation to a shared expression builder fails at this call.
fn one_source(pinned: PinnedDefinitions) -> Side<sutura_exec_datafusion::DataFusionWarehouse> {
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
fn validating_on_one_source(data: &Path, pinned: PinnedDefinitions) -> Attempted<sutura_exec_datafusion::DataFusionWarehouse> {
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
fn two_sources(pinned: PinnedDefinitions) -> Side<sutura_exec_duckdb::DuckDbWarehouse> {
    let (bundle, warehouses) = validating_on_two_sources(&derived().data, pinned);
    Side {
        bundle: bundle.expect("the anchors and the declarations hold on two sources"),
        warehouses,
    }
}

/// **The two-source side a release can actually run: one ENGINE per source.**
///
/// The same topology [`two_sources`] builds, with the adapter swapped for the one both shipped
/// binaries link. Each engine attaches only its own source's tables, so neither can reach the
/// other's - the isolation is which tables were registered, not which directory they came from,
/// which is exactly what `sutura-serve`'s `open_files` does per declared `files` entry.
fn two_engines(pinned: PinnedDefinitions) -> Side<sutura_exec_datafusion::DataFusionWarehouse> {
    let (bundle, warehouses) = validating_on_two_engines(&derived().data, pinned);
    Side {
        bundle: bundle.expect("the anchors and the declarations hold on two engines"),
        warehouses,
    }
}

/// The two-engine registry, and whatever the bundle validated to. [`validating_on_two_sources`]'s
/// twin, split out for its reason: the violated corpus reads the `Err` this one unwraps, and on this
/// topology that refusal is the one a RELEASE would give.
fn validating_on_two_engines(data: &Path, pinned: PinnedDefinitions) -> Attempted<sutura_exec_datafusion::DataFusionWarehouse> {
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
fn engine_on(data: &Path, name: &SourceName, pinned: &PinnedDefinitions) -> sutura_exec_datafusion::DataFusionWarehouse {
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
fn validating_on_two_sources(data: &Path, pinned: PinnedDefinitions) -> Attempted<sutura_exec_duckdb::DuckDbWarehouse> {
    let warehouses = sutura_app::Warehouses::of(duckdb_on(data, &source(), &pinned))
        .and(duckdb_on(data, &lookup_source(), &pinned))
        .expect("two sources, one registry");
    let validated = sutura_app::verify_and_validate(pinned, &warehouses);
    (validated, warehouses)
}

/// One side of the differential: a bundle whose anchors it reproduced, and what answers it.
struct Side<W> {
    bundle: Validated<PinnedDefinitions>,
    warehouses: sutura_app::Warehouses<W>,
}

fn duckdb_on(data: &Path, name: &SourceName, pinned: &PinnedDefinitions) -> sutura_exec_duckdb::DuckDbWarehouse {
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
fn tables_on(data: &Path, name: &SourceName, pinned: &PinnedDefinitions) -> Vec<(sutura_domain::model::TableName, PathBuf)> {
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

/// **Two instances of the ENGINE hold the legs, which is the first two-source side a release can
/// run.**
///
/// `DataFusionWarehouse` is non-optional in both shipped binaries and declares
/// `Warehouse::EXECUTES_LEGS`, so this is the shipped adapter type on BOTH sides of the comparison
/// for the first time - one instance answering whole, two answering as legs, over one derived
/// corpus.
///
/// **It does not replace the `DuckDB` pass**, for the reason this file's header gives - which is not
/// the reason first written there: a bug shared between `crate::leg`'s translation and the whole-plan
/// one does NOT go unseen here, because the anchor re-execution below compares against a literal the
/// catalog declares. What the `DuckDB` pass adds is the renderer's leg path and a second
/// Arrow-to-domain mapping.
///
/// **The engine emits no SQL, so there is no golden that can see this path** - `tests/golden/legs.rs`
/// pins rendered leg statements per dialect and the engine renders none. This cell and
/// `sutura-exec-datafusion`'s conformance binding are the whole of its evidence.
#[test]
fn two_engines_answer_what_one_engine_answers() {
    let derived = derived();
    let one = one_source(bundle(&derived.one_source));
    let two = two_engines(bundle(&derived.two_source));
    differential(&one, &two, "two in-process engines");
}

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
fn differential<W>(one: &Side<sutura_exec_datafusion::DataFusionWarehouse>, two: &Side<W>, topology: &str)
where
    W: sutura_domain::warehouse::Warehouse,
{
    let mut reached: Vec<(String, Reached)> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    for (name, query) in every_question() {
        let Split::Yes(federated) = split_or_not(&name, &query, one.bundle.get(), two.bundle.get()) else {
            continue;
        };
        let from_one = answered(one, &query, &name);
        let from_two = answered(two, &query, &name);
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
    // could be replaced by a panic and stay green.
    for wanted in [Reached::Agreed, Reached::FailedTogether, Reached::RefusedAsUnfederatable] {
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

/// **`telekom/sutura#354`, from both ends: a violated `many_to_one` and no answer at all.**
///
/// The corpus this reads is [`violated`] - the shared one plus a second, identical row for a
/// customer key that already had one. Before the boot check, both topologies ANSWERED it and their
/// answers were different numbers: the one-source `JOIN` matched twice and added the measure twice,
/// while the two-source lookup leg's `GROUP BY` collapsed the pair first, so the same question came
/// back as `29138` and as `22765` and neither side refused. `AmbiguousLink` cannot close that
/// half - it fires on lookup rows that DISAGREE, and this pair agrees.
///
/// What is asserted is therefore the thing that makes the two topologies agree again: **neither
/// bundle validates**, both name the same relationship, the same table and the same column, and no
/// key value appears in either message. A deployment that moves the dimension model to a second data
/// system gets the same refusal it got before it moved.
///
/// **Three topologies, and the third is the one a release can be:** one engine, two `DuckDB`
/// databases, and two ENGINES. The third exists because the engine now declares
/// `Warehouse::EXECUTES_LEGS` - so a two-source deployment is reachable from a published artefact,
/// and *a violated declaration never produces a number* has to hold on the topology that artefact
/// can be in, not only on the development one. It is the assertion `telekom/sutura#427` makes
/// required: the promise is the strong one, so what is asserted is a REFUSAL and never that two
/// topologies agree on a figure.
///
/// **What this does NOT establish**, and it is the same limit the rest of this file carries: every
/// side runs under one operating-system identity. It also measures exactly two adapters - the
/// engine a release links and the embedded database this differential also runs legs on; an adapter
/// that takes the port's default `declared_key` answers `NotAsked` and this bundle would validate
/// on it.
#[test]
fn a_violated_cardinality_declaration_is_refused_by_both_topologies() {
    let violated = violated();
    // The instrument's own control: the derived corpus really does hold two rows for one key, so a
    // refusal below is about the declaration rather than about a corpus that failed to derive.
    let duplicated = std::fs::read_to_string(violated.data.join(A_DUPLICATED_KEY.0))
        .expect("the violated corpus has a dimension file")
        .matches(A_DUPLICATED_KEY.1.trim_end())
        .count();
    assert_eq!(
        duplicated, 2,
        "the violated corpus must hold the duplicated dimension row twice, or nothing below is about a \
         violated declaration"
    );

    let (one, _) = validating_on_one_source(&violated.data, bundle(&violated.one_source));
    let (two, _) = validating_on_two_sources(&violated.data, bundle(&violated.two_source));
    let (shipped, _) = validating_on_two_engines(&violated.data, bundle(&violated.two_source));
    for (topology, refused) in [
        ("one source", one),
        ("two duckdb databases", two),
        ("two in-process engines", shipped),
    ] {
        let refused = refused
            .err()
            .unwrap_or_else(|| panic!("{topology}: a bundle whose declared join key the data contradicts must not validate"));
        let NotValidated::DeclaredKeyNotUnique(ref violation) = refused else {
            panic!("{topology}: a violated declaration is refused as one, not as {refused:?}");
        };
        assert_eq!(violation.relationship().as_str(), "subscription_customer", "{topology}");
        assert_eq!(violation.column().as_str(), "customer_key", "{topology}");
        // Forty customers and one of them twice, which is the corpus this derivation makes.
        assert_eq!(violation.counts().rows(), 41, "{topology}");
        assert_eq!(violation.counts().distinct(), 40, "{topology}");
        // The counts locate the table, and the message names it.
        let said = refused.to_string();
        assert!(said.contains("dim_customer"), "{topology}: {said}");
        // **What this does NOT prove, said here rather than left to read as proof.** An earlier
        // version asserted `!said.contains("C0002")` and review pointed out that it is structurally
        // unfailable: `KeyNotUnique` is built from a `DeclaredKey` plus two integers, so no field on
        // it can hold a cell of the dimension table and no edit to this file could make that
        // assertion fail. The claim *no key value reaches an operator's log* is held by the TYPE -
        // its five fields and its `Display` - and by review of them, not by a line here. What is
        // asserted instead is the positive half, which can fail: every part of the message is one of
        // those five fields.
        for part in ["subscription_customer", "customers", "customer_key", "41", "40"] {
            assert!(said.contains(part), "{topology}: the refusal must name {part}: {said}");
        }
    }
}

/// **A dimension row whose join key is ABSENT is not a duplicate, and the probe must not say it is.**
///
/// The other half of the arithmetic the boot check rests on. `COUNT(col)` beside
/// `COUNT(DISTINCT col)` skips nulls on both sides; a probe written with `COUNT(*)` would count the
/// two appended rows and refuse this deployment over rows that can join to nothing - a FALSE refusal
/// at startup, which is the loud direction but still a deployment that will not start.
///
/// **The control comes first**, because the assertion is that something did NOT happen: without it,
/// a corpus that failed to derive would pass this cell by holding no null key at all. So the file is
/// read and the two facts the case needs are asserted on it - two rows with an empty key, and no
/// duplicate among the rest - before either topology is asked.
#[test]
fn a_dimension_row_with_no_join_key_is_not_counted_as_a_duplicate() {
    let corpus = with_null_keys();
    let text =
        std::fs::read_to_string(corpus.data.join(NULL_DIMENSION_KEYS.0)).expect("the null-key corpus has a dimension file");
    let keys: Vec<&str> = text
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(|line| line.split(',').next().unwrap_or_default())
        .collect();
    let absent = keys.iter().filter(|key| key.is_empty()).count();
    let mut present: Vec<&&str> = keys.iter().filter(|key| !key.is_empty()).collect();
    let rows_with_a_key = present.len();
    present.sort_unstable();
    present.dedup();
    assert_eq!(
        absent, 2,
        "the null-key corpus must hold two rows with no join key, or this proves nothing"
    );
    assert_eq!(
        present.len(),
        rows_with_a_key,
        "the null-key corpus must hold no DUPLICATE key, or a refusal below would be about the wrong thing"
    );
    // The number a `COUNT(*)` probe would compare against `COUNT(DISTINCT ..)`, stated so the
    // difference this cell is about is visible rather than implied.
    assert_eq!(
        keys.len(),
        present.len() + absent,
        "the two counts a wrong probe would disagree on are these"
    );

    for (topology, validated) in [
        (
            "one source",
            validating_on_one_source(&corpus.data, bundle(&corpus.one_source)).0,
        ),
        (
            "two sources",
            validating_on_two_sources(&corpus.data, bundle(&corpus.two_source)).0,
        ),
    ] {
        if let Err(refused) = validated {
            panic!("{topology}: a null join key duplicates no fact row, so this bundle must validate: {refused}");
        }
    }
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

/// **Which registered data systems can run a leg, expanded over the registry itself, and this file
/// has a two-source pass for each of them.**
///
/// An entry that cannot run a leg cannot be either half of a federated answer, so every `true` here
/// owes this file a pass: `DuckDB` has
/// [`a_two_source_answer_is_the_same_answer_as_one_source`] and the engine has
/// [`two_engines_answer_what_one_engine_answers`]. A cell rather than a sentence, so registering a
/// THIRD leg-executing adapter REDDENS here and the diff that enrols it arrives beside the
/// registration. `sutura-conformance`'s binding holds the per-adapter agreement between the tag and
/// the constant; what this holds is the SET.
///
/// **`LEG_EXECUTING` is a list of names rather than one comparison**, which is the correction the
/// second entry earned: written as `name == "duckdb"` the assertion had nowhere for a second
/// adapter to go except a boolean expression that grows, and the list is what a reader can compare
/// against the two passes above.
///
/// [`Warehouse::EXECUTES_LEGS`]: sutura_domain::warehouse::Warehouse::EXECUTES_LEGS
const LEG_EXECUTING: &[&str] = &["datafusion", "duckdb"];

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
                    super::LEG_EXECUTING.contains(&name),
                    "{name} changed its leg capability; \
                     crates/sutura-app/tests/differential/federated.rs is where a leg-executing \
                     adapter gets enrolled in a two-source pass, and every entry in \
                     `LEG_EXECUTING` owes this file one"
                );
            }
        }
    };
}

crate::adapters::registered!(data_systems: leg_capability);
