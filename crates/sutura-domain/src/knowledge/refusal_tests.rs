//! Every way a set of notes can fail to hold together, provoked once each.
//!
//! Split out of `tests.rs` before it reached the thousand-line limit rather than after: the fixture
//! and the newtypes are one file, the refusals are this one, and `cargo xtask max-lines` fails at a
//! thousand lines under `crates/` with no exemption available. `*_tests.rs` rather than a name of its
//! own choosing because `.cargo-crap.toml` excludes that suffix from the coverage walk - a split test
//! file with any other name would be scored as production code.
//!
//! **A check nobody has seen fire is a check nobody knows works**, and these run at load and never
//! again, so a test that reaches each one is the only evidence they do anything. Two of the tests
//! below assert the complement instead - that a phrase merely RESEMBLING a metric name still loads,
//! and that a well-formed example survives - because a check that refuses everything is as useless as
//! one that refuses nothing.

use super::bundle::identifier_shape;
use super::tests::{
    absence, accepts, caveat, declared_value, dimension_name, example, glossary_entry, june, metric_name, note_name,
    only_absences, only_caveats, only_examples, only_glossary, phrase, question, refuses, revenue,
};
use super::{
    Capability, Caveat, InconsistentKnowledge, KnowledgeCapabilities, KnowledgeInput, MAX_KNOWLEDGE_BYTES, MAX_NOTE_BODY_BYTES,
    NoteBody, Referent,
};
use crate::model::Grain;
use crate::query::{Filter, Query};

#[test]
fn a_glossary_entry_naming_an_undefined_metric_does_not_load() {
    assert_eq!(
        refuses(only_glossary(vec![glossary_entry(
            "lifetime value",
            &[],
            Referent::Metric {
                metric: metric_name("clv")
            },
        )])),
        InconsistentKnowledge::GlossaryUnknownMetric {
            term: phrase("lifetime value"),
            metric: metric_name("clv"),
        }
    );
}

#[test]
fn a_glossary_entry_naming_an_undeclared_dimension_does_not_load() {
    assert_eq!(
        refuses(only_glossary(vec![glossary_entry(
            "by area",
            &[],
            Referent::Dimension {
                metric: metric_name("recurring_revenue"),
                dimension: dimension_name("region"),
            },
        )])),
        InconsistentKnowledge::GlossaryUnknownDimension {
            term: phrase("by area"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("region"),
        }
    );
}

#[test]
fn a_glossary_entry_naming_a_value_the_allowlist_does_not_carry_does_not_load() {
    // THE INTERESTING ONE. A glossary saying "business customers means segment b2b" while the
    // allowlist says `business` produces no wrong number: it produces an agent that writes
    // `segment = b2b`, gets `DimensionValueNotAllowed`, and is told only which dimension was at
    // fault. The document is the fault, and this is where it is caught.
    assert_eq!(
        refuses(only_glossary(vec![glossary_entry(
            "business customers",
            &["B2B"],
            Referent::Value {
                metric: metric_name("recurring_revenue"),
                dimension: dimension_name("segment"),
                value: declared_value("b2b"),
            },
        )])),
        InconsistentKnowledge::GlossaryValueNotAllowed {
            term: phrase("business customers"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
            value: String::from("b2b"),
        }
    );
}

#[test]
fn a_glossary_entry_naming_a_value_of_an_unfilterable_dimension_does_not_load() {
    // `product_name` declares no allowlist, so it permits no value at all - and a glossary phrase
    // that resolves to one of its values is a phrase whose every use would be refused.
    assert_eq!(
        refuses(only_glossary(vec![glossary_entry(
            "the flagship tariff",
            &[],
            Referent::Value {
                metric: metric_name("recurring_revenue"),
                dimension: dimension_name("product_name"),
                value: declared_value("Tariff L"),
            },
        )])),
        InconsistentKnowledge::GlossaryValueNotAllowed {
            term: phrase("the flagship tariff"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("product_name"),
            value: String::from("Tariff L"),
        }
    );
}

#[test]
fn one_phrase_resolving_to_two_things_does_not_load() {
    // A phrase resolves to at most one thing across the whole glossary. Without this the second
    // entry wins in a map and the first body is rendered for neither.
    assert_eq!(
        refuses(only_glossary(vec![
            glossary_entry("revenue", &[], revenue()),
            glossary_entry(
                "turnover",
                &["revenue"],
                Referent::Metric {
                    metric: metric_name("voice_minutes"),
                },
            ),
        ])),
        InconsistentKnowledge::AmbiguousPhrase {
            phrase: phrase("revenue"),
            first: phrase("revenue"),
            second: phrase("turnover"),
        }
    );
}

#[test]
fn a_term_declared_twice_is_the_same_refusal() {
    // The check is on the CLAIM rather than on what it resolves to, so two entries for one term are
    // refused even when they agree - two bodies for one phrase is still a document that lost one.
    let twice = refuses(only_glossary(vec![
        glossary_entry("revenue", &[], revenue()),
        glossary_entry("revenue", &[], revenue()),
    ]));
    assert!(
        matches!(twice, InconsistentKnowledge::AmbiguousPhrase { .. }),
        "a term claimed twice must not load: {twice:?}"
    );
}

#[test]
fn a_caveat_about_an_undefined_metric_does_not_load() {
    assert_eq!(
        refuses(only_caveats(vec![caveat(
            "stale",
            vec![Referent::Metric {
                metric: metric_name("arpu")
            }],
        )])),
        InconsistentKnowledge::CaveatUnknownMetric {
            name: note_name("stale"),
            metric: metric_name("arpu"),
        }
    );
}

#[test]
fn a_caveat_about_an_undeclared_dimension_does_not_load() {
    assert_eq!(
        refuses(only_caveats(vec![caveat(
            "stale",
            vec![Referent::Dimension {
                metric: metric_name("recurring_revenue"),
                dimension: dimension_name("region"),
            }],
        )])),
        InconsistentKnowledge::CaveatUnknownDimension {
            name: note_name("stale"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("region"),
        }
    );
}

#[test]
fn a_caveat_about_a_value_the_allowlist_does_not_carry_does_not_load() {
    // The same resolution as the glossary's, dressed as the caveat's own error. A check that existed
    // only on the glossary would be a check the caveat kind did not have.
    assert_eq!(
        refuses(only_caveats(vec![caveat(
            "wholesale_is_not_here",
            vec![Referent::Value {
                metric: metric_name("recurring_revenue"),
                dimension: dimension_name("segment"),
                value: declared_value("wholesale"),
            }],
        )])),
        InconsistentKnowledge::CaveatValueNotAllowed {
            name: note_name("wholesale_is_not_here"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
            value: String::from("wholesale"),
        }
    );
}

#[test]
fn a_caveat_about_nothing_does_not_load() {
    // THE CHECK THAT REMOVES THE GLOBAL TEXT CHANNEL. An unscoped caveat is a paragraph about the
    // deployment at large, rendered into the prompt's preamble, authored by whoever wrote the
    // catalog - which is the shape the injection answer depends on not existing.
    assert_eq!(
        refuses(only_caveats(vec![caveat("read_this_first", Vec::new())])),
        InconsistentKnowledge::CaveatAboutNothing {
            name: note_name("read_this_first"),
        }
    );
}

#[test]
fn two_caveats_with_one_name_do_not_load() {
    assert_eq!(
        refuses(only_caveats(vec![
            caveat("grain", vec![revenue()]),
            caveat("grain", vec![revenue()]),
        ])),
        InconsistentKnowledge::DuplicateCaveat {
            name: note_name("grain")
        }
    );
}

#[test]
fn a_phrase_both_given_a_meaning_and_declared_undefined_does_not_load() {
    assert_eq!(
        refuses(KnowledgeInput::new(
            KnowledgeCapabilities::of([Capability::Glossary, Capability::Absences]),
            vec![glossary_entry("revenue", &["takings"], revenue())],
            Vec::new(),
            vec![absence("takings", &[])],
            Vec::new(),
        )),
        InconsistentKnowledge::PhraseBothDefinedAndNot {
            phrase: phrase("takings"),
        }
    );
}

#[test]
fn one_phrase_declared_undefined_twice_does_not_load() {
    assert_eq!(
        refuses(only_absences(vec![
            absence("customer lifetime value", &["CLV"]),
            absence("lifetime value", &["CLV"]),
        ])),
        InconsistentKnowledge::DuplicateAbsence { phrase: phrase("CLV") }
    );
}

#[test]
fn an_absence_that_names_a_defined_metric_does_not_load() {
    // **What stops the absence list rotting into a lie.** The day somebody certifies the metric, the
    // note saying it does not exist is a document that teaches an agent to decline a question this
    // bundle answers - and the load fails rather than the prompt saying it.
    //
    // Both spellings, because the comparison is on identifier SHAPE: nobody writes a prose note
    // about `recurring_revenue`, they write one about "recurring revenue".
    for spelling in ["recurring revenue", "Recurring Revenue", "recurring_revenue"] {
        assert_eq!(
            refuses(only_absences(vec![absence(spelling, &[])])),
            InconsistentKnowledge::AbsenceNamesADefinedMetric {
                phrase: phrase(spelling),
                metric: metric_name("recurring_revenue"),
            },
            "{spelling} names a defined metric"
        );
    }
    // A synonym is checked too, or the check is one rename away from being bypassed.
    assert_eq!(
        refuses(only_absences(vec![absence("customer lifetime value", &["voice minutes"])])),
        InconsistentKnowledge::AbsenceNamesADefinedMetric {
            phrase: phrase("voice minutes"),
            metric: metric_name("voice_minutes"),
        }
    );
}

#[test]
fn a_phrase_that_merely_resembles_a_metric_name_still_loads() {
    // The other side of the same check: it must not refuse every note whose words overlap a metric's
    // name, or an absence list is unwritable next to any catalog. "monthly recurring revenue" is not
    // `recurring_revenue`.
    let knowledge = accepts(only_absences(vec![absence("monthly recurring revenue growth", &[])]));
    assert_eq!(knowledge.absences().len(), 1);
    assert_eq!(
        identifier_shape("Customer Lifetime Value (CLV)"),
        "customer_lifetime_value_clv"
    );
    assert_eq!(identifier_shape("  --  "), "");
}

#[test]
fn an_example_about_an_undefined_metric_does_not_load() {
    let asking = Query::new(metric_name("arpu"), Grain::Month, june(), Vec::new(), Vec::new());
    assert_eq!(
        refuses(only_examples(vec![example("arpu_by_month", asking)])),
        InconsistentKnowledge::ExampleUnknownMetric {
            name: note_name("arpu_by_month"),
            metric: metric_name("arpu"),
        }
    );
}

#[test]
fn an_example_at_a_grain_the_metric_does_not_declare_does_not_load() {
    // A worked example is what an agent copies. One asking at a grain the metric never declared
    // teaches it to send a question that is refused, which is worse than no example.
    assert_eq!(
        refuses(only_examples(vec![example(
            "revenue_by_day",
            question(Grain::Day, Vec::new(), Vec::new()),
        )])),
        InconsistentKnowledge::ExampleGrainNotSupported {
            name: note_name("revenue_by_day"),
            metric: metric_name("recurring_revenue"),
            grain: Grain::Day,
        }
    );
}

#[test]
fn an_example_grouping_by_an_undeclared_dimension_does_not_load() {
    assert_eq!(
        refuses(only_examples(vec![example(
            "revenue_by_region",
            question(Grain::Month, vec![dimension_name("region")], Vec::new()),
        )])),
        InconsistentKnowledge::ExampleDimensionNotPermitted {
            name: note_name("revenue_by_region"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("region"),
        }
    );
}

#[test]
fn an_example_filtering_on_a_value_the_allowlist_does_not_carry_does_not_load() {
    assert_eq!(
        refuses(only_examples(vec![example(
            "revenue_for_b2b",
            question(
                Grain::Month,
                Vec::new(),
                vec![Filter::new(dimension_name("segment"), declared_value("b2b"))],
            ),
        )])),
        InconsistentKnowledge::ExampleValueNotAllowed {
            name: note_name("revenue_for_b2b"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
            value: String::from("b2b"),
        }
    );
    // And a filter on a dimension that permits no value at all is the same fact about the document:
    // the question in it would be refused, so it is not an example of anything.
    assert_eq!(
        refuses(only_examples(vec![example(
            "revenue_for_one_tariff",
            question(
                Grain::Month,
                Vec::new(),
                vec![Filter::new(dimension_name("product_name"), declared_value("Tariff L"))],
            ),
        )])),
        InconsistentKnowledge::ExampleValueNotAllowed {
            name: note_name("revenue_for_one_tariff"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("product_name"),
            value: String::from("Tariff L"),
        }
    );
}

#[test]
fn two_examples_with_one_name_do_not_load() {
    assert_eq!(
        refuses(only_examples(vec![
            example("in_june", question(Grain::Month, Vec::new(), Vec::new())),
            example("in_june", question(Grain::Month, Vec::new(), Vec::new())),
        ])),
        InconsistentKnowledge::DuplicateExample {
            name: note_name("in_june"),
        }
    );
}

#[test]
fn a_well_formed_example_loads_with_its_question_intact() {
    let knowledge = accepts(only_examples(vec![example(
        "revenue_for_business_customers",
        question(
            Grain::Month,
            vec![dimension_name("segment")],
            vec![Filter::new(dimension_name("segment"), declared_value("business"))],
        ),
    )]));
    let note = knowledge
        .examples()
        .get(&note_name("revenue_for_business_customers"))
        .expect("the name is the key");
    assert_eq!(note.question().metric(), &metric_name("recurring_revenue"));
    assert_eq!(note.question().grain(), Grain::Month);
    assert_eq!(note.asked(), [phrase("how much revenue")]);
}

#[test]
fn enough_conforming_notes_to_exceed_the_aggregate_cap_do_not_load() {
    // The reason the aggregate cap exists at all: every note below is well inside every per-note
    // bound, and together they are a prompt nobody would read. N conforming notes must not be able
    // to do what one oversized note cannot.
    let filler = NoteBody::parse("y".repeat(MAX_NOTE_BODY_BYTES)).expect("exactly the note cap is a body");
    let notes: Vec<Caveat> = (0..16_u32)
        .map(|index| Caveat::new(note_name(&format!("note_{index}")), vec![revenue()], filler.clone()))
        .collect();
    let error = refuses(only_caveats(notes));
    match error {
        InconsistentKnowledge::KnowledgeTooLarge { bytes, limit } => {
            assert_eq!(limit, MAX_KNOWLEDGE_BYTES);
            assert!(bytes > MAX_KNOWLEDGE_BYTES, "{bytes} must exceed {MAX_KNOWLEDGE_BYTES}");
        }
        other => panic!("the aggregate cap must be what refuses this: {other:?}"),
    }
    // And the cap is not so tight that a realistic set of notes trips it: seven of the same bodies
    // are 28 KiB of authored text and load.
    let inside: Vec<Caveat> = (0..7_u32)
        .map(|index| Caveat::new(note_name(&format!("note_{index}")), vec![revenue()], filler.clone()))
        .collect();
    assert_eq!(accepts(only_caveats(inside)).caveats().len(), 7);
}

// ---------------------------------------------------------------- adversarial review findings ---
//
// Each test below states a property this module's own documentation claims and FAILS against the
// code as committed. Written as assertions rather than as prose so the finding cannot be lost.

use super::tests::{body, definitions};
use super::{Example, Knowledge};
use crate::calendar::{Date, TimeRange};
use crate::query::MAX_RANGE_DAYS;

/// FINDING. `sutura_app::prompt::knowledge`'s `EXAMPLES_INTRO` tells an agent that "a question below
/// is one this deployment answers rather than one it would decline". `Knowledge::check_question`
/// checks only the four things a metric declares, so an example whose period is longer than
/// `MAX_RANGE_DAYS` loads, renders, and is refused as `TimeRangeTooLong` when an agent copies it.
/// Both caps live in this same crate, so checking them here is not a second opinion about what a
/// permitted question is - it is the same opinion.
#[test]
fn a_worked_example_asking_for_more_history_than_a_request_may_does_not_load() {
    let span = TimeRange::new(
        Date::parse("2000-01-01").expect("a test date is a date"),
        Date::parse("2026-01-01").expect("a test date is a date"),
    )
    .expect("twenty-six years is a range");
    let asking = Query::new(metric_name("recurring_revenue"), Grain::Month, span, Vec::new(), Vec::new());
    assert!(
        Knowledge::assemble(&definitions(), only_examples(vec![example("everything_ever", asking)])).is_err(),
        "an example over the {MAX_RANGE_DAYS}-day cap is a shape an agent is told to copy and the surface declines"
    );
}

/// FINDING. The claims index spans the glossary and the absences and never sees an example's
/// `asked:` phrases, so one phrase can be declared undefined - which the prompt renders as "a
/// question about one of them is to be DECLINED" - and be the phrasing of a worked question the same
/// prompt tells the agent to copy.
#[test]
fn a_phrase_declared_undefined_is_not_also_the_way_a_worked_question_was_asked() {
    let input = KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Absences, Capability::Examples]),
        Vec::new(),
        Vec::new(),
        vec![absence("customer lifetime value", &["CLV"])],
        vec![Example::new(
            note_name("collides"),
            vec![phrase("CLV")],
            question(Grain::Month, Vec::new(), Vec::new()),
            body(),
        )],
    );
    assert!(
        Knowledge::assemble(&definitions(), input).is_err(),
        "a phrase recorded as undefined must not also be a worked question's own phrasing"
    );
}

/// FINDING. `AbsenceNamesADefinedMetric` compares through `identifier_shape`, which lower-cases.
/// `AmbiguousPhrase` and `PhraseBothDefinedAndNot` compare a `Phrase` by bytes. So one module treats
/// "Recurring Revenue" and `recurring_revenue` as one phrase and "CLV" and "clv" as two, and the
/// second pair renders a glossary line and an absence line for what a reader sees as one word.
#[test]
fn a_phrase_given_a_meaning_is_not_declared_undefined_in_another_case() {
    let input = KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Glossary, Capability::Absences]),
        vec![glossary_entry("CLV", &[], revenue())],
        Vec::new(),
        vec![absence("clv", &[])],
        Vec::new(),
    );
    assert!(
        Knowledge::assemble(&definitions(), input).is_err(),
        "case is not meaning: one phrase must not be both given a meaning and declared undefined"
    );
}

/// FINDING. Two glossary entries whose terms differ only in case, only in an invisible code point,
/// or only in a run of spaces all load and all render - so an agent resolving the phrase a person
/// typed picks one of two referents by whichever spelling it happened to match.
#[test]
fn two_glossary_entries_that_a_reader_cannot_tell_apart_do_not_load() {
    for (first, second) in [
        ("MRR", "mrr"),
        ("mrr", "m\u{200b}rr"),
        // The three ranges the narrower copy of this set was missing, and the reason it mattered:
        // a soft hyphen draws only where a line breaks, a word joiner draws nowhere, and an
        // interlinear annotation anchor hides one run of text behind another - so each of these was a
        // second glossary entry for a word a reader sees exactly once, and `AmbiguousPhrase`,
        // `DuplicateAbsence`, `TermIsItsOwnSynonym` and `PhraseBothDefinedAndNot` all missed the
        // pair, because every one of them is keyed on `phrase_identity`.
        ("mrr", "m\u{00ad}rr"),
        ("mrr", "m\u{2060}rr"),
        ("mrr", "m\u{fff9}rr"),
        ("monthly revenue", "monthly  revenue"),
    ] {
        let input = KnowledgeInput::new(
            KnowledgeCapabilities::of([Capability::Glossary]),
            vec![
                glossary_entry(first, &[], revenue()),
                glossary_entry(
                    second,
                    &[],
                    Referent::Metric {
                        metric: metric_name("voice_minutes"),
                    },
                ),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert!(
            Knowledge::assemble(&definitions(), input).is_err(),
            "{first:?} and {second:?} are one phrase to whoever reads the prompt"
        );
    }
}

/// FINDING. `AbsenceNamesADefinedMetric` guards metric names only. A note declaring the phrase
/// "segment" undefined loads beside a metric block advertising `segment` as a dimension a question
/// may group by and filter on, and the prompt then tells an agent to decline a question the bundle
/// answers - which is the rot the metric-name check exists to prevent.
#[test]
fn an_absence_naming_a_declared_dimension_or_value_does_not_load() {
    for undefined in ["segment", "business"] {
        assert!(
            Knowledge::assemble(&definitions(), only_absences(vec![absence(undefined, &[])])).is_err(),
            "{undefined:?} is something this bundle declares, so it is not undefined here"
        );
    }
}

/// FINDING. `NoteBody::parse` refuses an empty body so the prompt cannot render a heading over empty
/// space, but it trims only whitespace - and `quote` drops every control character. A body of control
/// characters or of zero-width spaces therefore loads and renders as nothing at all, which is the
/// state the emptiness check exists to make unreachable.
#[test]
fn a_body_that_renders_as_nothing_is_not_a_body() {
    for raw in ["\u{7}\u{7}\u{7}", "\u{200b}\u{200b}", "\u{feff}"] {
        assert!(
            NoteBody::parse(raw).is_err(),
            "{raw:?} carries no prose, so it is not a note body"
        );
    }
    // FINDING, and the half the three cases above missed: every one of them is a body made ENTIRELY
    // of such characters, which renders as a blank heading somebody notices. One mixed into prose
    // renders as a paragraph that reads correctly and is not what it says, and that is the one an
    // agent acts on. `a_note_body_with_an_invisible_character_mixed_into_prose_is_refused` walks
    // every range; this is the case that names why the file has the test.
    assert!(
        NoteBody::parse("Counts rows where status = '\u{202e}evitca'.").is_err(),
        "a body whose rendered text is not its own text is not a note body"
    );
}

// ------------------------------------------------------- one test per refusal the fixes added ---
//
// The findings above assert only that a bundle does not load, which is the property. These name the
// variant, because a variant nobody has seen fire is a variant nobody knows names the right thing -
// and `docs/crap.md`'s gate scores this crate.

use crate::query::MAX_DIMENSIONS;

#[test]
fn a_glossary_entry_that_claims_one_phrase_twice_says_which_document_to_open() {
    // One document, one phrase, two claims. Its own refusal rather than `AmbiguousPhrase`, which
    // would name this entry as both of the two entries at fault and send whoever reads it looking
    // for a second file that does not exist.
    assert_eq!(
        refuses(only_glossary(vec![glossary_entry("revenue", &["Revenue"], revenue())])),
        InconsistentKnowledge::TermIsItsOwnSynonym {
            term: phrase("revenue"),
            phrase: phrase("Revenue"),
        }
    );
    // Two synonyms that are one phrase are the same fault, in the same one document.
    assert_eq!(
        refuses(only_glossary(vec![glossary_entry("turnover", &["MRR", "mrr"], revenue())])),
        InconsistentKnowledge::TermIsItsOwnSynonym {
            term: phrase("turnover"),
            phrase: phrase("mrr"),
        }
    );
}

#[test]
fn an_absence_naming_a_declared_dimension_or_value_says_which_one_it_found() {
    // The same rot `AbsenceNamesADefinedMetric` exists to stop, one and two levels down: the prompt
    // would say "segment is deliberately NOT defined - DECLINE" three sections above a metric block
    // advertising `segment` as something a question may group by and filter on.
    assert_eq!(
        refuses(only_absences(vec![absence("Segment", &[])])),
        InconsistentKnowledge::AbsenceNamesADeclaredDimension {
            phrase: phrase("Segment"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
        }
    );
    assert_eq!(
        refuses(only_absences(vec![absence("customer lifetime value", &["Business"])])),
        InconsistentKnowledge::AbsenceNamesADeclaredValue {
            phrase: phrase("Business"),
            metric: metric_name("recurring_revenue"),
            dimension: dimension_name("segment"),
            value: String::from("business"),
        }
    );
}

#[test]
fn a_worked_example_is_held_to_the_two_bounds_a_request_is_held_to() {
    // Both caps are read from `crate::query`, so this is the same opinion about what a permitted
    // question is rather than a second one - which is what the module used to argue against having.
    let span = TimeRange::new(
        Date::parse("2000-01-01").expect("a test date is a date"),
        Date::parse("2026-01-01").expect("a test date is a date"),
    )
    .expect("twenty-six years is a range");
    let days = span.days();
    assert!(days > MAX_RANGE_DAYS, "{days} has to exceed the cap for this to test it");
    let asking = Query::new(metric_name("recurring_revenue"), Grain::Month, span, Vec::new(), Vec::new());
    assert_eq!(
        refuses(only_examples(vec![example("everything_ever", asking)])),
        InconsistentKnowledge::ExampleRangeTooLong {
            name: note_name("everything_ever"),
            days,
            limit: MAX_RANGE_DAYS,
        }
    );

    let wide: Vec<_> = ["one", "two", "three", "four", "five"]
        .iter()
        .map(|n| dimension_name(n))
        .collect();
    assert_eq!(wide.len(), MAX_DIMENSIONS.saturating_add(1));
    assert_eq!(
        refuses(only_examples(vec![example(
            "by_everything",
            question(Grain::Month, wide, Vec::new()),
        )])),
        InconsistentKnowledge::ExampleTooManyDimensions {
            name: note_name("by_everything"),
            requested: MAX_DIMENSIONS.saturating_add(1),
            limit: MAX_DIMENSIONS,
        }
    );

    // The other side of both, or the check refuses a question the surface would accept: ten calendar
    // years is exactly the cap, and `resolve` lets it through.
    let ten_years = TimeRange::new(
        Date::parse("2016-01-01").expect("a test date is a date"),
        Date::parse("2026-01-01").expect("a test date is a date"),
    )
    .expect("ten years is a range");
    assert_eq!(ten_years.days(), MAX_RANGE_DAYS);
    let at_the_cap = Query::new(
        metric_name("recurring_revenue"),
        Grain::Month,
        ten_years,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        accepts(only_examples(vec![example("ten_years", at_the_cap)]))
            .examples()
            .len(),
        1
    );
}

#[test]
fn a_worked_question_may_repeat_a_glossary_phrase_and_may_not_repeat_an_undefined_one() {
    // The narrow half of the check, and the reason it is narrow. An `asked` phrase is a sentence
    // somebody said; the glossary is what the words in it mean, so an overlap there is what the two
    // kinds are FOR. What must not overlap is the absence list, because the same document then says
    // "DECLINE questions about it" and, further down, "copy this question".
    let asked_undefined = KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Absences, Capability::Examples]),
        Vec::new(),
        Vec::new(),
        vec![absence("customer lifetime value", &["CLV"])],
        vec![Example::new(
            note_name("collides"),
            vec![phrase("clv")],
            question(Grain::Month, Vec::new(), Vec::new()),
            body(),
        )],
    );
    assert_eq!(
        refuses(asked_undefined),
        InconsistentKnowledge::ExampleAsksWhatIsDeclaredUndefined {
            name: note_name("collides"),
            phrase: phrase("clv"),
        }
    );

    let asked_defined = KnowledgeInput::new(
        KnowledgeCapabilities::of([Capability::Glossary, Capability::Examples]),
        vec![glossary_entry("how much revenue", &[], revenue())],
        Vec::new(),
        Vec::new(),
        vec![example("in_june", question(Grain::Month, Vec::new(), Vec::new()))],
    );
    assert_eq!(accepts(asked_defined).examples().len(), 1);
}
