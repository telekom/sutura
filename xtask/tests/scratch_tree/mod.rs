//! A throwaway tree for a gate test, swept when the binding drops.
//!
//! **One copy rather than one per gate test module.** The shape - `temp_dir().join(format!(..
//! process::id()))`, `create_dir_all`, write the fixture, `remove_dir_all` - is written in roughly
//! thirty test modules in this tree already, and the migration in
//! `github.com/telekom/sutura#619` needed three more of it *plus* a `chmod` and a restore. A copy
//! that has to restore a mode before `remove_dir_all` is a copy that leaks a mode-`000` directory
//! into the system temp directory whenever an assert fires first, so the restore belongs in a
//! `Drop` that runs either way.
//!
//! **The copy that keys the path on the process id AND creates it exclusively is the one that
//! fails, and it fails LATER than the run that wrote it** - `github.com/telekom/sutura#938`. A pid
//! is unique among live processes and not across runs, so a killed or panicking run leaves the
//! directory and the next run with that pid recycled panics in SETUP:
//! `new fixture directory: Os { code: 17, kind: AlreadyExists }`, before asserting anything. Worst
//! of all it surfaces under `just gates` and not `just validate`, because the leg that runs those
//! cells (`check-default-feature-tests`) is one of the two `just validate` does not run - so it is
//! invisible to CI and blames whoever gates locally. `Tree::of` sweeps before it creates, which
//! keeps the exclusivity that pattern wanted, and `Drop` sweeps on the way out so a killed run
//! poisons nothing. What holds it: `clippy::create_dir` is `restriction`, so `just lint` refuses a
//! bare `std::fs::create_dir` anywhere in the workspace - test code included - and after #938 no
//! `#[expect]` for it is left. THE LIMIT: an author can still write the allowance back, so the
//! lint refuses the silent copy rather than a deliberate one.
//!
//! **jscpd would not have reported the next copy**: its threshold is 30 lines / 250 tokens and
//! each copy is around fifteen. That is the signal's stated limit rather than a reason to lower it.
//! Nothing here weakens a gate - this module is `#[cfg(test)]` and ships in no binary.
//!
//! **The home is `xtask/tests/scratch_tree/`, not `xtask/src/`.** It is test-support: the in-crate
//! unit tests reach it through `#[cfg(test)] #[path = "../tests/scratch_tree/mod.rs"]` on
//! `main.rs`, and each integration test includes the same file with `#[path = "scratch_tree/mod.rs"]`.
//! That spelling is not taste - `xtask test-causality` refuses a declared module whose own file is
//! not in the diff, and a file beside the tests it serves is in the diff whenever an include of it
//! is, so the declaration and the file always move together.

// The definitions live in [`tree`] - `clippy::definition_in_module_root` refuses a type, an impl
// or an alias in the `mod.rs` the include points at, so this file is glue and `tree.rs` is content.
mod tree;

pub(crate) use tree::Tree;
