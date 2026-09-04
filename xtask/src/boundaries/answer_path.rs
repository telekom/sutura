//! A caller of the driving port reaches the answer path THROUGH it.
//!
//! The invariants tree says every outcome is recorded before it is returned, and names
//! `sutura_app::surface::LocalService::start` as what holds it: a constructor that takes an audit
//! sink and has no form that omits one, whose `answer` writes one record before the `Ok`. **A
//! caller that never builds a `LocalService` is outside that mechanism**, and `sutura query` was -
//! it called `sutura_app::answer` directly and dropped the deadline with `into_outcome`, so a
//! shipped command answered questions with no record while the row said otherwise. That is issue
//! #266's A1, and it is the failure mode this repository names as its worst: prose describing a
//! control as stronger than the code.
//!
//! # Why a gate and not the compiler
//!
//! The fix the finding offered was to narrow `answer` to `pub(crate)`, and that would be stronger -
//! a bypass that does not compile. It is not available: `crates/sutura-app/tests/golden/service.rs`
//! and `crates/sutura-exec-bigquery/tests/corpus.rs` are separate crates and they assert on
//! `ServiceError`'s VARIANTS, which the driving port erases into `SurfaceFailure` by design. Making
//! the bypass impossible that way would take the typed-error assertions out of the conformance
//! suite, which is a worse trade than this scan.
//!
//! So the rule is scoped to where it matters: the `src/` of a crate that calls the driving port -
//! the four composition roots and transports - is where a shipped bypass would live.
//!
//! # Who a caller is
//!
//! The same derived set [`super::ports`] uses, and for the same reason: a hardcoded list of
//! transports would not cover the next one. Zero callers is an error there, not a pass.
//!
//! # Limits
//!
//! * **`#[cfg(test)]` is skipped**, through [`regions::scope`] - the classifier the causality gate
//!   already owns, so "which lines of this file are test code" has one implementation. That is not
//!   a weakening: test code is not in a shipped binary, and the row is about what a published
//!   binary does. `crates/sutura-cli/src/sources.rs` holds such a call today - a unit test that
//!   answers through a declared source to prove the witness the registry carries reaches the
//!   adapter - and it is test vocabulary, like everything under `tests/`.
//! * **It reads a PATH rooted at `sutura_app`.** `use sutura_app as app;` and then `app::answer(..)`
//!   is invisible, and so is a re-export of `answer` through a third crate. `use sutura_app::*` is
//!   not a hole: `clippy::wildcard_imports` is on through `pedantic` and the gate runs with
//!   `-D warnings`.
//! * Comments and multi-line string interiors are blanked first, through the serde gate's
//!   [`code_lines`](crate::serde_parse::scan::code_lines) - which is load-bearing, because this
//!   file's own header names the forbidden path and several doc comments in the tree do too. A
//!   SINGLE-line string literal keeps its content, so one holding that path would be reported;
//!   none exists.
//! * It cannot see a caller that reaches the answer path some other way - an adapter's own
//!   `Warehouse::execute`, say. It holds the one door the application opens.

use super::ports::{Caller, callers_of_the_application, is_rust};
use crate::causality::regions;
use crate::repo;

/// The application crate, as a Rust path spells it.
const APPLICATION_PATH: &str = "sutura_app";

/// The function on it that answers, and the only public one on that path.
///
/// `answer_federated` is `pub(crate)`, so this is the whole door.
const ANSWER: &str = "answer";

/// What the check looked at, and what it found.
pub(super) struct Report {
    /// The callers it read, so a rule that found none cannot report `ok`.
    pub(super) callers: Vec<String>,
    /// Rust files examined.
    pub(super) files: usize,
    /// `sutura_app::…` paths read, test code included.
    ///
    /// **Zero is a failure and not a pass.** If no caller's source names the application at all,
    /// either the crate was renamed or the spelling changed, and this rule is reading nothing while
    /// printing `ok` - which is the dead-gate shape the whole invariants tree is written against.
    pub(super) paths: usize,
    /// One line per violation, already formatted for stderr.
    pub(super) problems: Vec<String>,
}

/// Every caller of the driving port, scanned for a direct call to the answer path.
pub(super) fn check(meta: &serde_json::Value) -> Result<Report, String> {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        return Err(String::from("could not determine the repo root"));
    };
    let callers = callers_of_the_application(meta, &root)?;
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let mut problems = Vec::new();
    let mut scanned = 0_usize;
    let mut paths = 0_usize;

    for rel in &files {
        let Some(caller) = caller_of(&callers, rel) else {
            continue;
        };
        if !is_rust(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        let tests = regions::scope(rel, &read);
        for (line, reaches) in application_paths(&crate::serde_parse::scan::code_lines(&text).join("\n")) {
            paths = paths.saturating_add(1);
            if reaches && !tests.covers(line) {
                problems.push(format!(
                    "{rel}:{line}: `{}` names `{APPLICATION_PATH}::{ANSWER}` in its own source",
                    caller.name
                ));
            }
        }
    }
    if scanned == 0 {
        return Err(format!(
            "found {} caller(s) of the driving port but no .rs file in them",
            callers.len()
        ));
    }
    Ok(Report {
        callers: callers.into_iter().map(|caller| caller.name).collect(),
        files: scanned,
        paths,
        problems,
    })
}

/// Which caller's `src/` this file is in, if any.
fn caller_of<'callers>(callers: &'callers [Caller], rel: &str) -> Option<&'callers Caller> {
    callers.iter().find(|caller| rel.starts_with(caller.src.as_str()))
}

/// Every path rooted at [`APPLICATION_PATH`] in `code`: its 1-based line, and whether its segments
/// name [`ANSWER`].
///
/// Whole-token matched at the root, so `not_sutura_app::answer` is not one of ours. From there it
/// walks the characters a path or a `use` tree is made of and stops at the first that is neither -
/// a `(`, a `;`, a `<`, an operator - so `sutura_app::Warehouses::of(x.answer())` ends at the
/// parenthesis and is not a match. `use sutura_app::{answer, Warehouses}` and
/// `use sutura_app::answer as ask` both are.
fn application_paths(code: &str) -> Vec<(usize, bool)> {
    let mut found = Vec::new();
    for (at, _) in code.match_indices(APPLICATION_PATH) {
        let Some(before) = code.get(..at) else {
            continue;
        };
        if before.chars().next_back().is_some_and(is_ident) {
            continue;
        }
        let Some(rest) = code.get(at.saturating_add(APPLICATION_PATH.len())..) else {
            continue;
        };
        // The root must be a whole token: `sutura_appx::answer` is somebody else's crate.
        if rest.chars().next().is_some_and(is_ident) {
            continue;
        }
        found.push((before.matches('\n').count().saturating_add(1), names_the_answer(rest)));
    }
    found
}

/// Do the segments of the path or `use` tree at the start of `rest` include [`ANSWER`]?
fn names_the_answer(rest: &str) -> bool {
    let mut segment = String::new();
    for character in rest.chars() {
        if is_ident(character) {
            segment.push(character);
            continue;
        }
        if segment == ANSWER {
            return true;
        }
        segment.clear();
        // What a path and a `use` tree are made of, and nothing else. A `(`, a `;`, a `<` or an
        // operator ends the path.
        if !matches!(character, ':' | '{' | '}' | ',' | '*') && !character.is_whitespace() {
            return false;
        }
    }
    segment == ANSWER
}

/// A character an identifier is made of.
const fn is_ident(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// Printed when this half fails, because a rule whose reason is unstated gets reverted.
pub(super) fn explain() {
    eprintln!("Every outcome is recorded before it is returned, and what holds that is the driving");
    eprintln!("port's one implementor: `LocalService::start` takes an audit sink and has no form");
    eprintln!("that omits one, and `Surface::answer` writes a record before the `Ok`. A composition");
    eprintln!("root that calls `sutura_app::answer` itself gets the answer and skips the record -");
    eprintln!("which is exactly what `sutura query` did, on the shipped binary, while the invariants");
    eprintln!("row named a mechanism it was outside of.");
    eprintln!();
    eprintln!("So compose the service instead: `LocalService::start(catalog, engines, sink, broker,");
    eprintln!("working_set)` and then `Surface::answer`. `crates/sutura-cli/src/mcp.rs` and");
    eprintln!("`crates/sutura-cli/src/commands.rs` are both that shape.");
    eprintln!();
    eprintln!("A unit test is exempt - `#[cfg(test)]` is skipped, and so is everything under");
    eprintln!("`tests/`, which is where the typed `ServiceError` is asserted from.");
}

#[cfg(test)]
mod tests {
    use super::{application_paths, names_the_answer};

    #[test]
    fn a_direct_call_to_the_answer_path_is_found_with_its_line() {
        let code = "fn a() {\n    let x = sutura_app::answer(&v, &q);\n}\n";
        assert_eq!(application_paths(code), vec![(2, true)]);
    }

    #[test]
    fn every_other_path_into_the_application_is_left_alone() {
        // The shape the fix leaves behind, and the shape the rest of the tree is full of. A rule
        // that reported these would be reverted within a day.
        let code = "use sutura_app::surface::{LocalService, Surface as _};\n\
                    use sutura_app::prompt::CatalogProse;\n\
                    let w = sutura_app::Warehouses::of(engine);\n\
                    let o = service.answer(&context, &query)?;\n\
                    let a: sutura_app::Answered = todo!();\n";
        assert_eq!(
            application_paths(code),
            vec![(1, false), (2, false), (3, false), (5, false)],
            "only a path rooted at the application counts, and none of these names `answer`"
        );
    }

    #[test]
    fn an_import_is_a_match_however_it_is_spelled() {
        // The hole a `sutura_app::answer(` scan would have: an import makes the call site read as a
        // bare `answer(`, indistinguishable from a local function of that name - and this crate's
        // own `sutura-http` and `sutura-mcp` both have one.
        assert!(names_the_answer("::answer;"));
        assert!(names_the_answer("::{answer, Warehouses};"));
        assert!(names_the_answer("::{\n    answer,\n    Warehouses,\n};"));
        assert!(names_the_answer("::answer as ask;"));
    }

    #[test]
    fn a_path_ends_where_the_code_does_and_does_not_run_on() {
        // The over-capture this would have had without a terminator set: everything after the
        // parenthesis is a different expression, and one of the arguments could be called `answer`.
        assert!(!names_the_answer("::Warehouses::of(answer)"));
        assert!(!names_the_answer("::verify_and_validate(pinned)?; let answer = 1;"));
        // A longer identifier is not the segment: `answer_federated` is `pub(crate)` and could not
        // be called from a caller anyway, and this is what keeps the match a whole token.
        assert!(!names_the_answer("::answer_federated;"));
        assert!(!names_the_answer("::Answered;"));
    }

    #[test]
    fn the_root_is_matched_as_a_whole_token() {
        assert_eq!(application_paths("use not_sutura_app::answer;"), Vec::new());
        assert_eq!(application_paths("use sutura_apps::answer;"), Vec::new());
    }
}
