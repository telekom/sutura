//! A changed file that a file kept at HEAD still declares as a module stays at HEAD with it.
//!
//! `github.com/telekom/sutura#1103`: reverting a file the branch ADDED deletes it, so a kept
//! file's `mod x;` or `#[path = ".."] mod x;` then names nothing and the base does not build - a
//! split that moves helpers into a new module with no test of its own left every moved test
//! unmeasured. Such a file is taken out of the revert instead, transitively, with the declaration
//! resolved by `place` as every other `#[path]` question here is.
//!
//! **An existing file too, when it is test code under a test file.** The same split edits a
//! sibling test module's `use` lines to follow the helpers it moved. Restoring that sibling puts
//! base imports under a HEAD parent that no longer has them, and the base fails to build the same
//! way. So a file whose every line is test code (`regions::TestScope::WholeFile`), declared by a
//! file that stays at HEAD on BOTH attempts, stays with it. Nothing else that existed at base is
//! kept: a `src/` module a test file declares may be the implementation the proof reverts.
//!
//! **Why HEAD is safe on both attempts.** A kept added file enters a build only through a
//! declaration; the file that declares it is at HEAD, or on the retry is reverted to a base that
//! never declared a file which did not exist. So it is either compiled beside its declarer or not
//! compiled at all. A kept existing file is only ever reached from a declarer the retry keeps too.
//!
//! **What this does NOT cover.** The walk evaluates no `cfg` and skips no comment, as
//! `place::declared_route` does not: a commented-out declaration still keeps its file. A kept file
//! that is ALSO the implementation a reverted file declares stays at HEAD too, which is the
//! fail-closed direction - its tests go green on base and the gate refuses.

use super::diff::ChangedFile;
use super::is_compiled_rust;
use super::place::{declared_module_files, relocated};
use super::plan::{Plan, Separable};
use super::regions::{PostImage, TestScope, module_name, scope};

/// `plan` with every file [`keep`]'s rule holds at HEAD moved out of the revert.
///
/// Each pending declarer carries whether it stays at HEAD on the retry as well: the test files do,
/// the held files do not, and a kept file inherits its declarer's answer.
pub(super) fn keep(plan: Plan, read: &PostImage<'_>, base: &PostImage<'_>) -> Plan {
    let Plan::Separable(mut separable) = plan else {
        return plan;
    };
    let mut pending: Vec<(String, bool)> = separable.held().into_iter().map(|path| (path, false)).collect();
    pending.extend(separable.test_files.iter().map(|path| (path.clone(), true)));
    while let Some((path, always)) = pending.pop() {
        let Some(text) = read(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            let Some(name) = module_name(line) else {
                continue;
            };
            for candidate in declared_module_files(&path, name, relocated(&lines, at).as_deref()) {
                if let Some(index) = separable.revert.iter().position(|f| *f == candidate)
                    && (base(&candidate).is_none() || (always && scope(&candidate, read) == TestScope::WholeFile))
                {
                    pending.push((separable.revert.remove(index), always));
                }
            }
        }
    }
    Plan::Separable(separable)
}

/// Name each file [`keep`] took out of the revert.
///
/// Read as the complement: `plan::partition` puts every compiled file in `revert`, `test_files`,
/// `held_back` or `test_only`, so a compiled file in none of them is one [`keep`] moved.
pub(super) fn report_kept(files: &[ChangedFile], separable: &Separable) {
    for file in files.iter().filter(|f| is_compiled_rust(&f.path)) {
        let path = &file.path;
        let listed = [
            &separable.revert,
            &separable.test_files,
            &separable.held_back,
            &separable.test_only,
        ];
        if !listed.iter().any(|list| list.contains(path)) {
            println!("  kept:      {path}  (declared by a file kept at HEAD)");
        }
    }
}
