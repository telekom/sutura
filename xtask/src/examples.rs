//! Every directory under `examples/` is run by a test, or it does not exist.
//!
//! The plan's own rule for deployment variants is *"every deployment variant gets a WORKING
//! end-to-end example, and 'working' means a test runs it. Not a README describing what would
//! happen"* - and `github.com/telekom/sutura#130` measured what it looks like when nothing holds
//! it: `examples/multi-player` was a README and nothing else, describing a shape no binary here
//! could open. Both examples satisfy the rule today. Nothing made them.
//!
//! **A README is the failure this catches**, because a directory of prose under `examples/` reads
//! as something you can run. So the rule is: for each `examples/<name>`, some line of TEST CODE A
//! RUN HERE REACHES must name a path under `examples/<name>` that git publishes.
//!
//! # What a line of test code is, and why neither half is written here
//!
//! Both halves are questions this repository has already answered once, so this gate asks rather
//! than answers them. A gate that scans a language has to LEX it, and the first version of this
//! one hand-rolled both halves and got both wrong in the same direction - permissively.
//!
//! * **What is code** is [`code_lines`](crate::serde_parse::scan::code_lines): every line with its
//!   comments and its multi-line string interiors blanked out. A doc comment naming a path is a
//!   claim; a line of code naming it is a read - and both examples here are named in prose several
//!   times over, so a rule that accepted a comment would have passed the README-only directory it
//!   exists to catch. Rust has THREE comment forms and `!line.starts_with("//")` saw one: measured
//!   on this tree, one sentence about a README-only variant written as `code; // ...` or inside
//!   `/* ... */` turned the verdict green. That lexer also blanks a string that SPANS LINES, which
//!   matters for a second reason - the whole evidence for the flagship example was once one line of
//!   a gate's `\`-continued remedy prose.
//! * **Which lines are test code** is [`regions::scope`](crate::causality::regions::scope): a
//!   dedicated `tests/` target, a `#![cfg(test)]` file, an out-of-line module a parent declares
//!   under `#[cfg(test)]`, or the brace-balanced range of each `#[cfg(test)]` item. Per LINE, which
//!   is the half a whole-file predicate got wrong twice over: `#[test]` appearing ANYWHERE in a
//!   file made the file's production half passing evidence, and five files in this tree qualified
//!   on the substring alone - the sharpest being a module doc that reads *"a file that adds no
//!   `#[test]`"*, classified as test code because the sentence contains the string.
//! * **Which of those lines a run here reaches** is
//!   [`attributes::cells`](crate::causality::attributes::cells): the attribute vocabulary
//!   `super::causality` scopes its own runs with, asked in the other direction - from a LINE to
//!   the test containing it. TWO LEVELS, because fixing one level does not fix the next and this
//!   tree is the proof. A line inside an `#[ignore]`d test is not evidence; AND a file whose
//!   every declared test is `#[ignore]`d yields none at all. The second is what the first cannot
//!   reach: the only line reaching `examples/multi-player` sits in a HELPER the tests call rather
//!   than in a test body, so the per-line rule alone would leave `#[ignore]` on both of that
//!   file's cells with the reach still standing. Measured before either was written - `#[ignore]`
//!   on both cells left the verdict byte-identical to the healthy one.
//!
//! # The limits, next to the claim
//!
//! * **A mention is not an execution.** `let _ = "examples/foo";` in a test would satisfy this. The
//!   gate proves a test REACHES for the directory, not that it asserts anything about what came
//!   back - that is the reviewer's job, and `crates/sutura-cli/tests/example.rs` is the bar.
//! * **The literal has to sit on ONE line, anchored at a path boundary.** What passes is
//!   `examples/<name>` either starting a string literal or reached by `../` - the two shapes a
//!   repo-relative reach takes here. So `join("../../examples/multi-player/question.json")` is a
//!   read and `join("examples").join("multi-player")` is not, and neither is `format!` over a
//!   variant name, because `{` is outside the path charset and the name comes out empty. The
//!   failure message says which shape it looked for, so a false red is diagnosable rather than
//!   being read as the remedy it prints.
//! * **The anchor is a path boundary, not the repo root, and the reach has to RESOLVE.** The
//!   anchor rejects `my_examples/<name>`, a per-crate `crates/<name>/examples/` and a
//!   `format!("{tmp}/examples/<name>")`, all of which satisfied the unanchored scan; the
//!   resolution rejects a path git does not publish, which is the shape a synthetic corpus takes
//!   and the shape a rename leaves behind. **What neither reaches, stated because it was
//!   measured:** a literal is repo-relative only under an assumption about the root it is joined
//!   to, and the root is routinely on ANOTHER LINE - `crates/sutura-cli/tests/documented.rs`
//!   joins `"examples/single-player/README.md"` to a `repo_root()` two functions up, and
//!   `tmp.join("examples/multi-player")` is the same shape with a different root. Resolution does
//!   not separate them either, and that was measured rather than assumed:
//!   `examples/multi-player` is a directory this repository publishes, so the temp-root form
//!   `github.com/telekom/sutura#308` names still counts. What it removes is the FABRICATED path
//!   at that anchor - `tmp.join("examples/multi-player/corpus-with-two-metrics.json")` - and the
//!   stale one. The residue is held by review.
//! * **An attribute that DECIDES the run and cannot be evaluated is not evidence, and the cell it
//!   sits on is where that stops.** `#[cfg(..)]` other than the exact `#[cfg(test)]`, and any
//!   `#[cfg_attr(..)]`, make "does a run here reach this line" a question about features, targets
//!   and flags this scan does not have; each such cell is dropped from the evidence and NAMED in
//!   the verdict. What that does NOT reach is the same attribute over something CONTAINING the
//!   cell - `crates/sutura-serve/tests/served.rs` gates a whole `mod tests` on `#[cfg(unix)]` -
//!   because closing that needs the cfg item's brace range, which is `regions::item_end`'s
//!   instrument aimed one level out. Held by review, and the shapes are
//!   `git grep -n '#\[cfg(' -- 'crates/**/*.rs'`.
//! * **A dropped cell is stated, not refused, and that is a correction to `#400`'s own remedy.**
//!   That issue asks for the `unresolvable` refusal, and routing an unevaluable cell there turns
//!   this gate RED on the healthy tree: eight cells across
//!   `crates/sutura-cli/src/sources/bigquery.rs` and `crates/sutura-serve/src/tests.rs` are
//!   legitimately written under `#[cfg(feature = "bigquery")]` or its negation, and the remedy for
//!   a feature-gated test cannot be to delete it. Fail-closed for the CLAIM keeps the gate: the
//!   cell is not evidence, so a variant whose only reach sits in one fails on the variant.
//! * **The corpus is the workspace's MEMBERS**, off the root manifest's `[workspace] members`,
//!   the authority `crate::fmt` already uses for the neighbouring reason. Measured on merged
//!   `main`: a variant whose only reach was `vendor/mimalloc_rust/src/lib.rs:73` passed at exit
//!   0, where that crate is `[workspace] exclude`d and `cargo nextest run --workspace` never
//!   compiles it. What this costs: a member declared by a glob is a refusal rather than a guess,
//!   and a member the scan reaches no file of is a refusal rather than a smaller number.
//! * **`causality::scoped` shares the blank-line gap and is NOT changed**, because the direction of
//!   failure differs: there a missed `#[ignore]` puts the test into a filterset and nextest exits 4
//!   with `no tests to run`, which is loud, and here it was a green verdict. One lexer, two callers,
//!   two directions - the refusal is in [`attributes::cells`](crate::causality::attributes::cells)
//!   rather than in the shared walk for that reason.
//! * **A reach in a helper is reached through its own file only.** The two `#[ignore]` levels
//!   answer *does anything in THIS file run*; a helper here that only an `#[ignore]`d test in
//!   ANOTHER file calls is still evidence, because a call graph is not a line scan. The narrower
//!   version of the same gap: a file with four running tests, whose reach sits in a helper only
//!   its one `#[ignore]`d cell calls, passes both levels.
//! * **A variant is a directory git would publish**, read off the same listing as the evidence -
//!   see [`variants`] for the two ways the filesystem disagreed with it. What that costs, stated:
//!   an EMPTY `examples/<name>` is in no listing, so nothing watches it, in this venue or any
//!   other. Measured: `mkdir examples/empty-probe` leaves the verdict at its healthy line, and one
//!   file git would publish inside it - tracked, or merely unignored - fails the gate by name.
//! * **It says nothing about what is IN the directory.** An example whose data was deleted fails
//!   the test that reads it, in `nextest`, which is the venue for that.
//!
//! # What the verdict WITNESSES, and what a count cannot
//!
//! Eight fail-closed arms in [`problems`], because this gate exists to stop a directory going
//! unwatched and every one of them would otherwise read as a clean tree: no variant, no line of
//! test code a run reaches, no test DECLARATION out of files that carry test code, an exclusion
//! that removed nothing, a workspace member the scan reached no file of, a file the scan could not
//! read, a cell whose run this cannot decide, and a variant nothing reaches. Five more refuse
//! before it: no repo root, an unreadable root manifest, a member list this cannot state, a path
//! the walk left with no verdict, and a corpus file the scan loop skipped.
//!
//! **The seventh arm has FOUR causes and the last two are one door into this gate's whole
//! failure mode**, both found by review and reproduced on `110591d5` as well as on the branch
//! that fixed the other four: no item under the declaration; a `#[cfg]` or `#[cfg_attr]` over
//! the cell; ONE blank line below a cell's attributes, which `item_below` skips and `attached`
//! treats as a block boundary, so the item resolved and the block came back EMPTY; and a
//! `/* .. */` between an `#[ignore]` and its `#[test]`, which `attached` skips for `//` and
//! for nothing else, so the block came back NON-EMPTY and missing the `#[ignore]`. Both read
//! as a cell with no attributes at all: a reach in the body of an `#[ignore]`d cell printed
//! `reached from 1 file - ..:120` at exit 0 with the per-line rule, the block-anchored region
//! and the per-file floor defeated together, `xtask hygiene: ok - 33 gate(s)` over the same
//! tree, and `rustc --test` calling that cell `ignored`. `attributes::cells` now requires the
//! block to CONTAIN the declaration AND to be the whole block; see that function for why
//! "no such spelling exists in this tree" was not allowed to be the answer.
//!
//! **Three of the arms are pairs rather than numbers**, and `github.com/telekom/sutura#414` is why:
//! a gate's "how much did I read" number derived from its own loop cannot detect the loop
//! narrowing. `test_files`/`declared` is files against the attributes inside them from a second
//! read; the other two are [`corpus`]'s, which carries the measurements and the reason the
//! denominator is the whole listing. A truncated loop is a SOURCE mutation no input tree can
//! express, so the two arms that refuse on one are held by mutation review rather than by a
//! fixture - stated because it is exactly the claim `#414` says gets overstated.

mod corpus;

use std::collections::{BTreeMap, BTreeSet};

use corpus::{Corpus, EXAMPLES, Members, gate_crate, publishes, reaches_for, resolved, variants};

use crate::Verdict;
use crate::causality::{attributes, regions};
use crate::repo;
use crate::serde_parse::scan::code_lines;

/// Which files reach a variant, and the first line in each that names it.
type Reaches = BTreeMap<String, BTreeMap<String, usize>>;

/// What the scan found.
#[derive(Debug, Default)]
struct Evidence {
    /// Variant name to the files whose test code reaches for it.
    reaches: Reaches,
    /// Files carrying at least one line of test code a run in this venue reaches.
    test_files: usize,
    /// Tests declared across those files that a run reaches.
    ///
    /// The SECOND number, and it is here because one number cannot witness this scan: the count
    /// above is over files and this one is over the attributes inside them, so the vocabulary
    /// silently ceasing to match leaves `test_files` at its healthy line and collapses this to
    /// zero. [`problems`] refuses on that pair rather than printing it.
    declared: usize,
    /// Files whose every declared test is `#[ignore]`d, from which nothing was taken as evidence.
    all_ignored: usize,
    /// Files carrying a test-declaring attribute whose item could not be resolved, with the count.
    ///
    /// A REFUSAL, not a note: a test this cannot resolve is one this cannot say runs, so every
    /// line in that file is evidence of unknown reach. Measured over this tree before it was made
    /// one - see [`run`]'s verdict, which states the number it resolved.
    unresolvable: Vec<String>,
    /// Cells whose run is decided by an attribute the scan cannot evaluate, so their lines are
    /// not evidence, as `<attribute> over the test at line <n> in <file>`.
    ///
    /// STATED rather than refused, and that is a correction to
    /// `github.com/telekom/sutura#400`'s own remedy 3 rather than a softening of it: routing these
    /// to the `unresolvable` refusal turns this gate RED on the healthy tree, because eight cells
    /// in `crates/sutura-cli/src/sources/bigquery.rs` and `crates/sutura-serve/src/tests.rs` are
    /// legitimately written under `#[cfg(feature = "bigquery")]` or its negation. Fail-closed for
    /// the CLAIM is what this gate needs: the cell is not a run this venue reaches, so a variant
    /// whose only reach sits in one fails on the variant, with this note beside it.
    undecidable: BTreeSet<String>,
    /// Reaches dropped for naming a path git does not publish, as `<path> (<file>:<line>)`.
    ///
    /// Carried rather than dropped silently: a stale or fabricated path stops being evidence
    /// here, and a variant that then has none fails on the message about the variant - which
    /// reads as *nothing reaches it* when the truth is *what reaches it names nothing*. Stated
    /// rather than refused, because a path a test builds and does not read is legitimate and this
    /// gate is not the venue for what is IN the directory.
    unpublished: BTreeSet<String>,
}

/// Every `examples/<name>` a line of test code reaches for, and where from.
///
/// `files` maps a repo-relative path to that file's RAW text, and the blanking happens HERE rather
/// than in the caller - so a unit test over this function covers the blanking too, which is the
/// difference between testing the rule and testing the plumbing above it. One map for the whole
/// scan rather than a file at a time, because `regions::scope` follows a `#[cfg(test)] mod x;` into
/// the PARENT file: an out-of-line test module is test code and nothing inside the file says so.
///
/// `published` is the listing a reach has to resolve against, and it comes from the same place
/// [`variants`] does - see [`resolved`] for what that removes and what it does not.
///
/// `Err` when the loop below did not visit every file it was handed. The corpus's own length is a
/// different derivation from this loop's counter - the length comes off [`Corpus`]'s private field,
/// which only its walk fills - so narrowing either one breaks the equality rather than moving both,
/// which is what `github.com/telekom/sutura#414` measured five gates doing. A truncated walk here
/// gave 12 of 205 files, 69 of 1333 declarations and exit 0.
fn evidence(files: &BTreeMap<String, String>, published: &BTreeSet<String>) -> Result<Evidence, String> {
    let blanked: BTreeMap<String, String> = files
        .iter()
        .map(|(rel, text)| (rel.clone(), code_lines(text).join("\n")))
        .collect();
    // The owned `String` is the `PostImage` port's signature rather than a borrow-checker dodge,
    // and only the parent-module lookup asks for one.
    let read = |path: &str| blanked.get(path).cloned();
    let mut found = Evidence::default();
    let mut looked = 0_usize;
    for (rel, text) in &blanked {
        looked = looked.saturating_add(1);
        let tests = regions::scope(rel, &read);
        // BOTH images: the file as written, whose comment lines keep an attribute block together,
        // and the blanked one, in which a commented-out `#[test]` is not a declaration.
        let cells = attributes::cells(files.get(rel).map_or("", String::as_str), text);
        for entry in cells.unresolved() {
            found.unresolvable.push(format!("`{rel}`: {entry}"));
        }
        for entry in cells.undecidable() {
            found.undecidable.insert(format!("{entry} in `{rel}`"));
        }
        if cells.nothing_runs() {
            // THE FILE-LEVEL FLOOR. Nothing in it runs, so a helper in it is unreachable from
            // this venue through this file - which is the half a per-line rule cannot reach, and
            // the half this repository's own flagship evidence needs: see `Cells::nothing_runs`.
            found.all_ignored = found.all_ignored.saturating_add(1);
            continue;
        }
        found.declared = found.declared.saturating_add(cells.runs());
        let mut carries_test_code = false;
        for (index, line) in text.lines().enumerate() {
            let number = index.saturating_add(1);
            if !tests.covers(number) {
                continue;
            }
            carries_test_code = true;
            // THE LINE-LEVEL RULE, beside the floor rather than instead of it: a file with four
            // running tests and the reach inside its one `#[ignore]`d cell passes the floor.
            if cells.unreached(number) {
                continue;
            }
            for reach in reaches_for(line) {
                let Some(variant) = resolved(&reach, published) else {
                    found.unpublished.insert(format!("`{reach}` ({rel}:{number})"));
                    continue;
                };
                found.reaches.entry(variant).or_default().entry(rel.clone()).or_insert(number);
            }
        }
        if carries_test_code {
            found.test_files = found.test_files.saturating_add(1);
        }
    }
    if looked != files.len() {
        return Err(format!(
            "looked at {looked} of the {} file(s) in the corpus - the rest were never scanned, so a variant this calls unreached may be reached from the part the loop skipped",
            files.len()
        ));
    }
    Ok(found)
}

/// The verdict, as a list of problems. Pure, so every arm is testable without a repo - which is
/// the point: an arm reached only through the filesystem is an arm nothing holds.
fn problems(variants: &BTreeSet<String>, evidence: &Evidence, scan: &Corpus) -> Vec<String> {
    let mut problems = Vec::new();
    let crate_dir = scan.crate_dir();
    for member in scan.barren() {
        // FAIL CLOSED on a SET OF NAMES, which is the half a pair of counts cannot hold: an item
        // the walk never counted cannot be caught by a floor over the count, so narrowing the
        // SCOPE predicate moves files from `read` to `outside` and leaves the accounting equal.
        // The member list comes off the root manifest and the members reached come off the
        // listing, so neither number is read off the other.
        problems.push(format!(
            "read no file of the workspace member `{member}/` - a run here compiles it, so a corpus that reached none of it is a narrowed scan rather than a crate with no code"
        ));
    }
    if scan.own() == 0 {
        // FAIL CLOSED. The self-exclusion is what keeps this a gate rather than a mirror, and it
        // fails OPEN when it stops matching - so it proves it removed something.
        problems.push(format!(
            "excluded no file under `{crate_dir}/` - this gate's own fixtures name paths under `{EXAMPLES}` and would satisfy it, so an exclusion that matches nothing is a broken scan"
        ));
    }
    for entry in scan.unreachable() {
        // NOT swallowed. `test_files` is a count over the files that WERE read, so it cannot see
        // this: one unreadable test file made the gate blame the tree for the reach it had missed.
        problems.push(format!(
            "could not read {entry} - a variant this scan calls unreached may be reached from the part it could not read"
        ));
    }
    for entry in &evidence.unresolvable {
        // FAIL CLOSED, for the reason above one line over: whether a line is evidence now depends
        // on whether the test around it runs, so a cell whose run this cannot decide is a file
        // whose reaches are of unknown reach. Silently counting them is the fail-OPEN direction
        // and is the state this gate was in, for two spellings: no item under the declaration, and
        // a `#[cfg(..)]` or `#[cfg_attr(..)]` over the cell that decides the run per build.
        problems.push(format!(
            "could not decide whether a run here reaches {entry} - a reach inside a test this scan cannot see is a reach it cannot say runs"
        ));
    }
    if variants.is_empty() {
        problems.push(format!(
            "no directory under `{EXAMPLES}` - this gate watches deployment variants and found none, so the scan is broken rather than the tree clean"
        ));
        return problems;
    }
    if evidence.test_files == 0 {
        // FAIL CLOSED, and instead of the arm below rather than beside it: with no line of test
        // code read, every variant is unreached and that message would blame the tree for a scan
        // that never ran. The `#[ignore]` count is stated because it is the OTHER way this
        // reaches zero, and the two ask for opposite things: a broken scan is fixed here, a
        // workspace whose every test is ignored is fixed in the tree.
        problems.push(format!(
            "read no line of test code a run here reaches, out of the workspace ({} file(s) dropped for declaring only `#[ignore]`d tests) - the scan is broken, not the examples",
            evidence.all_ignored
        ));
        return problems;
    }
    if evidence.declared == 0 {
        // FAIL CLOSED on the PAIR, which is the only thing that witnesses this scan. `test_files`
        // is a count over files and this is a count over the attributes inside them, from a
        // different read - so the attribute vocabulary silently ceasing to match leaves the first
        // at its healthy line and collapses this to zero, and every `#[ignore]` in the tree
        // becomes invisible with nothing to see it by.
        problems.push(format!(
            "read no test declaration out of {} file(s) of test code - one such file is ordinary and all of them is not, so the attribute scan is broken rather than the tree clean",
            evidence.test_files
        ));
        return problems;
    }
    for variant in variants.iter().filter(|variant| !evidence.reaches.contains_key(*variant)) {
        problems.push(format!(
            "`{EXAMPLES}{variant}/` is named by no test that runs here - a directory of prose under `{EXAMPLES}` reads as something a reader can run, so give it a test that reaches for it or delete it until the deployment it describes exists. Looked for: the literal `{EXAMPLES}{variant}` on one line of test code, starting a string literal or reached by `../`, naming a path git publishes, and outside every `#[ignore]`d test"
        ));
    }
    problems
}

/// One variant's evidence, for the verdict.
///
/// The verdict used to state the SCAN's size - `each reached by one of N test file(s)` - which is a
/// count over a set that is not the claim being made. Measured: one variant was reached from a
/// dozen-odd files and the other from exactly one, and the line stated neither, so thinning the
/// evidence for a variant down to two fixtures inside this crate printed a verdict byte-identical
/// to the healthy one. No figure is written here, because the two the verdict prints move with the
/// tree and a copy of them rots first. The file is named when there is only one, because one is
/// the state worth reading.
fn report(variant: &str, from: &BTreeMap<String, usize>) -> String {
    match from.iter().next() {
        Some((rel, line)) if from.len() == 1 => format!("{variant}: reached from 1 file - {rel}:{line}"),
        _ => format!("{variant}: reached from {} file(s)", from.len()),
    }
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-examples: could not determine the repo root");
        return Verdict::Fail;
    };
    let Some(crate_dir) = gate_crate() else {
        eprintln!("xtask check-examples: could not derive this gate's own crate directory");
        return Verdict::Fail;
    };

    // THE MANIFEST IS THE AUTHORITY on whose code a run here compiles, and the two refusals below
    // are its own: an unreadable root manifest and a member list this cannot state.
    let manifest = match std::fs::read_to_string(root.join("Cargo.toml")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-examples: could not read the root manifest: {error}");
            return Verdict::Fail;
        }
    };
    let members = match Members::parse(&manifest) {
        Ok(members) => members,
        Err(problem) => {
            eprintln!("xtask check-examples: {problem}");
            return Verdict::Fail;
        }
    };
    let scan = match Corpus::read(&root, &files, &members, crate_dir) {
        Ok(scan) => scan,
        Err(problem) => {
            eprintln!("xtask check-examples: {problem}");
            return Verdict::Fail;
        }
    };

    let found = variants(&files);
    let evidence = match evidence(scan.sources(), &publishes(&files)) {
        Ok(evidence) => evidence,
        Err(problem) => {
            eprintln!("xtask check-examples: {problem}");
            return Verdict::Fail;
        }
    };
    let problems = problems(&found, &evidence, &scan);

    if problems.is_empty() {
        println!(
            "xtask check-examples: ok - {} variant(s) under {EXAMPLES}, {}, from {} file(s) of test code declaring {} test(s) that run here ({} file(s) dropped for declaring only `#[ignore]`d tests)",
            found.len(),
            scan.witness(),
            evidence.test_files,
            evidence.declared,
            evidence.all_ignored
        );
        let nowhere = BTreeMap::new();
        for variant in &found {
            println!("  {}", report(variant, evidence.reaches.get(variant).unwrap_or(&nowhere)));
        }
        for entry in &evidence.unpublished {
            println!("  note: {entry} names no path git publishes, so it is not evidence");
        }
        for entry in &evidence.undecidable {
            println!("  note: {entry} decides its own run, so its lines are not evidence");
        }
        return Verdict::Pass;
    }

    eprintln!("xtask check-examples: FAILED");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    for entry in &evidence.unpublished {
        // ON THIS SIDE TOO, and this is the side it is for: a variant whose only reach named a
        // path git does not publish fails on `named by no test`, which reads as *nothing reaches
        // it* when the truth is *what reaches it names nothing*.
        eprintln!("  note: {entry} names no path git publishes, so it is not evidence");
    }
    for entry in &evidence.undecidable {
        // Same reason, one cause over: a variant whose only reach sits in a cell gated by an
        // attribute this cannot evaluate fails on `named by no test`, and the cause is here.
        eprintln!("  note: {entry} decides its own run, so its lines are not evidence");
    }
    eprintln!();
    eprintln!("An example is a promise that something runs, and a test is the only thing that makes it.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::corpus::{Corpus, anchored, is_under, publishes, reaches_for, variants};
    use super::{Evidence, evidence, problems, report};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    /// The scan's input: repo-relative paths to RAW file text, exactly what `run` hands it.
    fn files(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(rel, text)| (String::from(*rel), String::from(*text)))
            .collect()
    }

    /// The listing the fixtures resolve against, through the same function `run` uses.
    ///
    /// A fixture path that is NOT in here is the point of one of the tests below, so this is
    /// written out rather than derived from whatever the fixtures happen to say.
    fn corpus() -> BTreeSet<String> {
        publishes(&[
            String::from("examples/a-variant/README.md"),
            String::from("examples/b-variant/README.md"),
            String::from("examples/x/README.md"),
            String::from("examples/x/q.json"),
            String::from("examples/x/mod.rs"),
            String::from("examples/one/data/rows.csv"),
            String::from("examples/two/README.md"),
        ])
    }

    fn scan(entries: &[(&str, &str)]) -> Evidence {
        evidence(&files(entries), &corpus()).expect("the loop visits every file it is handed")
    }

    fn reached(entries: &[(&str, &str)]) -> BTreeSet<String> {
        scan(entries).reaches.keys().cloned().collect()
    }

    /// A corpus that removed something and dropped nothing, so the other arms are what is under
    /// test. Through the `#[cfg(test)]` door, because [`Corpus`]'s fields are private and its
    /// shipping constructor is the walk - which is the property that keeps the verdict's numbers
    /// off a caller's arithmetic.
    fn whole() -> Corpus {
        Corpus::fixture(73, Vec::new(), Vec::new())
    }

    #[test]
    fn a_variant_reached_from_test_code_is_clean() {
        let found = scan(&[(
            "crates/x/tests/example.rs",
            "#[test]\nfn t() { Path::new(\"../../examples/a-variant\"); }\n",
        )]);
        assert_eq!(found.reaches.keys().cloned().collect::<BTreeSet<_>>(), set(&["a-variant"]));
        assert_eq!(found.test_files, 1);
        assert_eq!(found.declared, 1, "the second number the verdict states, from the other read");
        assert!(problems(&set(&["a-variant"]), &found, &whole()).is_empty());
    }

    #[test]
    fn a_variant_only_a_comment_mentions_is_a_problem() {
        // The README-only directory this gate exists for was already DISCUSSED in the test that
        // could not run it, so a rule accepting a comment would have passed it.
        let found = scan(&[(
            "crates/x/tests/example.rs",
            "//! `Path::new(\"../../examples/a-variant\")` is the deployment shape.\n#[test]\nfn t() {}\n",
        )]);
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        let said = problems(&set(&["a-variant"]), &found, &whole());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first().is_some_and(|first| first.contains("named by no test")),
            "{said:?}"
        );
        // The remedy names the shape that was looked for, so a false red is diagnosable.
        assert!(said.first().is_some_and(|first| first.contains("Looked for:")), "{said:?}");
    }

    #[test]
    fn a_comment_is_not_code_in_any_of_rusts_three_forms() {
        // Every fixture below wraps a path the ANCHOR would accept, so the blanking is the only
        // thing that can make it not a reach. THAT IS THE POINT: a fixture whose path is
        // unanchored passes this test with the lexer taken out, which is the vacuous shape the
        // review that produced this test was about. Measured - removing `code_lines` reddens it.
        for source in [
            "// let p = \"examples/x\";\n",
            "    /// `Path::new(\"../../examples/x\")` is the corpus.\n",
            "    //! see `Path::new(\"../../examples/x\")`\n",
            // The two forms the predicate this replaced could not see at all.
            "let n = 1; // was let p = \"examples/x\";\n",
            "/* let p = \"examples/x\"; */\n",
            "/*\n * let p = \"examples/x\";\n */\n",
            "/* /* let p = \"examples/x\"; */ */\n",
            // A string that SPANS LINES is prose about a path, not a read of one - which is what
            // the flagship example's whole evidence once was.
            "const R: &str = \"restore ../../examples/x \\\n    and rerun\";\n",
        ] {
            assert!(reached(&[("crates/x/tests/t.rs", source)]).is_empty(), "{source}");
        }
        // Code that names a path still counts, in both of the shapes this tree writes.
        assert_eq!(reached(&[("crates/x/tests/t.rs", "let p = \"examples/x\";\n")]), set(&["x"]));
        assert_eq!(
            reached(&[("crates/x/tests/t.rs", "#[path = \"../../examples/x/mod.rs\"]\n")]),
            set(&["x"])
        );
    }

    #[test]
    fn a_reach_has_to_be_anchored_at_a_path_boundary() {
        assert!(anchored(""), "the start of a line");
        assert!(anchored("let p = \""), "the start of a string literal");
        assert!(anchored("Path::new(\"../../"), "a walk up out of the manifest directory");
        assert!(!anchored("let p = \"my_"), "my_examples/ is a different directory");
        assert!(
            !anchored("format!(\"{tmp}/"),
            "a synthetic tree under a temp root is not this one"
        );
        assert!(
            !anchored("\"crates/x/"),
            "cargo's per-crate examples/ convention is not this one"
        );
        // End to end, because the anchor is only worth what the scan does with it.
        assert!(reached(&[("crates/x/tests/t.rs", "let p = \"/tmp/s/my_examples/x/q.json\";\n")]).is_empty());
        assert_eq!(
            reached(&[("crates/x/tests/t.rs", "let p = \"../../examples/x/q.json\";\n")]),
            set(&["x"])
        );
    }

    #[test]
    fn only_the_test_lines_of_a_file_are_evidence() {
        // THE defect this closes: `#[test]` anywhere in a file made the WHOLE file evidence, so a
        // dead `const` in the production half of any `src/*.rs` carrying a unit-test module
        // satisfied the gate.
        let source = "const SHAPE: &str = \"examples/x\";\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n";
        assert!(
            reached(&[("crates/x/src/lib.rs", source)]).is_empty(),
            "production code in a file with a test module is not test code"
        );
        let inside = "#[cfg(test)]\nmod tests {\n    const SHAPE: &str = \"examples/x\";\n    #[test]\n    fn t() {}\n}\n";
        assert_eq!(
            reached(&[("crates/x/src/lib.rs", inside)]),
            set(&["x"]),
            "the same line inside the test module is"
        );
    }

    #[test]
    fn a_file_whose_only_test_marker_is_prose_is_not_test_code() {
        // Measured on this tree: five files were test code by the `#[test]` substring and by
        // nothing else, the sharpest being a module doc reading "a file that adds no `#[test]`".
        let prose =
            "//! A file that adds no `#[test]` is one the causality gate may revert.\nconst SHAPE: &str = \"examples/x\";\n";
        let found = scan(&[("crates/x/src/notes.rs", prose)]);
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        assert_eq!(found.test_files, 0, "a sentence about a test is not a test");
    }

    #[test]
    fn several_variants_on_one_line_are_all_read() {
        // `find` used to be read once per line, so a second path on the same line was invisible.
        assert_eq!(
            reaches_for("let both = (\"examples/one/data\", \"examples/two\");"),
            vec![String::from("examples/one/data"), String::from("examples/two")],
            "the whole path, so the listing can be asked about what the line names"
        );
        assert_eq!(
            reaches_for("let p = \"../../examples/x/\";"),
            vec![String::from("examples/x")],
            "a trailing separator names the directory, not a child of it"
        );
    }

    #[test]
    fn a_variant_is_a_directory_git_would_publish() {
        // The same listing the evidence comes from, so the two halves cannot disagree. An empty
        // directory is in NO listing, which is exactly why the filesystem was the wrong authority.
        assert_eq!(
            variants(&[
                String::from("examples/README.md"),
                String::from("examples/a-variant/README.md"),
                String::from("examples/b-variant/data/rows.csv"),
                String::from("crates/x/examples/not-a-variant/main.rs"),
            ]),
            set(&["a-variant", "b-variant"]),
            "a file directly under examples/ is not a variant, and a per-crate examples/ is not ours"
        );
        assert!(
            variants(&[String::from("examples/a-variant")]).is_empty(),
            "a file, not a directory"
        );
    }

    #[test]
    fn the_gate_excludes_its_own_crate_and_says_so_when_it_excluded_nothing() {
        assert!(is_under("xtask/src/examples.rs", "xtask"));
        assert!(
            is_under("xtask/src/examples/mod.rs", "xtask"),
            "a move inside the crate stays excluded"
        );
        assert!(
            is_under("xtask/src/changes.rs", "xtask"),
            "the classification fixture next door too"
        );
        assert!(!is_under("xtaskish/src/lib.rs", "xtask"), "a whole segment, not a prefix");
        assert!(!is_under("crates/x/tests/t.rs", "xtask"));
    }

    #[test]
    fn the_verdict_states_each_variants_evidence_and_names_a_lone_file() {
        let mut from = BTreeMap::new();
        from.insert(String::from("crates/x/tests/t.rs"), 69_usize);
        assert_eq!(
            report("multi-player", &from),
            "multi-player: reached from 1 file - crates/x/tests/t.rs:69"
        );
        from.insert(String::from("crates/y/tests/u.rs"), 7_usize);
        assert_eq!(report("multi-player", &from), "multi-player: reached from 2 file(s)");
    }

    #[test]
    fn an_examples_directory_with_no_variants_is_a_broken_scan() {
        let empty = Evidence {
            test_files: 1,
            declared: 1,
            ..Evidence::default()
        };
        let said = problems(&BTreeSet::new(), &empty, &whole());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first().is_some_and(|first| first.contains("the scan is broken")),
            "{said:?}"
        );
    }

    #[test]
    fn no_line_of_test_code_blames_the_scan_rather_than_the_tree() {
        let nothing = Evidence::default();
        let said = problems(&set(&["a-variant"]), &nothing, &whole());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("read no line of test code a run here reaches")),
            "{said:?}"
        );
        assert!(
            said.iter().all(|problem| !problem.contains("named by no test")),
            "the unreached message would blame the tree for a scan that never ran: {said:?}"
        );
    }

    #[test]
    fn an_exclusion_that_removed_nothing_is_a_broken_scan() {
        // THE direction that made this worth a mechanism: a self-exclusion that stops matching
        // fails OPEN, into exactly the mirror it exists to prevent, and the only tell was a file
        // count moving by one.
        let reached = scan(&[(
            "crates/x/tests/example.rs",
            "#[test]\nfn t() { Path::new(\"../../examples/a-variant\"); }\n",
        )]);
        let nothing_removed = Corpus::fixture(0, Vec::new(), Vec::new());
        let said = problems(&set(&["a-variant"]), &reached, &nothing_removed);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("excluded no file under `xtask/`")),
            "{said:?}"
        );
    }

    #[test]
    fn a_reach_inside_an_ignored_test_is_not_evidence() {
        // LEVEL ONE. `#[ignore]` on the cell whose own body reaches for the variant: nextest runs
        // nothing and the line scan could not tell, so the verdict was byte-identical to a healthy
        // one - measured on this tree before this existed.
        let ignored = "#[test]\n#[ignore = \"needs a deployment\"]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n#[test]\nfn u() {}\n";
        let found = scan(&[("crates/x/tests/t.rs", ignored)]);
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        assert_eq!(found.declared, 1, "the neighbour that does run is still declared");
        assert_eq!(found.all_ignored, 0, "the FILE is not dropped - one of its cells runs");
        // The same line in the cell that runs is evidence, so the region and not the file is what
        // moved. Without this the fixture above passes with the ignore check taken out entirely.
        let runs = "#[test]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        assert_eq!(reached(&[("crates/x/tests/t.rs", runs)]), set(&["x"]));
    }

    #[test]
    fn a_reach_on_the_ignore_line_itself_is_not_evidence() {
        // #400's FIRST fail-open, measured on merged `main`: the ignored region started at the
        // DECLARING attribute, so an `#[ignore = ".."]` sitting ABOVE `#[test]` was one line above
        // its own cell and a path named in its reason string counted as a reach from a running
        // test - `reached from 1 file - ../multi_player.rs:88`, exit 0, where line 88 was the
        // `#[ignore]`. Rustfmt-stable, so nothing else moved it. The second cell runs, so the
        // FILE-level floor cannot be what reddens this.
        let reason = "#[ignore = \"restore ../../examples/x/q.json first\"]\n#[test]\nfn t() {}\n#[test]\nfn u() {}\n";
        let found = scan(&[("crates/x/tests/t.rs", reason)]);
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        assert_eq!(found.declared, 1, "the neighbour that runs is still declared");
        assert_eq!(found.all_ignored, 0, "and the file is not dropped whole");
        // The same line above a cell that RUNS is evidence, so the region and not the attribute
        // spelling is what moved.
        let runs = "#[test]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        assert_eq!(reached(&[("crates/x/tests/t.rs", runs)]), set(&["x"]));
    }

    #[test]
    fn a_reach_in_a_cell_an_attribute_decides_is_not_evidence_and_is_named() {
        // #400's THIRD fail-open: `#[cfg(feature = "..")]` on both cells of the one file reaching a
        // variant left the verdict BYTE-IDENTICAL to the healthy one at exit 0, and
        // `#[cfg_attr(all(), ignore)]` did the same. Neither is evaluable here, so neither is a run
        // this venue reaches - and the drop is NAMED, because a variant that then has none fails on
        // the message about the variant.
        for gate in ["#[cfg(feature = \"bigquery\")]", "#[cfg_attr(all(), ignore)]", "#[cfg(unix)]"] {
            let source =
                format!("{gate}\n#[test]\nfn t() {{\n    let p = \"../../examples/x/q.json\";\n}}\n#[test]\nfn u() {{}}\n");
            let found = scan(&[("crates/x/tests/t.rs", source.as_str())]);
            assert!(found.reaches.is_empty(), "{gate}: {:?}", found.reaches);
            assert_eq!(found.undecidable.len(), 1, "{gate}: {:?}", found.undecidable);
            assert!(
                found
                    .undecidable
                    .iter()
                    .next()
                    .is_some_and(|entry| entry.contains(gate) && entry.contains("in `crates/x/tests/t.rs`")),
                "{gate}: {:?}",
                found.undecidable
            );
        }
        // Exactly `#[cfg(test)]` is evaluable and true in a test build, so it is not a drop - or
        // every unit-test module in the workspace would be one.
        let ordinary = "#[cfg(test)]\nmod tests {\n    #[cfg(test)]\n    #[test]\n    fn t() {\n        let p = \"../../examples/x/q.json\";\n    }\n}\n";
        let found = scan(&[("crates/x/src/lib.rs", ordinary)]);
        assert_eq!(found.reaches.keys().cloned().collect::<BTreeSet<_>>(), set(&["x"]));
        assert!(found.undecidable.is_empty(), "{:?}", found.undecidable);
    }

    #[test]
    fn a_reach_below_a_blank_line_in_an_ignored_cell_is_a_refusal_rather_than_evidence() {
        // THE FIFTH DOOR, at the gate rather than at the resolver, because this is where it was
        // exit 0. `causality::attributes`' two halves disagree about a blank line - one skips it to
        // reach the item, the other treats it as a block boundary - so one blank line below a
        // cell's attributes gave a cell with no attributes at all: no `#[ignore]`, no `#[cfg]`,
        // counted as running. Reviewed and reproduced on `110591d5` AND on this branch with all
        // four region fixes present: the reach in the BODY of the `#[ignore]`d cell printed
        // `multi-player: reached from 1 file - ..multi_player.rs:120`, exit 0, with
        // `xtask hygiene: ok - 33 gate(s)` over the same tree. Not over-determined, and
        // `rustfmt --check` exits 0 on it.
        //
        // Cell 1 RUNS here, exactly as the reviewer's sharpest case had it, so the per-file floor
        // cannot be what reddens this.
        let blank = "#[test]\nfn u() {}\n#[ignore = \"needs a deployment\"]\n#[test]\n\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        let found = scan(&[("crates/x/tests/t.rs", blank)]);
        assert_eq!(found.all_ignored, 0, "the file floor cannot fire - cell 1 runs: {found:?}");
        // THE VARIANT IS STILL REACHED, and that is the point rather than an oversight: the reach
        // is recorded, so the `named by no test` arm does NOT fire and this fixture is a clean pass
        // against base. What reddens it is the refusal, and nothing else - so the assertion below
        // is `exactly one problem, and it is the refusal`, which is empty against base.
        assert_eq!(found.reaches.keys().cloned().collect::<BTreeSet<_>>(), set(&["x"]));
        let said = problems(&set(&["x"]), &found, &whole());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("could not decide whether a run here reaches")
                    && first.contains("does not reach its item")),
            "{said:?}"
        );
        // The same file with the blank line removed resolves, the cell is `#[ignore]`d as it should
        // be, the reach is gone and there is nothing to refuse - so the blank line is what moved,
        // rather than the fixture being unreadable either way.
        let joined = "#[test]\nfn u() {}\n#[ignore = \"needs a deployment\"]\n#[test]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        let closed = scan(&[("crates/x/tests/t.rs", joined)]);
        assert!(closed.reaches.is_empty(), "{:?}", closed.reaches);
        assert!(closed.unresolvable.is_empty(), "{closed:?}");
        assert_eq!(closed.declared, 1, "and the cell that runs is still counted: {closed:?}");
    }

    #[test]
    fn a_workspace_member_the_scan_reached_no_file_of_is_a_narrowed_scan() {
        // #414's property, and the one a pair of counts cannot hold: an item the walk never counted
        // cannot be caught by a floor over the count, so a narrowed SCOPE predicate moves files
        // from `read` to `outside the workspace` and the accounting still balances. The members
        // reached come off the listing and the members declared come off the root manifest, so
        // neither number is read off the other.
        let reached = scan(&[(
            "crates/x/tests/example.rs",
            "#[test]\nfn t() { Path::new(\"../../examples/a-variant\"); }\n",
        )]);
        let narrowed = Corpus::fixture(73, Vec::new(), vec![String::from("crates/sutura-http")]);
        let said = problems(&set(&["a-variant"]), &reached, &narrowed);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("read no file of the workspace member `crates/sutura-http/`")),
            "{said:?}"
        );
    }

    #[test]
    fn a_file_where_nothing_runs_is_no_evidence_even_from_a_helper() {
        // LEVEL TWO, and the level that matters here: fixing one does not fix the next. The only
        // line reaching `examples/multi-player` in this repository sits in a HELPER the tests
        // call, so the per-line rule above leaves `#[ignore]` on every cell of that file with the
        // reach still standing. This is that file's shape.
        let helper = "fn corpus() -> PathBuf {\n    Path::new(root).join(\"../../examples/x/q.json\")\n}\n#[test]\n#[ignore = \"needs a deployment\"]\nfn t() {\n    corpus();\n}\n";
        let found = scan(&[("crates/x/tests/t.rs", helper)]);
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        assert_eq!(found.all_ignored, 1, "the file is dropped whole");
        assert_eq!(found.test_files, 0, "and it is not counted as evidence-bearing either");
        // One running cell puts the helper back, which is what keeps this a floor rather than a
        // rule about helpers: a helper in a file where something runs is reachable.
        let mixed = format!("{helper}#[test]\nfn u() {{ corpus(); }}\n");
        assert_eq!(reached(&[("crates/x/tests/t.rs", mixed.as_str())]), set(&["x"]));
    }

    #[test]
    fn a_commented_out_test_neither_runs_nor_hides_an_ignore() {
        // TWO IMAGES OF ONE FILE, and each fixture fails a different single-image reading. In the
        // first the only `#[test]` is inside a block comment, so a scan over the written text
        // alone counts a running test and the file survives the floor.
        let commented =
            "/*\n#[test]\nfn dead() {}\n*/\n#[test]\n#[ignore = \"x\"]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        let found = scan(&[("crates/x/tests/t.rs", commented)]);
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        assert_eq!(found.all_ignored, 1, "{found:?} - the commented-out cell does not run");
        // In the second a doc comment sits between the attributes and the item. A scan over the
        // BLANKED text alone reads that as a blank line, which ends an attribute block, and the
        // `#[ignore]` is lost.
        let documented =
            "#[test]\n#[ignore = \"x\"]\n/// What this would prove.\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        let still = scan(&[("crates/x/tests/t.rs", documented)]);
        assert!(still.reaches.is_empty(), "{:?}", still.reaches);
        assert_eq!(still.all_ignored, 1, "{still:?} - the doc comment does not detach the ignore");
    }

    #[test]
    fn a_reach_that_names_no_published_path_is_not_evidence() {
        // The half of `#308`'s second limit that IS mechanisable: a synthetic corpus names a
        // FABRICATED path at the real anchor, and that shape stops counting. `examples/x/q.json`
        // is in the listing and `examples/x/nope.json` is not.
        let fabricated = "#[test]\nfn t() {\n    let p = tmp.join(\"examples/x/nope.json\");\n}\n";
        let found = scan(&[("crates/x/tests/t.rs", fabricated)]);
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        assert_eq!(
            found.unpublished.len(),
            1,
            "{:?} - stated, not silently dropped",
            found.unpublished
        );
        assert!(
            found
                .unpublished
                .iter()
                .next()
                .is_some_and(|entry| entry.contains("`examples/x/nope.json` (crates/x/tests/t.rs:3)")),
            "{:?}",
            found.unpublished
        );
        // MEASURED AND STATED IN THE MODULE DOC: the shape the issue itself names is a path this
        // repository publishes, so resolution does not reject it. The limit is narrowed, not gone.
        let coincides = "#[test]\nfn t() {\n    let p = tmp.join(\"examples/x\");\n}\n";
        assert_eq!(reached(&[("crates/x/tests/t.rs", coincides)]), set(&["x"]));
    }

    #[test]
    fn no_test_declaration_out_of_files_of_test_code_is_a_broken_scan() {
        // THE PAIR, and it is a pair because one number could not witness this: `test_files`
        // counts files and `declared` counts the attributes inside them, from a second read of
        // the same text. The attribute vocabulary ceasing to match leaves the first at its
        // healthy line, collapses the second, and takes every `#[ignore]` in the tree out of view.
        let vocabulary_broke = Evidence {
            test_files: 204,
            declared: 0,
            ..Evidence::default()
        };
        let said = problems(&set(&["a-variant"]), &vocabulary_broke, &whole());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("read no test declaration out of 204 file(s)")),
            "{said:?}"
        );
        assert!(
            said.iter().all(|problem| !problem.contains("named by no test")),
            "the unreached message would blame the tree for a scan that stopped seeing tests: {said:?}"
        );
    }

    #[test]
    fn an_unresolvable_test_declaration_is_a_problem_rather_than_a_silent_pass() {
        // Whether a line is evidence now depends on whether the test around it runs, so an
        // attribute this cannot resolve to its item is a file whose reaches are of unknown reach.
        // Counting it as running is the fail-OPEN direction, and is where this gate was.
        let dangling = Evidence {
            test_files: 1,
            declared: 1,
            unresolvable: vec![String::from("`crates/y/tests/t.rs`: `#[cfg(unix)]` over the test at line 12")],
            ..Evidence::default()
        };
        let said = problems(&set(&["a-variant"]), &dangling, &whole());
        assert!(
            said.iter()
                .any(|problem| problem
                    .contains("could not decide whether a run here reaches `crates/y/tests/t.rs`: `#[cfg(unix)]`")),
            "{said:?}"
        );
    }

    #[test]
    fn a_file_the_scan_could_not_read_is_a_problem_rather_than_a_silent_drop() {
        // `.ok()` swallowed this, and the `test_files` floor cannot catch it: the count is over
        // the files that WERE read, so a partial read never fires the arm that exists for it.
        let reached = scan(&[(
            "crates/x/tests/example.rs",
            "#[test]\nfn t() { Path::new(\"../../examples/a-variant\"); }\n",
        )]);
        let partial = Corpus::fixture(
            73,
            vec![String::from("`crates/y/tests/t.rs`: stream did not contain valid UTF-8")],
            Vec::new(),
        );
        let said = problems(&set(&["a-variant"]), &reached, &partial);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("could not read `crates/y/tests/t.rs`")),
            "{said:?}"
        );
    }
}
