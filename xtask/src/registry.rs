//! The gate registry's TYPES: what a task concluded, and what a task is.
//!
//! **Here rather than in `main.rs` because that file hit the 1000-line cap `max-lines` holds over
//! `xtask/`** - the same pressure `crate::falsifier`'s own header records, and the reason the
//! table itself now lives in `crate::task_table`, split by area. That module's header carries the
//! measurement and the argument; this one holds only the types.
//!
//! `Verdict` and `Reads` are re-exported from the crate root, so every gate's `crate::Verdict`
//! resolves unchanged and this split is invisible to the thirty-odd modules that use it.

use std::process::ExitCode;

/// What a task concluded.
///
/// Not `ExitCode`: that type is opaque - it cannot be compared or read back - so a task that
/// runs other tasks could not tell whether they passed. `main` converts this to an `ExitCode`
/// once, at the process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Nothing to report.
    Pass,
    /// A violation. The task has already printed it.
    Fail,
    /// Invoked wrongly - bad or missing arguments. Distinct from a violation, because a
    /// mistyped command is not a repo problem.
    Usage,
    /// The gate reached no verdict about the change: its precondition held, it ran, and what it
    /// exists to measure was not measurable. Neither a violation nor a clean bill.
    ///
    /// WHY IT IS A THIRD EXIT CODE RATHER THAN A SENTENCE. A required CI step reads the exit code
    /// and nothing else, so a gate that could not measure and exits 0 hands its reader a green
    /// check over no evidence - measured twice on finished branches, and the record is
    /// `github.com/telekom/sutura#307`. Failing instead was weighed and rejected: the changes that
    /// land on `causality`'s inconclusive arms are legitimate ones (a harness move, a changed
    /// public signature a held-at-HEAD test calls) and a gate that reddens correct work gets
    /// disabled. So the decision moves to the venue, and the DEFAULT is closed: any consumer that
    /// does not recognise this code fails on it, because 3 is not 0.
    ///
    /// **What it is not**: a venue may still choose to continue over it - `ci.yml`'s causality step
    /// and `devenv.nix`'s `ship-check` both do, at one line each, and surface the verdict instead.
    /// What changed is that continuing is now a stated decision in one readable place rather than
    /// an exit code no reader can tell from a proof.
    Inconclusive,
}

impl Verdict {
    // Not const: `ExitCode::from` is not a const fn.
    pub(crate) fn exit_code(self) -> ExitCode {
        match self {
            Self::Pass => ExitCode::SUCCESS,
            Self::Fail => ExitCode::FAILURE,
            Self::Usage => ExitCode::from(2),
            Self::Inconclusive => ExitCode::from(3),
        }
    }
}

/// What a gate does: read the repo, print a verdict.
pub(crate) type Gate = fn(&[String]) -> Verdict;

/// Whether a task belongs to the `hygiene` sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Cheap, argument-free, judges the whole repo. Collected into `hygiene`, and it says what
    /// it reads - see [`Reads`], and `gate_classification` for what that buys.
    Hygiene(Reads),
    /// Everything else: takes arguments, changes files, or runs other tasks. Never collected,
    /// which is also what stops `hygiene` from recursing into itself.
    Standalone,
}

/// Which side of the prose/code line a hygiene gate's INPUTS fall on.
///
/// A payload on [`Kind::Hygiene`] rather than a separate field, so a new gate cannot be added
/// without answering the question: the compiler asks it, no test has to. It exists because a
/// workflow skips the `hygiene` build for a diff of `docs/*.md` and `mkdocs.yml` alone, and the
/// argument for that skip is a CLASSIFICATION of the sweep - not its size. A count would stay
/// green while a gate joined the set unclassified, which is the drift that already happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reads {
    /// Nothing a `docs/*.md`-or-`mkdocs.yml` diff can change: Rust, manifests, lock files, nix,
    /// workflow YAML, the justfile, the hook configuration, the skills tree, `Cargo.lock`.
    /// Prose OUTSIDE `docs/` is on this side too - the classification is about reachability from
    /// that diff, not about whether a gate reads English. Skipping it on such a diff loses nothing.
    Code,
    /// At least one file such a diff CAN change - so skipping it DEFERS a real verdict, and
    /// which verdict is what the plan's table has to say.
    Prose,
}

impl Reads {
    /// The word the plan's table is keyed by. One spelling, in the type.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Prose => "prose",
        }
    }
}

/// A hygiene gate's own-rule falsifier: the seed that must make ITS substantive rule fire, and
/// the in-scope proviso that stops a missing-input or empty-scan refusal being mistaken for it.
///
/// `github.com/telekom/sutura#371`. The hole is that the shared guard drives `run()` and judges
/// only its exit code, so a gate that flips its real Finding `Fail -> Pass` stays green by
/// refusing on a DIFFERENT arm - an absent input or an empty scan - which #371 can see. The
/// `falsifier` slot makes the compiler ask the question a test used to be left to answer alone:
/// a hygiene gate cannot be registered without declaring what proves its own-rule Fail arm.
/// `crate::falsifier` carries the tree and the sweep that drives it; this is the declaration.
///
/// Non-`Option` by design, and for [`Reads`]'s reason on [`Kind::Hygiene`]: a new gate is forced
/// to answer rather than reminded to. The seed-programme marker [`Falsifier::declared_in_programme`]
/// is where a gate that has not yet written its own-rule seed lands - the sweep still asserts it
/// refuses the shared tree (it fails closed), and the programme replaces it gate by gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Falsifier {
    /// Files to seed into the falsifier tree, each a REAL violation of this one gate's own rule,
    /// as `(repo-relative path, contents)`. Empty for a gate the shared tree already falsifies
    /// on its own rule, and for the seed-programme placeholder.
    pub(crate) seeds: &'static [(&'static str, &'static str)],
    /// A seeded subject this gate's scan MUST reach, checked by the sweep. A refusal on a tree
    /// that does not visibly carry the gate's subject is not the own-rule refusal this slot is
    /// for - it is the absent-input / empty-scan floor #371 names. `None` on the placeholder.
    pub(crate) in_scope: Option<&'static str>,
}

impl Falsifier {
    /// The seed-programme marker. The sweep still asserts the gate refuses the shared tree
    /// (fails closed rather than going unjudged), but its own-rule seed has not been written.
    /// Also the value a `Standalone` task carries, which the hygiene sweep never consults.
    pub(crate) const fn declared_in_programme() -> Self {
        Self {
            seeds: &[],
            in_scope: None,
        }
    }
}

/// A task: the name, the `--help` line, whether it is a hygiene gate, and the code it runs.
///
/// The handler is IN the table, so `--help` and dispatch cannot disagree. They did once - six
/// dispatched tasks were missing from the list, so `--help` lied and `check-guidance` reported
/// every mention of them as a deleted gate. A table plus a separate `match` is two lists.
///
/// `kind` is here for the same reason. The hygiene list used to be hand-transcribed in the
/// justfile, twice in devenv.nix, in flake.nix and as eight separate hooks - and the order
/// differed in three of them. `check-guidance` verifies that a NAMED task exists, so it catches
/// a rename but is blind to an omission: adding a gate and forgetting one of five call sites
/// was invisible. Now there is one list and the callers ask for it by name.
///
/// `falsifier` is here for the reason `Reads` is a payload on `Kind::Hygiene`: `github.com/
/// telekom/sutura#371` - a gate whose own-rule Fail arm nothing proves is a gate whose green
/// says nothing. A non-`Option` slot means a new gate does not compile until it answers what
/// falsifies it. See [`Falsifier`].
pub(crate) struct Task {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: Kind,
    pub(crate) falsifier: Falsifier,
    pub(crate) run: Gate,
}
