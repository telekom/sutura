//! **A crossing relationship with more than one key, executed** (`telekom/sutura#1490`): the shipped
//! `usage_subscription`, which pairs `subscription_key` with a key that truncates `usage_date` to its
//! month, with the usage on a second data system and the snapshot it targets beside the products.
//!
//! Each key is one link column: the fact leg projects the plain column or the column truncated at its
//! own source, the lookup leg projects each target as stored, and the combine joins on every part. A
//! link on `subscription_key` alone multiplies a day by every month the subscription existed, and one
//! on the raw day matches no snapshot row; both are held off by the literal answers below, which no
//! code under test produced.
//!
//! **What this does NOT establish.** Dates only, as in `fiscal.rs`: no cell holds a timestamp or a
//! session zone that is not UTC. Every leg runs under one operating-system identity, as in the rest of
//! `federated.rs`. The second-fact cell compares two engines to a literal only: a ratio of two facts
//! has no one-source answer here.

use std::path::Path;

use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::{InternalLabel, KeyValue, PlanKey, RowCeiling};
use sutura_domain::query::Query;
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content};
use sutura_domain::warehouse::{Real, RowSet, Value};
use sutura_semantic::{Compiled, compile};

use super::super::fiscal::{rows, side};
use super::super::harness::{bundle, validating_on_one_source, validating_on_two_engines, validating_on_two_sources};
use super::{Derived, Edit, TWO_FACT_BY_QUARTER, USAGE_ROWS, apply, derive_into, with_fiscal_months, write};

const VOICE: &str =
    "metrics: [voice_minutes]\ngrain: day\nrange:\n  start: 2026-06-30\n  end: 2026-07-02\ndimensions: [product_family]\n";

const VOICE_ON_THE_FIRST: &str = "metrics: [voice_minutes_on_the_first]\ngrain: day\nrange:\n  start: 2026-06-30\n  end: 2026-07-02\ndimensions: [product_family]\n";

/// Subscription 1001 held a mobile tariff in June and holds a TV one in July: the same key, two
/// months, two product families.
const JULY_SNAPSHOT: &str = "2026-07-01,1001,1,6,active,1249,false,annual\n";

/// A usage row with no subscription: one null part of the key.
const UNATTRIBUTED_USAGE: &str = "2026-07-01,,1.0,5.0\n";

/// The same two keys as `usage_subscription`, both plain: a usage day meets the snapshot only on the
/// first of a month.
const PLAIN_RELATIONSHIP: &str = "---\nkind: relationship\nname: usage_subscription_on_the_first\norigin:\n  model: daily_usage\ntarget:\n  model: subscriptions\nkeys:\n  - { origin: subscription_key, target: subscription_key }\n  - { origin: usage_date, target: month }\njoin_type: many_to_one\n---\nA usage day to the snapshot row that starts on that day.\n";

const PLAIN_METRIC: &str = "---\nkind: metric\nname: voice_minutes_on_the_first\nmodel: daily_usage\nmeasure:\n  simple: { aggregate: sum, column: voice_min }\ntime_column: usage_date\ngrains: [day, month]\naudience: open\ndimensions:\n  - name: product_family\n    column: product_family\n    via: [usage_subscription_on_the_first, subscription_product]\n    values: [convergent, fixed_internet, mobile, tv]\n    description: The kind of product of the snapshot row that starts on the usage day.\n---\nOutgoing voice minutes, reaching the product through a join on two plain keys.\n";

/// The corpus beside a July snapshot, with the usage on a second data system: the compound hop is the
/// crossing, and the hop after it, to the product, joins inside the lookup leg.
fn with_a_july_snapshot(stem: &str, usage: &'static str) -> Derived {
    let derived = derive_into(stem);
    apply(&derived.data.join("fct_usage_daily.csv"), &Edit::Appended(USAGE_ROWS));
    apply(&derived.data.join("fct_usage_daily.csv"), &Edit::Appended(usage));
    apply(
        &derived.data.join("fct_subscription_monthly.csv"),
        &Edit::Appended(JULY_SNAPSHOT),
    );
    apply(
        &derived.two_source.join("models/daily_usage.md"),
        &Edit::Rewrite {
            find: "source: local\ntable:",
            with: "source: geo\ntable:",
        },
    );
    for catalog in [&derived.one_source, &derived.two_source] {
        write(
            &catalog.join("relationships/usage_subscription_on_the_first.md"),
            PLAIN_RELATIONSHIP,
        );
        write(&catalog.join("metrics/voice_minutes_on_the_first.md"), PLAIN_METRIC);
    }
    derived
}

fn voice_rows(rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(
        vec![
            String::from("product_family"),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            String::from("voice_minutes"),
        ],
        rows,
    )
    .expect("the hand-worked answer is rectangular")
}

fn row(family: Option<&str>, day: &str, minutes: f64) -> Vec<Value> {
    vec![
        family.map_or(Value::Null, |name| Value::Text(name.into())),
        Value::Text(day.into()),
        Value::Real(Real::parse(minutes).expect("a hand-worked sum is finite")),
    ]
}

fn compiled_question(catalog: &Path, question: &str) -> Compiled {
    let query: Query = serde_norway::from_str(question).expect("the question is a question");
    compile(&query, &ScopedView::everything(&bundle(catalog)), RowCeiling::DEFAULT).expect("the derived catalog is consistent")
}

/// What every topology must answer: the one source, two engines and two `DuckDB`s.
fn agree_with_the_literal(corpus: &Derived, question: &str, name: &str, expected: &RowSet) {
    assert!(
        matches!(compiled_question(&corpus.one_source, question), Compiled::Planned { .. }),
        "{name}: with every model on one source the question is one statement"
    );
    assert!(
        matches!(compiled_question(&corpus.two_source, question), Compiled::Federated { .. }),
        "{name}: with the usage on a second data system the question is two legs"
    );
    let one = rows(
        &side(validating_on_one_source(&corpus.data, bundle(&corpus.one_source))),
        question,
        name,
    );
    let engines = rows(
        &side(validating_on_two_engines(&corpus.data, bundle(&corpus.two_source))),
        question,
        name,
    );
    let duckdbs = rows(
        &side(validating_on_two_sources(&corpus.data, bundle(&corpus.two_source))),
        question,
        name,
    );
    for (which, answer) in [("one source", &one), ("two engines", &engines), ("two DuckDB", &duckdbs)] {
        if let Err(d) = agree_on_content(expected, answer, RealTolerance::DIFFERENTIAL) {
            panic!("{name}: {which} did not answer the hand-worked rows - {d}\n{answer:?}");
        }
    }
}

/// **A compound key with a truncated part answers what one source answers, on a month boundary.**
///
/// Usage on 06-30 meets June's snapshot and usage on 07-01 meets July's, so subscription 1001's
/// minutes land under mobile on the 30th and under tv on the 1st. Joining on `subscription_key` alone
/// would add both months to each day, and a link on the raw day would match nothing.
#[test]
fn a_compound_key_with_a_truncated_part_answers_what_one_source_answers() {
    let corpus = with_a_july_snapshot("federated-differential-compound-truncated", "");
    let Compiled::Federated { plan } = compiled_question(&corpus.two_source, VOICE) else {
        panic!("the question is two legs");
    };
    let fact_keys = plan.fact().keys();
    let labels: Vec<&str> = fact_keys.iter().map(PlanKey::label).collect();
    assert_eq!(
        labels,
        [
            InternalLabel::link(0).label(),
            InternalLabel::Link { leg: 0, part: 1 }.label()
        ],
        "the fact leg carries one link column per key"
    );
    assert!(
        matches!(fact_keys.get(1).map(PlanKey::value), Some(KeyValue::Truncated { .. })),
        "the second part is the usage day truncated at the fact's own source"
    );
    let expected = voice_rows(vec![
        row(Some("mobile"), "2026-06-30", 7.5),
        row(Some("tv"), "2026-06-30", 2.5),
        row(Some("tv"), "2026-07-01", 4.0),
    ]);
    agree_with_the_literal(&corpus, VOICE, "a-compound-truncated-key", &expected);
}

/// **Two plain parts answer what one source answers.** The key meets the snapshot only on the first of
/// a month: the 30th matches no row and keeps its minutes under a null family, the 1st meets July's.
#[test]
fn a_compound_key_of_two_plain_parts_answers_what_one_source_answers() {
    let corpus = with_a_july_snapshot("federated-differential-compound-plain", "");
    let expected = voice_rows(vec![row(None, "2026-06-30", 10.0), row(Some("tv"), "2026-07-01", 4.0)]);
    agree_with_the_literal(&corpus, VOICE_ON_THE_FIRST, "a-compound-plain-key", &expected);
}

/// **A null part matches no snapshot row, on one source and on two.** The usage row with no
/// subscription keeps its minutes under a null family: `NULL = x` is not true in the join of either
/// topology, and the row is not dropped.
#[test]
fn a_null_key_part_matches_no_lookup_row_as_on_one_source() {
    let corpus = with_a_july_snapshot("federated-differential-compound-null", UNATTRIBUTED_USAGE);
    let expected = voice_rows(vec![
        row(Some("mobile"), "2026-06-30", 7.5),
        row(Some("tv"), "2026-06-30", 2.5),
        row(Some("tv"), "2026-07-01", 4.0),
        row(None, "2026-07-01", 5.0),
    ]);
    agree_with_the_literal(&corpus, VOICE, "a-compound-key-with-a-null-part", &expected);
}

/// **A part declared as a kind that cannot join is refused before either leg runs, whichever part it
/// is.** The truncated part reaches the combine as a date and the plain one as an integer, so a text
/// snapshot column on either never matches. The control is the corpus as derived, which federates.
#[test]
fn a_compound_key_part_against_a_text_target_is_refused_before_either_leg_runs() {
    let control = with_a_july_snapshot("federated-differential-compound-types-control", "");
    assert!(
        matches!(compiled_question(&control.two_source, VOICE), Compiled::Federated { .. }),
        "the control declares both parts as they are stored and must federate"
    );
    for (stem, find, with) in [
        (
            "first-part",
            "  - name: subscription_key\n    type: BIGINT",
            "  - name: subscription_key\n    type: VARCHAR",
        ),
        (
            "second-part",
            "  - name: month\n    type: DATE",
            "  - name: month\n    type: VARCHAR",
        ),
    ] {
        let corpus = with_a_july_snapshot(&format!("federated-differential-compound-types-{stem}"), "");
        apply(
            &corpus.two_source.join("models/subscriptions.md"),
            &Edit::Rewrite { find, with },
        );
        let Compiled::Refused { reason } = compiled_question(&corpus.two_source, VOICE) else {
            panic!("{stem}: a text part against a stored date or integer is refused, not planned");
        };
        assert_eq!(reason.code(), "federation_link_type_mismatch", "{stem}: {reason:?}");
    }
}

/// The ratio's two links over one lookup, each a month and a contract term: the first fact declares
/// the month first and the second declares the term first.
fn with_two_keyed_links(stem: &str) -> Derived {
    let derived = with_fiscal_months(stem);
    write(
        &derived.data.join("dim_fiscal_month.csv"),
        "month,contract_term,fiscal_quarter\n2026-05-01,monthly,FY26-Q4\n2026-05-01,annual,FY27-Q1\n\
         2026-06-01,monthly,FY26-Q4\n2026-06-01,annual,FY27-Q1\n2026-07-01,monthly,FY26-Q4\n2026-07-01,annual,FY27-Q1\n",
    );
    write(
        &derived.data.join("fct_ticket_monthly.csv"),
        "month,customer_key,tickets,opened_on,contract_term\n2026-05-01,2,4,2026-05-17,monthly\n\
         2026-05-01,5,3,2026-05-02,annual\n2026-06-01,1,2,2026-06-30,annual\n2026-06-01,2,3,2026-06-03,monthly\n",
    );
    for catalog in [&derived.one_source, &derived.two_source] {
        // `usage_fiscal_month` declares the lookup unique on its month alone, which it no longer is.
        for gone in ["relationships/usage_fiscal_month.md", "metrics/voice_by_fiscal_quarter.md"] {
            std::fs::remove_file(catalog.join(gone)).expect("the derived catalog holds the usage link and its metric");
        }
        let rewrite = |at: &str, find: &'static str, with: &'static str| {
            apply(&catalog.join(at), &Edit::Rewrite { find, with });
        };
        rewrite(
            "models/fiscal_months.md",
            "  - name: fiscal_quarter\n    type: VARCHAR\n",
            "  - name: contract_term\n    type: VARCHAR\n    description: The contract length the row applies to.\n    nullable: false\n  - name: fiscal_quarter\n    type: VARCHAR\n",
        );
        rewrite(
            "models/fiscal_months.md",
            "primary_key: [month]",
            "primary_key: [month, contract_term]",
        );
        rewrite(
            "models/tickets.md",
            "    description: The day the ticket was opened.\n    nullable: false\n",
            "    description: The day the ticket was opened.\n    nullable: false\n  - name: contract_term\n    type: VARCHAR\n    description: The contract length of the subscription the ticket concerns.\n    nullable: false\n",
        );
        rewrite(
            "relationships/subscription_fiscal_month.md",
            "  - { origin: month, grain: month, target: month }\n",
            "  - { origin: month, grain: month, target: month }\n  - { origin: contract_term, target: contract_term }\n",
        );
        rewrite(
            "relationships/ticket_fiscal_month.md",
            "  - { origin: opened_on, grain: month, target: month }\n",
            "  - { origin: contract_term, target: contract_term }\n  - { origin: opened_on, grain: month, target: month }\n",
        );
    }
    derived
}

/// **A second fact linking through the same compound key answers the ratio**, however it orders the
/// key's parts. May's and June's snapshots hold 42 monthly and 20 annual subscriptions each, and the
/// tickets are 4 and 3 in May and 3 and 2 in June, by term. A link that dropped the term would meet two
/// lookup rows that disagree on the quarter and be refused instead.
#[test]
fn a_second_fact_linking_through_a_compound_key_answers_the_ratio() {
    let corpus = with_two_keyed_links("federated-differential-compound-two-fact");
    let query: Query = serde_norway::from_str(TWO_FACT_BY_QUARTER).expect("the question is a question");
    let Compiled::Federated { plan } = compile(
        &query,
        &ScopedView::everything(&bundle(&corpus.two_source)),
        RowCeiling::DEFAULT,
    )
    .expect("the question compiles") else {
        panic!("a two-fact question over the two-source catalog federates");
    };
    let second = plan
        .second_fact()
        .expect("the plan reads the second fact in a leg of its own");
    let second_keys = second.keys();
    assert_eq!(
        second_keys.iter().map(PlanKey::label).collect::<Vec<_>>(),
        [
            InternalLabel::link(0).label(),
            InternalLabel::Link { leg: 0, part: 1 }.label()
        ],
        "the second fact carries the first fact's parts, whatever order it declares its keys in"
    );
    assert!(
        matches!(second_keys.first().map(PlanKey::value), Some(KeyValue::Truncated { .. }))
            && matches!(second_keys.get(1).map(PlanKey::value), Some(KeyValue::Column(_))),
        "part 0 is the ticket's day truncated to its month and part 1 the contract term as stored"
    );
    let real = |figure: f64| Value::Real(Real::parse(figure).expect("a hand-worked ratio is finite"));
    let text = |text: &str| Value::Text(text.into());
    let expected = RowSet::new(
        vec![
            String::from("fiscal_quarter"),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            String::from("tickets_per_subscription_by_fiscal_quarter"),
        ],
        vec![
            vec![text("FY26-Q4"), text("2026-05-01"), real(0.095_238_095_238_095_23)],
            vec![text("FY27-Q1"), text("2026-05-01"), real(0.15)],
            vec![text("FY26-Q4"), text("2026-06-01"), real(0.071_428_571_428_571_42)],
            vec![text("FY27-Q1"), text("2026-06-01"), real(0.1)],
        ],
    )
    .expect("the hand-worked answer is rectangular");
    let engines = side(validating_on_two_engines(&corpus.data, bundle(&corpus.two_source)));
    let answer = rows(&engines, TWO_FACT_BY_QUARTER, "a-second-fact-over-a-compound-key");
    if let Err(d) = agree_on_content(&expected, &answer, RealTolerance::DIFFERENTIAL) {
        panic!("two engines did not answer the hand-worked ratios - {d}\n{answer:?}");
    }
}
