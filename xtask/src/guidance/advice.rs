//! The advice a failure PRINTS, held to the citation standard its prose is already held to.
//!
//! `AGENTS.md` states the rule as *"Cite a `just` task, never a raw command line"*. Until this
//! check the rule had a mechanism only where a reader is DOCUMENTED to: [`super::run`]'s scope is
//! `.md`, `.nix`, `.yml`, `.yaml`, `.toml` and `.sh`, and that module's own header confesses the
//! hole - no gate in this repository read a Rust string literal, so the rule held everywhere
//! except where a reader is actually TOLD, at the moment something failed.
//!
//! `github.com/telekom/sutura#243` is the incident. A remedy in `sutura-dev` cited
//! `xtask dev-up --with <profile>` where its four siblings cited `just` tasks - a raw command line,
//! in the one place a reader had no page to check it against - and nothing was red. That was fixed
//! with a unit test in the module that prints it, which is a mechanism for that module and not for
//! the rule: `AGENTS.md` is explicit that an invariant held by recall is not held, and the next
//! crate to print a command line at the moment of failure would have gone the same way.
//!
//! # What this checks
//!
//! Every `*.rs` file the repository publishes, production regions only: a backtick span that begins
//! `just ` or `cargo xtask ` must name something that exists. Both authorities are DERIVED and
//! neither is a list kept here - the recipes come from the justfile through the one parser that
//! knows what a recipe header looks like, and the gates come from the table `--help` and dispatch
//! are already built from.
//!
//! # What it deliberately does not catch
//!
//! Stated here rather than left to be discovered, because a citation rule widened past prose
//! without a precision story becomes noise, and noise is how a gate gets disabled. Measured before
//! it was written: across `crates/`, `dev/` and `xtask/` the whole population of these citations in
//! production Rust is small, and every one of them resolves.
//!
//! * **A span that invokes something other than `just`.** The half that caught #243 - *a multi-word
//!   backtick span IS an invocation, so it has to begin `just `* - is not generalised here, and that
//!   is the precision trade. This tree prints `` `nix run .#crap` ``, `` `nix build .#checks...` ``
//!   and shell fragments in backticks legitimately, so the general form would fire on correct
//!   advice. It stays a unit test in the module whose four sibling remedies establish the house
//!   form. **So this gate closes the rename half and not the raw-command-line half.**
//! * **An interpolated NAME.** `` `just dev-up-{profile}` `` becomes a recipe only once the format
//!   argument is known, and a gate may not guess. An interpolated ARGUMENT is a different thing and
//!   IS checked - `` `just dev-endpoint {}` `` names its task in full - see [`task_name`], which
//!   records what the lazier reading cost.
//! * **A citation with no backticks.** `"just crap"` inside an aligned usage block is not read as a
//!   span, the same limit every citation checker here carries: "just" is an English adverb this repo
//!   uses constantly, and the backticks are what separate the citation from the adverb.
//! * **A path, a flag, a placeholder or a flake attribute.** Only the two invocation prefixes are
//!   read. A path citation in a printed string is usually built at runtime from a worktree root, so
//!   resolving it against this tree would report a file that was never claimed to be in it.
//! * **Test code**, by the same region walk `crate::refusals` uses, and for the same reason: a
//!   fixture is not advice. `xtask`'s own tests cite `just no-such-recipe` in order to prove the
//!   checker reports it, and a scan that read fixtures would report its own reasoning - the failure
//!   mode this repository has already deleted a gate over. **That walk treats a whole `tests/`
//!   target as test code**, so a skip reason on an `ignore` attribute in an integration test is out
//!   of reach even though a developer reads it at the moment the test skips. Two such citations
//!   exist and both resolve, measured; narrowing that exclusion needs a way to tell a fixture from
//!   advice inside one integration file, which the region walk cannot do.
//! * **A comment.** A stale comment beside Rust code is reviewed with that code, which is the
//!   position [`super`] already takes. The one over-reach is a trailing `// ...` on a line that also
//!   carries code: its spans are read, and a citation there must exist like any other.

use std::path::Path;

use crate::causality::regions;

/// Where a `.rs` file's citations are somebody else's claim about their own tree.
const MIRRORED: &str = "vendor/";

/// What a printed span sends a reader to. Two shapes and no third - see the module header.
enum Cited<'a> {
    /// A `just <name>` recipe.
    Recipe(&'a str),
    /// A `cargo xtask <name>` gate.
    Gate(&'a str),
}

/// The task name at the start of `tail`, unless an interpolation is part of the NAME.
///
/// [`super::task_name_at`] does the parse - a flag (`just --list`) and a placeholder
/// (`just <task>`) are correct prose in a page and correct prose in a printed line, and a second
/// copy of that judgement would be a second thing to keep true. What is added here is the one
/// judgement a printed line needs and a page does not: a format argument.
///
/// **The distinction is between an interpolated ARGUMENT and an interpolated NAME**, and getting it
/// wrong in the safe-looking direction is what this function exists to record. Skipping any span
/// holding a brace looks conservative and is nearly vacuous: `` `just dev-endpoint {}` `` is most of
/// the population, its task name is fully written down, and dropping it left the check green over
/// the very citation `github.com/telekom/sutura#243` is about. So the argument is ignored and the
/// name is resolved. `` `just dev-up-{profile}` `` is the other case - the name itself is half a
/// literal, no scan can complete it, and the module that prints it holds that half by enumerating
/// its services.
fn task_name(tail: &str) -> Option<&str> {
    let rest = tail.trim_start();
    let name = super::task_name_at(rest)?;
    // The character the name stopped at. A brace there means the name was cut short by an
    // interpolation; anything else - a space, a backtick, the end - means the name is complete and
    // whatever follows is an argument.
    (!rest.get(name.len()..).is_some_and(|after| after.starts_with('{'))).then_some(name)
}

/// Read one backtick span as a citation, or `None` when it is not one.
fn cited(span: &str) -> Option<Cited<'_>> {
    if let Some(tail) = span.strip_prefix("just ") {
        return task_name(tail).map(Cited::Recipe);
    }
    if let Some(tail) = span.strip_prefix("cargo xtask ") {
        return task_name(tail).map(Cited::Gate);
    }
    None
}

/// Is this line a comment rather than something the program hands a reader?
///
/// The whole-line form only. A trailing comment after code is not distinguished, because telling a
/// `//` marker from the one inside a URL literal needs a lexer; reading such a line's spans anyway
/// is the direction that over-checks rather than under-checks, and the module header says so.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// Every citation printed by production code in `rel`, with its 1-based line.
fn printed_citations<'text>(rel: &str, text: &'text str, read: &regions::PostImage<'_>) -> Vec<(usize, Cited<'text>)> {
    let tests = regions::scope(rel, read);
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        if tests.covers(number) || is_comment(line) {
            continue;
        }
        out.extend(super::spans(line).into_iter().filter_map(cited).map(|one| (number, one)));
    }
    out
}

/// Every task a printed line cites must exist. Returns the problems and how many citations were read.
///
/// The count comes back rather than being logged here, because a scan that reads NOTHING passes
/// everything: [`super::run`] turns zero into a failure the same way `check-scope` and the remedy
/// scan do, and for the same reason.
pub(super) fn advice_problems(root: &Path, files: &[String]) -> (Vec<String>, usize) {
    let gates = super::known_tasks();
    let recipes = crate::tasks::recipe_names(root);
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let mut problems = Vec::new();
    let mut found = 0_usize;

    for rel in in_scope(files) {
        let Some(text) = read(rel) else {
            continue;
        };
        for (line, one) in printed_citations(rel, &text, &read) {
            found = found.saturating_add(1);
            match one {
                Cited::Recipe(name) => match &recipes {
                    Some(known) if !known.contains(name) => problems.push(format!(
                        "{rel}:{line}: prints `just {name}`, which names no recipe - it was renamed, \
                         deleted or never existed"
                    )),
                    None => problems.push(format!(
                        "{rel}:{line}: prints `just {name}` and the justfile could not be read to check it"
                    )),
                    Some(_) => {}
                },
                Cited::Gate(name) if !gates.contains(name) => {
                    problems.push(format!("{rel}:{line}: prints `cargo xtask {name}`, which is not a task"));
                }
                Cited::Gate(_) => {}
            }
        }
    }
    (problems, found)
}

/// Which `.rs` files this check reads.
///
/// [`advice_problems`] iterates this rather than filtering inline, so the test that asserts the
/// scope asserts the scope the gate actually walks. A second copy of the filter would be a test
/// that restates it and passes while the walk reads something else.
fn in_scope(files: &[String]) -> std::collections::BTreeSet<&str> {
    files
        .iter()
        .map(String::as_str)
        .filter(|rel| super::has_ext(rel, &["rs"]) && !rel.starts_with(MIRRORED))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Cited, MIRRORED, advice_problems, cited, in_scope, printed_citations};

    #[test]
    fn a_printed_recipe_and_gate_are_citations_and_the_rest_is_prose() {
        assert!(matches!(cited("just dev-up"), Some(Cited::Recipe("dev-up"))));
        assert!(matches!(cited("cargo xtask hygiene"), Some(Cited::Gate("hygiene"))));
        // AN INTERPOLATED ARGUMENT IS STILL A CITATION, and this is the assertion that stops the
        // check going vacuous: skipping every span with a brace in it left `dev-endpoint` - the
        // most-printed citation in `sutura-dev` - unread, and the gate green over exactly the
        // module #243 was filed against.
        assert!(matches!(cited("just dev-endpoint {}"), Some(Cited::Recipe("dev-endpoint"))));
        assert!(matches!(
            cited("cargo xtask text-hygiene --fix"),
            Some(Cited::Gate("text-hygiene"))
        ));
        // The shapes the header promises to leave alone.
        assert!(cited("just dev-up-{profile}").is_none(), "an interpolated NAME is not a name");
        assert!(
            cited("cargo xtask {name}").is_none(),
            "a wholly interpolated name is not a name"
        );
        assert!(cited("just --list").is_none(), "a flag is not a task");
        assert!(cited("just <task>").is_none(), "a placeholder is not a task");
        assert!(
            cited("nix run .#crap").is_none(),
            "another invocation is not this gate's business"
        );
        assert!(cited("endpoints.json").is_none(), "a filename is not an invocation");
    }

    #[test]
    fn a_comment_is_not_advice_and_a_test_region_is_not_either() {
        // WHY BOTH EXCLUSIONS ARE ONE TEST: they are the same claim - only what production code
        // hands a reader is judged - and each of them is load-bearing against a real file in this
        // tree. Fifty doc comments cite `cargo xtask max-lines`, and `xtask`'s own fixtures cite a
        // recipe that deliberately does not exist.
        let source = "\
// see `just no-such-recipe`
fn advice() -> &'static str {
    \"run `just dev-up` first\"
}
#[cfg(test)]
mod tests {
    const FIXTURE: &str = \"`just also-not-a-recipe`\";
}
";
        // The region walk reads the file's POST-IMAGE through this, so the fixture has to be
        // reachable that way or it gets no regions at all and the second half of this test proves
        // nothing. That is exactly how it failed first: a reader returning `None` reads as
        // production code everywhere, which is `regions`' documented conservative direction.
        let read = |_: &str| Some(String::from(source));
        let found = printed_citations("crates/x/src/lib.rs", source, &read);
        let names: Vec<&str> = found
            .iter()
            .map(|(_, one)| match *one {
                Cited::Recipe(name) | Cited::Gate(name) => name,
            })
            .collect();
        assert_eq!(names, vec!["dev-up"], "read something other than the printed line: {names:?}");
        assert_eq!(found.first().map(|(line, _)| *line), Some(3), "the line number is wrong");
    }

    #[test]
    fn a_printed_recipe_that_names_no_task_is_reported_and_this_tree_prints_none() {
        // RED BEFORE THE FIX THIS SHIPS WITH, on this repository's own source: `sutura-dev` prints
        // `just dev-endpoint`, the recipe is written `@dev-endpoint` for quietness, and
        // `crate::tasks::recipes` kept the `@` in the name - so the only authority for "is this a
        // recipe" said no. Two hand-parsers of one file had disagreed about that prefix, which is
        // the second-thing-to-keep-true this gate's own existence argues against.
        let root = crate::repo::root().expect("the repo root");
        let crate::repo::RepoFiles { files, .. } = crate::repo::all_files().expect("could not list the repo");
        let (problems, found) = advice_problems(&root, &files);
        assert!(
            found > 0,
            "read no citation out of any printed line - the scan is broken, not the tree"
        );
        assert_eq!(problems, Vec::<String>::new(), "this tree prints a task that does not exist");
    }

    #[test]
    fn mirrored_rust_is_out_of_scope_and_ours_is_in_it() {
        // Upstream source makes claims about ITS repo, the reason `run` already keeps
        // `.agents/skill-library/` out of the prose scan. Asserted against the tree rather than a
        // fixture, so vendoring a second crate does not quietly widen this.
        let crate::repo::RepoFiles { files, .. } = crate::repo::all_files().expect("could not list the repo");
        let scope = in_scope(&files);
        assert!(
            scope.contains("dev/src/provisioned.rs"),
            "the file #243 was filed about is not read"
        );
        assert!(
            scope.contains("xtask/src/guidance/advice.rs"),
            "this gate does not read itself"
        );
        assert!(
            files
                .iter()
                .any(|rel| rel.starts_with(MIRRORED) && crate::guidance::has_ext(rel, &["rs"])),
            "the exclusion has nothing to exclude, so it proves nothing"
        );
        assert!(
            !scope.iter().any(|rel| rel.starts_with(MIRRORED)),
            "vendored Rust is in scope: {scope:?}"
        );
    }
}
