//! **A cross-model ratio whose second fact sits on another data system, executed**
//! (`telekom/sutura#1471`): the tickets move to the system that holds the customers, with a calendar
//! of their own listed there.
//!
//! Each fact leg joins the calendar listed on its own system, so the second statement reads no table
//! of the first system, and the combine joins the two on the customer and the month. The one-source
//! answer is no reference (it has no dimension to link through), so the answer is held to the one the
//! same two engines give with the tickets on the first system, which `two_fact.rs` holds to
//! hand-worked figures: the same rows, moved.
//!
//! **What this does NOT establish.** The two calendars hold the same rows by construction here;
//! sutura cannot check that two calendar tables do, and two that disagree bucket the legs differently
//! with no refusal. Every leg runs under one operating-system identity, as in the rest of
//! `federated.rs`, and only the in-process engine is asked: no tiered data system runs this shape.

use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::Ceilings;
use sutura_domain::query::Query;
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_semantic::{Compiled, compile};

use super::super::fiscal::{rows, side};
use super::super::harness::{bundle, validating_on_two_engines};
use super::{Derived, Edit, TWO_FACT_QUESTION, apply, derive_into, derived, write};
use crate::adapters::DIM_CALENDAR;

const GEO_CALENDAR: &str = "---\nkind: model\nname: geo_calendar\nsource: geo\ntable: dim_calendar_geo\ncolumns:\n  - name: month\n    type: DATE\n    description: The first day of the month.\n    nullable: false\nprimary_key: [month]\n---\nThe calendar listed on the second data system, which the tickets bucket through there.\n";

/// The shared corpus with the tickets beside the customers on the second data system, joined there
/// to a calendar of that system's own.
fn with_the_tickets_on_the_lookup_source(stem: &str) -> Derived {
    let derived = derive_into(stem);
    let catalog = &derived.two_source;
    write(&catalog.join("models/geo_calendar.md"), GEO_CALENDAR);
    write(&derived.data.join("dim_calendar_geo.csv"), DIM_CALENDAR);
    for (at, find, with) in [
        ("models/tickets.md", "source: local", "source: geo"),
        (
            "relationships/ticket_calendar.md",
            "target:\n  model: calendar",
            "target:\n  model: geo_calendar",
        ),
        (
            "metrics/tickets_per_subscription.md",
            "shared_calendar: [calendar]",
            "shared_calendar: [calendar, geo_calendar]",
        ),
    ] {
        apply(&catalog.join(at), &Edit::Rewrite { find, with });
    }
    derived
}

/// The tickets on the second system answer the rows they answered on the first, through the calendar
/// each system lists: with the second leg joined to the first system's calendar, the second system
/// holds no such table and the answer fails.
#[test]
fn a_second_fact_on_another_system_answers_through_the_calendar_listed_there() {
    const NAME: &str = "two-fact-second-fact-on-the-lookup-source";
    let corpus = with_the_tickets_on_the_lookup_source("federated-differential-second-source");
    let query: Query = serde_norway::from_str(TWO_FACT_QUESTION).expect("the question is a question");
    let Compiled::Federated { plan } = compile(
        &query,
        &ScopedView::everything(&bundle(&corpus.two_source)),
        Ceilings::DEFAULT,
    )
    .expect("the question compiles") else {
        panic!("{NAME}: a two-fact question over the two-source catalog federates");
    };
    let second = plan
        .second_fact()
        .expect("the plan reads the second fact in a leg of its own");
    assert_eq!(
        second.source().as_str(),
        super::LOOKUP_SOURCE,
        "{NAME}: the tickets are read where they live"
    );
    let answer = rows(
        &side(validating_on_two_engines(&corpus.data, bundle(&corpus.two_source))),
        TWO_FACT_QUESTION,
        NAME,
    );
    let reference = rows(
        &side(validating_on_two_engines(&derived().data, bundle(&derived().two_source))),
        TWO_FACT_QUESTION,
        NAME,
    );
    if let Err(disagreement) = agree_on_content(&reference, &answer, RealTolerance::DIFFERENTIAL) {
        panic!("{NAME}: moving the tickets changed the rows - {disagreement}\n{answer:?}");
    }
    if let Err(disagreement) = agree_on_order(&reference, &answer, RealTolerance::DIFFERENTIAL) {
        panic!("{NAME}: moving the tickets changed the order - {disagreement}\n{answer:?}");
    }
}
