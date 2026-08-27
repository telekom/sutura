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
    absence, accepts, caveat, dimension_name, example, glossary_entry, june, metric_name, note_name, only_absences, only_caveats,
    only_examples, only_glossary, phrase, question, refuses, revenue,
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
                value: String::from("b2b"),
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
                value: String::from("Tariff L"),
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
                value: String::from("wholesale"),
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
                vec![Filter::new(dimension_name("segment"), String::from("b2b"))],
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
                vec![Filter::new(dimension_name("product_name"), String::from("Tariff L"))],
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
            vec![Filter::new(dimension_name("segment"), String::from("business"))],
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
