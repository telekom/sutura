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
//! is measured here rather than stated - see [`recorded_identities`](harness::recorded_identities),
//! which is also what makes the limit below a measurement.
//!
//! # Two two-source sides, and neither one is redundant
//!
//! [`a_two_source_answer_is_the_same_answer_as_one_source`](harness::a_two_source_answer_is_the_same_answer_as_one_source)
//! runs the legs on two `DuckDB` databases; [`two_engines_answer_what_one_engine_answers`] runs
//! them on two instances of the ENGINE, which is the adapter a release links. Both compare against
//! the same one-`DataFusion` side through [`differential`](harness::differential), so what differs
//! between them is only which adapter holds the legs.
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
//! [`recorded_identities`](harness::recorded_identities) reads that back off each answer rather
//! than stating it. So this is single-player federation - nothing here says two subjects get different rows.
//!
//! **The `DuckDB` side still runs on a development dependency**, so that pass measures the
//! implemented federation path rather than a deployment's answer. What changed is the engine side:
//! it declares `Warehouse::EXECUTES_LEGS`, and it is non-optional in the shipped binary.
//!
//! **Neither side is the served deployment.** These sides open adapters in-process from a bundle
//! this file loads; `crates/sutura-cli/tests/served.rs` is the only place a two-source question
//! reaches the composed binary over HTTP.

use sutura_domain::pinned::NotValidated;
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::Value;
use sutura_semantic::{Compiled, compile};

// `#[path]` for the reason `tests/golden.rs` gives, one level down: a bare `mod corpus;` in a
// submodule of a test target resolves beside the target root, not beside this file.
#[path = "federated/bounds.rs"]
mod bounds;
#[path = "federated/corpus.rs"]
mod corpus;
// A NEW file rather than inline, but it must carry a test of its own or `xtask test-causality`'s
// base reconstruction REMOVES it (a new file with no `#[test]` in it is dropped outright) while
// keeping this file - which calls into it - at HEAD, breaking the base build. `two_kinds::tests`
// is what causality's own "held: … (carries its own tests)" rule keys on.
#[path = "federated/two_kinds.rs"]
mod two_kinds;
// The leg-capability declaration and the cells that hold it against the tree, carved out for
// `max-lines`. It carries the `#[test]`s the `registered!` expansion emits, which is what keeps
// causality's base reconstruction from dropping the file.
#[path = "federated/leg_evidence.rs"]
mod leg_evidence;

// The sides, the differential and the classifier, split out for `cargo xtask max-lines`. It carries
// one `#[test]` so `just causality` keeps it at HEAD: a new file with none is dropped on the base
// image while this file, which calls into it, stays.
#[path = "federated/harness.rs"]
mod harness;

use corpus::{
    A_DUPLICATED_KEY, LOOKUP_SOURCE, NULL_DIMENSION_KEYS, derived, derived_question, remote_products, violated, with_null_keys,
};
use two_kinds::two_kinds;

#[test]
fn two_remote_sources_exceed_the_two_source_plan_limit() {
    let corpus = remote_products("federation-three-sources", "source: inventory");
    let one = harness::bundle(&corpus.one_source);
    let three = harness::bundle(&corpus.two_source);
    let [mono, customer, product, refused] = [
        harness::compiled_dimensions(&one, "region, product_family"),
        harness::compiled_dimensions(&three, "region"),
        harness::compiled_dimensions(&three, "product_family"),
        harness::compiled_dimensions(&three, "region, product_family"),
    ];
    assert_eq!(
        [
            matches!(mono, Compiled::Planned { .. }),
            matches!(customer, Compiled::Federated { .. }),
            matches!(product, Compiled::Federated { .. }),
        ],
        [true; 3],
        "the whole question plans locally and each remote relationship alone can federate"
    );
    let Compiled::Refused {
        reason: RefusalReason::PlanSpansTooManySources { sources, limit },
    } = refused
    else {
        panic!("the combined question must refuse its three sources, not {refused:?}");
    };
    assert_eq!((sources, limit), (3, 2));
}

#[test]
fn two_relationships_on_one_remote_source_have_no_single_federation_link() {
    let corpus = remote_products("federation-ambiguous-links", "source: geo");
    let one = harness::bundle(&corpus.one_source);
    let two = harness::bundle(&corpus.two_source);
    let [mono, customer, product, same_link, refused] = [
        harness::compiled_dimensions(&one, "region, product_family"),
        harness::compiled_dimensions(&two, "region"),
        harness::compiled_dimensions(&two, "product_family"),
        harness::compiled_dimensions(&two, "region, segment"),
        harness::compiled_dimensions(&two, "region, product_family"),
    ];
    assert_eq!(
        [
            matches!(mono, Compiled::Planned { .. }),
            matches!(customer, Compiled::Federated { .. }),
            matches!(product, Compiled::Federated { .. }),
            matches!(same_link, Compiled::Federated { .. }),
        ],
        [true; 4],
        "one-source, single-link and two-dimensions-on-one-link controls must remain supported"
    );
    let Compiled::Refused {
        reason: RefusalReason::FederationLinkAmbiguous { source },
    } = refused
    else {
        panic!("the combined question must refuse its two remote relationships, not {refused:?}");
    };
    assert_eq!(source.as_str(), LOOKUP_SOURCE);
}

/// `github.com/telekom/sutura#777`'s case 2 - the one case the splitter can produce. Case 1 had
/// no reachable cell and was cut with the pushdown; a federated `top` always ranks above the
/// combine. Here `region` is remote, so the rank waits for the combine rather than refusing
/// outright.
#[test]
fn top_on_a_federated_plan_ranks_the_combine_when_a_key_is_lookup_side() {
    let corpus = derived();
    let one = harness::bundle(&corpus.one_source);
    let two = harness::bundle(&corpus.two_source);
    let case_2: Query = serde_norway::from_str(
        "metrics: [recurring_revenue]\ngrain: month\nrange: { start: 2026-07-01, end: 2026-08-01 }\n\
         dimensions: [region]\ntop: { n: 3, by: metric, direction: desc }\n",
    )
    .expect("a top question is a question");

    let Compiled::Federated { plan: case_2_plan } =
        compile(&case_2, &ScopedView::everything(&two), RowCeiling::DEFAULT).expect("case 2 compiles")
    else {
        panic!("a region question over the two-source catalog must federate");
    };
    assert!(case_2_plan.top().is_some(), "case 2 ranks the combine instead");
    // `top` does not narrow what the mono side can do: it still plans whole on one source.
    let mono = compile(&case_2, &ScopedView::everything(&one), RowCeiling::DEFAULT).expect("the mono question compiles");
    assert!(matches!(mono, Compiled::Planned { .. }), "{mono:?}");
}

// ----------------------------------------------------------------------------- the instrument ---

/// **Two instances of the ENGINE hold the legs, which is the first two-source side a release can
/// run.**
///
/// `DataFusionWarehouse` is non-optional in the shipped binary and declares
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
    let one = harness::one_source(harness::bundle(&derived.one_source));
    let two = harness::two_engines(harness::bundle(&derived.two_source));
    harness::differential(&one, &two, "two in-process engines");
}

/// **The first side that mixes TWO KINDS of adapter (`telekom/sutura#112`)**, not two instances of
/// one. Reaches [`differential`](harness::differential)'s whole `MUST_BE_REACHED` table exactly
/// like the other two sides, so a defect in `Warehouse::executes_legs`'s per-leg reading reddens here too.
#[test]
fn two_sources_of_two_kinds_answer_the_same_rows_as_one_source_over_the_same_data() {
    let derived = derived();
    let one = harness::one_source(harness::bundle(&derived.one_source));
    let two = two_kinds(harness::bundle(&derived.two_source));
    harness::differential(&one, &two, "two kinds: duckdb and the engine");
}

/// **A zero denominator in ONE subgroup, and the neighbours it must not reach.**
///
/// The differential above proves the two sides AGREE on this answer; without this, the agreement
/// could be over an answer with no null in it at all. The divide happens above both legs - a guard
/// applied inside one would be a wrong number rather than a refusal - so what is asserted is that
/// the group whose denominator summed to zero is null while the group beside it is a figure.
#[test]
fn a_subgroup_with_no_denominator_is_null_and_its_neighbours_are_not() {
    let two = harness::two_sources(harness::bundle(&derived().two_source));
    let name = "two-source-a-zero-denominator-in-one-subgroup";
    let query = derived_question(name);
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    let outcome = harness::answered(&two, &query, name, &combiner).unwrap_or_else(|e| panic!("{e}"));
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

    let (one, _) = harness::validating_on_one_source(&violated.data, harness::bundle(&violated.one_source));
    let (two, _) = harness::validating_on_two_sources(&violated.data, harness::bundle(&violated.two_source));
    let (shipped, _) = harness::validating_on_two_engines(&violated.data, harness::bundle(&violated.two_source));
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
        assert_eq!(violation.keys().first().unwrap().as_str(), "customer_key", "{topology}");
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
            harness::validating_on_one_source(&corpus.data, harness::bundle(&corpus.one_source)).0,
        ),
        (
            "two sources",
            harness::validating_on_two_sources(&corpus.data, harness::bundle(&corpus.two_source)).0,
        ),
    ] {
        if let Err(refused) = validated {
            panic!("{topology}: a null join key duplicates no fact row, so this bundle must validate: {refused}");
        }
    }
}
