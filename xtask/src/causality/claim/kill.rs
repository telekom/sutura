//! Whether a mutated run killed its cell - the panic-site verdict, and the guard that keeps a
//! run the gate never attested to from reading as one.
//!
//! Split out of `claim.rs` (`the 1000-line cap is unexemptable under xtask/`): the panic-site
//! classification (`MutationKill`, `classify_mutation` and the fn-region readers it walks) and
//! [`attest`], the ONE caller that turns a classification into the arm's own [`Cause`], are one
//! concept - *did this run's own text discriminate the cell's assertion* - and belong beside each
//! other rather than split by which one a caller happens to need.

use std::ops::Range;

use crate::causality::base;
use crate::causality::place::AddedTest;
use crate::causality::regions::{PostImage, TestScope, item_end, scope as test_scope};
use crate::causality::scoped::function_name;

use super::Cause;

/// Why a mutated run did or did not kill its cell.
///
/// The verdict's whole mechanism, reads the PANIC SITE, not the patched set. A patch whose only
/// change is `panic!` / `unwrap()` on `None` at the top of a reached function kills ANY cell that
/// reaches it - it proves reachability, not that the cell's assertion discriminates, which is the
/// "looks like coverage" test AGENTS.md refuses. So a run is a KILL only when nextest's
/// `panicked at <path>:<line>` lands in the cell's OWN file, inside the cell's OWN test fn
/// ([`test_fn_region`], located by the fn's name on the post-image, never by a line number): a
/// genuine assertion-fail panics at the assert's own line in the cell's test code, and every
/// other death - `process::exit`/`abort`/signal (no site), a panic in a production file (a
/// downstream `.expect()` in ordinary careless mutation), a panic inside ANOTHER file's test
/// region (a shared `tests/common` helper) - carries no site in the cell's own fn and is refused.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum MutationKill {
    /// The run reported `cell` failing, and a `panicked at` site landed in the cell's own file,
    /// inside the cell's own test fn - the assertion the cell carries discriminated.
    Killed,
    /// The run did not REPORT `cell` failing - a different test, a green run, or a failed compile.
    NotAsserted,
    /// The run reports `cell` failing but no `panicked at` site lands in the cell's own test fn -
    /// the cell died some other way. `site` is the first `panicked at <path>:<line>` the run
    /// carried, empty when there was none.
    NotByAssertion { site: String },
}

/// Did the run REPORT `cell` failing by its own assertion?
///
/// Pure, and the verdict's whole mechanism: a mutation "kills" a cell exactly when a run of that
/// cell, with the mutation applied, fails, names that cell, AND carries a `panicked at <path>:<line>`
/// in the cell's OWN file inside the cell's OWN test fn ([`test_fn_region`]) - only an assertion
/// fail panics there. A panic inside production (patched or not) proves reachability and not the
/// assertion; a panic inside ANOTHER file's test region (a shared `tests/common` helper) proves
/// nothing about this cell's assertion either; an exit/abort/signal death or a FAIL with no site
/// proves nothing about the assertion either. The naming goes through the SAME key the base run
/// uses ([`base::failures`] + [`base::is_scoped`] + [`AddedTest::claims`]), so a
/// mutation that reddened a DIFFERENT test reads as *does not kill* rather than as evidence. A
/// run that compiled and passed names no failure; a run that failed to compile names none either -
/// neither kills.
///
/// `read` supplies each panic site's file ([`AddedTest::file`]) and its content so the cell's own
/// test fn can be located on the POST-image - a parameter rather than a filesystem call, so the
/// classifier is testable without a checkout, exactly as the region reader itself is.
pub(super) fn classify_mutation(text: &str, cell: &AddedTest, read: &PostImage<'_>) -> MutationKill {
    let named = base::failures(text)
        .iter()
        .any(|failure| base::is_scoped(failure, std::slice::from_ref(cell)));
    if !named {
        return MutationKill::NotAsserted;
    }
    let sites = panic_sites(text);
    if sites
        .iter()
        .any(|(path, line)| is_cells_own_assertion(cell, path, *line, read))
    {
        MutationKill::Killed
    } else {
        MutationKill::NotByAssertion {
            site: sites.first().map(|(path, line)| format!("{path}:{line}")).unwrap_or_default(),
        }
    }
}

/// Is `site` at `line` in `path` the cell's OWN assertion - `path` is the cell's OWN file and the
/// line sits inside that file's own test fn, located by the fn's name on the post-image?
///
/// One comparison more than "any test region": the cell's `AddedTest` carries its file
/// ([`AddedTest::file`]), so a panic inside a SHARED helper's test region in another file - or in
/// a sibling test fn of the same module - is refused rather than read as this cell's kill.
fn is_cells_own_assertion(cell: &AddedTest, path: &str, line: usize, read: &PostImage<'_>) -> bool {
    if path != cell.file() {
        return false;
    }
    let Some(text) = read(path) else {
        return false;
    };
    let scope = test_scope(path, read);
    test_fn_region(&text, cell.name(), &scope).is_some_and(|region| region.contains(&line))
}

/// The 1-based, half-open line span of the test item `fn <name>`, located by CONTENT, not by a
/// line number carried from another image.
///
/// `scope` says what counts as test code for `text`: for a dedicated test target
/// ([`TestScope::WholeFile`]) the whole file is searched; for a mixed file the search stays
/// inside the `#[cfg(test)]` regions. The fn's own braces are walked from its `fn <name>` line,
/// so a real assertion panic - which fires at a line inside its own fn - is covered, and anything
/// outside that fn (a sibling test's line, a production caller) is not. This is what keeps the
/// kill an assertion kill and immune to line-shifts in the same patch.
fn test_fn_region(text: &str, name: &str, scope: &TestScope) -> Option<Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if !scope.covers(index + 1) {
            continue;
        }
        if !fn_line_is(line, name) {
            continue;
        }
        let last = item_end(&lines, index);
        return Some(index + 1..last + 2);
    }
    None
}

/// Is `line` a `fn <name>(` declaration - `async fn`, `pub fn`, `pub(crate) async fn`, any
/// visibility or `async` in front, included?
///
/// Reuses [`function_name`] rather than a second bare-`fn` parser: that extractor
/// already carries the shapes a test's signature actually takes (#347), and a matcher here that
/// only recognised bare `fn` never located `async fn`/`pub fn` cells - `test_fn_region` returned
/// `None` for every one and a real assertion kill read `NotByAssertion` (both #761 cells are
/// `#[tokio::test] async fn`). Fail-closed in the direction it broke: never a false `Killed`, only
/// a real kill going unrecognised.
fn fn_line_is(line: &str, name: &str) -> bool {
    function_name(line).is_some_and(|ident| ident.as_str() == name)
}

/// The `(path, line)` of every `panicked at <path>:<line>:` site nextest printed, for panics inside
/// a test's stack.
fn panic_sites(text: &str) -> Vec<(String, usize)> {
    text.lines()
        .filter_map(|line| {
            let rest = line.split_once(" panicked at ")?.1;
            let mut parts = rest.split(':');
            let path = parts.next()?.to_owned();
            let line: usize = parts.next()?.trim().parse().ok()?;
            Some((path, line))
        })
        .collect()
}

/// The claim arm's own verdict on a mutated run - [`classify_mutation`]'s answer, gated by
/// whether the run ever reached a test at all.
///
/// `classify_mutation` cannot tell "the cell ran and stayed green" from "the tree never compiled
/// enough to run it" - by design, both leave it naming no failure for the cell, and its own tests
/// pin that (`a_run_that_names_no_failure_is_not_a_kill` covers a compile error explicitly).
/// Folding a build failure into `Cause::NotKilled` there would report *the mutation does not kill
/// it* about a build the gate never attested to at all - measured on `#855`, where the CI venue's
/// own contention made this the leading hypothesis for a DIFFERENT wrong verdict that turned out
/// to have a different cause; this guard is shipped regardless, because a nested build failing for
/// an unrelated reason is a real, separate way to reach the same false verdict. `ok` and `text` are
/// exactly what [`crate::causality::runner::cargo_test`] returns, so this is pure over its result
/// and testable without a subprocess.
///
/// **THE SECOND GUARD, BELOW `could_not_attest`'s THREE NAMED SHAPES.** A subprocess killed by a
/// signal before nextest ever printed a `Summary`/`error: test run failed` trailer - an OOM kill,
/// a runner-level timeout, contention none of the three known patterns name - leaves `ok = false`
/// and a `text` `could_not_attest` does not recognise, and `classify_mutation` then answers
/// `NotAsserted` exactly as it would for a genuinely green run: the two are indistinguishable to
/// it BY DESIGN. `base.rs`'s own ordinary proof already holds this half of the asymmetry -
/// `classify`'s `failed.is_empty()` arm answers `BaseOutcome::Unattributed` rather than claiming
/// red-on-base, for a failed run naming no test at all - and this arm lacked the equivalent.
///
/// **NOT the same predicate as `classify_mutation`'s scoping**, which is why this reads
/// [`base::failures`] again rather than reusing its verdict: a run that failed because a
/// DIFFERENT, unrelated test broke (`UNRELATED`'s own shape) names a REAL failure - the run
/// executed and reported - so `NotAsserted` there still means *this cell did not fail*, a
/// legitimate `NotKilled`. Only an EMPTY failure list, paired with `!ok`, means the run never
/// reported anything at all - not about this cell, not about any other - which is the shape
/// `NotKilled` (*the mutation does not kill it*) is not entitled to claim.
pub(super) fn attest(ok: bool, text: &str, cell: &str, added: &AddedTest, read: &PostImage<'_>) -> Result<(), Cause> {
    if !ok && let Some(why) = base::could_not_attest(text) {
        return Err(Cause::BuildFailed {
            cell: cell.to_owned(),
            why: why.to_owned(),
        });
    }
    if !ok && base::failures(text).is_empty() {
        return Err(Cause::BuildFailed {
            cell: cell.to_owned(),
            why: String::from("the run failed but named no test failing at all - not a verdict about the declaration"),
        });
    }
    match classify_mutation(text, added, read) {
        MutationKill::Killed => Ok(()),
        MutationKill::NotAsserted => Err(Cause::NotKilled { cell: cell.to_owned() }),
        MutationKill::NotByAssertion { site } => Err(Cause::NotByAssertion {
            cell: cell.to_owned(),
            site,
        }),
    }
}
