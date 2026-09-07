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
//!   on both cells left the verdict byte-identical to the healthy one. The region a cell occupies
//!   starts at the top of its ATTRIBUTE BLOCK rather than at the declaring attribute, because
//!   `#[ignore]` is legal above `#[test]` and the anchor left its own line outside: measured on
//!   this tree, `reached from 1 file - ..:95` at exit 0, where line 95 was the
//!   `#[ignore = ".."]` reason naming the variant.
//! * **Which files a run here reaches at all** is the workspace member list, from
//!   `cargo metadata` - `just test` is `--workspace --all-features`, so a package
//!   `[workspace] exclude` keeps out is one no run here reaches. Measured: one reach planted in
//!   `vendor/mimalloc_rust/src/lib.rs` was accepted as a variant's SOLE evidence at exit 0, and
//!   the vendored allocator put 10 declarations and 2 files into the counts the verdict printed.
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
//! * **An unevaluable run-deciding attribute is read as NOT running, and that costs false reds.**
//!   `#[cfg(..)]` other than the exact `#[cfg(test)]`, and every `#[cfg_attr(..)]`, need the
//!   feature resolution and the target of the run to evaluate, which this scan has not got. Both
//!   spellings were measured as fail-OPEN before: `#[cfg_attr(all(), ignore)]` on both cells of
//!   the one file reaching a variant, and `#[cfg(feature = "..")]` on the same two, each left the
//!   verdict byte-identical to the healthy one at exit 0. The price is stated because it is a
//!   real one: `just test` passes `--all-features`, so a `#[cfg(feature = "x")]` cell DOES run
//!   there and this calls it unreached. Cells written that way ARE live here and none of them
//!   reaches `examples/`, so today the whole cost is a smaller `declared`. **What it still does
//!   not reach** is a `cfg` on an ANCESTOR: a
//!   `#[cfg(feature = "x")] mod tests { .. }` puts the attribute on the module, and the block scan
//!   sees only what is attached to the cell itself.
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
//! Eight fail-closed arms, because this gate exists to stop a directory going unwatched and every
//! one of them would otherwise read as a clean tree: no variant, no line of test code a run
//! reaches, no test DECLARATION out of files that carry test code, an exclusion that removed
//! nothing, a file the scan could not read, a test-declaring attribute whose item it could not
//! resolve, and the two WALK pairs.
//!
//! Three of those are PAIRS rather than numbers, and a pair is the only thing that witnesses a
//! scan. `test_files` counts files and `declared` counts the attributes inside them, from a second
//! read of the same text, so the attribute vocabulary silently ceasing to match leaves the first
//! at its healthy line and takes every `#[ignore]` in the tree out of view with it. The other two
//! are the walks, at BOTH levels, because fixing a floor at one level does not fix the next -
//! `Scan::expected` is counted off the listing and `Scan::read` off the loop that reads it, and
//! `Evidence::visited` is counted off the loop over what that produced. Measured before those
//! existed: the first 38 of 253 files gave `33 file(s) of test code declaring 156 test(s)`, both
//! variants reached, exit 0. A single number in a verdict is exactly what let this gate's earlier
//! defects through.
//!
//! Two submodules, split by the question they answer rather than by size: [`scope`] is *which
//! files a run here could reach at all* - workspace membership, the self-exclusion, and the walk's
//! own counts - and [`reach`] is *what a line names, and what the listing says about it*. What is
//! left here is the rule that turns those two into a verdict.

mod reach;
mod scope;

use std::collections::{BTreeMap, BTreeSet};

use crate::Verdict;
use crate::causality::{attributes, regions};
use crate::repo;
use crate::serde_parse::scan::code_lines;
use reach::{EXAMPLES, publishes, reaches_for, resolved, variants};
use scope::{Scan, gate_crate, in_scope, is_candidate, is_under, workspace};

/// Which files reach a variant, and the first line in each that names it.
type Reaches = BTreeMap<String, BTreeMap<String, usize>>;

/// What the scan found.
#[derive(Debug, Default)]
struct Evidence {
    /// Variant name to the files whose test code reaches for it.
    reaches: Reaches,
    /// Files the INNER walk actually visited, whatever it then made of them.
    ///
    /// The second denominator, and it is a second one because fixing a floor at one level does
    /// not fix the next: [`Scan::read`] witnesses the walk that FILLS the map, and this witnesses
    /// the walk over it. A gate elsewhere in this tree stayed green at `204 of 204 files` with
    /// its inner walk broken. Measured here before it existed: truncating this loop to the first
    /// 38 of 253 files printed `33 file(s) of test code declaring 156 test(s)`, both variants
    /// reached, exit 0.
    visited: usize,
    /// Files carrying at least one line of test code a run in this venue reaches.
    test_files: usize,
    /// Tests declared across those files that a run reaches.
    ///
    /// The SECOND number, and it is here because one number cannot witness this scan: the count
    /// above is over files and this one is over the attributes inside them, so the vocabulary
    /// silently ceasing to match leaves `test_files` at its healthy line and collapses this to
    /// zero. [`problems`] refuses on that pair rather than printing it.
    declared: usize,
    /// Files declaring tests of which none is known to run here, from which nothing was taken as
    /// evidence. `#[ignore]` is one way; an unevaluable `#[cfg(..)]` or `#[cfg_attr(..)]` on the
    /// cell is the other - see [`attributes::cells`].
    none_runs: usize,
    /// Files carrying a test-declaring attribute whose item could not be resolved, with the count.
    ///
    /// A REFUSAL, not a note: a test this cannot resolve is one this cannot say runs, so every
    /// line in that file is evidence of unknown reach. Measured over this tree before it was made
    /// one - see [`run`]'s verdict, which states the number it resolved.
    unresolvable: Vec<String>,
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
fn evidence(files: &BTreeMap<String, String>, published: &BTreeSet<String>) -> Evidence {
    let blanked: BTreeMap<String, String> = files
        .iter()
        .map(|(rel, text)| (rel.clone(), code_lines(text).join("\n")))
        .collect();
    // The owned `String` is the `PostImage` port's signature rather than a borrow-checker dodge,
    // and only the parent-module lookup asks for one.
    let read = |path: &str| blanked.get(path).cloned();
    let mut found = Evidence::default();
    for (rel, text) in &blanked {
        // COUNTED BEFORE ANY `continue`, because this is the INNER walk's numerator and every arm
        // below leaves the loop body early. `run` compares it against the map it handed in.
        found.visited = found.visited.saturating_add(1);
        let tests = regions::scope(rel, &read);
        // BOTH images: the file as written, whose comment lines keep an attribute block together,
        // and the blanked one, in which a commented-out `#[test]` is not a declaration.
        let cells = attributes::cells(files.get(rel).map_or("", String::as_str), text);
        if cells.unresolved() > 0 {
            found
                .unresolvable
                .push(format!("`{rel}`: {} test-declaring attribute(s)", cells.unresolved()));
        }
        if cells.nothing_runs() {
            // THE FILE-LEVEL FLOOR. Nothing in it runs, so a helper in it is unreachable from
            // this venue through this file - which is the half a per-line rule cannot reach, and
            // the half this repository's own flagship evidence needs: see `Cells::nothing_runs`.
            found.none_runs = found.none_runs.saturating_add(1);
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
            // running tests and the reach inside its one `#[ignore]`d cell passes the floor. The
            // region starts at the top of the cell's ATTRIBUTE BLOCK, so a reach written into an
            // `#[ignore = ".."]` reason above the `#[test]` is inside it - measured on this tree
            // as `reached from 1 file - ..:95` at exit 0, where line 95 was that reason.
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
    found
}

/// The verdict, as a list of problems. Pure, so every arm is testable without a repo - which is
/// the point: an arm reached only through the filesystem is an arm nothing holds.
fn problems(variants: &BTreeSet<String>, evidence: &Evidence, scan: &Scan) -> Vec<String> {
    let mut problems = Vec::new();
    let crate_dir = scan.crate_dir;
    if scan.excluded == 0 {
        // FAIL CLOSED. The self-exclusion is what keeps this a gate rather than a mirror, and it
        // fails OPEN when it stops matching - so it proves it removed something.
        problems.push(format!(
            "excluded no file under `{crate_dir}/` - this gate's own fixtures name paths under `{EXAMPLES}` and would satisfy it, so an exclusion that matches nothing is a broken scan"
        ));
    }
    if scan.targets == 0 {
        // FAIL CLOSED on the SECOND AUTHORITY itself. The arm below compares cargo's answer
        // against the walk's, and an empty left-hand side satisfies it vacuously - the
        // floor-with-no-denominator shape one level up.
        problems.push(String::from(
            "compared no workspace target root against the scan - cargo names what this workspace compiles, and with none of them in scope there is nothing checking the git listing against anything but itself"
        ));
    }
    for rel in &scan.unseen {
        // THE PAIR FROM TWO DIFFERENT PLACES. Every other count here is git's listing checked
        // against git's listing, so a walk that stops inside `repo::all_files` moves them in
        // step - measured, `.step_by(2)` there printed `127 of 127 in-scope file(s)` at exit 0
        // over half the tree. Cargo does not read git.
        problems.push(format!(
            "`{rel}` is a target root cargo compiles and this scan never read - the listing it walked is smaller than the workspace, so this verdict is about less than a run here covers"
        ));
    }
    if scan.read != scan.expected {
        // THE OUTER WALK'S PAIR. Two numbers from two places - the listing filtered by `in_scope`,
        // and the loop that reads it - because one number cannot witness a walk that stopped.
        problems.push(format!(
            "reached {} of {} in-scope file(s) - the walk stopped early, so this verdict is about part of the tree",
            scan.read, scan.expected
        ));
    }
    // What the outer walk actually handed on: reached, less the ones it could not read. Derived
    // rather than a third field, so it cannot disagree with the two numbers above it.
    let handed_on = scan.read.saturating_sub(scan.unreadable.len());
    if evidence.visited != handed_on {
        // THE INNER WALK'S PAIR, and it is a SECOND one because fixing a floor at one level does
        // not fix the next: the outer walk can fill the map completely and the loop over it still
        // stop. Measured before this existed - the first 38 of 253 files, both variants reached,
        // exit 0.
        problems.push(format!(
            "visited {} of {handed_on} file(s) read - the evidence walk stopped early, so this verdict is about part of what was read",
            evidence.visited
        ));
    }
    for entry in &scan.unreadable {
        // NOT swallowed. `test_files` is a count over the files that WERE read, so it cannot see
        // this: one unreadable test file made the gate blame the tree for the reach it had missed.
        problems.push(format!(
            "could not read {entry} - a variant this scan calls unreached may be reached from the part it could not read"
        ));
    }
    for entry in &evidence.unresolvable {
        // FAIL CLOSED, for the reason above one line over: whether a line is evidence now depends
        // on whether the test around it runs, so an attribute this cannot resolve to its item is
        // a file whose reaches are of unknown reach. Silently counting them is the fail-OPEN
        // direction and is the state this gate was in.
        problems.push(format!(
            "could not resolve the item under {entry} - a reach inside a test this scan cannot see is a reach it cannot say runs"
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
            "read no line of test code a run here reaches, out of the workspace ({} file(s) dropped for declaring no test a run reaches) - the scan is broken, not the examples",
            evidence.none_runs
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
            "`{EXAMPLES}{variant}/` is named by no test that runs here - a directory of prose under `{EXAMPLES}` reads as something a reader can run, so give it a test that reaches for it or delete it until the deployment it describes exists. Looked for: the literal `{EXAMPLES}{variant}` on one line of test code, in a file of a workspace member, starting a string literal or reached by `../`, naming a path git publishes, and inside a test a run here reaches - which no `#[ignore]`d cell is, and no cell whose `#[cfg(..)]` or `#[cfg_attr(..)]` this cannot evaluate"
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
    let cargo = match workspace(&root) {
        Ok(cargo) => cargo,
        Err(error) => {
            eprintln!("xtask check-examples: {error}");
            return Verdict::Fail;
        }
    };
    let members = cargo.dirs;

    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    let mut scan = Scan {
        crate_dir,
        excluded: 0,
        // From the LISTING, before the walk touches it.
        expected: files.iter().filter(|rel| in_scope(rel, &members, crate_dir)).count(),
        read: 0,
        unseen: Vec::new(),
        targets: 0,
        unreadable: Vec::new(),
    };
    for rel in files.iter().filter(|rel| is_candidate(rel, &members)) {
        if is_under(rel, crate_dir) {
            scan.excluded = scan.excluded.saturating_add(1);
            continue;
        }
        scan.read = scan.read.saturating_add(1);
        reached.insert(rel.clone());
        match std::fs::read_to_string(root.join(rel)) {
            Ok(text) => {
                sources.insert(rel.clone(), text);
            }
            Err(error) => scan.unreadable.push(format!("`{rel}`: {error}")),
        }
    }
    // THE PAIR FROM TWO DIFFERENT PLACES, and the only one here that is not git checked against
    // git: cargo names what this workspace compiles, and a target root the walk never reached is
    // a scan that is about less than the venue runs. Measured before this arm existed:
    // `.step_by(2)` inside `repo::all_files` moved `expected` and `read` in step and printed
    // `127 of 127 in-scope file(s)` at exit 0 over half the tree.
    for rel in cargo.target_roots.iter().filter(|rel| in_scope(rel, &members, crate_dir)) {
        scan.targets = scan.targets.saturating_add(1);
        if !reached.contains(rel) {
            scan.unseen.push(rel.clone());
        }
    }

    let found = variants(&files);
    let evidence = evidence(&sources, &publishes(&files));
    let problems = problems(&found, &evidence, &scan);

    if problems.is_empty() {
        println!(
            "xtask check-examples: ok - {} variant(s) under {EXAMPLES}, from {} file(s) of test code declaring {} test(s) that run here, out of {} of {} in-scope file(s) across {} workspace member(s) whose {} target root(s) the scan all read ({} file(s) dropped for declaring no test a run reaches, {} under `{crate_dir}/` excluded)",
            found.len(),
            evidence.test_files,
            evidence.declared,
            evidence.visited,
            scan.expected,
            members.len(),
            scan.targets,
            evidence.none_runs,
            scan.excluded
        );
        let nowhere = BTreeMap::new();
        for variant in &found {
            println!("  {}", report(variant, evidence.reaches.get(variant).unwrap_or(&nowhere)));
        }
        for entry in &evidence.unpublished {
            println!("  note: {entry} names no path git publishes, so it is not evidence");
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
    eprintln!();
    eprintln!("An example is a promise that something runs, and a test is the only thing that makes it.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::reach::{anchored, publishes};
    use super::{Evidence, Scan, evidence, problems, report};

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
        evidence(&files(entries), &corpus())
    }

    fn reached(entries: &[(&str, &str)]) -> BTreeSet<String> {
        scan(entries).reaches.keys().cloned().collect()
    }

    /// A scan that removed something, dropped nothing and walked all of what it listed - so the
    /// arms under test are the other ones.
    ///
    /// Built FROM the evidence it will be judged beside, because the two walk pairs compare the
    /// two against each other: a fixture with its own literal numbers would fire an arm every
    /// test below would then have to spell around.
    fn whole(evidence: &Evidence) -> Scan {
        Scan {
            crate_dir: "xtask",
            excluded: 73,
            expected: evidence.visited,
            read: evidence.visited,
            unseen: Vec::new(),
            targets: 39,
            unreadable: Vec::new(),
        }
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
        assert!(problems(&set(&["a-variant"]), &found, &whole(&found)).is_empty());
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
        let said = problems(&set(&["a-variant"]), &found, &whole(&found));
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
        let said = problems(&BTreeSet::new(), &empty, &whole(&empty));
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first().is_some_and(|first| first.contains("the scan is broken")),
            "{said:?}"
        );
    }

    #[test]
    fn no_line_of_test_code_blames_the_scan_rather_than_the_tree() {
        let nothing = Evidence::default();
        let said = problems(&set(&["a-variant"]), &nothing, &whole(&nothing));
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
        let nothing_removed = Scan {
            excluded: 0,
            ..whole(&reached)
        };
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
        assert_eq!(found.none_runs, 0, "the FILE is not dropped - one of its cells runs");
        // The same line in the cell that runs is evidence, so the region and not the file is what
        // moved. Without this the fixture above passes with the ignore check taken out entirely.
        let runs = "#[test]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        assert_eq!(reached(&[("crates/x/tests/t.rs", runs)]), set(&["x"]));
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
        assert_eq!(found.none_runs, 1, "the file is dropped whole");
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
        assert_eq!(found.none_runs, 1, "{found:?} - the commented-out cell does not run");
        // In the second a doc comment sits between the attributes and the item. A scan over the
        // BLANKED text alone reads that as a blank line, which ends an attribute block, and the
        // `#[ignore]` is lost.
        let documented =
            "#[test]\n#[ignore = \"x\"]\n/// What this would prove.\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        let still = scan(&[("crates/x/tests/t.rs", documented)]);
        assert!(still.reaches.is_empty(), "{:?}", still.reaches);
        assert_eq!(still.none_runs, 1, "{still:?} - the doc comment does not detach the ignore");
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
        let said = problems(&set(&["a-variant"]), &vocabulary_broke, &whole(&vocabulary_broke));
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
            unresolvable: vec![String::from("`crates/y/tests/t.rs`: 1 test-declaring attribute(s)")],
            ..Evidence::default()
        };
        let said = problems(&set(&["a-variant"]), &dangling, &whole(&dangling));
        assert!(
            said.iter()
                .any(|problem| problem.contains("could not resolve the item under `crates/y/tests/t.rs`")),
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
        // The walk REACHED both files and handed on one, so the two walk pairs must stay silent:
        // an unreadable file is not a walk that stopped, and reporting it as one would name the
        // wrong remedy twice.
        let partial = Scan {
            read: reached.visited.saturating_add(1),
            expected: reached.visited.saturating_add(1),
            unreadable: vec![String::from("`crates/y/tests/t.rs`: stream did not contain valid UTF-8")],
            ..whole(&reached)
        };
        let said = problems(&set(&["a-variant"]), &reached, &partial);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("could not read `crates/y/tests/t.rs`")),
            "{said:?}"
        );
    }

    #[test]
    fn a_reach_on_the_ignore_line_of_an_ignored_cell_is_not_evidence() {
        // MEASURED ON THIS TREE AT EXIT 0 before the region moved: `#[ignore]` is legal ABOVE
        // `#[test]`, and a region anchored on the declaring attribute leaves the line above it
        // outside - so the gate reported the reach from the very attribute that stops the test
        // running. An `#[ignore]` reason naming what the test would need is the ordinary way to
        // write one, and rustfmt does not reorder attributes.
        let above = "#[ignore = \"needs a ../../examples/x deployment\"]\n#[test]\nfn t() {}\n#[test]\nfn u() {}\n";
        let found = scan(&[("crates/x/tests/t.rs", above)]);
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        assert_eq!(found.declared, 1, "the neighbour that runs is still declared");
        // The other order too, which the old anchor happened to cover - so this cannot pass by
        // having moved the region somewhere that only handles one side.
        let below = "#[test]\n#[ignore = \"needs a ../../examples/x deployment\"]\nfn t() {}\n#[test]\nfn u() {}\n";
        assert!(scan(&[("crates/x/tests/t.rs", below)]).reaches.is_empty());
        // And the same reason text on a cell that RUNS is evidence, so what moved is the region
        // and not a rule that an attribute line can never be a reach.
        let runs = "#[test]\n#[doc = \"see ../../examples/x\"]\nfn t() {}\n";
        assert_eq!(reached(&[("crates/x/tests/t.rs", runs)]), set(&["x"]));
    }

    #[test]
    fn a_cell_behind_an_attribute_this_cannot_evaluate_is_not_evidence() {
        // BOTH SPELLINGS WERE MEASURED FAIL-OPEN on this tree, each byte-identical to the healthy
        // verdict at exit 0: `#[cfg_attr(all(), ignore)]` on both cells of the one file reaching a
        // variant, and `#[cfg(feature = "..")]` on the same two.
        for source in [
            "#[cfg_attr(all(), ignore)]\n#[test]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n",
            "#[test]\n#[cfg_attr(unix, ignore)]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n",
            "#[cfg(feature = \"off\")]\n#[test]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n",
            "#[test]\n#[cfg(not(feature = \"on\"))]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n",
        ] {
            let found = scan(&[("crates/x/tests/t.rs", source)]);
            assert!(found.reaches.is_empty(), "{source} -> {:?}", found.reaches);
            assert_eq!(found.none_runs, 1, "{source} - and the file is dropped whole");
        }
        // `#[cfg(test)]` is the one spelling that IS evaluable - every venue running a `#[test]`
        // runs it with `cfg(test)` on - so it must not take a cell out of the evidence.
        let evaluable = "#[cfg(test)]\n#[test]\nfn t() {\n    let p = \"../../examples/x/q.json\";\n}\n";
        assert_eq!(reached(&[("crates/x/tests/t.rs", evaluable)]), set(&["x"]));
    }

    #[test]
    fn a_walk_that_stopped_early_is_refused_at_both_levels() {
        // TWO PAIRS, because fixing a floor at one level does not fix the next: a gate elsewhere
        // in this tree stayed green at `204 of 204 files` with its inner walk broken. Measured
        // here before these existed - the first 38 of 253 files printed `33 file(s) of test code
        // declaring 156 test(s)`, both variants reached, exit 0.
        let found = scan(&[(
            "crates/x/tests/example.rs",
            "#[test]\nfn t() { Path::new(\"../../examples/a-variant\"); }\n",
        )]);
        let outer = Scan {
            expected: found.visited.saturating_add(220),
            ..whole(&found)
        };
        let said = problems(&set(&["a-variant"]), &found, &outer);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first().is_some_and(|first| first.contains("in-scope file(s)")),
            "{said:?}"
        );
        // The OUTER pair AGREES and the INNER one does not: the map was filled whole and the loop
        // over it stopped, which is the level the first arm cannot see.
        let inner = Scan {
            expected: found.visited.saturating_add(220),
            read: found.visited.saturating_add(220),
            ..whole(&found)
        };
        let said = problems(&set(&["a-variant"]), &found, &inner);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("the evidence walk stopped early")),
            "{said:?}"
        );
    }

    #[test]
    fn a_listing_smaller_than_the_workspace_is_refused_against_cargos_own_answer() {
        // THE PAIR FROM TWO DIFFERENT PLACES, and it exists because the other two are not one.
        // `expected` and `read` are two loops over ONE git listing, so a walk that stops inside
        // `repo::all_files` moves both together - measured on this tree, `.step_by(2)` there
        // printed `127 of 127 in-scope file(s)` at exit 0 having read half the tree. Cargo does
        // not read git, so a target root it compiles that the scan never saw is a disagreement
        // between two authorities.
        let found = scan(&[(
            "crates/x/tests/example.rs",
            "#[test]\nfn t() { Path::new(\"../../examples/a-variant\"); }\n",
        )]);
        let short = Scan {
            unseen: vec![String::from("crates/y/tests/t.rs")],
            ..whole(&found)
        };
        let said = problems(&set(&["a-variant"]), &found, &short);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("`crates/y/tests/t.rs` is a target root cargo compiles")),
            "{said:?}"
        );
        // AND THE FLOOR ON THE FLOOR: with nothing to compare, the arm above is vacuous, so an
        // empty comparison is itself the refusal rather than a silent pass.
        let nothing_compared = Scan {
            targets: 0,
            ..whole(&found)
        };
        let said = problems(&set(&["a-variant"]), &found, &nothing_compared);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("compared no workspace target root")),
            "{said:?}"
        );
    }
}
