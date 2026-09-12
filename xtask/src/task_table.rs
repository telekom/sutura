//! The task table: what `cargo xtask <task>` runs, and what the `hygiene` sweep collects.
//!
//! **Split by AREA, one module per subject, composed here by [`tasks`] into one sequence.** Every
//! row reaches both `--help` and dispatch through that one accessor, so there is no second list
//! to disagree with the first. That failure is why the handler is in the table at all: six
//! dispatched tasks were once missing from a separate list, so `--help` lied and
//! `check-guidance` read every mention of them as a deleted gate.
//!
//! **Moved out of `main.rs` because that file reached 992 of the 1000-line cap `max-lines` holds
//! over `xtask/`, which no `devco/max-lines-ignore` entry can exempt** -
//! `github.com/telekom/sutura#610`. Measured on the base of that move: a BARE registration - a
//! `mod` line plus the seven-line entry - reached exactly 1000 and PASSED with nothing to spare
//! (`lines <= max` is the predicate), and one line more, the *why it is here* comment most rows
//! carry, gave `max-lines: FAILED`. So the cap refused this table's own convention, which is a
//! WALL rather than a forcing function: it does not push a new rule toward an arm on a gate that
//! already walks the right files - usually the better shape - it removes the choice between them.
//!
//! **The cap never blocked a registration, and that correction outranks the count.** From 992 a
//! bare registration passed. Neither change `#610` cites as redirected blames the cap in its own
//! record: one wrote that its arm *"needs no task registration"* and named the lane holding
//! `TASKS`, the other argues from its seam and mentions no cap. Three files cited it while
//! transcribing 999 against a tree measuring 992, each also giving a sufficient reason of its
//! own. A measurement copied into prose rots; `just hygiene` is the only current answer.
//!
//! **`registry.rs` recorded the opposite decision** - the types moved and the table stayed,
//! because *"moving it would put a rename in the path of every concurrent branch"*. That cost is
//! paid by branches that register a gate, and it measured zero: no open pull request touched
//! `main.rs`, and no linked checkout held an uncommitted edit to it.
//!
//! # What this does not hold
//!
//! Nothing here reports how close any file is to refusing, so the next near-cap file is as
//! invisible as this one was - and `main.rs` was never the tightest. Counted on the base of this
//! change, EIGHT files under `crates/` or `xtask/` were longer than its 992, one of them at
//! exactly 1000 with nothing to spare. A rank is the wrong shape for that - it moves with the
//! scope you measure - so this records the count at a fixed commit and leaves the live answer to
//! `just hygiene`, which is the only thing that cannot go stale.
//!
//! It does not make the table cheaper to merge: two sessions adding a gate to the same area still
//! collide, one file further down. The split narrows that to an area rather than removing it.
//!
//! A registration costs two files, where the crate root cost one: the `mod` line in `main.rs`,
//! and the row plus its handler import in the area module. Measured while falsifying the move - a
//! probe row's `run:` target did not resolve until the module was named in that `use crate::{..}`.

mod architecture;
mod ci;
mod deps;
mod dev;
mod files;
mod prose;
mod release;

use crate::registry::Task;

/// The areas, in `--help` order, and the ONE place they are enumerated.
///
/// An area module left out of this list does not silently drop its rows: its `TASKS` becomes
/// unreachable and `-D dead_code` refuses the build. That is the mechanism `crate::conformance`
/// measured when a gate's own entry was deleted - 61 errors, not a green run.
///
/// Falsified here rather than assumed: deleting the `dev::TASKS` line gave exit 101 and, by
/// rustc's own tally, `due to 301 previous errors` - the area carried the `hygiene` row, so its
/// removal orphaned everything reachable only through it.
const AREAS: &[&[Task]] = &[
    architecture::TASKS,
    files::TASKS,
    deps::TASKS,
    prose::TASKS,
    ci::TASKS,
    release::TASKS,
    dev::TASKS,
];

/// Every task, in `--help` order.
///
/// A flattening iterator rather than a concatenated `const`: building one array needs indexing,
/// and `clippy::indexing_slicing` is denied here - it rejected the const form with 8 errors even
/// though `cargo build` accepted it, which is the reason `just lint` is the gate and not a build.
pub(crate) fn tasks() -> impl Iterator<Item = &'static Task> {
    AREAS.iter().copied().flatten()
}
