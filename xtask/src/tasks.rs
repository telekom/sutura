//! A task that checks PART of this workspace has to SAY so, in its own output.
//!
//! This gate exists because of one incident, and the incident is the same shape as the
//! `disallowed-methods` entry `AGENTS.md` uses as its canonical example - a mechanism that reads
//! as enforcement and checks nothing. `just check` is `cargo check -p sutura-domain
//! --no-default-features`, deliberately narrow and deliberately sub-second. Its output was
//! cargo's own `Finished dev profile ... in 0.10s`, which says nothing about scope, so a passing
//! run read as "the tree compiles" and a branch whose `sutura-config` had an unclosed delimiter
//! was pushed on the strength of it. `just lint` found it one step later.
//!
//! The fix is not a wider `check` - a sub-second loop is worth having, and `AGENTS.md` pins that
//! width. The fix is that a narrowed task states its scope. That is already the house pattern:
//! `cargo xtask fmt` prints `formatted N package(s): ...`, `check-crap` prints `scope:
//! sutura-domain`, `crap` prints `coverage over sutura-domain`. `just check` was the outlier.
//!
//! What is mechanical here, stated plainly because the alternative is a claim: a gate cannot know
//! what a developer BELIEVED a task covered, and this one does not try. What it makes impossible
//! is the notice going stale - the printed scope is checked against the `-p` flags in the same
//! recipe, so widening the compile without widening the sentence fails, and so does pointing at a
//! task that does not exist or is not actually a workspace check.
//!
//! Three rules, over the `justfile` and nothing else:
//!
//! * **states its scope** - a recipe whose cargo VERIFICATION line narrows to named packages
//!   prints every one of those names.
//! * **names the wider gate** - that same recipe cites, in its output, a `just` task whose own
//!   body verifies `--workspace`. Mechanical: the pointer is checked to be broader, not trusted.
//! * **cites nothing dead** - every `just <task>` in a backtick span anywhere in the file names a
//!   recipe that exists. The `justfile` is scanned by NEITHER citation checker today:
//!   `check-guidance` filters to `.md`, `.nix`, `.yml`, `.yaml`, `.toml`, `.sh` and this file has
//!   no extension, and `.github/scripts/check-task-citations.sh` reads `*.md`. So a recipe could
//!   point a reader at a task that was renamed away, which is exactly what would rot the pointer
//!   the first rule requires.
//!
//! FAIL CLOSED, in the three directions a text scan fails silently: no recipes parsed, no cargo
//! verification line found at all, or no citation found at all - each means the SCAN broke rather
//! than the file being clean. `scan_broke` holds those, separately from the rules, because they are
//! properties of THIS justfile and not of the policy.

use std::collections::BTreeSet;

use crate::Verdict;
use crate::repo;

/// The one file this reads. The task names a developer types live here.
const JUSTFILE: &str = "justfile";

/// The recipe names in this repo's justfile, or `None` when it could not be read.
///
/// Exposed for the citation checkers that are not this gate. `check-guidance` reads the remedy text
/// in `xtask/src/guidance/claims.rs` and, since `github.com/telekom/sutura#243`, every `just <task>`
/// a program PRINTS - both rot exactly the way a citation in a page does. This is the only parser in
/// the workspace that knows what a recipe header looks like, and a second copy of it is the
/// transcription this module's header objects to.
pub(crate) fn recipe_names(root: &std::path::Path) -> Option<BTreeSet<String>> {
    let text = std::fs::read_to_string(root.join(JUSTFILE)).ok()?;
    Some(recipes(&text).into_iter().map(|recipe| recipe.name).collect())
}

/// cargo subcommands that VERIFY first-party code, and therefore make a scope claim.
///
/// `cargo build` and `cargo run` are deliberately absent: `just setup` builds `xtask` and
/// `sutura-dev` and half the recipes run `cargo run -q -p xtask`, and neither is a statement
/// about whether this workspace compiles - they are how a tool gets invoked. Including them
/// would put a scope notice on every recipe that calls a gate, which is noise, and noise is how
/// a rule gets routed around.
///
/// A task that narrows in RUST rather than on a cargo line - `cargo xtask crap`, scoped by
/// `crap::SCOPE` - is not matched here and does not need to be: it prints its own scope, and
/// `check-crap` fails if that scope names a package the workspace does not have.
const VERIFYING: &[&str] = &["cargo check", "cargo clippy", "cargo nextest", "cargo test"];

/// A recipe: its name, the line its header is on, and the indented lines under it.
struct Recipe {
    name: String,
    line: usize,
    body: Vec<String>,
}

/// What a recipe's cargo verification lines say about scope.
struct Scope {
    /// Packages named by `-p` / `--package` on a line with no `--workspace`.
    narrowed: BTreeSet<String>,
    /// Did any verification line cover the whole workspace?
    covers_workspace: bool,
}

/// Split the file into recipes.
///
/// A recipe header is an unindented line carrying a `:`; the body is every line after it that is
/// indented or blank. Hand-parsed, like `package_name` in `changes.rs` and for the same reason:
/// `xtask` has one dependency, and the shape being read is a file this repo controls.
fn recipes(text: &str) -> Vec<Recipe> {
    let mut out: Vec<Recipe> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        let indented = raw.starts_with([' ', '\t']);
        if indented || raw.trim().is_empty() {
            if let Some(current) = out.last_mut() {
                current.body.push(String::from(raw));
            }
            continue;
        }
        // Unindented and not blank: a comment, a setting, or a recipe header. Anything that is
        // not a header closes the recipe above it, which is why this pushes nothing and does
        // not fall through to the body arm.
        if raw.starts_with('#') {
            continue;
        }
        if let Some((head, _)) = raw.split_once(':') {
            // `@` IS NOT PART OF THE NAME. `just` reads a leading `@` on a header as "run this
            // recipe quietly", and `just @dev-endpoint` is not how anybody invokes it - `printed`
            // below already strips the same prefix off a body line. Keeping it made this parser
            // disagree with the other hand-parse of this file, and the disagreement was not
            // theoretical: `sutura-dev` prints `just dev-endpoint` at the moment a service is
            // missing, the header is `@dev-endpoint service:`, and the only authority for "is that
            // a recipe" answered no. Latent for the two rules below - nothing cites a quiet recipe
            // in the justfile itself - and live the moment a gate read a printed line, which
            // `check-guidance`'s `advice` check now does.
            let name = head.split_whitespace().next().unwrap_or_default().trim_start_matches('@');
            // `:=` is an assignment, not a recipe. None exist today; the guard costs a line and
            // stops one being read as a recipe called `set`.
            if !name.is_empty() && !head.ends_with(":=") && !raw.contains(":=") {
                out.push(Recipe {
                    name: String::from(name),
                    line: number,
                    body: Vec::new(),
                });
            }
        }
    }
    out
}

/// The package a `-p` / `--package` token names, given the tokens that follow it.
///
/// `None` for `{{ ... }}`: a recipe parameter is not a name this gate can check, and guessing
/// one would make the rule report a scope nobody wrote.
fn package_at(token: &str, next: Option<&&str>) -> Option<String> {
    for flag in ["--package=", "-p="] {
        if let Some(value) = token.strip_prefix(flag) {
            return (!value.starts_with('{')).then(|| String::from(value));
        }
    }
    if token != "-p" && token != "--package" {
        return None;
    }
    let value = next?;
    (!value.starts_with('{')).then(|| String::from(*value))
}

/// What the recipe's verification lines cover.
fn scope_of(recipe: &Recipe) -> Scope {
    let mut scope = Scope {
        narrowed: BTreeSet::new(),
        covers_workspace: false,
    };
    for line in &recipe.body {
        let text = line.trim();
        if text.starts_with('#') || !VERIFYING.iter().any(|verb| text.contains(verb)) {
            continue;
        }
        let tokens: Vec<&str> = text.split_whitespace().collect();
        if tokens.contains(&"--workspace") {
            scope.covers_workspace = true;
            continue;
        }
        let mut rest = tokens.as_slice();
        while let Some((token, tail)) = rest.split_first() {
            if let Some(name) = package_at(token, tail.first()) {
                scope.narrowed.insert(name);
            }
            rest = tail;
        }
    }
    scope
}

/// The recipe's own output: what an `echo` or a `printf` in its body puts in front of a reader.
fn printed(recipe: &Recipe) -> Vec<&str> {
    recipe
        .body
        .iter()
        .map(String::as_str)
        .filter(|line| {
            let text = line.trim_start().trim_start_matches('@');
            text.starts_with("echo ") || text.starts_with("printf ")
        })
        .collect()
}

/// The task name in a `just <name>` citation, or `None` when the span is not one.
///
/// The three exclusions are the ones `check-guidance::task_name_at` and
/// `.github/scripts/check-task-citations.sh` already carry, for the reason both record: a flag
/// (`just --list`), a placeholder (`just <task>`) and a bare `just` are each correct prose, and
/// reading one as a deleted task is how a gate comes to fail on the page documenting it.
fn cited_task(span: &str) -> Option<&str> {
    let rest = span.strip_prefix("just ")?.trim_start();
    if rest.starts_with(['-', '<']) {
        return None;
    }
    let name = rest.split(|c: char| !c.is_ascii_alphanumeric() && c != '-').next()?;
    (!name.is_empty()).then_some(name)
}

/// Every `just <task>` citation inside a backtick span, with its line number.
///
/// Backticks and not the bare word, because "just" is an English adverb this repo uses
/// constantly - `just tasks and a CI job`, `it just never runs`. Fields 2, 4, 6 ... of a
/// backtick split are the spans; the bound stops an unterminated trailing backtick being read as
/// one. Same walk the citation script uses.
fn citations(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let spans: Vec<&str> = raw.split('`').collect();
        let mut at: usize = 1;
        while at.saturating_add(1) < spans.len() {
            if let Some(span) = spans.get(at)
                && let Some(name) = cited_task(span)
            {
                out.push((index.saturating_add(1), String::from(name)));
            }
            at = at.saturating_add(2);
        }
    }
    out
}

/// Did the SCAN break, rather than the file being clean?
///
/// Separate from [`problems`] rather than folded into it, because these are properties of THIS
/// repo's justfile and not of the rules: a small justfile with no citation in it is not a defect,
/// while this one losing its recipes, its cargo lines or its citations means the parser stopped
/// reading and a gate that reads nothing passes everything. Tested on its own for the same reason.
fn scan_broke(text: &str) -> Vec<String> {
    let recipes = recipes(text);
    if recipes.is_empty() {
        return vec![String::from(
            "parsed no recipes out of the justfile - the scan is broken, not the file",
        )];
    }
    let mut out = Vec::new();
    if !recipes
        .iter()
        .map(scope_of)
        .any(|scope| scope.covers_workspace || !scope.narrowed.is_empty())
    {
        out.push(String::from(
            "found no cargo verification line in any recipe - the scan is broken, not the file",
        ));
    }
    if citations(text).is_empty() {
        out.push(String::from(
            "parsed no `just <task>` citation out of the justfile - the scan is broken, not the file",
        ));
    }
    out
}

/// The three rules, over the text of one justfile. Returns one message per violation.
fn problems(text: &str) -> Vec<String> {
    let recipes = recipes(text);
    let names: BTreeSet<&str> = recipes.iter().map(|r| r.name.as_str()).collect();
    let scopes: Vec<Scope> = recipes.iter().map(scope_of).collect();

    // A task that verifies the whole workspace, by name. What the rule below accepts as "the
    // wider gate" is derived from the recipes rather than listed here, so renaming `lint` moves
    // this set with it.
    let workspace_gates: BTreeSet<&str> = recipes
        .iter()
        .zip(&scopes)
        .filter_map(|(recipe, scope)| scope.covers_workspace.then_some(recipe.name.as_str()))
        .collect();

    let mut out = Vec::new();
    for (recipe, scope) in recipes.iter().zip(&scopes) {
        if scope.narrowed.is_empty() {
            continue;
        }
        let output: Vec<&str> = printed(recipe);
        let says = |needle: &str| output.iter().any(|line| line.contains(needle));

        for package in &scope.narrowed {
            if !says(package) {
                out.push(format!(
                    "justfile:{}: `just {}` compiles {package} and only {} package(s), and its \
                     output never says so\n      \
                     a passing run then reads as a passing workspace - print the scope, the way \
                     `cargo xtask fmt` and `cargo xtask check-crap` do",
                    recipe.line,
                    recipe.name,
                    scope.narrowed.len()
                ));
            }
        }

        let points_wider = output
            .iter()
            .flat_map(|line| citations(line))
            .any(|(_, name)| workspace_gates.contains(name.as_str()));
        if !points_wider && !scope.covers_workspace {
            let wider: Vec<&str> = workspace_gates.iter().copied().collect();
            out.push(format!(
                "justfile:{}: `just {}` is narrowed and its output names no task that covers the \
                 whole workspace\n      \
                 cite one in a backtick span, so the reader is told where the wider check is: {}",
                recipe.line,
                recipe.name,
                wider.join(", ")
            ));
        }
    }

    for (line, name) in citations(text) {
        if !names.contains(name.as_str()) {
            out.push(format!(
                "justfile:{line}: `just {name}` names no recipe - it was renamed, deleted or never existed"
            ));
        }
    }

    out
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-scope: could not determine the repo root");
        return Verdict::Fail;
    };
    let path = root.join(JUSTFILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-scope: could not read {}: {error}", path.display());
            eprintln!("  A gate that cannot read the file it judges has judged nothing.");
            return Verdict::Fail;
        }
    };

    let mut found = scan_broke(&text);
    found.extend(problems(&text));
    if found.is_empty() {
        let recipes = recipes(&text);
        println!(
            "xtask check-scope: ok - {} recipe(s), {} citation(s)",
            recipes.len(),
            citations(&text).len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-scope: FAILED");
    for problem in &found {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("A task whose output does not state its scope gets read as covering everything.");
    eprintln!("That is how a branch that did not compile got pushed on a green `just check`.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{citations, cited_task, problems, recipes, scope_of};

    /// A justfile with the shape the rules are about: one narrowed recipe that states its scope
    /// and points at the wider one, and the wider one itself.
    const HONEST: &str = "\
# a comment
check:
    cargo check -p sutura-domain --no-default-features
    @echo 'compiled sutura-domain only - `just lint` is the workspace gate'

lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings
";

    #[test]
    fn a_recipe_that_states_its_scope_and_points_wider_passes() {
        assert_eq!(problems(HONEST), Vec::<String>::new());
    }

    #[test]
    fn a_narrowed_recipe_whose_output_hides_the_scope_fails() {
        // The incident, reduced: the compile is narrow and the output says nothing.
        let silent = HONEST.replace(
            "    @echo 'compiled sutura-domain only - `just lint` is the workspace gate'\n",
            "",
        );
        let found = problems(&silent);
        assert!(
            found.iter().any(|p| p.contains("output never says so")),
            "a silent narrowed recipe must fail: {found:?}"
        );
    }

    #[test]
    fn widening_the_compile_without_widening_the_sentence_fails() {
        // The rot this gate exists for: a second `-p` arrives, the notice is not updated, and
        // the printed scope is now a lie the reader has no way to see.
        let widened = HONEST.replace(
            "cargo check -p sutura-domain --no-default-features",
            "cargo check -p sutura-domain -p sutura-config --no-default-features",
        );
        let found = problems(&widened);
        assert!(
            found.iter().any(|p| p.contains("sutura-config")),
            "the package missing from the notice must be named: {found:?}"
        );
    }

    #[test]
    fn a_notice_pointing_at_nothing_wider_fails() {
        let dead_end = HONEST.replace("`just lint` is the workspace gate", "run the gates");
        let found = problems(&dead_end);
        assert!(
            found
                .iter()
                .any(|p| p.contains("names no task that covers the whole workspace")),
            "a notice with no wider pointer must fail: {found:?}"
        );
    }

    #[test]
    fn a_pointer_at_a_task_that_does_not_exist_fails() {
        // The rule that makes the pointer non-rotting. The justfile is scanned by neither
        // citation checker, so without this a rename leaves the notice pointing at nothing.
        let renamed = HONEST.replace("\nlint:\n", "\nlint-everything:\n");
        let found = problems(&renamed);
        assert!(
            found.iter().any(|p| p.contains("names no recipe")),
            "a dead citation must fail: {found:?}"
        );
    }

    #[test]
    fn a_workspace_recipe_needs_no_notice() {
        // `just lint` and `just test` say nothing about scope and are honest anyway: the name is
        // generic because the coverage is total. The rule must not ask them for a disclaimer.
        let only_wide = "lint:\n    cargo clippy --workspace --all-targets --all-features\n";
        assert_eq!(problems(only_wide), Vec::<String>::new());
    }

    #[test]
    fn building_a_tool_is_not_a_scope_claim() {
        // `just setup` builds xtask and sutura-dev, and half the recipes run `cargo run -q -p
        // xtask`. Neither says anything about whether this workspace compiles.
        let tools = "\
setup:
    cargo build -q -p xtask -p sutura-dev

classify:
    cargo run -q -p xtask -- classify --since main

lint:
    cargo clippy --workspace
";
        assert_eq!(problems(tools), Vec::<String>::new());
    }

    #[test]
    fn a_quiet_recipes_name_does_not_carry_its_at_sign() {
        // `just @dev-endpoint` is not how anything invokes it, and this file's own comment at line
        // 786 cites the recipe without the prefix. RED BEFORE THE FIX beside it: the name came back
        // as `@dev-endpoint`, so `recipe_names` - the one authority every citation checker in this
        // workspace resolves a `just <task>` against - denied that a recipe `sutura-dev` prints
        // exists. `dev/src/provisioned.rs`'s own hand-parse of this file already stripped it, which
        // is what kept the disagreement invisible.
        let quiet = "@dev-endpoint service:\n    cargo run -q -p xtask -- dev-endpoint {{ service }}\nlint:\n    cargo clippy --workspace\n";
        let parsed = recipes(quiet);
        let names: BTreeSet<&str> = parsed.iter().map(|recipe| recipe.name.as_str()).collect();
        assert!(names.contains("dev-endpoint"), "the `@` is still in the name: {names:?}");
        assert!(!names.contains("@dev-endpoint"), "both spellings parsed: {names:?}");

        // And against the real file, so a rename of the one quiet recipe does not make this vacuous.
        let root = crate::repo::root().expect("the repo root");
        let known = super::recipe_names(&root).expect("the justfile is readable");
        assert!(
            !known.iter().any(|name| name.starts_with('@')),
            "a recipe name still carries the quiet prefix: {known:?}"
        );
    }

    #[test]
    fn a_recipe_parameter_is_not_a_package_name() {
        let parametrised = "check-one pkg:\n    cargo check -p {{ pkg }}\nlint:\n    cargo clippy --workspace\n";
        let parsed = recipes(parametrised);
        let scope = scope_of(parsed.first().expect("the recipe parses"));
        assert!(scope.narrowed.is_empty(), "an interpolation is not a name to state");
        assert_eq!(problems(parametrised), Vec::<String>::new());
    }

    #[test]
    fn the_body_ends_at_the_next_unindented_line() {
        let parsed = recipes(HONEST);
        assert_eq!(parsed.len(), 2);
        let first = parsed.first().expect("check parses");
        assert_eq!(first.name, "check");
        assert!(
            first.body.iter().all(|line| !line.contains("clippy")),
            "the second recipe's body leaked into the first"
        );
    }

    #[test]
    fn english_prose_is_not_a_citation() {
        // Every one of these appears in the real justfile.
        assert_eq!(cited_task("just tasks"), Some("tasks"));
        assert_eq!(citations("`just` lists the tasks").len(), 0);
        assert_eq!(citations("it just never runs").len(), 0);
        assert_eq!(citations("these are just tasks and a CI job").len(), 0);
        assert_eq!(citations("`just --summary` is the authority").len(), 0);
        assert_eq!(citations("`just <task>` is a usage line").len(), 0);
        assert_eq!(
            citations("run `just crap-delta <base>` after")
                .first()
                .map(|(_, n)| n.as_str()),
            Some("crap-delta")
        );
    }

    #[test]
    fn an_empty_file_is_a_broken_scan_rather_than_a_clean_one() {
        use super::scan_broke;

        // The failure mode a text-scanning gate is most prone to: passing by reading nothing.
        // Three ways this parser could stop reading, and each has to be loud rather than green.
        let found = scan_broke("");
        assert!(
            found.iter().any(|p| p.contains("no recipes")),
            "an empty justfile must not pass: {found:?}"
        );
        let no_cargo = "docs:\n    pixi run --frozen -e docs docs\n";
        let found = scan_broke(no_cargo);
        assert!(
            found.iter().any(|p| p.contains("no cargo verification line")),
            "a justfile with no verification line must not pass: {found:?}"
        );
        assert!(
            found.iter().any(|p| p.contains("no `just <task>` citation")),
            "a justfile this repo's size with no citation means the span walk broke: {found:?}"
        );
    }

    #[test]
    fn this_repos_own_justfile_passes() {
        // The rules judge the real file, not only fixtures. This is the test that was RED before
        // `just check` printed its scope, and it is what fails if the notice is deleted later.
        let root = super::repo::root().expect("the repo root is discoverable");
        let text = std::fs::read_to_string(root.join(super::JUSTFILE)).expect("the justfile is readable");
        assert_eq!(super::scan_broke(&text), Vec::<String>::new());
        assert_eq!(problems(&text), Vec::<String>::new());
    }
}
