//! One plan, computed by the engine and by every registered data system, compared.
//!
//! The sides are not several implementations of one thing, and reading them that way overstates what
//! this proves. `DataFusion` is the ENGINE, executing a plan locally over Arrow. A data source is
//! something the compiler renders a statement for and pushes down to. Comparing them is comparing "we
//! computed it here" against "we asked a database to compute it", and for a simple aggregate they
//! agree trivially.
//!
//! **So this is a cheap regression net, not a proof of correctness.** It is kept for one class of bug
//! nothing else here catches: a rendered statement that is VALID SQL with different semantics. Every
//! such bug found while building the renderer was of that kind - a truncated date coming back as a
//! timestamp, an integer division silently truncating, a week starting on the wrong day. An anchor
//! check compares one number and would miss most of them; the parse golden proves a statement is well
//! formed and says nothing about what it means. A row-by-row comparison against a real SQL engine is
//! what covers the gap between those two.
//!
//! **It is a cell of the data-system axis, expanded from `adapters::registered` like the rest.** It was
//! a hand-written comparison of two named adapters, which meant a third data system was compared
//! against nothing until somebody edited this file. Now registering one enrols it here too.
//!
//! The engine's own cell compares two independently opened engines rather than nothing: that is a
//! determinism check, which is weaker than the comparison the other cells make and is what "compare
//! this against the reference" degenerates to when the entry *is* the reference. It is named as such on
//! the assertion rather than skipped, because a skipped cell reads as coverage.
//!
//! **The two-source half of the same comparison is `differential/federated.rs`.** This file compares
//! one plan across data systems; that one compares one QUESTION across topologies - answered whole by
//! one engine, and split across two data systems and combined - which is the only place the splitter,
//! two real executions and the combiner run as one path.
//!
//! What this is NOT: a reason to keep two execution paths. When federation moves the engine above the
//! `Warehouse` port, `DataFusion` stops being a peer of a data source and this test's shape changes
//! with it.
//!
//! Two things it caught on first being written are noted on the assertions below. Both were shallow -
//! column labels and row ordering - which is about the yield to expect from it.
//!
//! **What makes two answers the same answer is decided in one place, and it is not here.**
//! `sutura_domain::warehouse::agreement` holds the policy; this file and the `BigQuery` acceptance leg
//! both call it. Each used to hold its own copy, and both copies compared cells through
//! `Value::render` - a display form, so the cell TYPE was erased and a null compared equal to the
//! text `"null"`. That module's header carries the finding, the four properties the policy holds and
//! the limits.

#[cfg(test)]
#[path = "adapters/adapters.rs"]
mod adapters;

// `#[path]` for the reason `tests/golden.rs` gives: a bare `mod federated;` at a crate root resolves
// to `tests/federated.rs`, which cargo would build as a test target of its own - and this module has
// to be a submodule of THIS target rather than one of its own, because `adapters` is dead-code-clean
// only where a target reaches all of it.
#[cfg(test)]
#[path = "differential/federated.rs"]
mod federated;

#[cfg(test)]
mod tests {
    use sutura_app::{answer, verify_anchors};
    use sutura_domain::query::ToolOutcome;
    use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
    use sutura_domain::warehouse::{RowSet, Value};

    use crate::adapters::{DataSystemUnderTest, ReferenceCatalog, load, open, questions, read_question, stem};

    /// The side every registered data system is compared against.
    ///
    /// The ENGINE, and that is the reason rather than an alphabetical accident: a data source exists to
    /// run a subplan of what the engine could otherwise compute itself, so the engine's answer is the
    /// one a pushdown has to reproduce. It is named here rather than in the registry because it is what
    /// THIS file is about; the registry says what the entries are, not which of them is the yardstick.
    type Engine = sutura_exec_datafusion::DataFusionWarehouse;

    /// The two sides of one question, compared - through the ONE shared policy.
    ///
    /// `sutura_domain::warehouse::agreement` decides what makes two answers the same answer, and the
    /// `BigQuery` acceptance leg calls the same two functions. What used to be here was a local
    /// `rendered()` that compared cells through `Value::render`, and its copy in that leg compared
    /// them the same way - so both erased the variant, and a `Null` answered as the text `"null"` or a
    /// count answered as the text `"1"` compared EQUAL in a comparison whose whole job is to find
    /// exactly that class of divergence. The float tolerance that comment argued for survives as
    /// `RealTolerance::DIFFERENTIAL`, where its reasoning is written once and applies to
    /// `Value::Real` and to nothing else.
    ///
    /// Content first and order second, deliberately: the first symptom of a wrong number would
    /// otherwise be reported as a sort order. Both are asserted, because a plan that emits `ORDER BY`
    /// claims an order - this corpus's questions all do, which is why there is no third argument
    /// asking whether one was promised.
    fn agreement_between(name: &str, against: &str, from_engine: &RowSet, from_other: &RowSet) {
        if let Err(disagreement) = agree_on_content(from_engine, from_other, RealTolerance::DIFFERENTIAL) {
            panic!("{name}: the engine and {against} returned different rows - {disagreement}");
        }
        if let Err(disagreement) = agree_on_order(from_engine, from_other, RealTolerance::DIFFERENTIAL) {
            panic!(
                "{name}: the engine and {against} returned the same rows in different orders, and the \
                 plan's ORDER BY claims one order - {disagreement}"
            );
        }
    }

    /// One cell, as a whole result, for the two comparisons below.
    fn one_cell(label: &str, cell: Value) -> RowSet {
        RowSet::new(vec![String::from(label)], vec![vec![cell]]).expect("a one-cell result is rectangular")
    }

    /// **The comparison this leg makes is type-aware, and this is where that stops being a claim.**
    ///
    /// It runs with no data system at all, because [`agreement_between`] is a pure function of two
    /// results - so the property is checked on every `just test` rather than only where an adapter is
    /// available. Against the comparator this replaced, both of these PASSED: `Value::render` answers
    /// `"null"` for a null and for the word, so the two compared equal.
    ///
    /// **The `expected` string names the CONTENT diagnosis, and it has to.** It stopped at *the
    /// engine and a data system* first, which is a prefix of both panics [`agreement_between`] can
    /// raise - so with `agree_on_content` made vacuous these two cells stayed green on the order
    /// panic while five of `sutura_domain::warehouse::agreement`'s own tests reddened. A cell that
    /// passes for the wrong reason reads as coverage.
    #[test]
    #[should_panic(
        expected = "a-null-is-not-the-word-null: the engine and a data system returned different rows \
                    - one side answered a row 1 time(s) and the other 0 time(s)"
    )]
    fn a_null_and_the_word_null_do_not_agree_in_this_leg_s_comparison() {
        agreement_between(
            "a-null-is-not-the-word-null",
            "a data system",
            &one_cell("region", Value::Null),
            &one_cell("region", Value::Text(String::from("null"))),
        );
    }

    /// The other half of the same hole: a count and the text of that count.
    #[test]
    #[should_panic(
        expected = "an-integer-is-not-its-text: the engine and a data system returned different rows \
                    - one side answered a row 1 time(s) and the other 0 time(s)"
    )]
    fn an_integer_and_its_own_text_do_not_agree_in_this_leg_s_comparison() {
        agreement_between(
            "an-integer-is-not-its-text",
            "a data system",
            &one_cell("subscriptions", Value::Integer(1)),
            &one_cell("subscriptions", Value::Text(String::from("1"))),
        );
    }

    /// An error and every cause beneath it, as one string.
    ///
    /// `Display` on a `thiserror` enum prints the outermost message and stops, and the outermost message
    /// from the service is "the data system did not answer" - true of an outage, a rejected statement
    /// and a cell that could not be carried alike. What tells those apart is one and two levels down.
    fn chain(error: &dyn core::error::Error) -> String {
        let mut out = error.to_string();
        let mut cursor = error.source();
        while let Some(cause) = cursor {
            out.push_str("\n  caused by: ");
            out.push_str(&cause.to_string());
            cursor = cause.source();
        }
        out
    }

    /// Both sides of `zero_denominator: fails` failed, and this asserts they failed HONESTLY, per
    /// adapter.
    ///
    /// The measure is projected under the metric's own name, so that is the column both sides have to
    /// name where they can. Compared through the chain because the outermost message is "the data
    /// system did not answer", which is true of every failure.
    ///
    /// The ENGINE side must name the column and the non-finite cell, as it always does. The OTHER side
    /// has to fail too, but the HOW is that data system's own business: an IEEE one (`DuckDB`) refuses a
    /// non-finite cell and names the column, while a raising one (`Postgres`) gets the server's typed
    /// `division by zero`. Both honor `zero_denominator: fails`, and the assertion is a type switch on
    /// the adapter rather than one weakened shape for both.
    fn refused_together<W>(
        engine_error: &dyn core::error::Error,
        other_error: &dyn core::error::Error,
        name: &str,
        metric: &dyn core::fmt::Display,
    ) where
        W: DataSystemUnderTest,
    {
        let expected = format!("column {metric}");
        let rendered_engine = chain(engine_error);
        assert!(
            rendered_engine.contains(&expected),
            "{name}: {} did not name the column it could not carry:\n{rendered_engine}",
            <Engine as DataSystemUnderTest>::NAME
        );
        assert!(
            rendered_engine.contains("is not a finite number"),
            "{name}: {} failed for some other reason:\n{rendered_engine}",
            <Engine as DataSystemUnderTest>::NAME
        );
        let rendered_other = chain(other_error);
        match W::NAME {
            "postgres" => {
                assert!(
                    rendered_other.contains("division by zero"),
                    "{name}: postgres neither refused a non-finite cell nor reported the server's \
                 division-by-zero:\n{rendered_other}"
                );
            }
            adapter => {
                assert!(
                    rendered_other.contains(&expected) && rendered_other.contains("is not a finite number"),
                    "{name}: {adapter} neither named the column it could not carry nor refused it as \
                 non-finite:\n{rendered_other}"
                );
            }
        }
    }

    /// Whether this entry is the reference itself, for a message that says which comparison was made.
    ///
    /// A `&str` comparison rather than a type-id one: the registry's whole currency is the name, and an
    /// entry that renamed itself into the reference's name would be a bigger problem than this.
    fn is_the_reference<W>() -> bool
    where
        W: DataSystemUnderTest,
    {
        W::NAME == <Engine as DataSystemUnderTest>::NAME
    }

    /// Every question, answered by the engine and by `W`, compared row for row.
    ///
    /// A wrong number has to be produced twice, the same way, by the engine and by the data source -
    /// one building a logical plan over Arrow, one executing rendered SQL.
    ///
    /// Two things this caught when it was first written, both of which a snapshot would have happily
    /// pinned as correct: the column LABELS disagreed, because one engine took them from the driver's
    /// result schema and the other built them from the plan; and the two disagreed on row ORDER until
    /// both sorted by the grouped expressions.
    // No `#[expect(clippy::too_many_lines)]` any more, and that is worth a line rather than a silent
    // deletion, because neither half of the story is visible from here. This branch added the two
    // working-set arguments to the `answer` calls below, which took the body from 99 code lines to 101
    // and made the suppression correct against its own base. `main` then lifted the both-sides-refused
    // assertions out into `refused_together`, which brought it back to 90 - so the cause is gone, and
    // *a suppression cannot outlive its cause*.
    //
    // The lesson is about WHICH lint this was: a threshold lint's cause is a number, so two branches
    // can each move it correctly and only the merge is wrong. Both were green alone. The next change
    // that pushes this body over 100 is the one that decides whether more of it moves out the way
    // `refused_together` did, or the expectation comes back.
    fn agrees_with_the_engine_on_every_question<W>()
    where
        W: DataSystemUnderTest,
    {
        if !W::available() {
            return;
        }
        let pinned = load::<ReferenceCatalog>();
        // Two registries, each holding one adapter under the SAME source name - which is the whole
        // instrument: one plan, run through two data systems that both answer to `local`, rows
        // compared. `Warehouses` is keyed by the adapter's own source, so the two cannot be in one
        // registry, and that is correct rather than awkward: a deployment holds one adapter per source.
        let engine = sutura_app::Warehouses::of(open::<Engine>(&pinned));
        let other = sutura_app::Warehouses::of(open::<W>(&pinned));
        let against = if is_the_reference::<W>() {
            "a second, independently opened engine"
        } else {
            "the engine"
        };
        let validated = sutura_app::verify_and_validate(pinned, &engine).expect("the anchors hold");

        let mut compared = 0_usize;
        let mut refused = 0_usize;
        for path in questions() {
            let question = read_question(&path);
            let name = stem(&path);

            let from_engine = answer(
                &validated,
                &question,
                &crate::adapters::a_caller(),
                &crate::adapters::shared_credential(),
                &engine,
                1 << 30,
            );
            let from_other = answer(
                &validated,
                &question,
                &crate::adapters::a_caller(),
                &crate::adapters::shared_credential(),
                &other,
                1 << 30,
            );

            // A third outcome, and it is the one that used to be missing.
            // `revenue_per_churned_subscription` declares `zero_denominator: fails`, and its January
            // statement divides by zero: both sides cast the numerator to a floating type first, so both
            // got `inf` back, so both ANSWERED and this test compared "inf" against "inf" and passed.
            // The two sides now have to fail together, for the same column, which is a comparison
            // rather than an unwrap.
            let (from_engine, from_other) = match (from_engine, from_other) {
                (Ok(engine_outcome), Ok(other_outcome)) => (engine_outcome.into_outcome(), other_outcome.into_outcome()),
                (Err(ref engine_error), Err(ref other_error)) => {
                    refused_together::<W>(engine_error, other_error, &name, &question.metric());
                    refused = refused.saturating_add(1);
                    continue;
                }
                (engine_result, other_result) => {
                    panic!(
                        "{name}: {} answered and {against} did not, or the other way about\n  engine: \
                         {engine_result:?}\n  {}: {other_result:?}",
                        W::NAME,
                        W::NAME
                    );
                }
            };

            match (from_engine, from_other) {
                (ToolOutcome::Answer { rows: ref a, .. }, ToolOutcome::Answer { rows: ref b, .. }) => {
                    agreement_between(&name, against, a, b);
                    compared = compared.saturating_add(1);
                }
                (ToolOutcome::Refusal { reason: ref a }, ToolOutcome::Refusal { reason: ref b }) => {
                    // A refusal is decided by the compiler, above both adapters, so the two must always
                    // agree. If they ever do not, something below the plan is deciding governance,
                    // which is the thing that must never happen.
                    assert_eq!(
                        format!("{a:?}"),
                        format!("{b:?}"),
                        "{name}: {} and {against} refused for different reasons",
                        W::NAME
                    );
                }
                (engine_outcome, other_outcome) => {
                    panic!(
                        "{name}: one side answered and the other refused\n  engine: \
                         {engine_outcome:?}\n  {}: {other_outcome:?}",
                        W::NAME
                    );
                }
            }
        }
        assert!(
            compared > 0,
            "no question produced an answer from both sides, so this compared nothing"
        );
        // And at least one that both sides refused, because the corpus is what decides whether the
        // third arm above is reachable. Without a question that reaches `zero_denominator: fails`, that
        // arm is dead and this test is back to comparing "inf" against "inf" the day one arrives.
        assert!(
            refused > 0,
            "no question was refused by both sides, so the value check at the port proved nothing here"
        );
    }

    /// An anchor is the number somebody certified.
    ///
    /// Checking it both ways is what makes "the definition still means what it claimed" independent of
    /// which side computed it, and it is what would catch an arithmetic difference between two adapters
    /// - exactly the class of thing a one-sided anchor check cannot see.
    fn reproduces_every_anchor_the_engine_does<W>()
    where
        W: DataSystemUnderTest,
    {
        if !W::available() {
            return;
        }
        let pinned = load::<ReferenceCatalog>();
        let engine = sutura_app::Warehouses::of(open::<Engine>(&pinned));
        let other = sutura_app::Warehouses::of(open::<W>(&pinned));
        let from_engine = verify_anchors(&pinned, &engine);
        let from_other = verify_anchors(&pinned, &other);
        assert_eq!(
            from_engine.checks(),
            from_other.checks(),
            "the engine and {} disagree about whether the anchors hold",
            W::NAME
        );
        assert!(
            !from_engine.checks().is_empty(),
            "the example catalog declares no anchor, so this proved nothing"
        );
        // And once more through the operation that mints the proof. It re-runs the anchors rather than
        // being handed the report above, which is the point: a report is evidence a caller could have
        // written by hand, and a `Validated` bundle is not.
        drop(
            sutura_app::verify_and_validate(pinned, &other).expect("a data system that reproduced every anchor is fit to serve"),
        );
    }

    /// One cell of the comparison.
    macro_rules! cell {
        ($name:ident, $adapter:ty) => {
            mod $name {
                #[test]
                fn it_agrees_with_the_engine_on_every_question() {
                    super::agrees_with_the_engine_on_every_question::<$adapter>();
                }

                #[test]
                fn it_reproduces_every_anchor_the_engine_does() {
                    super::reproduces_every_anchor_the_engine_does::<$adapter>();
                }
            }
        };
    }

    crate::adapters::registered!(data_systems: cell);
}
