//! An added file that a file kept at HEAD still declares as a module stays at HEAD.
//!
//! `github.com/telekom/sutura#1103`: reverting a file the branch ADDED deletes it, so a kept
//! file's `mod x;` or `#[path = ".."] mod x;` then names nothing and the base does not build - a
//! split that moves helpers into a new module with no test of its own left every moved test
//! unmeasured. Such a file is taken out of the revert instead, transitively, with the declaration
//! resolved by `place` as every other `#[path]` question here is.
//!
//! **Why HEAD is safe on both attempts.** A kept added file enters a build only through a
//! declaration; the file that declares it is at HEAD, or on the retry is reverted to a base that
//! never declared a file which did not exist. So it is either compiled beside its declarer or not
//! compiled at all, and keeping it never breaks a build removal would have fixed.
//!
//! **What this does NOT cover.** The walk evaluates no `cfg` and skips no comment, as
//! `place::declared_route` does not: a commented-out declaration still keeps its file, which
//! leaves an uncompiled file at HEAD. An added file that is ALSO the implementation a reverted file
//! declares stays at HEAD too, which is the fail-closed direction - its tests go green on base and
//! the gate refuses. A file that existed at base is always reverted: its base version resolves the
//! declaration.

use super::diff::ChangedFile;
use super::is_compiled_rust;
use super::place::{declared_module_files, relocated};
use super::plan::{Plan, Separable};
use super::regions::{PostImage, module_name};

/// `plan` with every added file that a kept file declares moved out of the revert.
pub(super) fn keep(plan: Plan, read: &PostImage<'_>, base: &PostImage<'_>) -> Plan {
    let Plan::Separable(mut separable) = plan else {
        return plan;
    };
    let mut pending = separable.at_head_first_attempt();
    while let Some(path) = pending.pop() {
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
                    && base(&candidate).is_none()
                {
                    pending.push(separable.revert.remove(index));
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
            println!("  kept:      {path}  (added in this branch, declared by a file kept at HEAD)");
        }
    }
}
