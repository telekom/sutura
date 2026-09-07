//! What a discovery walk found, as a witness rather than as a `Vec`.
//!
//! Five instances of one shape were measured across four gates
//! (`github.com/telekom/sutura#414`): a gate's *"how much did I read"* number is derived from the
//! same collection its loop walks, so narrowing the loop narrows the witness too and the gate
//! reports `ok` over a subject it mostly did not read. `.take(100)` dropped 99.46% of one walk at
//! exit 0; another left 152 of 156 files unscanned at exit 0; `chmod 000 .github/actions` produced
//! `ok - 2 literal(s) across 8 file(s)` at exit 0 - **the denominator moved with the numerator**.
//!
//! One sentence explains all of them: every gate-local fix in this tree sits **above the
//! discovery**, and a gate comparing `inspected` against `discovered` where both come off the same
//! collection cannot see a failure that happened before the collection was populated.
//!
//! So the remedy is not a better number. It is that **the count stops being the control**: an
//! unreachable subject refuses whether or not anybody reads a count, and a narrowed loop is not
//! expressible **as an iterator narrowing** whether or not anybody compares two integers. It was
//! once written here without that qualifier, and the qualifier is the honest half: an ordinal
//! narrowing respelt as a counter *inside* the caller's closure was three lines and green
//! (`1 of 1170 subject(s) judged`, exit 0), which is why [`Scope`] is a bare `fn` pointer now.
//!
//! **And the read is no longer the caller's**, which is the second correction this module carries.
//! Handing a gate a path and asking it to classify what it found put all three instruments -
//! accounting, loop count and the `must_judge` anchor - behind one arm the caller wrote itself: a
//! closure answering *judged* for a path it never opened discharged the anchor, moved the
//! numerator with no read behind it, and left the verdict **byte-identical** to a clean tree's
//! (measured on an unreadable anchor: `Unreachable` exit 1, `OutOfScope` exit 1, **`Judged` exit
//! 0**). [`Census::inspect`] performs the read itself now, so *judged* is not a claim a caller can
//! make - it is what the census observed.
//!
//! Generalised from `warm_start::pairing`, which already ships this argument for one gate -
//! `Discovered(Vec<Taking>)` with a private field and one constructor, `Swept::over` refusing a
//! length mismatch, and a verdict printing the witness's own length. That spelling stays where it
//! is on purpose: two independent derivations of one argument, and migrating it would delete the
//! reference implementation at the same moment the generalisation is first relied on.

use std::path::PathBuf;

/// Everything a discovery walk found, and everything it was meant to reach and could not.
///
/// `of` is PRIVATE and [`Census::found`] is `pub(super)`, so `crate::repo` is the only module that
/// can mint one - **one constructor by visibility, not by convention**. A caller cannot conjure a
/// denominator by reaching fewer subjects, and there is no rule to satisfy: the type checker is the
/// thing asking.
///
/// There is deliberately no `IntoIterator`, no `Deref`, no `Borrow`, no `iter` and no `as_slice`.
/// **A caller that cannot hold the iterator cannot narrow it**, which is the whole mechanism, and
/// it is why [`Census::inspect`] takes a closure rather than returning a sequence.
///
/// **That absence is held by `check-newtype-leaks` now, and it was held by nobody before.**
/// Measured on `565ebaae`: four lines of `impl IntoIterator for Census` compiled, `.take(3)` at the
/// migrated call site then printed a verdict over three subjects at exit 0, and the gate counted
/// the new impl (`244` trait impls to `245`) **without refusing** - so the headline property was
/// held by nobody adding four lines. That gate's `SEALED` list names this type, so the impl is a
/// refusal naming this file, and a method here whose signature hands out the sequence is one too.
/// [`Census::into_listing`] is the single declared exception, and its own bound is
/// [`UNMIGRATED_DOORS`].
pub(crate) struct Census {
    /// Repo-relative paths with `/` separators, in discovery order.
    of: Vec<String>,
    /// Absolute root to join one of them to.
    root: PathBuf,
    /// Subjects the walk was meant to reach and could not, already worded for a reader.
    unreachable: Vec<String>,
}

/// Which subjects a gate's rule is about, decided from the path.
///
/// **A bare `fn` pointer rather than a closure, and that is a mechanism rather than a style
/// choice.** A non-capturing function has nowhere to keep a counter, so the ordinal narrowing
/// measured on `github.com/telekom/sutura#419` - `let mut seen = 0; if seen > 60 { out of scope }`,
/// three lines in the caller's closure, `1 of 1170 subject(s) judged` at exit 0 - does not compile
/// here: `error[E0308]: expected fn pointer, found closure`. It is not HANDED the content and is
/// not told the read's outcome, so it cannot be *written* as a reaction to a failed read the way
/// the classification enum this replaced could.
///
/// **What it does NOT prevent, because the sentence here once claimed otherwise and the measurement
/// refutes it.** A `Scope` is a safe `fn`, and Rust has no effect system, so **nothing in this type
/// can forbid it opening the file itself** - and that fail-open is real, not hypothetical. Measured
/// at the production call site with `&& File::open(root.join(rel)).is_ok()` added to
/// `check-expect-thresholds`' predicate and `chmod 000` on one tracked `.rs`: `384 of 1177
/// subject(s) judged, 793 out of scope` at **exit 0**, where the unmutated gate refuses at exit 1
/// naming the file. **The type is doing everything a type can do here; what holds the rest is this
/// paragraph and the diff a reviewer reads**, because a `Scope` is a named, greppable, top-level
/// `fn` and an IO call inside one is conspicuous.
///
/// **Which of the five printed numbers such a `Scope` can move, measured rather than assumed** -
/// it matters, because "the numbers move" was published here as the defence and only part of it is
/// true:
///
/// | attempt | result |
/// | --- | --- |
/// | inflate `out_of_scope` to hide a subject | **works**, and it is the shape above |
/// | discharge an anchor it never read | **cannot.** `outstanding.retain` is on the READ path, so a
///   subject the census did not open cannot satisfy `must_judge` whatever a `Scope` does. That is a
///   STRUCTURAL bound rather than a count, and it is the strongest thing here |
/// | inflate `read_bytes` | **cannot.** It is summed from the reads themselves, so a `Scope` can
///   only make it smaller, or unrepresentative of the subject - never larger |
/// | rename the subject from inside the `Scope` | routes it to `absent`, with `out_of_scope`
///   **byte-identical** to the control - so `absent` is the tell, not `out_of_scope` |
/// | rewrite the subject to the same LENGTH | moves **none of the five**. The byte count is a
///   witness of what was read, never of what was true |
///
/// **The other residue.** A scope predicate that simply excludes most of the tree is still writable
/// and still legal - narrowing `check-expect-thresholds` to `xtask/` alone leaves `128 of 1177` at
/// exit 0 with 257 files unjudged, because the anchor is inside the narrowed scope. What closes
/// that is a `must_judge` set DERIVED from `[workspace] members` rather than declared, which is a
/// later PR in that issue's stack.
pub(crate) type Scope = fn(&str) -> bool;

/// Why a census could not produce a verdict. Each arm is a measured defect, not a hypothesis.
#[derive(Debug)]
pub(crate) enum Refusal {
    /// The repo root could not be determined, so nothing was discovered. `repo::root`'s own
    /// comment records this happening from a store path, where the walk found nothing and the gate
    /// announced `ok - 0 text file(s) checked` at exit 0.
    NoRoot,
    /// The walk was meant to reach subjects and could not. Names them.
    Unreachable(Vec<String>),
    /// A path the caller declared it cannot have a verdict without was not judged.
    ///
    /// Generalises `warm_start::pairing::nix_files`' `flake.nix` anchor, and it is stronger than a
    /// count floor: it survives a scope predicate that stopped matching, which a `== 0` floor does
    /// not.
    ///
    /// **The limit, and it is the gap a later PR closes.** This is a DECLARED name, not a set
    /// derived from `[workspace] members`, so it catches a narrowing that EXCLUDES the anchor and
    /// not one that keeps it: measured, narrowing `check-expect-thresholds`' scope to `xtask/`
    /// alone left `125 of 1170 subject(s) judged` at **exit 0** with 253 files silently unjudged,
    /// because `xtask/src/main.rs` is still inside the narrowed scope. A derived member set is a
    /// second derivation rather than a rewording of this one - `github.com/telekom/sutura#414`.
    NotJudged {
        /// The anchor that was declared and not judged.
        path: String,
        /// How many subjects the walk did discover, so a reader can tell an empty walk from a
        /// broken predicate.
        discovered: usize,
    },
    /// Subjects were discovered and the caller judged NONE of them.
    ///
    /// **This arm is why deleting a gate's `== 0` floor costs nothing**, and it was added because
    /// a mutation showed otherwise: with `must_judge` empty and the scope predicate broken,
    /// `check-expect-thresholds` printed `0 of 1166 subject(s) judged` at exit 0, where the
    /// `rs_files == 0` floor it replaced had refused. The two are not interchangeable and the tree
    /// needs both, exactly as `warm_start::Swept` keeps its empty arm beside its subset arm:
    /// **this catches a predicate that matched nothing, [`Refusal::NotJudged`] catches a predicate
    /// that matched plenty and not the file that mattered.**
    NothingJudged {
        /// What the walk did reach, so a reader can tell a broken predicate from an empty tree.
        discovered: usize,
    },
    /// Nothing was discovered at all.
    Empty,
}

impl Refusal {
    /// The sentence a gate prints. The wording lives here once, so no gate writes its own.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::NoRoot => String::from(
                "could not determine the repo root, so NOTHING was discovered - and a verdict over \
                 nothing is the failure a gate exists to prevent, not a pass",
            ),
            Self::Unreachable(subjects) => format!(
                "could not reach {} subject(s) this walk was meant to cover, so every count below \
                 would agree with itself over a tree it never looked at: {}",
                subjects.len(),
                subjects.join("; ")
            ),
            Self::NotJudged { path, discovered } => format!(
                "discovered {discovered} subject(s) and did not judge `{path}`, which this gate \
                 declared it cannot have a verdict without - so the scan is broken rather than the \
                 tree, whatever the count says"
            ),
            Self::NothingJudged { discovered } => format!(
                "discovered {discovered} subject(s) and judged NONE of them, so this gate's own \
                 rule never fired - the scan is broken rather than the tree satisfying it"
            ),
            Self::Empty => String::from(
                "discovered no subject at all, so this verdict would be about an empty tree rather \
                 than about this one",
            ),
        }
    }
}

/// A gate whose loop has NOT been moved inside [`Census::inspect`] yet.
///
/// **A closed, declared list rather than a convention.** A variant whose last caller migrates is
/// `dead_code` - `error: variant Docs is never constructed`, measured under `--all-targets` even
/// though a `#[cfg(test)]` test in this file constructs it - so the migrating PR is pushed into
/// deleting it.
///
/// **What the enum does NOT bound, and the bound that replaces the claim.** This list used to
/// claim that *"a new gate reaching for the plain `Vec` is a one-line diff in this file"*. It is
/// not: any variant with a second call site is a usable key, so a new consumer declaring
/// `Unmigrated::Guidance` - which has six - compiled with **no diff here at all**
/// (`cargo check -p xtask --all-features --all-targets`, exit 0, no diagnostics). The count in the
/// other direction was held by nothing too: no test and no gate referenced this enum's size. What
/// holds both now is [`UNMIGRATED_DOORS`], an exact call-site count checked against the live tree,
/// so opening the door anywhere is a diff in THIS file whichever variant is named.
///
/// **The limits, because the ratchet reads stronger than it is.** `dead_code` is `deny` in this
/// workspace's `Cargo.toml` and not `forbid`, and one crate-level `#![allow(dead_code)]` makes the
/// shrink half disappear - measured. And [`UNMIGRATED_DOORS`] is an EQUALITY, which makes the
/// number current rather than monotone: lowering it is forced by a migration, and nothing
/// mechanical stops a future diff raising it. Both halves are therefore strictly weaker than the
/// properties the type system holds here - `error[E0624]` on [`Census::found`] from another
/// module, and `error[E0599]: census::Census is not an iterator` on any attempt to narrow the
/// walk.
///
/// When the list is empty, `into_listing` and this enum go with it. `WarmStart` is the one entry
/// that stays: `check-warm-start` keeps its own witness deliberately (see this module's header).
pub(crate) enum Unmigrated {
    BootOrder,
    BoundedWait,
    Boundaries,
    Causality,
    Conformance,
    Docs,
    Examples,
    FeatureRemedies,
    Guidance,
    MaxLines,
    NewtypeLeaks,
    OneBound,
    Refusals,
    SerdeParse,
    ShippedBinaries,
    TextHygiene,
    UnusedDeps,
    Venues,
    WarmStart,
    Workflows,
}

#[cfg(test)]
/// How many places in `xtask/src` still call [`Census::into_listing`]. **Checked against the live
/// tree, so opening the transitional door is a diff in this file.**
///
/// `Unmigrated` alone could not hold this: naming an existing variant needs no diff here, which
/// was measured on `565ebaae`. An exact count does, whichever variant a new caller names - and it
/// forces the number DOWN when a gate migrates, so the figure a reader sees is current rather than
/// a high-water mark. Test-only call sites count: each one holds a plain `Vec` it can narrow, and
/// deciding otherwise means deciding what a `#[cfg(test)]` block is for.
///
/// **Every file is counted, including this one.** The scan used to skip `census.rs` wholesale,
/// which meant a second door declared here - or a call to one from here - was invisible to the
/// count that exists to see exactly that. What is excluded is the DECLARATION (`fn into_listing`),
/// not the file.
///
/// **The limit:** this is an equality, not a ratchet. Raising it is a one-line diff, exactly like
/// adding a variant - what changed is that there is now a line to diff.
///
/// It counts textual occurrences in blanked code, and `serde_parse::scan::code_lines` blanks the
/// interior of a MULTI-line string only - so a single-line fixture spelling the call reads as one.
/// Measured on the first run of this test, which reported 45 against 44 real call sites; the extra
/// was a `check-newtype-leaks` fixture, and it is built from parts now, the way that gate's own
/// fixtures already avoid reporting their own source. **44 became 47 when the file skip went**,
/// and all three are this module's own `#[cfg(test)]` calls - which the old rule could not see.
pub(crate) const UNMIGRATED_DOORS: usize = 48;

impl Census {
    /// Mint one. `pub(super)`, so `crate::repo` is the only caller there can be.
    pub(super) const fn found(root: PathBuf, of: Vec<String>, unreachable: Vec<String>) -> Self {
        Self { of, root, unreachable }
    }

    /// **The only way to consume a census with a per-subject rule.**
    ///
    /// The loop is HERE, so the caller never receives an iterator and `.take(n)` has nowhere to be
    /// written. **And the READ is here too**, which is the correction #419 needed: `judge` is
    /// handed the bytes of a subject this census opened successfully, so *judged* is an
    /// observation rather than the caller's claim. `FnMut`, so a gate accumulates findings by
    /// capturing its own `&mut Vec<_>` - which is how every walk in this tree already accumulates.
    ///
    /// **What the read being here makes unconstructible.** The closure returns `()`. There is no
    /// arm in which to say *judged*, *out of scope* or *unreachable* about a subject, so the shape
    /// measured on `565ebaae` - a closure answering `Looked::Judged` for a file it could not open,
    /// which discharged the anchor, incremented the numerator and left the verdict byte-identical
    /// to a clean tree's at exit 0 - is not expressible. The three instruments no longer share one
    /// caller-written arm:
    ///
    /// * the numerator is incremented by a successful `read`, never by the closure;
    /// * `must_judge` is discharged by a successful `read`, never by the closure;
    /// * `read_bytes` is minted by the reads themselves, so no caller can state it.
    ///
    /// `must_judge` is the paths this gate declares it cannot have a verdict without. An empty set
    /// is permitted only for a gate whose subject may legitimately be absent, and that is a
    /// reviewable choice rather than a default. An anchor its own [`Scope`] excludes can never be
    /// discharged, so mis-declaring one fails closed.
    ///
    /// **`NotFound` is counted, not refused**, and that split lives here once rather than in each
    /// gate's closure. `all_files` prefers `git ls-files`, which reads the INDEX, so a tracked file
    /// deleted in the working tree with the deletion unstaged is offered by the listing and is not
    /// on disk: refusing it made `just hygiene` red for anyone mid-edit, blaming the walk for what
    /// the index said. It stays visible - [`Inspected::verdict`] prints it - rather than silent.
    ///
    /// Refuses five ways before it returns, in this order: an unreachable subject from the walk,
    /// an empty discovery, a subject in scope this census could not READ, a scope predicate that
    /// judged nothing, and an anchor that was never judged. [`Refusal::NoRoot`] is
    /// `repo::all_files`' and cannot reach here.
    pub(crate) fn inspect(
        self,
        must_judge: &[&str],
        scope: Scope,
        mut judge: impl FnMut(&str, &[u8]),
    ) -> Result<Inspected, Refusal> {
        // BEFORE the loop: a partial tree makes every number below a number about a subset, so
        // there is nothing worth inspecting yet.
        if !self.unreachable.is_empty() {
            return Err(Refusal::Unreachable(self.unreachable));
        }
        if self.of.is_empty() {
            return Err(Refusal::Empty);
        }

        let discovered = self.of.len();
        let mut outstanding: Vec<&str> = must_judge.to_vec();
        let mut judged = 0_usize;
        let mut out_of_scope = 0_usize;
        let mut absent = 0_usize;
        let mut read_bytes = 0_usize;
        let mut unreachable: Vec<String> = Vec::new();

        for rel in &self.of {
            if !scope(rel) {
                out_of_scope = out_of_scope.saturating_add(1);
                continue;
            }
            match std::fs::read(self.root.join(rel)) {
                Ok(bytes) => {
                    read_bytes = read_bytes.saturating_add(bytes.len());
                    judge(rel, &bytes);
                    // Discharged by the READ, above the closure's reach. This line running only
                    // under a caller-supplied `Judged` arm is what made an unreadable anchor pass.
                    outstanding.retain(|anchor| anchor != rel);
                    judged = judged.saturating_add(1);
                }
                // Absent is not unreachable - `repo::walk`'s own split at its `read_dir`, made
                // once here instead of once per gate.
                Err(why) if why.kind() == std::io::ErrorKind::NotFound => absent = absent.saturating_add(1),
                Err(why) => unreachable.push(format!("{rel}: {why}")),
            }
        }

        if !unreachable.is_empty() {
            return Err(Refusal::Unreachable(unreachable));
        }
        if judged == 0 {
            return Err(Refusal::NothingJudged { discovered });
        }
        if let Some(anchor) = outstanding.first() {
            return Err(Refusal::NotJudged {
                path: String::from(*anchor),
                discovered,
            });
        }
        Ok(Inspected {
            judged,
            discovered,
            out_of_scope,
            absent,
            read_bytes,
        })
    }

    /// The transitional door: the whole listing, for a gate whose loop has not moved inside
    /// [`Census::inspect`] yet.
    ///
    /// **State the limit next to the claim.** This still refuses on an unreachable subject and on
    /// an empty discovery, so the *discovery* half of the mechanism holds at every call site the
    /// day this lands. What it does NOT hold is the *loop* half: a caller holding this `Vec` can
    /// write `.take(n)` on it, exactly as it can today. Each migration in
    /// `github.com/telekom/sutura#414`'s stack deletes one [`Unmigrated`] variant, and the
    /// compiler refuses to let a spent one linger.
    ///
    /// Taking the token by value rather than by reference so a caller cannot keep one around to
    /// re-open the door with later.
    pub(crate) fn into_listing(self, _caller: Unmigrated) -> Result<Listing, Refusal> {
        if !self.unreachable.is_empty() {
            return Err(Refusal::Unreachable(self.unreachable));
        }
        if self.of.is_empty() {
            return Err(Refusal::Empty);
        }
        Ok((self.root, self.of))
    }
}

/// What an unmigrated gate gets: the root, and the discovered paths as a plain `Vec`.
///
/// Named rather than a tuple because `type_complexity` is tightened in this workspace, and because
/// a name is a place to say what this is: **the transitional shape, not the destination.**
pub(crate) type Listing = (PathBuf, Vec<String>);

/// A verdict that accounted for EVERY subject the walk discovered.
///
/// Fields private, no public constructor, and [`Census::inspect`] is the only thing that returns
/// one - after it refused four ways. So none of the accessors can state a number the walk did not
/// reach: `discovered` is set inside `inspect` from the census's own length, and
/// `judged + out_of_scope + absent == discovered` holds by construction.
///
/// **`read_bytes` is the number a caller cannot fabricate**, and it is here because every other
/// number in the sentence is a count of decisions while this one is a count of bytes that came off
/// the disk. A narrowing that keeps the anchor and moves the counts a little moves this by orders
/// of magnitude, so it is the tell a reader has when the ratio looks plausible.
pub(crate) struct Inspected {
    judged: usize,
    discovered: usize,
    out_of_scope: usize,
    absent: usize,
    read_bytes: usize,
}

impl Inspected {
    /// One spelling, so no gate writes its own arithmetic into a `format!`.
    pub(crate) fn verdict(&self) -> String {
        format!(
            "{} of {} subject(s) judged, {} out of scope, {} absent, {} byte(s) read",
            self.judged, self.discovered, self.out_of_scope, self.absent, self.read_bytes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Census, Refusal, UNMIGRATED_DOORS, Unmigrated};
    use std::path::{Path, PathBuf};

    /// A scratch tree holding the named files, removed when the test ends.
    ///
    /// Needed because [`Census::inspect`] performs the read itself now: a listing over a root that
    /// does not exist no longer proves anything about judging, which is the point of the change.
    struct Tree(PathBuf);

    impl Drop for Tree {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    fn tree(name: &str, files: &[(&str, &str)]) -> Tree {
        let root = std::env::temp_dir().join(format!("sutura-census-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(&root).expect("a scratch tree");
        for (rel, body) in files {
            let at = root.join(rel);
            if let Some(parent) = at.parent() {
                std::fs::create_dir_all(parent).expect("a parent");
            }
            std::fs::write(&at, body).expect("a file");
        }
        Tree(root)
    }

    fn census_in(root: &Path, of: &[&str], unreachable: &[&str]) -> Census {
        Census::found(
            root.to_path_buf(),
            of.iter().map(|s| String::from(*s)).collect(),
            unreachable.iter().map(|s| String::from(*s)).collect(),
        )
    }

    /// A listing over a root that holds nothing, for the arms that refuse before any read.
    fn census(of: &[&str], unreachable: &[&str]) -> Census {
        census_in(Path::new("/nowhere"), of, unreachable)
    }

    fn rust(rel: &str) -> bool {
        Path::new(rel).extension().is_some_and(|found| found == "rs")
    }

    fn everything(_rel: &str) -> bool {
        true
    }

    fn nothing(_rel: &str) -> bool {
        false
    }

    fn nix_file(rel: &str) -> bool {
        Path::new(rel).extension().is_some_and(|found| found == "nix")
    }

    #[test]
    fn an_unreachable_subject_refuses_before_the_closure_runs() {
        // THE measured defect: `chmod 000 .github/actions` gave `ok - 2 literal(s) across 8
        // file(s)` at exit 0 because the denominator moved with the numerator. Here the walk's own
        // finding refuses, and it refuses BEFORE any count exists to compare - so no arithmetic a
        // gate writes can agree with itself over the missing subtree.
        let mut ran = 0_usize;
        let refused = census(&["a.rs", "b.rs"], &[".github/actions: Permission denied"])
            .inspect(&[], everything, |_, _| ran = ran.saturating_add(1));
        assert!(ran == 0, "the closure must not run over a partial tree, ran {ran} time(s)");
        match refused {
            Err(Refusal::Unreachable(subjects)) => {
                assert_eq!(subjects.len(), 1, "{subjects:?}");
            }
            Err(other) => panic!("wrong arm: {}", other.describe()),
            Ok(inspected) => panic!("a partial tree produced a verdict: {}", inspected.verdict()),
        }
    }

    #[test]
    fn a_subject_the_census_cannot_read_is_a_refusal_not_a_continue() {
        // Deterministic on every platform and with no mode bits: `fs::read` on a DIRECTORY fails
        // with something other than `NotFound`, which is the class `chmod 000` produces. The read
        // is the census's own now, so the caller has no arm in which to call this a scope decision.
        let at = tree("unreadable", &[("a.rs", "// fine\n")]);
        std::fs::create_dir_all(at.0.join("b.rs")).expect("a directory wearing a file's name");

        let refused = census_in(&at.0, &["a.rs", "b.rs"], &[]).inspect(&[], everything, |_, _| {});

        let Err(Refusal::Unreachable(subjects)) = refused else {
            panic!("a subject that cannot be read was not a refusal");
        };
        assert_eq!(subjects.len(), 1, "{subjects:?}");
        assert!(subjects.first().is_some_and(|why| why.starts_with("b.rs: ")), "{subjects:?}");
    }

    #[test]
    fn an_unreadable_anchor_cannot_be_discharged_by_the_caller() {
        // **Finding 7b on #419, as a test.** With the read in the caller's hands, a closure
        // answering `Judged` for a path it never opened discharged the anchor, moved the numerator
        // and printed a verdict byte-identical to a clean tree's at exit 0. The closure here does
        // nothing at all - it has no arm to lie in - and the anchor still refuses.
        let at = tree("anchor-unreadable", &[("a.rs", "// fine\n")]);
        std::fs::create_dir_all(at.0.join("anchor.rs")).expect("an unreadable anchor");

        let refused = census_in(&at.0, &["a.rs", "anchor.rs"], &[]).inspect(&["anchor.rs"], everything, |_, _| {});

        match refused {
            Err(Refusal::Unreachable(subjects)) => {
                assert!(
                    subjects.first().is_some_and(|why| why.starts_with("anchor.rs: ")),
                    "{subjects:?}"
                );
            }
            Err(other) => panic!("wrong arm: {}", other.describe()),
            Ok(inspected) => panic!("an unreadable anchor produced a verdict: {}", inspected.verdict()),
        }
    }

    #[test]
    fn an_anchor_out_of_scope_can_never_be_discharged() {
        // Fail-closed the other way: the anchor is on disk and readable, and the scope predicate
        // excludes it. Nothing the closure does can help, because it is never called for it.
        let at = tree("anchor-out-of-scope", &[("a.rs", "// fine\n"), ("anchor.md", "text\n")]);

        let refused = census_in(&at.0, &["a.rs", "anchor.md"], &[]).inspect(&["anchor.md"], rust, |_, _| {});

        match refused {
            Err(Refusal::NotJudged { path, discovered }) => {
                assert_eq!(path, "anchor.md");
                assert_eq!(discovered, 2);
            }
            Err(other) => panic!("wrong arm: {}", other.describe()),
            Ok(inspected) => panic!("an out-of-scope anchor produced a verdict: {}", inspected.verdict()),
        }
    }

    #[test]
    fn the_closure_is_handed_the_bytes_the_census_read() {
        // The other half of the same change: `judged` is what the census OPENED, so the byte count
        // in the verdict is minted by those reads and no caller can state it.
        let at = tree("bytes", &[("a.rs", "abc\n"), ("b.md", "ignored\n"), ("c.rs", "de\n")]);
        let mut seen: Vec<(String, usize)> = Vec::new();

        let inspected = census_in(&at.0, &["a.rs", "b.md", "c.rs"], &[])
            .inspect(&[], rust, |rel, bytes| seen.push((String::from(rel), bytes.len())))
            .expect("nothing to refuse");

        assert_eq!(seen, vec![(String::from("a.rs"), 4), (String::from("c.rs"), 3)], "{seen:?}");
        // Both counts come off the census, `judged + out_of_scope + absent == discovered` by
        // construction, and the 7 bytes are the two reads' own lengths.
        assert_eq!(
            inspected.verdict(),
            "2 of 3 subject(s) judged, 1 out of scope, 0 absent, 7 byte(s) read"
        );
    }

    #[test]
    fn an_absent_subject_is_counted_rather_than_refused() {
        // `all_files` prefers `git ls-files`, which reads the INDEX, so a tracked file deleted in
        // the working tree with the deletion unstaged is offered by the listing and is not on
        // disk. Refusing it made `just hygiene` red for anyone mid-edit; the split lives here once
        // now rather than once per gate, and the verdict says so instead of shrugging.
        let at = tree("absent", &[("a.rs", "ab\n")]);

        let inspected = census_in(&at.0, &["a.rs", "gone.rs"], &[])
            .inspect(&[], rust, |_, _| {})
            .expect("an absent subject is not a refusal");

        assert_eq!(
            inspected.verdict(),
            "1 of 2 subject(s) judged, 0 out of scope, 1 absent, 3 byte(s) read"
        );
    }

    #[test]
    fn a_predicate_that_matched_nothing_refuses_even_with_no_anchor_declared() {
        // The regression a mutation of #419 found: deleting a gate's `== 0` floor in favour of
        // `must_judge` is only free if an EMPTY anchor set still refuses a scan that judged
        // nothing. `check-expect-thresholds` printed `0 of 1166 subject(s) judged` at exit 0
        // before this arm existed.
        let refused = census(&["a.md", "b.md"], &[]).inspect(&[], nothing, |_, _| {});
        match refused {
            Err(Refusal::NothingJudged { discovered }) => assert_eq!(discovered, 2),
            Err(other) => panic!("wrong arm: {}", other.describe()),
            Ok(inspected) => panic!("a predicate that matched nothing gave a verdict: {}", inspected.verdict()),
        }
    }

    #[test]
    fn an_anchor_that_was_never_judged_refuses_whatever_the_count_says() {
        // The other half, and the two are not interchangeable: here the predicate matched plenty
        // and missed the one file the gate is about, which no count floor can see.
        let at = tree("anchor-missed", &[("a.rs", "x\n"), ("flake.nix", "{}\n"), ("c.rs", "y\n")]);

        let refused = census_in(&at.0, &["a.rs", "flake.nix", "c.rs"], &[]).inspect(&["flake.nix"], rust, |_, _| {});

        match refused {
            Err(Refusal::NotJudged { path, discovered }) => {
                assert_eq!(path, "flake.nix");
                assert_eq!(discovered, 3, "the refusal names what the walk DID reach");
                // and the count floor above it was satisfied: two of three subjects were judged.
            }
            Err(other) => panic!("wrong arm: {}", other.describe()),
            Ok(inspected) => panic!("a missing anchor produced a verdict: {}", inspected.verdict()),
        }
    }

    #[test]
    fn an_anchor_that_was_judged_is_satisfied() {
        let at = tree("anchor-ok", &[("flake.nix", "{}\n"), ("a.md", "text\n")]);

        let inspected = census_in(&at.0, &["flake.nix", "a.md"], &[])
            .inspect(&["flake.nix"], nix_file, |_, _| {})
            .expect("the anchor was judged");

        assert_eq!(
            inspected.verdict(),
            "1 of 2 subject(s) judged, 1 out of scope, 0 absent, 3 byte(s) read"
        );
    }

    #[test]
    fn an_empty_discovery_is_a_failure_rather_than_a_pass() {
        // `repo::root`'s own comment records `ok - 0 text file(s) checked` at exit 0 from a store
        // path. Both doors out of a census refuse it now.
        assert!(matches!(
            census(&[], &[]).inspect(&[], everything, |_, _| {}),
            Err(Refusal::Empty)
        ));
        assert!(matches!(
            census(&[], &[]).into_listing(Unmigrated::MaxLines),
            Err(Refusal::Empty)
        ));
    }

    #[test]
    fn the_transitional_door_refuses_an_unreachable_subject_too() {
        // The property an unmigrated gate DOES get, stated as a test rather than as a sentence in
        // a PR body.
        let refused = census(&["a.rs"], &["docs: Permission denied"]).into_listing(Unmigrated::Docs);
        assert!(matches!(refused, Err(Refusal::Unreachable(_))));
    }

    #[test]
    fn every_transitional_door_in_the_tree_is_declared() {
        // The bound `Unmigrated` never had. A new consumer naming an EXISTING variant needed no
        // diff in this file at all - measured on `565ebaae`, `cargo check --all-targets` exit 0
        // with no diagnostics - so the enum held the shrink half and nothing else. This holds the
        // count, whichever variant a caller names, and forces it down when a gate migrates.
        //
        // Reads the tree through the production doors, so the gate that bounds the census is
        // subject to the census.
        let Some(root) = crate::repo::root() else {
            panic!("the repo root is what every gate here depends on");
        };
        let mut doors = 0_usize;
        let mut counted = 0_usize;
        crate::repo::collect_files(&root, &root.join("xtask/src"), &["rs"])
            .inspect(&[SELF], is_rust_source, |_rel, bytes| {
                counted = counted.saturating_add(1);
                for line in crate::serde_parse::scan::code_lines(&String::from_utf8_lossy(bytes)) {
                    // **The DECLARATION is excluded, not the FILE.** Skipping `census.rs` whole
                    // was a hole: a second door declared here, or a call to one from here, was
                    // invisible to the count that exists to see exactly that. The `fn `-prefixed
                    // spelling is the definition; every other one is a use, wherever it sits. Not
                    // written out here, so a reader's own grep and this count agree.
                    let uses = line
                        .matches(DOOR)
                        .count()
                        .saturating_sub(line.matches(&format!("fn {DOOR}")).count());
                    doors = doors.saturating_add(uses);
                }
            })
            .expect("xtask/src is this gate's own source tree");

        assert!(
            counted > 40,
            "only {counted} file(s) scanned - the scan is broken, not the tree"
        );
        assert_eq!(
            doors, UNMIGRATED_DOORS,
            "{doors} call site(s) of the transitional door, {UNMIGRATED_DOORS} declared. Migrating a \
             gate lowers the number in census.rs; opening the door raises it, and that diff is the \
             argument"
        );
    }

    /// This module's own path, as the census reports it - the anchor for the scan above, so a
    /// narrowing that drops the file declaring the door refuses rather than reporting zero.
    const SELF: &str = "xtask/src/repo/census.rs";

    /// The call the count above bounds, spelt once. In a `const` so this file's own source carries
    /// it as a string literal, which `code_lines` blanks.
    const DOOR: &str = "into_listing(";

    fn is_rust_source(rel: &str) -> bool {
        Path::new(rel).extension().is_some_and(|found| found == "rs")
    }

    #[test]
    fn every_refusal_says_what_it_refused() {
        for refusal in [
            Refusal::NoRoot,
            Refusal::NothingJudged { discovered: 7 },
            Refusal::Unreachable(vec![String::from("x")]),
            Refusal::NotJudged {
                path: String::from("flake.nix"),
                discovered: 3,
            },
            Refusal::Empty,
        ] {
            let said = refusal.describe();
            assert!(said.len() > 40, "a refusal a reader cannot act on: {said}");
        }
    }
}
