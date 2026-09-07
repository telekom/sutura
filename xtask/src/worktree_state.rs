//! A worktree's state is its own: nothing this repository writes may land on a machine-shared path.
//!
//! `github.com/telekom/sutura#405`. Several worktrees of this repository are open at once - that is
//! what stacked branches are for - and a second session works it from another machine. **Every
//! place a test, a gate or a tier writes to a path a second checkout also reaches is a
//! cross-worktree collision, and each one produces a confident wrong verdict rather than an
//! error.**
//!
//! # The instance this was written for, reproduced rather than argued
//!
//! `sutura-conformance`'s corpus renamed its rows onto
//! `<temp_dir>/sutura-conformance/<table>.csv` - a purpose and no key - and the argument beside it
//! was *the bytes are identical either side of the rename*, which is true per TREE and not per
//! machine.
//!
//! Measured 2026-09-07 with two worktrees of this repository, each running its own `corpus::
//! on_disk`, one row differing by one cent: the `DuckDB` binding failed
//! `total-by-region-and-day` and `total-by-region-and-day-as-a-leg` as CONTENT faults naming this
//! repository's own cases, while the run that overwrote the file was green. The window is wide
//! because `attach_csv` makes a VIEW over `read_csv_auto`, so the file is read at QUERY time; the
//! Postgres binding reads it seven times per binding at LOAD time.
//!
//! # What holds it, and what does not
//!
//! The DEFAULT answer is that state lives under the worktree, where the tree is the key and there
//! is nothing to derive - `sutura_dev::scope::Scope::state_dir`, and the corpus now. The EXCEPTION
//! is a writer that needs a short path, because a unix socket caps around 100 bytes: that one takes
//! the machine-shared root and keys it with `Scope::scratch`, which is the one derivation of such a
//! path.
//!
//! This gate is the rule over both. It reads every acquisition of a shared root in first-party
//! Rust and in the shell scripts that run on a developer's own machine, and refuses one that
//! nothing keys. `super::scan` carries the lexing argument and the two classes.
//!
//! # Limits, stated next to the claim
//!
//! * **`.nix` files are out of scope, and that is the largest gap.** `$TMPDIR` inside a nix
//!   DERIVATION is the build directory - private per build - while `$TMPDIR` inside a
//!   `writeShellApplication` a developer runs is the machine's. Nothing in the text of a `.nix`
//!   file distinguishes those two, and both are in `flake.nix` today: two derivation uses, and two
//!   tier scripts. Measured on the tree this gate landed on, both tiers are already keyed - the
//!   Postgres tier's data directory, socket directory and endpoint file are all per-worktree, and
//!   it listens on no TCP port at all - so `telekom/sutura#405`'s instance 5 is not reproducible as
//!   stated. What is unheld is a THIRD tier inventing its own key.
//! * **A log path an agent chooses is outside every gate.** `telekom/sutura#405`'s instance 2 is a
//!   `shipcheck.log` in a shared scratchpad, which is not a file in this repository. What this gate
//!   reaches is the shell that IS: `nix/*.sh`, where such a path would be written if it were
//!   committed. That scope holds nothing today - measured, zero takings - which is what a refusal
//!   over a shape nobody writes should do.
//! * **It reads text, not a program.** A shared root reached through a helper, an alias, or a
//!   binding two hops away reads as unkeyed, which is the safe direction; a MUTATION laundered into
//!   a helper makes a literal read as `Unwritten`, which is not.
//! * **`Scope::scratch` is a NAME, not an allocation.** Two worktrees whose canonical paths collide
//!   in four bytes of SHA-256 get one directory. That is a startup error somebody reads rather than
//!   a test that passes against the wrong fixture, and it is the same asymmetry
//!   `dev/src/scope.rs`'s header argues for naming over ports.

mod scan;

use scan::{Keyed, Language, Taking};

use crate::{Verdict, repo};

/// Evidence that every taking this scan FOUND was also adjudicated, over every file in scope.
///
/// **A count in a message is not a witness**, which is the shape `telekom/sutura#405` names as
/// property 3 and which this repository has measured going wrong five times. So the numbers the
/// verdict prints come out of a value whose fields are private to this module and whose only
/// constructor refuses a mismatch: `inspected == discovered` is a precondition of the type
/// existing, not a sentence beside it.
///
/// # TWO conservation laws, at two levels, and the second is the one that is easy to omit
///
/// **Per taking**, `scan::offered` counts occurrences of a root spelling and knows nothing about
/// narrowing, statements or adjudication, while `scan::takings` is the loop. So a `.take(n)` inside
/// the loop moves one side and not the other.
///
/// **Per file**, and this is the level a single law cannot see. A floor computed off the same
/// traversal the loop uses moves WITH the loop: measured on a sibling gate the night this landed, a
/// guard of `lines > files * 4` gave a floor of 624 against a control of 115088, so `.take(100)`
/// dropped 99.46% of the walk at exit 0 with 1030 of 1030 tests green. So the offered count is
/// taken over the CALLER's whole list by its own expression, before the in-scope vector is built at
/// all, and the read list is collected inside the loop. The repository's own words:
/// *"counting both off one filter is how a walk narrowed to one directory passed at exit 0 with a
/// real defect outside it."*
///
/// # And an ANCHOR, because neither law can see the predicate shrink
///
/// Both counts come off `scan::language_of`. If that predicate stops matching most of the tree the
/// two numbers agree, the remainder still holds takings, every floor is satisfied and the verdict
/// is over a subset with nothing saying so. `scan::MUST_READ` is the answer: named subjects the
/// walk has to have read, one per arm of the predicate, so the refusal does not depend on anybody
/// reading a number. **That is the point of the anchor - not a better floor, but the count ceasing
/// to be the control.**
///
/// The pattern is `crate::causality::isolation::Isolated` and `crate::conformance::Reconciled`, for
/// the same reason: an invariant a caller has to remember is one the compiler is not holding.
#[derive(Debug)]
struct Inspected {
    /// Files the caller's own list offered, counted before the loop's list existed.
    offered_files: usize,
    /// Files the loop actually read.
    read_files: usize,
    /// What the independent per-taking count offered.
    discovered: usize,
    /// Anchors the walk did not read. See [`Inspected::of`] for why this is carried rather than
    /// refused, and [`decide`] for what reports it.
    missed: Vec<&'static str>,
    /// One answer per taking, in file and line order.
    adjudicated: Vec<(Taking, Keyed)>,
}

impl Inspected {
    /// The only constructor. Refuses unless the walk reached every file and every taking has
    /// exactly one answer.
    fn of(offered_files: usize, read: &[String], discovered: usize, adjudicated: Vec<(Taking, Keyed)>) -> Result<Self, String> {
        let read_files = read.len();
        if offered_files != read_files {
            return Err(format!(
                "{offered_files} file(s) are in this gate's scope and the walk read {read_files}. \
                 A narrowed walk is a verdict about a tree the message names and the scan never \
                 reached"
            ));
        }
        if discovered != adjudicated.len() {
            return Err(format!(
                "the scan offered {discovered} taking(s) of a machine-shared root and adjudicated \
                 {}. A verdict over a subset is the defect this witness exists to make \
                 unrepresentable",
                adjudicated.len()
            ));
        }
        Ok(Self {
            offered_files,
            read_files,
            // AN ANCHOR, and it is the arm neither count can reach: both numbers come off
            // `scan::language_of`, so a predicate that stopped matching leaves them agreeing over
            // a subset. `telekom/sutura#414`'s reframing - the count stops being the control.
            //
            // COMPUTED HERE AND REPORTED BY `decide`, rather than refused here, and the ordering is
            // the whole reason: over `crate::falsifier`'s tree no anchor can exist, so refusing in
            // this constructor made the gate's refusal there come from a MISSING INPUT instead of
            // from its own rule - measured, and `telekom/sutura#405`'s property 4 is exactly that
            // distinction. A violation found is a violation whatever else was missed; only a CLEAN
            // scan needs its coverage attested.
            missed: scan::MUST_READ
                .iter()
                .filter(|anchor| !read.iter().any(|rel| rel == *anchor))
                .copied()
                .collect(),
            discovered,
            adjudicated,
        })
    }

    /// How many takings were offered.
    const fn discovered(&self) -> usize {
        self.discovered
    }

    /// How many files were offered, and how many the walk read - equal by construction, printed
    /// as a pair for the same reason the taking counts are.
    const fn files(&self) -> (usize, usize) {
        (self.read_files, self.offered_files)
    }

    /// How many were adjudicated. Equal to [`Inspected::discovered`] by construction, and printed
    /// beside it anyway: a reader of a verdict should be able to see the conservation rather than
    /// take it on trust.
    const fn inspected(&self) -> usize {
        self.adjudicated.len()
    }

    /// Anchors this gate's scope must reach and the walk did not.
    fn missed(&self) -> &[&'static str] {
        &self.missed
    }

    /// Every taking nothing keys.
    fn shared(&self) -> Vec<&Taking> {
        self.adjudicated
            .iter()
            .filter(|(_, keyed)| keyed.is_shared())
            .map(|(taking, _)| taking)
            .collect()
    }

    /// How many takings each holder accounts for, in the order the verdict prints them.
    fn by_holder(&self) -> Vec<(&'static str, usize)> {
        let mut counted: Vec<(&'static str, usize)> = Vec::new();
        for (_, keyed) in &self.adjudicated {
            let holder = keyed.holder();
            match counted.iter_mut().find(|(name, _)| *name == holder) {
                Some((_, count)) => *count = count.saturating_add(1),
                None => counted.push((holder, 1)),
            }
        }
        counted.sort_unstable_by(|left, right| left.0.cmp(right.0));
        counted
    }
}

/// What a finished inspection means.
///
/// **A third exhaustive match, and it exists because the ORDER of two refusals was a defect.** The
/// anchor was a constructor refusal first, which made the gate's verdict over
/// `crate::falsifier`'s tree - where no anchor can exist - come from a missing input rather than
/// from its own rule. `telekom/sutura#405`'s property 4 is precisely that distinction, and only
/// three of the gates in the sweep manage the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// The scan found a path a second worktree also reaches. Reported FIRST, whatever else the
    /// walk missed: a violation found is a violation.
    Violations,
    /// Nothing shared, and the walk did not read a subject this gate's scope must cover - so the
    /// clean bill is over a tree the verdict names and the scan never reached.
    MissedAnchors,
    /// Nothing shared, every anchor read.
    Clean,
}

/// The decision, from the witness alone.
///
/// A function of the witness rather than a chain of `if`s inside `run`, so the ORDER is a thing a
/// test can read - `a_violation_is_reported_even_when_the_walk_missed_an_anchor` is that test.
fn decide(inspected: &Inspected) -> Decision {
    if !inspected.shared().is_empty() {
        return Decision::Violations;
    }
    if inspected.missed().is_empty() {
        Decision::Clean
    } else {
        Decision::MissedAnchors
    }
}

/// The gate.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-worktree-state: could not determine the repo root");
        return Verdict::Fail;
    };

    // THE FILE FLOOR IS COUNTED OVER THE CALLER'S OWN LIST, before the loop's list exists - see
    // `Inspected`'s header for the measurement that makes this a separate expression rather than
    // `in_scope.len()`.
    let offered_files = files.iter().filter(|rel| scan::language_of(rel).is_some()).count();

    let in_scope: Vec<(String, Language)> = files
        .iter()
        .filter_map(|rel| scan::language_of(rel).map(|language| (rel.clone(), language)))
        .collect();

    // FAIL CLOSED ON AN EMPTY SCOPE. This gate's subject is first-party Rust and the shell that
    // gates it; a tree holding neither is not a clean tree, it is a tree this gate has not read.
    if in_scope.is_empty() {
        eprintln!("xtask check-worktree-state: FAILED - no file in this gate's scope");
        eprintln!("  It reads `.rs` under crates/, xtask/ and dev/, and `.sh` under nix/.");
        eprintln!("  A scan that opened none of those attests nothing. Check the workspace root.");
        return Verdict::Fail;
    }

    let mut offered = 0_usize;
    let mut read: Vec<String> = Vec::new();
    let mut adjudicated: Vec<(Taking, Keyed)> = Vec::new();
    let mut languages: Vec<(&'static str, usize)> = Vec::new();
    for (rel, language) in &in_scope {
        // FAIL CLOSED ON ONE UNREADABLE FILE, not only on every file being unreadable.
        // `sutura/gates` records that exact difference three times: *cannot say which* closed and
        // *cannot look at all* left open, with the floor satisfied by the files that did read.
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            eprintln!("xtask check-worktree-state: FAILED - could not read {rel}");
            eprintln!("  A file in scope that this gate cannot open is a verdict over a subset.");
            return Verdict::Fail;
        };
        read.push(rel.clone());
        offered = offered.saturating_add(scan::offered(*language, &text));
        adjudicated.extend(scan::takings(rel, *language, &text));
        let label = language.label();
        match languages.iter_mut().find(|(name, _)| *name == label) {
            Some((_, count)) => *count = count.saturating_add(1),
            None => languages.push((label, 1)),
        }
    }

    // FAIL CLOSED ON AN EMPTY SCAN. Measured on the tree this landed on: 36 root takings and 19
    // rooted literals. A run that found none read something other than this repository, and
    // `ok - 0 taking(s)` is a sentence about a tree it never saw.
    if offered == 0 {
        eprintln!(
            "xtask check-worktree-state: FAILED - no taking of a machine-shared root in {} file(s)",
            in_scope.len()
        );
        eprintln!("  This workspace has dozens. A scan that found none is a broken scan, not a");
        eprintln!("  clean tree - the rule would then be checking nothing at all.");
        return Verdict::Fail;
    }

    let inspected = match Inspected::of(offered_files, &read, offered, adjudicated) {
        Ok(witness) => witness,
        Err(why) => {
            eprintln!("xtask check-worktree-state: FAILED - {why}");
            return Verdict::Fail;
        }
    };

    let holders: Vec<String> = inspected
        .by_holder()
        .into_iter()
        .map(|(holder, count)| format!("{count} {holder}"))
        .collect();
    let scanned: Vec<String> = languages
        .into_iter()
        .map(|(label, count)| format!("{count} {label}"))
        .collect();

    // ONE EXHAUSTIVE MATCH over `Decision`, so a fourth outcome cannot be answered by an `else`.
    match decide(&inspected) {
        Decision::Clean => {
            let (read, offered_files) = inspected.files();
            println!(
                "xtask check-worktree-state: ok - inspected {} of {} taking(s) ({}) over {read} of \
                 {offered_files} file(s) ({})",
                inspected.inspected(),
                inspected.discovered(),
                holders.join(", "),
                scanned.join(", ")
            );
            Verdict::Pass
        }
        Decision::MissedAnchors => {
            eprintln!(
                "xtask check-worktree-state: FAILED - the walk did not read {} anchored \
                 subject(s), so a clean bill here is over a tree it never reached",
                inspected.missed().len()
            );
            for anchor in inspected.missed() {
                eprintln!("  {anchor}");
            }
            eprintln!();
            eprintln!("An ANCHOR rather than a count, because both counts above come off one scope");
            eprintln!("predicate: a predicate that stops matching leaves them agreeing over a subset");
            eprintln!("with every floor satisfied. `scan::MUST_READ` names one subject per arm, so an");
            eprintln!("unreachable subject refuses whether or not anybody reads a number.");
            Verdict::Fail
        }
        Decision::Violations => {
            let shared = inspected.shared();
            eprintln!(
                "xtask check-worktree-state: FAILED - {} of {} taking(s) reach a machine-shared path",
                shared.len(),
                inspected.discovered()
            );
            for taking in &shared {
                let narrowed = if taking.segment.trim().is_empty() {
                    String::from("nothing narrows it")
                } else {
                    format!("narrowed to `{}`", taking.segment.trim())
                };
                eprintln!("  {}:{}: takes `{}`, {narrowed}", taking.path, taking.line, taking.root);
            }
            explain();
            Verdict::Fail
        }
    }
}

/// What to do about it. Printed, because a gate that only says no gets worked around.
fn explain() {
    eprintln!();
    eprintln!("Several worktrees of this repository are open at once, and a second session works it");
    eprintln!("from another machine. A path with no key in it is one path for all of them, and the");
    eprintln!("failure is a confident wrong verdict rather than an error: reproduced 2026-09-07,");
    eprintln!("two worktrees writing one corpus file, and the `DuckDB` conformance binding failed");
    eprintln!("two cases as content faults while the run that overwrote the file was green.");
    eprintln!();
    eprintln!("Three ways out, in order of preference:");
    eprintln!("  * put it under the worktree. `sutura_dev::scope::Scope::state_dir` is that answer,");
    eprintln!("    and it needs no key at all - the tree IS the key. Most state belongs here.");
    eprintln!("  * key it with the worktree. `Scope::scratch(<purpose>)` is the ONE derivation of a");
    eprintln!("    path under a machine-shared root, and it exists for one reason: a unix socket");
    eprintln!("    caps around 100 bytes, so a server cannot sit under a deep worktree.");
    eprintln!("  * key it with the process, `std::process::id()`, for a scratch directory no second");
    eprintln!("    run needs to find. Put the key in the SAME statement as the taking: a `join` ten");
    eprintln!("    lines away is a taking this gate cannot attribute, and it says so rather than");
    eprintln!("    guessing.");
    eprintln!();
    eprintln!("  `github.com/telekom/sutura#405` carries the six measured collisions.");
}

#[cfg(test)]
mod tests {
    use super::{Inspected, Keyed, Taking, scan};

    /// Every anchor, as the read list a satisfied walk would hand the constructor.
    fn anchors() -> Vec<String> {
        scan::MUST_READ.iter().map(|rel| String::from(*rel)).collect()
    }

    /// A taking, as a value, so the witness can be tested without a tree.
    fn taking(line: usize) -> Taking {
        Taking {
            path: String::from("crates/x/src/lib.rs"),
            line,
            root: String::from("a root"),
            segment: String::from("a segment"),
        }
    }

    #[test]
    fn the_witness_refuses_a_verdict_over_a_subset() {
        // PROPERTY 3 OF `telekom/sutura#405`, held by the constructor rather than by the loop: a
        // count in a message is not a witness, and this repository has measured four gates
        // printing a number over a scan that reached less. `Inspected` cannot exist unless every
        // offered taking has exactly one answer, so `inspected N of N` is a precondition of the
        // type rather than a sentence beside it.
        let one = vec![(taking(1), Keyed::Process)];
        assert!(
            Inspected::of(1, &anchors(), 2, one).is_err(),
            "a subset must not mint a witness"
        );
    }

    #[test]
    fn the_witness_refuses_a_walk_that_read_fewer_files_than_the_scope_offered() {
        // THE SECOND LAW, at the level a single one cannot see. Measured on a sibling gate: a floor
        // computed off the loop's own traversal moved WITH a `.take(100)` and 99.46% of the walk
        // was dropped at exit 0. Both takings here are adjudicated, so the per-taking law is
        // satisfied and only this one can refuse.
        let one = vec![(taking(1), Keyed::Process)];
        assert!(
            Inspected::of(156, &anchors(), 1, one).is_err(),
            "a narrowed walk must not mint a witness"
        );
    }

    #[test]
    fn the_witness_prints_the_numbers_its_scan_reached() {
        let two = vec![(taking(1), Keyed::Process), (taking(9), Keyed::Worktree)];
        let witness = Inspected::of(anchors().len(), &anchors(), 2, two).expect("two offered, two adjudicated");
        assert_eq!(witness.discovered(), 2);
        assert_eq!(witness.inspected(), 2);
        assert_eq!(witness.files(), (anchors().len(), anchors().len()));
        assert!(witness.shared().is_empty());
        assert_eq!(witness.by_holder(), vec![("process", 1), ("worktree", 1)]);
    }

    #[test]
    fn a_shared_taking_is_reported_and_the_rest_are_counted() {
        let mixed = vec![
            (taking(1), Keyed::Shared),
            (taking(4), Keyed::Unwritten),
            (taking(7), Keyed::Shared),
        ];
        let witness = Inspected::of(anchors().len(), &anchors(), 3, mixed).expect("three offered, three adjudicated");
        let shared = witness.shared();
        assert_eq!(shared.len(), 2);
        assert_eq!(shared.iter().map(|t| t.line).collect::<Vec<usize>>(), vec![1, 7]);
        assert_eq!(witness.by_holder(), vec![("nothing", 2), ("unwritten", 1)]);
    }

    #[test]
    fn a_walk_that_missed_an_anchor_is_not_a_clean_bill() {
        // THE ARM NEITHER COUNT CAN REACH. Both numbers agree here and the taking is adjudicated,
        // so every conservation law is satisfied - what is wrong is that the walk never read a
        // subject this gate's scope must cover, which is what a predicate that stopped matching
        // looks like from the inside. `telekom/sutura#414`'s reframing: the point is that the count
        // stops being the control, so an unreachable subject refuses whether or not anybody reads a
        // number.
        let short: Vec<String> = anchors().into_iter().skip(1).collect();
        let one = vec![(taking(1), Keyed::Process)];
        let witness = Inspected::of(short.len(), &short, 1, one).expect("the counts agree");
        assert_eq!(witness.missed(), [scan::MUST_READ[0]]);
        assert_eq!(super::decide(&witness), super::Decision::MissedAnchors);
    }

    #[test]
    fn a_violation_is_reported_even_when_the_walk_missed_an_anchor() {
        // THE ORDER, AND IT WAS A DEFECT BEFORE IT WAS A TEST. The anchor started as a refusal
        // inside `Inspected::of`, which made the gate's verdict over `crate::falsifier`'s tree -
        // where no anchor can exist - come from a MISSING INPUT rather than from its own rule.
        // Measured: `FAILED - the walk did not read crates/sutura-conformance/src/corpus.rs` where
        // the honest answer was `1 of 1 taking(s) reach a machine-shared path`, naming the seeded
        // file and its line. `telekom/sutura#405`'s property 4 is exactly that distinction.
        let short: Vec<String> = anchors().into_iter().skip(1).collect();
        let shared = vec![(taking(2), Keyed::Shared)];
        let witness = Inspected::of(short.len(), &short, 1, shared).expect("the counts agree");
        assert!(!witness.missed().is_empty(), "the fixture must also miss an anchor");
        assert_eq!(super::decide(&witness), super::Decision::Violations);
    }

    #[test]
    fn a_clean_scan_that_read_every_anchor_is_the_only_pass() {
        let one = vec![(taking(1), Keyed::Process)];
        let witness = Inspected::of(anchors().len(), &anchors(), 1, one).expect("the counts agree");
        assert!(witness.missed().is_empty());
        assert_eq!(super::decide(&witness), super::Decision::Clean);
    }

    #[test]
    fn the_falsifier_tree_violates_this_gate_s_own_rule() {
        // PROPERTY 4 OF `telekom/sutura#405`, AND THE HALF THE FALSIFIER TEST CANNOT SEE. That test
        // reads an EXIT CODE, so it cannot tell a gate refusing because it found a violation from
        // one refusing because its input was missing - and over that tree only three of the
        // thirty-three manage the first. Delete `nix/shared-scratch.sh` from
        // `crate::falsifier::falsifier_tree` and this gate still exits 1, on the empty-scope arm,
        // with the falsifier test green: measured, and that is the mutation this cell reddens.
        //
        // It reads the seed through the constructor rather than naming the path a second time, so a
        // renamed seed is still this cell's subject. No `set_current_dir` - that is process-global,
        // and the falsifier test's own header records eleven siblings breaking on it.
        let tree = crate::falsifier::falsifier_tree();
        let mut found = Vec::new();
        let mut in_scope = 0_usize;
        for entry in walk(&tree) {
            let Some(rel) = entry.strip_prefix(&tree).ok().and_then(|p| p.to_str()) else {
                continue;
            };
            let Some(language) = scan::language_of(rel) else {
                continue;
            };
            in_scope = in_scope.saturating_add(1);
            let text = std::fs::read_to_string(&entry).expect("the seed is readable");
            found.extend(scan::takings(rel, language, &text));
        }
        drop(std::fs::remove_dir_all(&tree));

        assert!(in_scope > 0, "the falsifier tree holds no file in this gate's scope");
        assert!(
            found.iter().any(|(_, keyed)| keyed.is_shared()),
            "the falsifier tree carries no violation of this gate's own rule, so its refusal there \
             would come from a missing input: {found:?}"
        );
    }

    /// Every file under `dir`, recursively. Small on purpose: the falsifier tree is four files.
    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
        out
    }

    #[test]
    fn every_answer_the_scan_can_give_is_accounted_for_by_the_verdict() {
        // The other half of property 5. `Keyed::is_shared` and `Keyed::holder` are exhaustive
        // matches, so a fifth answer does not compile - but a variant nothing ever CLASSIFIES
        // would still be a hole, so this walks the whole set and asserts each one lands somewhere
        // a reader sees: either in the violation list or in the holder breakdown.
        for keyed in [Keyed::Worktree, Keyed::Process, Keyed::Unwritten, Keyed::Shared] {
            let witness = Inspected::of(anchors().len(), &anchors(), 1, vec![(taking(1), keyed)]).expect("one and one");
            assert_eq!(
                witness.shared().len(),
                usize::from(keyed.is_shared()),
                "{keyed:?} is not reported the way it classifies"
            );
            assert_eq!(witness.by_holder(), vec![(keyed.holder(), 1)], "{keyed:?}");
            assert!(!keyed.holder().is_empty(), "{keyed:?} has no word in the verdict");
        }
    }
}
