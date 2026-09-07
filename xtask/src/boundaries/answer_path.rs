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
//! * **It reads a PATH rooted at `sutura_app`.** Three ways past that, and only one is still open.
//!   Renaming the CRATE - `use sutura_app as app;` or `use sutura_app::{self as app};` - is
//!   [refused](Reaches::TheRootRenamed) rather than chased, the way `boot_order` refuses a rename of
//!   its own tracked name. `use sutura_app::*` is not a hole either: `clippy::wildcard_imports` is
//!   on through `pedantic` and the gate runs with `-D warnings`. What remains is a **re-export of
//!   `answer` through a third crate**, and a dependency renamed in a manifest
//!   (`app = { package = "sutura-app" }`) - neither is in this tree, and no scan of `src/` could
//!   see either.
//! * Comments and multi-line string interiors are blanked first, through the serde gate's
//!   [`code_lines`](crate::serde_parse::scan::code_lines). **What makes that load-bearing is the
//!   scanned set and not this file:** `xtask` declares no dependency on `sutura-app`, so this module
//!   is never a caller and its own header could name the path freely - but
//!   `crates/sutura-cli/src/commands.rs`, `src/mcp.rs` and `src/audit.rs` are all in the set and all
//!   name the path in doc comments. A SINGLE-line string literal keeps its content, so a scanned
//!   file holding the path in one would be reported; [`explain`] holds one and is out of scope for
//!   the same reason.
//! * It cannot see a caller that reaches the answer path some other way - an adapter's own
//!   `Warehouse::execute`, say. It holds the one door the application opens.

use super::ports::{callers_of_the_application, is_rust};
use crate::causality::regions;
use crate::repo;

/// The application crate, as a Rust path spells it.
const APPLICATION_PATH: &str = "sutura_app";

/// The function on it that answers, and the only public one on that path.
///
/// `answer_federated` is `pub(crate)`, so this is the whole door.
const ANSWER: &str = "answer";

/// Where the door is defined, relative to the repo root.
///
/// Declared rather than searched, so a move is a red gate somebody looks at rather than a scan that
/// quietly finds a function of that name somewhere else.
pub(super) const APPLICATION_LIB: &str = "crates/sutura-app/src/lib.rs";

/// How the door is spelled where it is defined.
///
/// Built from [`ANSWER`] so the needle this rule forbids in a caller and the needle it requires at
/// the definition cannot drift apart.
pub(super) fn door() -> String {
    format!("pub fn {ANSWER}")
}

/// What the check looked at, and what it found.
pub(super) struct Report {
    /// The callers it read, so a rule that found none cannot report `ok`.
    pub(super) callers: Vec<String>,
    /// The line [`APPLICATION_LIB`] defines the door on, and `None` if it no longer defines one.
    ///
    /// **`None` is a failure, and it is the liveness half [`paths`](Report::paths) does not
    /// provide.** That one counts paths rooted at [`APPLICATION_PATH`], so it catches a rename of
    /// the CRATE and nothing else; a rename of the FUNCTION leaves it in the dozens.
    pub(super) door: Option<usize>,
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
    let (root, files) = repo::all_files()
        .and_then(|census| census.into_listing(repo::Unmigrated::Boundaries))
        .map_err(|why| why.describe())?;
    let callers = callers_of_the_application(meta, &root)?;
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let Some(defines) = read(APPLICATION_LIB) else {
        return Err(format!(
            "`{APPLICATION_LIB}` could not be read, and that is where the door this rule guards is defined"
        ));
    };
    let door = door_line(&crate::serde_parse::scan::code_lines(&defines).join("\n"));
    let mut problems = Vec::new();
    let mut scanned = 0_usize;
    let mut paths = 0_usize;

    for rel in &files {
        let Some(caller) = callers.iter().find(|caller| rel.starts_with(caller.src.as_str())) else {
            continue;
        };
        if !is_rust(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        // Counted as examined above, then skipped before the region scan: a file that never names
        // the application has no path to classify, and [`regions::scope`] reads the file a second
        // time to resolve a `#[cfg(test)] mod` declaration. The sibling gate short-circuits the
        // same way for the same reason.
        if !text.contains(APPLICATION_PATH) {
            continue;
        }
        let tests = regions::scope(rel, &read);
        for path in application_paths(&crate::serde_parse::scan::code_lines(&text).join("\n")) {
            paths = paths.saturating_add(1);
            if tests.covers(path.line) {
                continue;
            }
            match path.reaches {
                Reaches::Elsewhere => {}
                Reaches::TheAnswer => problems.push(format!(
                    "{rel}:{}: `{}` names `{APPLICATION_PATH}::{ANSWER}` in its own source",
                    path.line, caller.name
                )),
                Reaches::TheRootRenamed => problems.push(format!(
                    "{rel}:{}: `{}` imports `{APPLICATION_PATH}` under another name, which makes every \
                     path through it invisible to this rule - import it as itself",
                    path.line, caller.name
                )),
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
        door,
        files: scanned,
        paths,
        problems,
    })
}

/// The line [`APPLICATION_LIB`]'s code defines the door on, if it still defines one.
///
/// **The liveness half that was missing, and it was measured rather than argued.**
/// [`Report::paths`] counts paths rooted at [`APPLICATION_PATH`], so `paths == 0` catches a rename
/// of the CRATE and nothing else. Rename `sutura_app::answer` to `ask`, or move it under
/// `sutura_app::surface`, and [`names_the_answer`] matches nothing forever while `paths` stays in
/// the dozens - every caller still writes `use sutura_app::surface::{LocalService, Surface as _};`.
/// Reproduced: with the door renamed in `sutura-app` and a bypass written to the new name in
/// `crates/sutura-cli/src/commands.rs`, this half printed
/// `ok - the answer path is reached through the port (94 path(s) in 54 file(s) ...)` and exited
/// zero - a dead gate over a tree where a shipped command answers with no record, which is the one
/// shape this module's header is written against. Found by review.
///
/// **`pub fn` and not the bare name**, so narrowing `answer` to `pub(crate)` is red too. That is the
/// right direction: the whole reason this scan exists instead of the compiler is that `answer` is
/// `pub` for the golden suites, so a narrowed door is a rule which has lost its reason and gets
/// deleted rather than left printing `ok`.
///
/// Whole-token at the end, so `pub fn answer_federated` - which is `pub(crate)` today - is not read
/// as the door.
fn door_line(code: &str) -> Option<usize> {
    let needle = door();
    code.match_indices(&needle).find_map(|(at, _)| {
        let rest = code.get(at.saturating_add(needle.len())..)?;
        if rest.chars().next().is_some_and(is_ident) {
            return None;
        }
        Some(code.get(..at)?.matches('\n').count().saturating_add(1))
    })
}

/// What a path rooted at [`APPLICATION_PATH`] actually reaches.
///
/// An enum rather than a `bool`, because there are two ways a caller can end up on the other side of
/// the driving port and only one of them is a call. A reader names the case instead of deciding what
/// `false` covered.
#[derive(Debug, PartialEq, Eq)]
enum Reaches {
    /// Something else on the application - a type, a module, the port trait, a call THROUGH it.
    /// The shape the fix leaves behind, and the shape the rest of the tree is full of.
    Elsewhere,
    /// [`ANSWER`] itself. The bypass this rule exists for.
    TheAnswer,
    /// The application crate under another name.
    ///
    /// **Refused rather than followed, for the reason `boot_order` gives about its own tracked
    /// name: text matching cannot chase a rename.** `use sutura_app as app;` and
    /// `use sutura_app::{self as app};` both leave every later call spelled `app::answer(..)`,
    /// which this scan has never heard of - the module header used to carry that as a stated limit,
    /// and a limit a sibling gate already knows how to close is a missing check rather than a
    /// boundary. Nothing in the four callers imports a crate that way, so the cost is one forbidden
    /// idiom; renaming an ITEM (`Surface as _`, `DatasetId as WireDataset`) is untouched, which is
    /// what keeps this a targeted refusal instead of a style rule.
    TheRootRenamed,
}

/// One path rooted at [`APPLICATION_PATH`]: where it is, and what it reaches.
///
/// Both halves are wanted at the call site and both are counted - the second decides a violation,
/// and the first is what makes a reported line one a reader can open. A named pair rather than a
/// tuple, so neither is read as the other.
#[derive(Debug, PartialEq, Eq)]
struct ApplicationPath {
    /// 1-based line in the file the code came from.
    line: usize,
    /// What this path reaches on the application.
    reaches: Reaches,
}

/// Every path rooted at [`APPLICATION_PATH`] in `code`.
///
/// Whole-token matched at the root, so `not_sutura_app::answer` is not one of ours. From there it
/// walks the characters a path or a `use` tree is made of and stops at the first that is neither -
/// a `(`, a `;`, a `<`, an operator - so `sutura_app::Warehouses::of(x.answer())` ends at the
/// parenthesis and is not a match. `use sutura_app::{answer, Warehouses}` and
/// `use sutura_app::answer as ask` both are.
fn application_paths(code: &str) -> Vec<ApplicationPath> {
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
        found.push(ApplicationPath {
            line: before.matches('\n').count().saturating_add(1),
            reaches: reaches(rest),
        });
    }
    found
}

/// Classify the path or `use` tree at the start of `rest`.
///
/// The rename is checked FIRST, because a renamed root makes the second question meaningless: the
/// answer would then be reached under a spelling this scan cannot see.
fn reaches(rest: &str) -> Reaches {
    if renames_the_root(rest) {
        Reaches::TheRootRenamed
    } else if names_the_answer(rest) {
        Reaches::TheAnswer
    } else {
        Reaches::Elsewhere
    }
}

/// Is the application crate itself being imported under another name?
///
/// Two spellings, and both are one keystroke from an idiom this tree already uses:
/// `use sutura_app as app;` and `use sutura_app::{self as app};`. An ITEM renamed on the way through
/// is not one of them - `use sutura_app::surface::{LocalService, Surface as _};` is in this tree
/// twice and stays legal, because every call site it produces still spells the root.
fn renames_the_root(rest: &str) -> bool {
    let aliased = |after: &str| after.chars().next().is_none_or(|next| !is_ident(next));
    let trimmed = rest.trim_start();
    if let Some(after) = trimmed.strip_prefix("as") {
        return aliased(after);
    }
    // `::{self as app}` - the same rename, one brace deeper.
    trimmed
        .strip_prefix("::")
        .map(str::trim_start)
        .and_then(|tree| tree.strip_prefix('{'))
        .map(str::trim_start)
        .and_then(|group| group.strip_prefix("self"))
        .map(str::trim_start)
        .and_then(|after_self| after_self.strip_prefix("as"))
        .is_some_and(aliased)
}

/// Does the path or `use` tree at the start of `rest` reach [`ANSWER`] itself?
///
/// **Immediately after the root, or inside a brace group - not any segment.** Any segment reported a
/// call THROUGH the port spelled with its full path: `sutura_app::surface::Surface::answer(&service,
/// ..)` was a violation, and [`explain`] then told the author to compose a service and call
/// `Surface::answer`, which is what they had just done. `crates/sutura-http/src/routes/v1/query.rs`
/// writes that spelling in prose, so it is a shape somebody reaches for. Found by review.
///
/// A brace group is NOT narrowed to its first segment: `use sutura_app::{surface::answer}` reaches
/// the same door by a longer route, and over-reporting inside a `use` tree costs an author one
/// spelling rather than an argument.
fn names_the_answer(rest: &str) -> bool {
    let mut segment = String::new();
    let mut depth = 0_usize;
    // The root's own `::` has not been passed yet, so the first segment to complete is the one
    // written directly on the application.
    let mut on_the_root = true;
    let reaches = |segment: &str, depth: usize, on_the_root: bool| segment == ANSWER && (depth > 0 || on_the_root);
    for character in rest.chars() {
        if is_ident(character) {
            segment.push(character);
            continue;
        }
        if !segment.is_empty() {
            if reaches(&segment, depth, on_the_root) {
                return true;
            }
            on_the_root = false;
            segment.clear();
        }
        match character {
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            // What a path and a `use` tree are made of, and nothing else. A `(`, a `;`, a `<` or an
            // operator ends the path.
            ':' | ',' | '*' => {}
            _ if character.is_whitespace() => {}
            _ => return false,
        }
    }
    reaches(&segment, depth, on_the_root)
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
    eprintln!("working_set)` and then `Surface::answer`. `crates/sutura-cli/src/commands.rs`'s");
    eprintln!("`started` is that shape, and both of that binary's commands go through it.");
    eprintln!();
    eprintln!("A unit test is exempt - `#[cfg(test)]` is skipped, and so is everything under");
    eprintln!("`tests/`, which is where the typed `ServiceError` is asserted from.");
    eprintln!();
    eprintln!("If the line reported was an IMPORT of the application under another name: this rule");
    eprintln!("finds a bypass by matching the application's path as text, so a crate renamed at the");
    eprintln!("`use` leaves every call site spelled something it has never heard of. Import the");
    eprintln!("crate as itself - renaming an item on the way through (`Surface as _`) is untouched.");
}

#[cfg(test)]
mod tests {
    use super::{
        APPLICATION_LIB, ApplicationPath, Reaches, application_paths, door, door_line, names_the_answer, renames_the_root,
    };
    use crate::serde_parse::scan::code_lines;

    /// A path at `line`, and what it reached.
    fn at(line: usize, reaches: Reaches) -> ApplicationPath {
        ApplicationPath { line, reaches }
    }

    /// The answer function itself, at `line`.
    fn answers(line: usize) -> ApplicationPath {
        at(line, Reaches::TheAnswer)
    }

    /// Some other path into the application, at `line`.
    fn elsewhere(line: usize) -> ApplicationPath {
        at(line, Reaches::Elsewhere)
    }

    #[test]
    fn a_direct_call_to_the_answer_path_is_found_with_its_line() {
        let code = "fn a() {\n    let x = sutura_app::answer(&v, &q);\n}\n";
        assert_eq!(application_paths(code), vec![answers(2)]);
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
            vec![elsewhere(1), elsewhere(2), elsewhere(3), elsewhere(5)],
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

    /// A turbofish is still the segment, because the match is the bare name.
    ///
    /// The hole a `sutura_app::answer(` scan would have had: `answer::<W, B>(` does not contain it.
    #[test]
    fn a_turbofish_call_is_still_the_answer_path() {
        assert_eq!(
            application_paths("let o = sutura_app::answer::<W, StaticCredentialBroker>(&v, &q);"),
            vec![answers(1)]
        );
    }

    /// A call THROUGH the port, spelled with its full path, is not a bypass.
    ///
    /// Red against the previous rule by construction: it accepted [`ANSWER`] as ANY segment, so both
    /// of these were reported and the printed advice was to do what the author had already done.
    #[test]
    fn a_call_through_the_port_is_not_the_answer_path() {
        assert_eq!(
            application_paths("let o = sutura_app::surface::Surface::answer(&service, &context, &query)?;"),
            vec![elsewhere(1)]
        );
        assert_eq!(
            application_paths("let o = <Composed<W> as sutura_app::surface::Surface>::answer(&service, &c, &q)?;"),
            vec![elsewhere(1)]
        );
        assert!(!names_the_answer("::surface::Surface::answer(&service, &context, &query)"));
    }

    /// Renaming the CRATE is refused, because text matching cannot follow a rename.
    ///
    /// Red against the previous rule by construction: both spellings were `Elsewhere` then, and the
    /// module header carried the hole as a stated limit rather than a check.
    #[test]
    fn importing_the_application_under_another_name_is_refused() {
        for aliased in [
            "use sutura_app as app;\n",
            "pub(crate) use sutura_app as app;\n",
            "use sutura_app::{self as app};\n",
            "use sutura_app::{ self as app };\n",
        ] {
            assert_eq!(application_paths(aliased), vec![at(1, Reaches::TheRootRenamed)], "{aliased}");
        }
    }

    /// And renaming an ITEM on the way through is NOT refused - this tree does it twice.
    ///
    /// Without this the rule would ban `Surface as _`, which every caller of the port writes, and a
    /// targeted refusal would have become a style rule nobody can satisfy.
    #[test]
    fn renaming_an_item_on_the_way_through_is_allowed() {
        for legal in [
            "use sutura_app::surface::{LocalService, Surface as _};\n",
            "use sutura_app::prompt::CatalogProse as Prose;\n",
            "use sutura_app::{Warehouses as Engines};\n",
        ] {
            let after_root = legal
                .strip_prefix("use sutura_app")
                .expect("every fixture here spells the root before the rename");
            assert!(!renames_the_root(after_root), "{legal}");
            assert_eq!(application_paths(legal), vec![elsewhere(1)], "{legal}");
        }
        // `answer` renamed at the import is still the answer, not a root rename.
        assert_eq!(application_paths("use sutura_app::answer as ask;\n"), vec![answers(1)]);
    }

    /// The door is where this rule believes it is, over the REAL file.
    ///
    /// The non-vacuity assertion this half was missing: without it a rename of the function leaves
    /// `names_the_answer` matching nothing and the gate printing `ok` over dozens of paths, because
    /// the only liveness check counted paths rooted at the CRATE.
    #[test]
    fn the_door_is_still_defined_where_this_rule_reads_it() {
        let root = crate::repo::root().expect("the repo root");
        let text = std::fs::read_to_string(root.join(APPLICATION_LIB)).expect("the application's lib.rs is readable");
        assert!(
            door_line(&code_lines(&text).join("\n")).is_some(),
            "`{}` is not in {APPLICATION_LIB} - this rule now forbids a path that names nothing",
            door()
        );
    }

    /// Every way the door can move out from under the rule, and none of them reads as the door.
    #[test]
    fn a_door_that_moved_or_narrowed_is_not_found() {
        for gone in [
            // Renamed.
            "pub fn ask<W, B>(\n",
            // Moved under a module, so the crate-rooted path is one segment longer.
            "pub mod surface {\n    pub fn ask<W, B>(\n}\n",
            // Narrowed - which is the rule losing its reason rather than a bypass, and it is still red.
            "pub(crate) fn answer<W, B>(\n",
            // A longer identifier is not the segment.
            "pub(crate) fn answer_federated<W, B>(\n",
            // And prose naming it is not a definition, which is what the blanking buys.
            "/// `pub fn answer` is the whole door.\n",
        ] {
            assert_eq!(door_line(&code_lines(gone).join("\n")), None, "{gone}");
        }
        assert_eq!(door_line(&code_lines("pub fn answer<W, B>(\n").join("\n")), Some(1));
    }
}
