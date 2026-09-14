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
//! * **Only `nix/*-tier.nix` is in scope, and every other `.nix` file is out.** `$TMPDIR` inside a
//!   nix DERIVATION is the build directory - private per build - while `$TMPDIR` inside a
//!   `writeShellApplication` a developer runs is the machine's, and nothing in the TEXT of a
//!   `.nix` file tells those apart. What tells them apart is the file's role, and the tier glob is
//!   that role: `crate::compose::file` refuses a `nix native` venue that does not name an existing
//!   `nix/<service>-tier.nix`, and `every_nix_tier_module_is_provisioned_by_a_nix_check` holds the
//!   mirror. **The residue, stated rather than implied:** that rule runs over the services with a
//!   compose block, and Postgres - the one tier that provisions a listening server - has no such
//!   block and is skipped by its own loop. So a THIRD tier is held if it is named like the two
//!   that exist, and a tier for a blockless service is held by the habit, not by the gate.
//! * **The worktree key HAD two spellings and nothing compared them.** `Scope` derives four bytes
//!   of SHA-256 over the canonical root; both tiers derived `printf '%s' "$root" | cksum`, a CRC-32
//!   over the same input. Both isolated, so nothing was shared - but they were different values, so
//!   no Rust writer could find a tier's directory and no gate noticed when one side moved. One
//!   spelling now: `nix/keycloak-tier.nix` derives no key at all - its home is under the worktree,
//!   where the tree IS the key (`telekom/sutura#528`) - and `nix/postgres-tier.nix`, which needs a
//!   short path for a unix socket, spells `Scope::scratch("pg")`. Held by
//!   `the_tier_and_the_rust_scope_derive_one_worktree_key` below, which RUNS the tier's own two
//!   lines rather than comparing their text. **Its limits, next to the claim:** it reads the
//!   dev-shell arm of ONE tier, so a second tier that takes a shared root is held by nothing here;
//!   and it needs `bash` and `sha256sum`, which every venue that runs this suite has and a bare
//!   host may not. `telekom/sutura#405`'s instance 5 is still not reproducible as stated - both
//!   tiers are keyed and Postgres listens on no TCP port at all - and that dismissal no longer
//!   rests on two unreconciled lines.
//! * **A log path an agent chooses is outside every gate.** `telekom/sutura#405`'s instance 2 is a
//!   `shipcheck.log` in a shared scratchpad, which is not a file in this repository. What this gate
//!   reaches is the shell that IS: `nix/*.sh`, where such a path would be written if it were
//!   committed. That scope holds nothing today - measured, zero takings - which is what a refusal
//!   over a shape nobody writes should do.
//! * **The git half of #405 is one instance, and it reaches less than the filesystem half.**
//!   `refs/stash` is one ref per REPOSITORY - a linked worktree does not get its own - so a file in
//!   this scope may not INSTRUCT an operation on it, and one doc comment did. That is all a
//!   repository can hold: git offers no hook on a stash, so the collision measured 2026-09-13, a
//!   pop taking a sibling worktree's entry, came from a command typed at a prompt and no gate here
//!   reaches it. [`global_ref`] carries the argument and prints no count, because the scope,
//!   census pass and anchor set are the ones above rather than a second opinion.
//! * **It reads text, not a program.** A shared root reached through a helper, an alias, or a
//!   binding two hops away reads as unkeyed, which is the safe direction; a MUTATION laundered into
//!   a helper makes a literal read as `Unwritten`, which is not.
//! * **`Scope::scratch` is a NAME, not an allocation.** Two worktrees whose canonical paths collide
//!   in four bytes of SHA-256 get one directory. That is a startup error somebody reads rather than
//!   a test that passes against the wrong fixture, and it is the same asymmetry
//!   `dev/src/scope.rs`'s header argues for naming over ports.

mod global_ref;
mod scan;

use scan::{Keyed, Taking};

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
/// # A THIRD law, `github.com/telekom/sutura#689`, at the level the other two cannot see either
///
/// `crate::repo::census::Census::inspect` holds *"did the walk offer this file at all"* - but the
/// closure it hands the bytes to is still this module's own code, and this module still keeps a
/// `Vec<String>` of the files it read, pushed to on the closure's happy path. Nothing compared that
/// count to the census's own: an early `return` inside the closure ahead of the push - a redundant
/// re-check of `scope`, say, or any future line above it - drops the file from `read` while
/// `Census::inspect` still counts it `judged`, because that count is incremented AFTER the closure
/// returns and does not look inside it. Measured: `ok - inspected 117 of 117 taking(s) ... over
/// 552 file(s)` beside `discovery: 553 of 1449 subject(s) judged` - the two adjacent lines already
/// disagree, and nothing read them together. [`Inspected::of`] now does: `read.len() ==
/// census_judged` is a precondition, comparing a count the CLOSURE keeps against a count
/// `Census::inspect` keeps independently of the closure, taken off different sources the way
/// `sutura/gates` asks for.
#[derive(Debug)]
struct Inspected {
    /// Files the loop read. Kept for the anchor check and the verdict line; the file-level
    /// CONSERVATION is `crate::repo::census`'s now - see [`Inspected::of`].
    read_files: usize,
    /// What the independent per-taking count offered.
    discovered: usize,
    /// Newlines the files offered, and newlines the lexer reached - see `scan::covered`.
    lines: (usize, usize),
    /// Anchors the walk did not read. See [`Inspected::of`] for why this is carried rather than
    /// refused, and [`decide`] for what reports it.
    missed: Vec<&'static str>,
    /// One answer per taking, in file and line order.
    adjudicated: Vec<(Taking, Keyed)>,
    /// Lines instructing an operation on a repository-GLOBAL git ref, as `(file, line)`. No count
    /// is derived from this and none is printed on a clean run - see [`global_ref`].
    instructed: Vec<(String, usize)>,
}

impl Inspected {
    /// The only constructor. Refuses unless the lexer reached every line and every taking has
    /// exactly one answer.
    ///
    /// **The per-FILE law left this type in #419's merge, and it went somewhere stronger.** It
    /// compared two numbers this gate derived itself, so it could only see a loop this gate
    /// narrowed - never a walk that failed to OFFER a file, which is the 86-files-vanish finding
    /// this PR published as a stated limit. `crate::repo::census::Census` holds that level now:
    /// the loop lives inside `inspect`, so `.take(n)` has nowhere to be written, and an
    /// unreachable subject from the walk itself is a refusal rather than a silent `continue`.
    /// Restating it here would be a second derivation of a weaker claim.
    ///
    /// **`census_judged` adds the law that level did not reach, `github.com/telekom/sutura#689`.**
    /// `read` is this module's OWN tally, pushed to inside the closure `Census::inspect` calls;
    /// `census_judged` is `Census::inspect`'s own count, incremented after the closure returns
    /// and blind to what happened inside it. An early `return` ahead of the push moves the first
    /// and not the second - two numbers from two sources, which is what makes the comparison
    /// worth having rather than a tautology restating one loop's own report of itself.
    fn of(
        read: &[String],
        census_judged: usize,
        lines: (usize, usize),
        discovered: usize,
        adjudicated: Vec<(Taking, Keyed)>,
        instructed: Vec<(String, usize)>,
        instructed_offered: usize,
    ) -> Result<Self, String> {
        let read_files = read.len();
        if read_files != census_judged {
            return Err(format!(
                "this walk recorded {read_files} file(s) of its own and the census independently \
                 judged {census_judged}. One of the two counts moved without the other - a closure \
                 that returns before recording a file drops it from this walk's own tally, or the \
                 census's own count moved independently of the closure - and either way the two no \
                 longer agree"
            ));
        }
        // THE LINE LAW, over how much of each file the lexer reached. `telekom/sutura#414`
        // measured a truncated extraction leaving the file law and every anchor satisfied,
        // because both take their numbers from the same lexer and an anchor asserts a file was
        // OPENED rather than read in full. The census cannot see this either: a file judged to
        // hold no taking is judged, whatever fraction of it was lexed.
        if lines.0 != lines.1 {
            return Err(format!(
                "the files in scope hold {} line(s) and the lexer reached {}. A verdict over part \
                 of a file is the same subset defect one level down from a narrowed walk",
                lines.0, lines.1
            ));
        }
        // THE SAME LAW FOR THE GIT ARM, and it is the only thing standing where a printed count
        // would. That arm prints no number, so a deleted `extend` in `run` would leave it reaching
        // nothing with every other number in the verdict still agreeing with itself.
        // `global_ref::offered` counts by its own expression over the whole text; this refuses the
        // disagreement. It catches a narrowed LOOP and not a wrong needle - see that function.
        if instructed_offered != instructed.len() {
            return Err(format!(
                "the scan offered {instructed_offered} instruction(s) on a repository-global git \
                 ref and carried {}. A rule that prints no count has this law instead",
                instructed.len()
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
            read_files,
            lines,
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
            instructed,
        })
    }

    /// How many takings were offered.
    const fn discovered(&self) -> usize {
        self.discovered
    }

    /// How many files this gate judged. The file-level conservation is the census's, so this is
    /// the read count alone rather than a pair: printing `N of N` here would restate a law this
    /// type no longer holds.
    const fn files(&self) -> usize {
        self.read_files
    }

    /// How many were adjudicated. Equal to [`Inspected::discovered`] by construction, and printed
    /// beside it anyway: a reader of a verdict should be able to see the conservation rather than
    /// take it on trust.
    const fn inspected(&self) -> usize {
        self.adjudicated.len()
    }

    /// Newlines offered and newlines reached - equal by construction, printed as a pair.
    const fn lines(&self) -> (usize, usize) {
        self.lines
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

    /// Every line instructing an operation on a ref the whole repository shares.
    fn instructed(&self) -> &[(String, usize)] {
        &self.instructed
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
    /// A file in scope instructs an operation on a repository-GLOBAL git ref. Ranked AHEAD of a
    /// missed anchor for the same reason `Violations` is - this is the gate's own rule rather than
    /// a missing input, which is `telekom/sutura#405`'s property 4 - and behind it only because
    /// the shared filesystem path is this gate's primary subject.
    Instructions,
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
    if !inspected.instructed().is_empty() {
        return Decision::Instructions;
    }
    if inspected.missed().is_empty() {
        Decision::Clean
    } else {
        Decision::MissedAnchors
    }
}

/// The files this gate's rule is about: the ones whose language its lexer knows.
///
/// A [`repo::Scope`], so it is a bare `fn` with nothing captured - it cannot count subjects and it
/// is not handed the content, and `language_of` is a pure function of the path either way. On what
/// a `Scope` is still free to do, see [`repo::Scope`]'s own limit.
fn in_a_known_language(rel: &str) -> bool {
    scan::language_of(rel).is_some()
}

/// The gate.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(refusal) => {
            eprintln!("xtask check-worktree-state: FAILED - {}", refusal.describe());
            return Verdict::Fail;
        }
    };
    let mut offered = 0_usize;
    let mut read: Vec<String> = Vec::new();
    let mut lines = (0_usize, 0_usize);
    let mut adjudicated: Vec<(Taking, Keyed)> = Vec::new();
    let mut instructed: Vec<(String, usize)> = Vec::new();
    let mut instructed_offered = 0_usize;
    let mut languages: Vec<(&'static str, usize)> = Vec::new();

    // **`must_judge` IS EMPTY HERE, AND THAT IS THE ONE REVIEWABLE CHOICE IN THIS MIGRATION.**
    // `crate::repo::census` refuses a missed anchor INSIDE `inspect`, ahead of the caller's rule.
    // This gate cannot take that order: `dd5a7348` fixed a defect of exactly that shape, where an
    // anchor refusal sitting ahead of the rule made the verdict over `crate::falsifier`'s tree -
    // where no anchor can exist - come from a MISSING INPUT rather than from this gate's own rule,
    // which is precisely the distinction `telekom/sutura#405`'s property 4 asks for and which only
    // three of the registered gates manage. So the SAME anchor set, `scan::MUST_READ`, is still
    // checked - carried by `Inspected` and reported by `decide` AFTER a violation. Same subjects,
    // same strictness, and the precedence the issue requires. **That is unchanged by the census
    // performing the read**: the read moved, the order of the two rules did not.
    //
    // What DID move, and it is the direction this gate wanted: the unreadable-file arm used to be
    // a `repo::Looked::Unreachable` this closure spelled, which meant a mis-labelled `OutOfScope`
    // was the one way to hide it. **Qualified deliberately, because `#438` DELETED that type when
    // the census took over the read** - and an unrelated private `Looked`, with an `Unreachable`
    // variant of its own, survives in `examples/corpus.rs`. So an unqualified name here does not
    // dangle where a reader would notice; it resolves to machinery this gate never used and reads
    // as live. The read is the census's now, so there is no arm to mis-label - the
    // refusal is `Refusal::Unreachable` from inside `inspect`, and it still fails closed on ONE
    // unreadable file rather than on every file being unreadable, which is the difference
    // `sutura/gates` records three times.
    let scope: repo::Scope = in_a_known_language;
    let counted = census.inspect(&[], scope, |rel, bytes| {
        // Re-derived rather than carried: `Scope` is a bare `fn` pointer, so it cannot hand a
        // value forward, and `language_of` is a pure function of the path.
        let Some(language) = scan::language_of(rel) else {
            return;
        };
        // Lossy rather than `read_to_string`, which turned a file that is not valid UTF-8 into an
        // `Unreachable` - a file that WAS reached. A subject the census opened is judged.
        let text = String::from_utf8_lossy(bytes);
        read.push(String::from(rel));
        let (raw, lexed) = scan::covered(language, &text);
        lines = (lines.0.saturating_add(raw), lines.1.saturating_add(lexed));
        offered = offered.saturating_add(scan::offered(language, &text));
        adjudicated.extend(scan::takings(rel, language, &text));
        instructed_offered = instructed_offered.saturating_add(global_ref::offered(&text));
        instructed.extend(
            global_ref::instructed_lines(&text)
                .into_iter()
                .map(|line| (String::from(rel), line)),
        );
        let label = language.label();
        match languages.iter_mut().find(|(name, _)| *name == label) {
            Some((_, count)) => *count = count.saturating_add(1),
            None => languages.push((label, 1)),
        }
    });
    // The file half of the witness is the census's now, and it is STRICTLY STRONGER than the law
    // it replaces: this gate's own per-file law compared two numbers it derived itself, so it could
    // not see a walk that never offered the file - the 86-files-vanish finding this PR published as
    // a limit. `Refusal::NothingJudged` is also the empty-scope arm this gate used to spell itself.
    let counted = match counted {
        Ok(counted) => counted,
        Err(refusal) => {
            eprintln!("xtask check-worktree-state: FAILED - {}", refusal.describe());
            return Verdict::Fail;
        }
    };

    // FAIL CLOSED ON AN EMPTY SCAN. Measured on the tree this landed on: dozens of takings. A run
    // that found none read something other than this repository, and `ok - 0 taking(s)` is a
    // sentence about a tree it never saw. The census cannot see this: it counts FILES judged, and
    // a file judged to hold no taking is judged.
    if offered == 0 {
        eprintln!(
            "xtask check-worktree-state: FAILED - no taking of a machine-shared root in {} file(s)",
            read.len()
        );
        eprintln!("  This workspace has dozens. A scan that found none is a broken scan, not a");
        eprintln!("  clean tree - the rule would then be checking nothing at all.");
        return Verdict::Fail;
    }

    let inspected = match Inspected::of(
        &read,
        counted.judged(),
        lines,
        offered,
        adjudicated,
        instructed,
        instructed_offered,
    ) {
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
            let (offered_lines, lexed_lines) = inspected.lines();
            println!(
                "xtask check-worktree-state: ok - inspected {} of {} taking(s) ({}) over {} file(s) \
                 ({}), {lexed_lines} of {offered_lines} line(s) lexed",
                inspected.inspected(),
                inspected.discovered(),
                holders.join(", "),
                inspected.files(),
                scanned.join(", ")
            );
            // THE CENSUS'S OWN SENTENCE, printed rather than paraphrased: `judged + out_of_scope
            // == discovered` is its invariant and it owns the wording, so this gate cannot state a
            // file-level number the walk did not reach.
            println!("  discovery: {}", counted.verdict());
            Verdict::Pass
        }
        Decision::Instructions => {
            eprintln!(
                "xtask check-worktree-state: FAILED - {} line(s) instruct an operation on a ref \
                 the whole repository shares",
                inspected.instructed().len()
            );
            for (path, line) in inspected.instructed() {
                eprintln!("  {path}:{line}");
            }
            eprintln!();
            eprintln!("`refs/stash` is one ref per REPOSITORY and a linked worktree does not get its");
            eprintln!("own, so one worktree's pop takes whichever entry another pushed. Keep the diff");
            eprintln!("in a patch file under this worktree, which is state the worktree owns. What this");
            eprintln!("rule does NOT reach is a command typed at a prompt - see `global_ref`'s header.");
            Verdict::Fail
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
    fn the_witness_refuses_a_scan_whose_own_tally_is_short_of_the_census() {
        // `telekom/sutura#689`: the walk read one fewer file than the census independently judged
        // - the shape an early `return` inside the closure produces, ahead of the push onto `read`.
        // Reproduced live: `ok - inspected 117 of 117 taking(s) ... over 552 file(s)` beside
        // `discovery: 553 of 1449 subject(s) judged`, both at exit 0, before this law existed.
        let short: Vec<String> = anchors().into_iter().skip(1).collect();
        let one = vec![(taking(1), Keyed::Process)];
        let refused = Inspected::of(&short, anchors().len(), (9, 9), 1, one, Vec::new(), 0)
            .expect_err("a walk short of the census's own count must not mint a witness");
        assert!(refused.contains("independently judged"), "{refused}");
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
            Inspected::of(&anchors(), anchors().len(), (9, 9), 2, one, Vec::new(), 0).is_err(),
            "a subset must not mint a witness"
        );
    }

    #[test]
    fn the_witness_refuses_a_scan_that_read_part_of_a_file() {
        // `telekom/sutura#414`, at the level BOTH other laws miss: they take their numbers from one
        // lexer, so a truncation inside it moves them together, and an anchor asserts a file was
        // OPENED rather than read in full. Measured on a sibling: 40 of 67 takings unread at exit 0
        // with every floor satisfied.
        let one = vec![(taking(1), Keyed::Process)];
        let refused = Inspected::of(&anchors(), anchors().len(), (115_088, 624), 1, one, Vec::new(), 0)
            .expect_err("a partially lexed tree must not mint a witness");
        assert!(
            refused.contains("part \nof a file") || refused.contains("part of a file"),
            "{refused}"
        );
    }

    #[test]
    fn the_witness_prints_the_numbers_its_scan_reached() {
        let two = vec![(taking(1), Keyed::Process), (taking(9), Keyed::Worktree)];
        let witness =
            Inspected::of(&anchors(), anchors().len(), (9, 9), 2, two, Vec::new(), 0).expect("two offered, two adjudicated");
        assert_eq!(witness.discovered(), 2);
        assert_eq!(witness.inspected(), 2);
        assert_eq!(witness.files(), anchors().len());
        assert!(witness.shared().is_empty(), "no shared takings were witnessed");
        assert_eq!(witness.by_holder(), vec![("process", 1), ("worktree", 1)]);
    }

    #[test]
    fn a_shared_taking_is_reported_and_the_rest_are_counted() {
        let mixed = vec![
            (taking(1), Keyed::Shared),
            (taking(4), Keyed::Unwritten),
            (taking(7), Keyed::Shared),
        ];
        let witness = Inspected::of(&anchors(), anchors().len(), (9, 9), 3, mixed, Vec::new(), 0)
            .expect("three offered, three adjudicated");
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
        let witness = Inspected::of(&short, short.len(), (9, 9), 1, one, Vec::new(), 0).expect("the counts agree");
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
        let witness = Inspected::of(&short, short.len(), (9, 9), 1, shared, Vec::new(), 0).expect("the counts agree");
        assert!(!witness.missed().is_empty(), "the fixture must also miss an anchor");
        assert_eq!(super::decide(&witness), super::Decision::Violations);
    }

    #[test]
    fn a_witness_refuses_when_the_git_arm_offered_more_than_it_carried() {
        // THE ARM GOING QUIET, which is the one thing a rule printing no count cannot show in its
        // verdict: delete the `extend` in `run` and every other number still agrees with itself.
        let one = vec![(taking(1), Keyed::Process)];
        let why = Inspected::of(&anchors(), anchors().len(), (9, 9), 1, one, Vec::new(), 1)
            .expect_err("one instruction offered and none carried must not mint a witness");
        assert!(why.contains("repository-global git ref"), "{why}");
    }

    #[test]
    fn an_instruction_on_a_repository_global_ref_refuses_a_tree_with_no_shared_path_in_it() {
        // THE REFUSAL, not the predicate. `global_ref`'s own tests hold the line-finding; what is
        // tested here is that a found line reaches a `Verdict::Fail` - the pair
        // `telekom/sutura#405` records as *predicate tested, refusal untested*, where neutralising
        // the refusal leaves `dead_code` quiet and every suite green.
        let one = vec![(taking(1), Keyed::Process)];
        let instructed = vec![(String::from("crates/sutura-sql/tests/adversarial_findings.rs"), 245)];
        let witness = Inspected::of(&anchors(), anchors().len(), (9, 9), 1, one, instructed, 1).expect("the counts agree");
        assert!(witness.missed().is_empty(), "no anchor was missed");
        assert!(witness.shared().is_empty(), "no filesystem path is shared in this fixture");
        assert_eq!(super::decide(&witness), super::Decision::Instructions);
    }

    #[test]
    fn a_shared_path_outranks_an_instruction_and_both_outrank_a_missed_anchor() {
        let short: Vec<String> = anchors().into_iter().skip(1).collect();
        let shared = vec![(taking(2), Keyed::Shared)];
        let instructed = vec![(String::from("dev/src/scope.rs"), 1)];
        let witness = Inspected::of(&short, short.len(), (9, 9), 1, shared, instructed, 1).expect("the counts agree");
        assert!(!witness.missed().is_empty(), "the fixture also misses an anchor");
        assert!(!witness.instructed().is_empty(), "and also instructs the operation");
        assert_eq!(super::decide(&witness), super::Decision::Violations);
    }

    #[test]
    fn a_clean_scan_that_read_every_anchor_is_the_only_pass() {
        let one = vec![(taking(1), Keyed::Process)];
        let witness = Inspected::of(&anchors(), anchors().len(), (9, 9), 1, one, Vec::new(), 0).expect("the counts agree");
        assert!(witness.missed().is_empty(), "no takings were missed");
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

    #[test]
    fn the_tier_and_the_rust_scope_derive_one_worktree_key() {
        // `telekom/sutura#405`'S PROPERTY 1 - one derivation of *this worktree's own state* - AND
        // IT IS NOT A TEXT COMPARISON. The tier derived a `cksum` CRC-32 and `Scope` four bytes of
        // SHA-256 over one canonical root: two keys for one worktree, so no Rust writer could name
        // the tier's directory and nothing reddened when either side moved. This RUNS the tier's
        // own two lines - taken out of the file rather than restated here - and compares the path
        // they build with `Scope::scratch("pg")`, which is the ONE derivation of a keyed path under
        // a machine-shared root.
        //
        // The lines are located by SHAPE, and a shape it cannot find is an assertion failure rather
        // than a pass over a tier whose key went somewhere else. That is the anchor: this gate's
        // module header records the same requirement one level up.
        let root = crate::repo::root().expect("this test runs inside a checkout");
        let scope = sutura_dev::scope::Scope::from_root(&root).expect("the repository root resolves");
        let expected = scope.scratch("pg");
        let shared = expected.parent().expect("a scratch path sits under a shared root");
        let text = std::fs::read_to_string(root.join("nix/postgres-tier.nix")).expect("the tier is readable");
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let derived: Vec<&str> = lines.iter().copied().filter(|line| line.starts_with("key=")).collect();
        let built: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|line| line.starts_with("pg=") && line.contains("TMPDIR"))
            .collect();
        assert_eq!(derived.len(), 1, "the tier derives its key on one line: {derived:?}");
        assert_eq!(built.len(), 1, "the tier builds its scratch path on one line: {built:?}");
        // `''${` is how a nix indented string spells a shell `${`, and `$root` is the only input
        // those two lines have. `TMPDIR` is handed the parent `Scope::scratch` chose, so both sides
        // read one shared root and what is compared is the key and the name under it.
        let key = derived[0];
        let path = built[0];
        let canonical = scope.root().display();
        let script = format!("set -eu\nroot='{canonical}'\n{key}\n{path}\nprintf '%s' \"$pg\"").replace("''${", "${");
        let ran = std::process::Command::new("bash")
            .arg("-c")
            .arg(&script)
            .env("TMPDIR", shared)
            .output()
            .expect("bash runs the tier's own derivation");
        assert!(ran.status.success(), "{script}\n{}", String::from_utf8_lossy(&ran.stderr));
        let answered = String::from_utf8_lossy(&ran.stdout);
        assert_eq!(
            std::path::Path::new(answered.trim()),
            expected,
            "the tier and `Scope::scratch` build two different paths for one worktree"
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
            let witness = Inspected::of(
                &anchors(),
                anchors().len(),
                (9, 9),
                1,
                vec![(taking(1), keyed)],
                Vec::new(),
                0,
            )
            .expect("one and one");
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
