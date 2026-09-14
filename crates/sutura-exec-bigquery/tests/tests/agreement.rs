//! `the_corpus_rows_agree_with_the_engine`, split out of `corpus.rs`'s own `mod tests` for that
//! file's `max-lines` cap - the same reason `naming.rs`/`support.rs` are their own files.
//!
//! Declared at `corpus.rs`'s TOP LEVEL rather than nested inside `mod tests {}`, for the same
//! reason `crates/sutura-serve/tests/served.rs` states at its own `mod harness`: a `#[path]`
//! inside an inline module resolves against THAT module's directory rather than this file's, and
//! `xtask/src/causality`'s own resolver additionally assumes a top-level declaration, so a nested
//! one read as pre-existing tests newly enabled and failed `xtask test-causality` outright. Being
//! a sibling of `tests` rather than a child means the fixtures it reads off that module -
//! `bundle`, `load_the_corpus`, `loader`, `engine`, `source`, `GrantsWhatEachSideDeclares`,
//! `posture_of_the_engine_presented`, `questions`, `stem`, `read_question`, `a_caller`,
//! `deadline`, `chain`, `DIVIDES_BY_ZERO`, `agreement_between`, `drop_the_corpus` - are
//! `pub(crate)` there instead of private, so this file can reach them at all.

use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::query::{Query, ToolOutcome};

use crate::naming::{run_token, suffixed_bundle};
use crate::support::{Connection, bounds, opened, presented};
use crate::tests::{
    DIVIDES_BY_ZERO, GrantsWhatEachSideDeclares, a_caller, agreement_between, bundle, chain, deadline, drop_the_corpus, engine,
    load_the_corpus, loader, posture_of_the_engine_presented, questions, read_question, source, stem,
};

/// One `answer` call, named so the loop below spells one call rather than eight arguments twice -
/// `locally` and `remotely` differ in which identity they grant, `engine` and `there` in which
/// data system answers, and nothing else does.
fn ask<W>(
    validated: &sutura_app::Validated<PinnedDefinitions>,
    question: &Query,
    broker: &GrantsWhatEachSideDeclares,
    warehouses: &sutura_app::Warehouses<W>,
    no_budget: &sutura_app::SpendLedger,
) -> sutura_app::Answering<W, GrantsWhatEachSideDeclares>
where
    W: sutura_domain::warehouse::Warehouse,
{
    sutura_app::answer(
        validated,
        question,
        &a_caller(),
        broker,
        warehouses,
        1 << 30,
        deadline(),
        no_budget,
    )
}

#[test]
#[ignore = "needs a real BigQuery project and dataset, named in the developer's own environment"]
fn the_corpus_rows_agree_with_the_engine() {
    // **`docs/adr/0017`'s second bullet, which is the one the smoke leg cannot reach at all.** One
    // plan, computed by the engine over Arrow and pushed down to `BigQuery` as `GoogleSQL`, rows
    // compared. A wrong number has to be produced twice, the same way, by two things that share
    // nothing below the plan.
    //
    // This is the check that reaches `ISOWEEK` and `DATE_TRUNC`'s argument order. Both render and
    // parse cleanly when wrong - `docs/adr/0017` measured that - so a golden cannot see them and
    // this can: a Sunday bucketed into the wrong week is a different row here.
    // Every table is named with THIS run's token and a per-test leg suffix, so this test and
    // its two siblings run under nextest's default parallelism without touching each other's
    // tables. The plan is compiled against the suffixed bundle, the engine reads the same
    // suffixed names, and the tables are dropped when the test finishes.
    let token = run_token();
    let committed = bundle();
    let pinned = suffixed_bundle(&committed, &token, "rows");
    let warehouse = opened(source(), Connection::required(), bounds());
    let loaded = load_the_corpus(&committed, &pinned, &loader());
    assert!(
        loaded > 1000,
        "the example corpus is over a thousand rows and {loaded} loaded"
    );

    let engine = sutura_app::Warehouses::of(engine(&committed, &pinned));
    let there = sutura_app::Warehouses::of(warehouse);
    let validated = sutura_app::verify_and_validate(pinned.clone(), &engine).expect("the anchors hold against the engine");
    let locally = GrantsWhatEachSideDeclares {
        presented: posture_of_the_engine_presented,
    };
    let remotely = GrantsWhatEachSideDeclares { presented };
    let no_budget = sutura_app::SpendLedger::no_budget();

    let mut compared = 0_usize;
    let mut refused = 0_usize;
    let mut excluded = 0_usize;
    for path in questions() {
        let name = stem(&path);
        let question = read_question(&path);
        let from_engine = ask(&validated, &question, &locally, &engine, &no_budget);
        let from_bigquery = ask(&validated, &question, &remotely, &there, &no_budget);

        let (here, over_there) = match (from_engine, from_bigquery) {
            (Ok(one), Ok(other)) => (one.into_outcome(), other.into_outcome()),
            (Err(ref locally), Err(ref remotely)) => {
                // **The one excluded question, and it is excluded from the COMPARISON and not from
                // the run.** Both sides fail and the reasons legitimately differ: the engine
                // returns `inf` and the port refuses a non-finite value, while `GoogleSQL` raises
                // on a zero divisor and the endpoint answers `400`. Each side is checked against
                // its own expected reason rather than against the other's, which is what makes
                // this an assertion instead of a shrug.
                assert_eq!(name, DIVIDES_BY_ZERO, "{name}: both sides failed and only one question may");
                let here = chain(locally);
                assert!(
                    here.contains("is not a finite number"),
                    "{name}: the engine failed otherwise:\n{here}"
                );
                let over_there = chain(remotely);
                // The endpoint's own words are not asserted on - Hyrum's Law applies to a service's
                // message as much as to ours, and this repository does not pin one. What is
                // asserted is that the failure came from the DATA SYSTEM rather than from the
                // compiler or the credential path.
                assert!(
                    over_there.contains("the data system did not answer"),
                    "{name}: BigQuery failed somewhere other than at the data system:\n{over_there}"
                );
                println!("bigquery-corpus: {name} failed on both sides, for their own reasons");
                excluded = excluded.saturating_add(1);
                continue;
            }
            (one, other) => {
                panic!("{name}: one side answered and the other did not\n  engine: {one:?}\n  bigquery: {other:?}");
            }
        };

        match (here, over_there) {
            (ToolOutcome::Answer { rows: ref a, .. }, ToolOutcome::Answer { rows: ref b, .. }) => {
                agreement_between(&name, a, b);
                compared = compared.saturating_add(1);
            }
            (ToolOutcome::Refusal { reason: ref a }, ToolOutcome::Refusal { reason: ref b }) => {
                // A refusal is decided by the compiler, above both adapters, so the two must always
                // agree. If they ever do not, something below the plan is deciding governance.
                assert_eq!(
                    format!("{a:?}"),
                    format!("{b:?}"),
                    "{name}: the engine and BigQuery refused for different reasons"
                );
                refused = refused.saturating_add(1);
            }
            (one, other) => {
                panic!("{name}: one side answered and the other refused\n  engine: {one:?}\n  bigquery: {other:?}");
            }
        }
    }
    // **The counts have to add up to the corpus, and that identity is what stops this test
    // silently shrinking.** A floor alone ("more than eight agreed") would pass a run that
    // `continue`d past half the corpus; a fixed expected total would be a test edit every time a
    // question is added. The sum is neither: it is a function of the directory.
    let total = questions().len();
    assert_eq!(
        compared.saturating_add(refused).saturating_add(excluded),
        total,
        "{compared} agreed + {refused} refused + {excluded} excluded is not the {total} questions \
     in the corpus"
    );
    assert!(compared > 8, "only {compared} questions produced rows from both sides");
    assert!(
        refused > 0,
        "no question was refused by both sides, so the compile-side corpus proved nothing here"
    );
    assert_eq!(
        excluded, 1,
        "the divide-by-zero question is the only exclusion and it has to be reached"
    );
    drop_the_corpus(&pinned, &loader());
    println!(
        "bigquery-corpus: {compared} answers agreed exactly on content AND order, {refused} refusals \
     agreed, {excluded} excluded, {total} in the corpus"
    );
}
