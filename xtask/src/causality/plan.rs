//! Which changed file is which: what to revert, what to hold, and what to measure.
//!
//! Split out of `causality.rs` at the unexemptable 1000-line cap, and the seam is the one this gate
//! is built on: everything here decides what a changed FILE is, from its path and its added lines,
//! and nothing here runs anything. `super::prove` acts on the answer.
//!
//! THE PARTITION IS THE VERDICT, which is why it keeps producing defects worth a mechanism rather
//! than a note. Three of them are recorded in the tests below, each as the level a caller sees:
//! an inseparable file reached the proof and was blamed for passing; a `#[cfg(test)]` HELPER was
//! read as a test and refused with a remedy nobody could act on; and a changed page was dropped
//! entirely, so a suite whose implementation is prose could not be proven at all. A fourth, the
//! subject of `a_held_back_implementation_is_reverted_when_a_scoped_test_reaches_it`: an
//! inseparable file that is the IMPLEMENTATION of a separable test in the same package stayed at
//! HEAD while that test was measured, so the test stayed green on a "base" that had never
//! reverted it and the gate blamed the author for a partition it chose. A fifth,
//! `an_edited_assertion_with_no_added_marker_is_a_test_file_not_an_implementation_change`
//! (`github.com/telekom/sutura#1025`): a file whose only added lines sit inside a test that
//! already existed added no marker `adds` reads, so it fell into `impl_only` and, absent any
//! OTHER test file in the diff, `test_files` stayed empty and the whole diff answered
//! `Plan::NotRequired` - *no changed tests* over a diff that changed one. A sixth,
//! `github.com/telekom/sutura#1031`: a pure DELETION of an assertion adds nothing either extractor
//! sees, so it also fell to `Plan::NotRequired`; [`edited::removed_in`] reads the BASE image -
//! threaded in as `plan`'s third argument - and answers `Plan::DeletedTests` instead, because
//! neither run can measure a line this diff took out.

use std::collections::BTreeSet;

use super::attributes::{Adds, adds};
use super::diff::ChangedFile;
use super::edited;
use super::place;
use super::provenance::Reach;
use super::regions::{PostImage, has_non_test_additions, scope};

/// What the gate concluded, so the shape is testable without git or cargo.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// No changed tests: nothing to prove.
    NotRequired,
    /// A REMOVED line sat inside a pre-existing `#[test]` - a deleted assertion, with nothing
    /// added in its place. Neither run measures it (base and head are both green), so the route is
    /// a named refusal, not a proof: the shape names the deleted test and asks the author to state
    /// the evidence. `Verdict::Fail` (`causality::report_deleted_tests`).
    DeletedTests(Vec<String>),
    /// Baseline can be reconstructed by reverting [`Separable::revert`].
    Separable(Separable),
    /// Impl and tests share a file; a human must state the evidence.
    ///
    /// It carries the build inputs for the reason [`Separable::build_inputs`] gives: every arm that
    /// says *this gate did not measure your change* has to be able to name the changed files it
    /// could not revert, and this is one of the three that returns before the proof block.
    NotSeparable { files: Vec<String>, build_inputs: Vec<String> },
}

/// The four groups the reconstruction sorts a diff's Rust files into.
///
/// A struct rather than positional arguments, because two of the four are told apart only by the
/// sentence printed beside them and a reader has to be able to see which is which.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Separable {
    /// Restored to base: nothing they added is test code, so together they ARE the old behaviour
    /// a test has to be red against.
    pub(crate) revert: Vec<String>,
    /// Kept at HEAD and MEASURED: the tests these added are the proof.
    pub(crate) test_files: Vec<String>,
    /// Kept at HEAD with their own tests OUT of the proof: each adds an implementation change and
    /// a test in one file, so reverting it would remove the test along with the fix. Carried
    /// rather than dropped, because reverting the others while these stay at HEAD is what leaves
    /// a tree mixing two versions of one API - the second attempt needs them.
    pub(crate) held_back: Vec<String>,
    /// Kept at HEAD, with nothing in them to measure: everything they added is `#[cfg(test)]`
    /// code that names no test. Reverting one takes a helper the held tests call out of the base
    /// tree, and requiring it to name a test is the refusal [`super::attributes`] records.
    pub(crate) test_only: Vec<String>,
    /// Changed, and NOT reverted: a manifest, a lockfile or a cargo configuration. Reverting one
    /// changes what cargo resolves rather than what the tests measure, and would take a dependency
    /// this branch added away from a test file kept at HEAD. Carried so the output can NAME them -
    /// a diff whose only implementation change is a manifest gets no verdict from this gate, and
    /// [`Reach::BuildInput`] is where that argument lives.
    pub(crate) build_inputs: Vec<String>,
}

impl Separable {
    /// Every file kept at HEAD on the first attempt whose own tests the proof does not measure.
    ///
    /// One list because the second attempt restores them together: that retry exists to put the
    /// tree coherently at base, and a held test helper calling a reverted neighbour is exactly a
    /// thing that does not compile until it goes too.
    pub(super) fn held(&self) -> Vec<String> {
        self.held_back.iter().chain(self.test_only.iter()).cloned().collect()
    }

    /// What the first attempt keeps at HEAD: the held files PLUS the test files.
    ///
    /// Composition rather than recall: the membership's withdrawal decision (`membership::withdrawn`)
    /// asks whether an added crate still has ANY file at HEAD, and a crate whose only held file is a
    /// TEST one counts - so the test files must reach that question or a crate carrying a provable
    /// test would be withdrawn under a tree that then cannot run it.
    pub(super) fn at_head_first_attempt(&self) -> Vec<String> {
        let mut at_head = self.held();
        at_head.extend(self.test_files.iter().cloned());
        at_head
    }

    /// Which build inputs the named attempt does NOT put at base - the held-at-HEAD remainder the
    /// output must name truthfully. Filtering rather than copying the list: a withdrawn manifest
    /// goes to base on the first attempt, and printing it as held at HEAD would be this gate
    /// describing a tree it did not build.
    pub(super) fn unreverted_from(&self, reverting: &[String]) -> Vec<String> {
        self.build_inputs
            .iter()
            .filter(|path| !reverting.contains(path))
            .cloned()
            .collect()
    }
}

/// Split the changed files into "added tests", "the old behaviour", and what may not be touched.
///
/// `read` returns a file's POST-IMAGE by repo-relative path. `base` is the same question over the
/// PRE-image, threaded in for SHAPE A - a REMOVED line exists only there, so only the base can say
/// whether a line this diff deleted sat inside a `#[test]` (`edited::removed_in`).
///
/// **A FILE CARGO DOES NOT COMPILE IS STILL AN IMPLEMENTATION**: reading `.rs` only made a
/// documentation-driven suite unprovable, a pass over a diff whose implementation was prose.
/// [`Reach`] sorts a changed path into what the reconstruction may do with it, so a page, a recipe
/// or a nix file is reverted like any other implementation and a build input is held back and
/// NAMED.
pub(crate) fn plan(files: &[ChangedFile], read: &PostImage<'_>, base: &PostImage<'_>) -> Plan {
    let mut test_files = Vec::new();
    let mut test_only = Vec::new();
    let mut impl_only = Vec::new();
    let mut build_inputs = Vec::new();
    let mut deleted_tests = Vec::new();

    for file in files {
        match Reach::of(&file.path) {
            // Nothing cargo builds reads it, so it holds no test any run can reach and reverting
            // it would change nothing either.
            Reach::Outside => {}
            Reach::BuildInput => build_inputs.push(file.path.clone()),
            // Not compiled, so no added line of it can declare a test - and revertible, so it is
            // the old behaviour a test that READS it has to be red against.
            Reach::Revertible => impl_only.push(file.path.clone()),
            Reach::Compiled => match adds(&file.added, &file.path, read) {
                // A marker names no function, and it stays a candidate for the proof on purpose:
                // that is the shape whose unnameable test is a deliberate refusal.
                Adds::NamedTest | Adds::TestModule => test_files.push(file.path.clone()),
                Adds::TestOnlyItem => test_only.push(file.path.clone()),
                // `github.com/telekom/sutura#1025`: an added line naming no marker may still sit
                // inside a test that already existed - a changed assertion, most often - and
                // `adds` cannot see that, because it reads what a line SAYS, never where it SITS.
                // `edited::touches` asks the position question over the file's PRE-existing
                // `#[test]`s. Kept at HEAD rather than reverted - reverting it would take the edit
                // this diff is about out of the tree the proof measures.
                Adds::Nothing => {
                    if edited::touches(&file.added, &file.path, read) {
                        test_files.push(file.path.clone());
                    } else if edited::deletes_from_test(&file.removed, &file.path, base) {
                        // SHAPE A (`github.com/telekom/sutura#1031`): no added line at all, and a
                        // REMOVED line sat inside a pre-existing `#[test]` - a pure deletion of an
                        // assertion. Neither run measures it, so this is a named refusal, never a
                        // proof: the author has to state the evidence a deletion took out.
                        deleted_tests.push(file.path.clone());
                    } else {
                        impl_only.push(file.path.clone());
                    }
                }
            },
        }
    }

    // SHAPE A refusal takes precedence over every other partition: a deleted assertion is
    // evidence this gate removed from any measurable tree, and nothing another file's provable
    // test can re-add proves what that line used to check. Fail-closed - naming the deleted
    // test rather than guessing it away.
    if !deleted_tests.is_empty() {
        return Plan::DeletedTests(deleted_tests);
    }

    if test_files.is_empty() {
        // A `#[cfg(test)]` helper is not a test, so a diff that added none has nothing to prove -
        // the same answer this gate gives any implementation change with no new test. Calling it a
        // test file instead produced a refusal the author could not act on, because no extractor
        // improvement can read a name off an item that is not a test.
        return Plan::NotRequired;
    }

    // A file that gained BOTH a test and non-test changes cannot be split by reverting whole
    // files. Detect it by asking whether any added line lands outside the file's test regions.
    let inseparable: Vec<String> = files
        .iter()
        .filter(|file| test_files.contains(&file.path) && has_non_test_additions(&file.added, &scope(&file.path, read)))
        .map(|file| file.path.clone())
        .collect();

    // A test in an inseparable file cannot be proven by reverting OTHER files: its own
    // implementation sits in the same file, not reverted precisely because it holds the tests. So
    // those tests are excluded from the proof rather than counted in it. The condition here used
    // to be `!inseparable.is_empty() && impl_only.is_empty()`: with even one separable impl file
    // present it took the Separable path, reverted only that, ran the inseparable tests anyway -
    // and they passed, because nothing they exercise had been reverted - reporting "green against
    // base behaviour" and blaming the author for a partition the gate itself had chosen.
    let provable: Vec<String> = test_files.iter().filter(|p| !inseparable.contains(p)).cloned().collect();

    if provable.is_empty() {
        return Plan::NotSeparable {
            files: inseparable,
            build_inputs,
        };
    }

    // A held-back file can be the implementation of a test this proof MEASURES. It is held back
    // because it mixes an implementation change and its own test in one file - but a SEPARABLE
    // test in the SAME package may exercise that implementation, and the base run reverts only
    // `impl_only`, leaving it at HEAD: such a test stays green on "base" for a partition the gate
    // chose, not a fact about the change. So a held-back file whose package is one the provable
    // tests compile into joins the revert - `place::package` is the scope `reverted::packages`
    // already keys its own excusing logic on. A held-back file in a package NO provable test lives
    // in stays held - the cross-package graph `github.com/telekom/sutura#358` left open.
    let scoped_packages = provable_packages(&provable, read);
    let mut held_back = Vec::new();
    let mut revert = impl_only;
    for path in inseparable {
        if scoped_packages
            .as_ref()
            .is_some_and(|pkgs| place::package(&path, read).is_some_and(|p| pkgs.contains(p.as_str())))
        {
            revert.push(path);
        } else {
            held_back.push(path);
        }
    }

    Plan::Separable(Separable {
        revert,
        test_files: provable,
        held_back,
        test_only,
        build_inputs,
    })
}

/// The packages the provable tests compile into, or `None` if any of them does not resolve.
///
/// The same refusal `reverted::packages` makes, for the same reason: the set is used to move a
/// held-back IMPLEMENTATION into the revert, so a package the resolver cannot name must leave
/// that held-back file held rather than guessed reverted.
fn provable_packages(test_files: &[String], read: &PostImage<'_>) -> Option<BTreeSet<String>> {
    test_files
        .iter()
        .map(|path| place::package(path, read).map(|name| String::from(name.as_str())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Plan, Separable, plan};
    use crate::causality::coverage::{Attributed, Coverage};
    use crate::causality::fixtures::{changed, changed_removing, manifest, tree};
    use crate::causality::scoped::Scan;

    /// A hand-built `Separable` for the two composition cells below. `plan` itself is covered by
    /// the cells above; these exercise what `causality::run` composes OUT of its result, which
    /// until now no cell held.
    fn separable() -> Separable {
        Separable {
            revert: vec![String::from("crates/x/src/a.rs")],
            test_files: vec![String::from("crates/new/tests/it.rs")],
            held_back: vec![String::from("crates/x/src/b.rs")],
            test_only: vec![String::from("crates/x/src/helper.rs")],
            build_inputs: vec![String::from("Cargo.toml"), String::from("crates/new/Cargo.toml")],
        }
    }

    /// The withdrawal question (`membership::withdrawn`) asks whether an added crate still has ANY
    /// file at HEAD - and a crate whose only held file is a TEST one counts, because a provable
    /// test inside the crate keeps the crate in the workspace on both attempts. The composition
    /// therefore must include the test files, or the membership would withdraw a crate under a
    /// tree whose nextest filterset is then empty - a red about this partition, not the change.
    #[test]
    fn the_first_attempt_keeps_the_test_files_at_head_as_well_as_the_held_ones() {
        let at_head = separable().at_head_first_attempt();
        assert!(at_head.contains(&String::from("crates/x/src/b.rs")), "held: {at_head:?}");
        assert!(
            at_head.contains(&String::from("crates/x/src/helper.rs")),
            "test-only: {at_head:?}"
        );
        assert!(
            at_head.contains(&String::from("crates/new/tests/it.rs")),
            "the test file reaches the membership's withdrawal question: {at_head:?}"
        );
    }

    /// A withdrawn manifest goes to base on the first attempt, and printing it as held at HEAD
    /// would be the gate describing a tree it did not build. The filter must therefore really
    /// drop every path the attempt reverts - not echo the build inputs back unchanged.
    #[test]
    fn the_unreverted_names_are_the_build_inputs_the_attempt_actually_keeps() {
        let reverting = vec![String::from("Cargo.toml"), String::from("crates/new/Cargo.toml")];
        assert_eq!(separable().unreverted_from(&reverting), Vec::<String>::new());
        let partial = vec![String::from("Cargo.toml")];
        assert_eq!(
            separable().unreverted_from(&partial),
            vec![String::from("crates/new/Cargo.toml")],
            "only the one still at HEAD is named"
        );
    }

    #[test]
    fn no_changed_tests_means_nothing_to_prove() {
        let files = vec![changed("src/a.rs", 1, &["fn f() {}"])];
        assert_eq!(plan(&files, &tree(&[]), &tree(&[])), Plan::NotRequired);
    }

    #[test]
    fn a_deleted_assertion_is_a_deleted_test_not_not_required() {
        // SHAPE A (`github.com/telekom/sutura#1031`): a REMOVED assertion inside an existing
        // `#[test]`, no added line at all. `adds` reads `Adds::Nothing`, `edited::touches` finds
        // nothing added, so this used to fall to `Plan::NotRequired`. `removed_in` reads the BASE
        // image and names it instead.
        let base_image = "pub fn open_engine() -> u8 {\n    2\n}\n#[cfg(test)]\nmod tests {\n    use super::open_engine;\n    #[test]\n    fn existing() {\n        assert_eq!(open_engine(), 2);\n    }\n}\n";
        let post_image = "pub fn open_engine() -> u8 {\n    2\n}\n#[cfg(test)]\nmod tests {\n    use super::open_engine;\n    #[test]\n    fn existing() {\n    }\n}\n";
        let files = vec![changed_removing(
            "crates/x/src/commands.rs",
            9,
            &[],
            9,
            &["        assert_eq!(open_engine(), 2);"],
        )];
        let read = tree(&[("crates/x/src/commands.rs", post_image)]);
        let base = tree(&[("crates/x/src/commands.rs", base_image)]);
        assert_eq!(
            plan(&files, &read, &base),
            Plan::DeletedTests(vec![String::from("crates/x/src/commands.rs")])
        );
    }

    #[test]
    fn removing_a_comment_or_blank_line_is_not_a_deleted_test() {
        // Negative twin: a deletion of a non-behaviour line stays `Plan::NotRequired`.
        // `removed_in` filters through `carries_no_behaviour`.
        let base_image = "#[test]\nfn t() {\n    // a comment\n    assert!(true);\n}\n";
        let post_image = "#[test]\nfn t() {\n    assert!(true);\n}\n";
        let files = vec![changed_removing("crates/x/src/a.rs", 4, &[], 3, &["    // a comment"])];
        let read = tree(&[("crates/x/src/a.rs", post_image)]);
        let base = tree(&[("crates/x/src/a.rs", base_image)]);
        assert_eq!(plan(&files, &read, &base), Plan::NotRequired);
    }

    #[test]
    fn an_edit_inside_a_called_helper_is_a_test_file_not_an_implementation_change() {
        // SHAPE B (`github.com/telekom/sutura#1031`): the added line is inside a `#[cfg(test)]`
        // HELPER fn, not the `#[test]`-declaring item a test calls. `edited_helper_caller` names
        // the CALLING test, so `touches` routes the file into `test_files`.
        let post_image = "#[cfg(test)]\nmod tests {\n    fn helper() -> u8 { 1 }\n    #[test]\n    fn existing() {\n        assert_eq!(helper(), 1);\n    }\n}\n";
        let files = vec![changed("crates/x/src/commands.rs", 3, &["    fn helper() -> u8 { 2 }"])];
        let read = tree(&[
            ("crates/x/src/commands.rs", post_image),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match &plan(&files, &read, &read) {
            Plan::Separable(one) => {
                assert!(one.revert.is_empty(), "nothing else changed: {:?}", one.revert);
                assert_eq!(one.test_files, vec![String::from("crates/x/src/commands.rs")]);
            }
            other => panic!("expected Separable with the helper file in test_files, got {other:?}"),
        }
    }

    #[test]
    fn an_edit_inside_a_helper_no_test_calls_is_not_a_test_file() {
        // Negative twin: the helper is never reached from a `#[test]`, so the edit has no caller
        // to prove it against - it stays `Plan::NotRequired`.
        let post_image = "#[cfg(test)]\nmod tests {\n    fn unused_helper() -> u8 { 1 }\n    #[test]\n    fn existing() {\n        assert!(true);\n    }\n}\n";
        let files = vec![changed(
            "crates/x/src/commands.rs",
            3,
            &["    fn unused_helper() -> u8 { 2 }"],
        )];
        let read = tree(&[("crates/x/src/commands.rs", post_image)]);
        assert_eq!(plan(&files, &read, &read), Plan::NotRequired);
    }

    #[test]
    fn an_edited_assertion_with_no_added_marker_is_a_test_file_not_an_implementation_change() {
        // THE DEFECT `github.com/telekom/sutura#1025` CLOSES. The only added line is the new
        // assertion INSIDE an existing `#[test]` fn - no `#[test]`, no `mod tests`, no production
        // line anywhere in the diff. `adds` reads `Adds::Nothing` because nothing added SAYS test:
        // before this decision that put the file straight into `impl_only`, and with no other test
        // file in the diff the whole answer was `Plan::NotRequired` - *no changed tests* over a
        // diff whose only change was to one.
        let post_image = concat!(
            "pub fn open_engine() -> u8 {\n",          // 1
            "    2\n",                                 // 2
            "}\n",                                     // 3
            "#[cfg(test)]\n",                          // 4
            "mod tests {\n",                           // 5
            "    use super::open_engine;\n",           // 6
            "    #[test]\n",                           // 7
            "    fn existing() {\n",                   // 8
            "        assert_eq!(open_engine(), 2);\n", // 9
            "    }\n",                                 // 10
            "}\n",                                     // 11
        );
        let files = vec![changed(
            "crates/x/src/commands.rs",
            9,
            &["        assert_eq!(open_engine(), 2);"],
        )];
        let read = tree(&[
            ("crates/x/src/commands.rs", post_image),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert!(one.revert.is_empty(), "nothing else changed: {:?}", one.revert);
                assert_eq!(one.test_files, vec![String::from("crates/x/src/commands.rs")]);
            }
            other => panic!("expected Separable with nothing to revert, got {other:?}"),
        }
    }

    #[test]
    fn separates_impl_from_test_file() {
        let files = vec![
            changed("crates/x/src/a.rs", 2, &["fn fixed() -> u8 { 2 }"]),
            changed("crates/x/tests/t.rs", 1, &["#[test]", "fn t() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/a.rs", "// header\nfn fixed() -> u8 { 2 }\n"),
            ("crates/x/tests/t.rs", "#[test]\nfn t() {}\n"),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/a.rs")]);
                assert_eq!(one.test_files, vec![String::from("crates/x/tests/t.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn one_file_with_both_is_reported_not_guessed() {
        let files = vec![changed(
            "crates/x/src/a.rs",
            1,
            &["fn fixed() -> u8 { 2 }", "#[cfg(test)]", "mod tests {", "    #[test]"],
        )];
        let read = tree(&[(
            "crates/x/src/a.rs",
            "fn fixed() -> u8 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
        )]);
        match plan(&files, &read, &read) {
            Plan::NotSeparable { files, .. } => {
                assert_eq!(files, vec![String::from("crates/x/src/a.rs")]);
            }
            other => panic!("expected NotSeparable, got {other:?}"),
        }
    }

    #[test]
    fn an_unrelated_impl_file_does_not_make_an_inseparable_one_provable() {
        let files = vec![
            changed(
                "xtask/src/workflows.rs",
                1,
                &["fn collect() {}", "#[cfg(test)]", "mod tests {", "    #[test]"],
            ),
            changed("xtask/src/main.rs", 1, &["mod workflows;"]),
        ];
        let read = tree(&[
            (
                "xtask/src/workflows.rs",
                "fn collect() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
            ("xtask/src/main.rs", "mod workflows;\n"),
        ]);
        match plan(&files, &read, &read) {
            Plan::NotSeparable { files, .. } => {
                assert_eq!(files, vec![String::from("xtask/src/workflows.rs")]);
            }
            other => panic!("expected NotSeparable, got {other:?}"),
        }
    }

    #[test]
    fn a_test_only_addition_in_one_file_is_separable() {
        // Only test code added: nothing to revert in that file, so it is not "inseparable".
        let files = vec![
            changed(
                "crates/x/src/a.rs",
                2,
                &["#[cfg(test)]", "mod tests {", "    #[test]", "    fn t() {}", "}"],
            ),
            changed("crates/x/src/b.rs", 1, &["fn fixed() {}"]),
        ];
        let read = tree(&[
            (
                "crates/x/src/a.rs",
                "pub fn existing() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
            ("crates/x/src/b.rs", "fn fixed() {}\n"),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/b.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn tests_appended_to_existing_test_modules_are_not_an_implementation_change() {
        let commands = concat!(
            "pub fn open_engine() -> u8 {\n",             // 1
            "    1\n",                                    // 2
            "}\n",                                        // 3
            "#[cfg(test)]\n",                             // 4
            "mod tests {\n",                              // 5
            "    use super::open_engine;\n",              // 6
            "    #[test]\n",                              // 7
            "    fn existing() {}\n",                     // 8
            "    use sutura_domain::model::TableName;\n", // 9
            "    #[test]\n",                              // 10
            "    fn added() {\n",                         // 11
            "        let _ = TableName::parse(\"t\");\n", // 12
            "    }\n",                                    // 13
            "}\n",                                        // 14
        );
        let serve = concat!(
            "fn main() {}\n",         // 1
            "#[cfg(test)]\n",         // 2
            "mod tests {\n",          // 3
            "    #[test]\n",          // 4
            "    fn existing() {}\n", // 5
            "    #[test]\n",          // 6
            "    fn added() {}\n",    // 7
            "}\n",                    // 8
        );
        let files = vec![
            changed(
                "crates/sutura-cli/src/commands.rs",
                9,
                &[
                    "    use sutura_domain::model::TableName;",
                    "    #[test]",
                    "    fn added() {",
                    "        let _ = TableName::parse(\"t\");",
                    "    }",
                ],
            ),
            changed("crates/sutura-serve/src/main.rs", 6, &["    #[test]", "    fn added() {}"]),
        ];
        let read = tree(&[
            ("crates/sutura-cli/src/commands.rs", commands),
            ("crates/sutura-serve/src/main.rs", serve),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert!(one.revert.is_empty(), "no implementation changed: {:?}", one.revert);
                assert!(
                    one.held_back.is_empty(),
                    "nothing carries an implementation change: {:?}",
                    one.held_back
                );
                assert_eq!(
                    one.test_files.len(),
                    2,
                    "both files are provable test files: {:?}",
                    one.test_files
                );
            }
            other => panic!("expected Separable with nothing to revert, got {other:?}"),
        }
    }

    #[test]
    fn a_non_rust_file_declares_no_test_whatever_it_contains() {
        let files = vec![changed("README.md", 1, &["#[test]"])];
        assert_eq!(plan(&files, &tree(&[]), &tree(&[])), Plan::NotRequired);
    }

    #[test]
    fn a_changed_page_is_the_implementation_of_the_test_that_reads_it() {
        let files = vec![
            changed("crates/x/tests/documented.rs", 1, &["#[test]", "fn the_page_runs() {}"]),
            changed("docs/getting-started.md", 4, &["    sutura ask 'a question'"]),
            changed("justfile", 9, &["    cargo run -q -p xtask -- something"]),
        ];
        let read = tree(&[
            ("crates/x/tests/documented.rs", "#[test]\nfn the_page_runs() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(
                    one.revert,
                    vec![String::from("docs/getting-started.md"), String::from("justfile")],
                    "the page and the recipe ARE the old behaviour"
                );
                assert_eq!(one.test_files, vec![String::from("crates/x/tests/documented.rs")]);
            }
            other => panic!("a changed page is something to revert, got {other:?}"),
        }
    }

    #[test]
    fn a_manifest_only_implementation_change_leaves_nothing_to_revert() {
        let files = vec![
            changed("crates/x/tests/t.rs", 1, &["#[test]", "fn uses_the_new_dependency() {}"]),
            changed("crates/x/Cargo.toml", 9, &["serde = \"1\""]),
        ];
        let read = tree(&[
            ("crates/x/tests/t.rs", "#[test]\nfn uses_the_new_dependency() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert!(one.revert.is_empty(), "a manifest is not revertible: {:?}", one.revert);
                assert_eq!(one.build_inputs, vec![String::from("crates/x/Cargo.toml")]);
            }
            other => panic!("expected Separable with nothing to revert, got {other:?}"),
        }
    }

    #[test]
    fn an_inseparable_diff_carries_its_build_inputs_too() {
        let files = vec![
            changed(
                "crates/x/src/a.rs",
                1,
                &["fn fixed() -> u8 { 2 }", "#[cfg(test)]", "mod tests {", "    #[test]"],
            ),
            changed("Cargo.lock", 40, &["name = \"serde\""]),
        ];
        let read = tree(&[(
            "crates/x/src/a.rs",
            "fn fixed() -> u8 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
        )]);
        assert_eq!(
            plan(&files, &read, &read),
            Plan::NotSeparable {
                files: vec![String::from("crates/x/src/a.rs")],
                build_inputs: vec![String::from("Cargo.lock")],
            }
        );
    }

    #[test]
    fn a_manifest_is_not_reverted_and_is_named_instead() {
        let files = vec![
            changed("crates/x/tests/t.rs", 1, &["#[test]", "fn t() {}"]),
            changed("crates/x/src/a.rs", 1, &["fn fixed() {}"]),
            changed("crates/x/Cargo.toml", 9, &["serde = \"1\""]),
            changed("Cargo.lock", 40, &["name = \"serde\""]),
        ];
        let read = tree(&[
            ("crates/x/tests/t.rs", "#[test]\nfn t() {}\n"),
            ("crates/x/src/a.rs", "fn fixed() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/a.rs")], "no manifest in here");
                assert_eq!(
                    one.build_inputs,
                    vec![String::from("crates/x/Cargo.toml"), String::from("Cargo.lock")]
                );
                assert!(
                    !one.held().iter().any(|f| f.ends_with("Cargo.toml")),
                    "a build input is not held for its own tests either: {:?}",
                    one.held()
                );
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn a_vendored_test_is_not_a_changed_test_this_gate_can_measure() {
        let files = vec![changed(
            "vendor/mimalloc_rust/src/lib.rs",
            1,
            &["    #[test]", "    fn allocates() {}"],
        )];
        assert_eq!(plan(&files, &tree(&[]), &tree(&[])), Plan::NotRequired);
    }

    #[test]
    fn a_file_carrying_its_own_tests_is_held_back_not_forgotten() {
        let files = vec![
            changed(
                "crates/x/src/pinned.rs",
                1,
                &[
                    "fn of(a: u8, b: u8) -> u8 { a }",
                    "#[cfg(test)]",
                    "mod tests {",
                    "    #[test]",
                ],
            ),
            changed("crates/x/src/definitions.rs", 1, &["fn changed() {}"]),
            changed("crates/x/tests/t.rs", 1, &["#[test]", "fn t() {}"]),
        ];
        let read = tree(&[
            (
                "crates/x/src/pinned.rs",
                "fn of(a: u8, b: u8) -> u8 { a }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
            ("crates/x/src/definitions.rs", "fn changed() {}\n"),
            ("crates/x/tests/t.rs", "#[test]\nfn t() {}\n"),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/definitions.rs")]);
                assert_eq!(one.test_files, vec![String::from("crates/x/tests/t.rs")]);
                assert_eq!(one.held_back, vec![String::from("crates/x/src/pinned.rs")]);
                assert!(one.test_only.is_empty(), "no test-only file here: {:?}", one.test_only);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn a_cfg_test_helper_with_no_test_beside_it_adds_no_test() {
        let tasks = concat!(
            "pub(crate) fn recipe_names() -> u8 {\n",             // 1
            "    1\n",                                            // 2
            "}\n",                                                // 3
            "#[cfg(test)]\n",                                     // 4
            "pub(crate) fn recipe_body(name: &str) -> usize {\n", // 5
            "    name.len()\n",                                   // 6
            "}\n",                                                // 7
        );
        let files = vec![
            changed(
                "xtask/src/tasks.rs",
                4,
                &[
                    "#[cfg(test)]",
                    "pub(crate) fn recipe_body(name: &str) -> usize {",
                    "    name.len()",
                    "}",
                ],
            ),
            changed("xtask/src/other.rs", 1, &["fn changed() {}"]),
        ];
        let read = tree(&[
            ("xtask/src/tasks.rs", tasks),
            ("xtask/src/other.rs", "fn changed() {}\n"),
            ("xtask/Cargo.toml", &manifest("xtask")),
        ]);
        assert_eq!(plan(&files, &read, &read), Plan::NotRequired);
    }

    #[test]
    fn a_cfg_test_helper_is_held_at_head_rather_than_reverted() {
        let files = vec![
            changed("xtask/src/tasks.rs", 2, &["#[cfg(test)]", "fn helper() -> u8 { 1 }"]),
            changed("xtask/src/other.rs", 1, &["fn changed() {}"]),
            changed("xtask/tests/t.rs", 1, &["#[test]", "fn t() {}"]),
        ];
        let read = tree(&[
            ("xtask/src/tasks.rs", "fn kept() {}\n#[cfg(test)]\nfn helper() -> u8 { 1 }\n"),
            ("xtask/src/other.rs", "fn changed() {}\n"),
            ("xtask/tests/t.rs", "#[test]\nfn t() {}\n"),
            ("xtask/Cargo.toml", &manifest("xtask")),
        ]);
        let helper = String::from("xtask/src/tasks.rs");
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.test_only, vec![helper.clone()]);
                assert_eq!(one.revert, vec![String::from("xtask/src/other.rs")]);
                assert_eq!(one.test_files, vec![String::from("xtask/tests/t.rs")]);
                assert!(one.held().contains(&helper), "kept at HEAD: {:?}", one.held());
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn a_cfg_test_helper_no_longer_decides_between_a_refusal_and_a_silent_pass() {
        let inseparable_file = concat!(
            "fn fixed() -> u8 { 2 }\n", // 1
            "#[cfg(test)]\n",           // 2
            "mod tests {\n",            // 3
            "    #[test]\n",            // 4
            "    fn t() {}\n",          // 5
            "}\n",                      // 6
        );
        let added = |texts: &[&str]| changed("crates/x/src/a.rs", 1, texts);
        let hunk = [
            "fn fixed() -> u8 { 2 }",
            "#[cfg(test)]",
            "mod tests {",
            "    #[test]",
            "    fn t() {}",
            "}",
        ];
        let read = tree(&[
            ("crates/x/src/a.rs", inseparable_file),
            (
                "crates/x/src/helper.rs",
                "fn kept() {}\n#[cfg(test)]\nfn helper() -> u8 { 1 }\n",
            ),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let inseparable = vec![String::from("crates/x/src/a.rs")];

        let with_helper = vec![
            added(&hunk),
            changed("crates/x/src/helper.rs", 2, &["#[cfg(test)]", "fn helper() -> u8 { 1 }"]),
        ];
        assert_eq!(
            plan(&with_helper, &read, &read),
            Plan::NotSeparable {
                files: inseparable.clone(),
                build_inputs: Vec::new(),
            }
        );
        // The pass carries its own limit rather than reading as a verdict about the change.
        assert_eq!(
            Coverage::of(&[], &with_helper, &read).measured(Attributed::Nothing),
            "0 of 1 added tests measured"
        );
        // And the answer does not depend on the helper being there, which is the property that
        // was missing: the same diff without it plans identically.
        assert_eq!(
            plan(&[added(&hunk)], &read, &read),
            Plan::NotSeparable {
                files: inseparable,
                build_inputs: Vec::new(),
            }
        );
    }

    #[test]
    fn a_test_module_declaration_still_refuses_when_it_names_no_test() {
        let files = vec![
            changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]", "mod tests;"]),
            changed("crates/x/src/other.rs", 1, &["fn changed() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
            ("crates/x/src/other.rs", "fn changed() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let planned = plan(&files, &read, &read);
        let Plan::Separable(ref one) = planned else {
            panic!("a test module declaration is a test file, got {planned:?}");
        };
        assert_eq!(one.test_files, vec![String::from("crates/x/src/lib.rs")]);
        match Scan::of(&files, &one.test_files, &read) {
            Scan::Enabled(ref refused) => {
                assert_eq!(refused.len(), 1);
                assert_eq!(
                    refused.first().map(|only| only.module.as_str()),
                    Some("crates/x/src/tests.rs")
                );
            }
            other => panic!("a declaration this diff cannot account for refuses, got {other:?}"),
        }
        // And with the module's own file in the diff it is `Unnamed` instead: the declaration is
        // accounted for, and no added line named a test anywhere.
        let mut accounted = files;
        accounted.push(changed("crates/x/src/tests.rs", 1, &["use super::f;"]));
        let read = tree(&[
            ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
            ("crates/x/src/other.rs", "fn changed() {}\n"),
            ("crates/x/src/tests.rs", "use super::f;\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        assert!(matches!(Scan::of(&accounted, &one.test_files, &read), Scan::Unnamed));
    }

    #[test]
    fn a_held_back_implementation_is_reverted_when_a_scoped_test_reaches_it() {
        let files = vec![
            changed(
                "xtask/src/default_features.rs",
                1,
                &[
                    "fn feature_preflight() -> bool { false }",
                    "#[cfg(test)]",
                    "mod tests {",
                    "    #[test]",
                ],
            ),
            changed("xtask/src/shipped.rs", 1, &["fn collect() {}"]),
            changed(
                "xtask/tests/default_features.rs",
                1,
                &["#[test]", "fn a_forbidden_feature_at_the_last_root_target() {}"],
            ),
        ];
        let read = tree(&[
            (
                "xtask/src/default_features.rs",
                "fn feature_preflight() -> bool { false }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
            ("xtask/src/shipped.rs", "fn collect() {}\n"),
            (
                "xtask/tests/default_features.rs",
                "#[test]\nfn a_forbidden_feature_at_the_last_root_target() {}\n",
            ),
            ("xtask/Cargo.toml", &manifest("xtask")),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                assert!(
                    one.revert.iter().any(|f| f == "xtask/src/default_features.rs"),
                    "the held-back implementation joins the revert: {:?}",
                    one.revert
                );
                assert!(
                    one.revert.iter().any(|f| f == "xtask/src/shipped.rs"),
                    "the separable impl is still reverted: {:?}",
                    one.revert
                );
                assert!(
                    !one.held_back.iter().any(|f| f == "xtask/src/default_features.rs"),
                    "moved out of held_back: {:?}",
                    one.held_back
                );
                assert_eq!(one.test_files, vec![String::from("xtask/tests/default_features.rs")]);
            }
            other => panic!("a scoped test reaching its held-back implementation is Separable, got {other:?}"),
        }
    }

    #[test]
    fn an_impl_with_tests_in_a_package_no_scoped_test_lives_in_stays_held() {
        let files = vec![
            changed(
                "crates/y/src/pinned.rs",
                1,
                &["fn of(a: u8) -> u8 { a }", "#[cfg(test)]", "mod tests {", "    #[test]"],
            ),
            changed("crates/x/src/definitions.rs", 1, &["fn changed() {}"]),
            changed("crates/x/tests/t.rs", 1, &["#[test]", "fn t() {}"]),
        ];
        let read = tree(&[
            (
                "crates/y/src/pinned.rs",
                "fn of(a: u8) -> u8 { a }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
            ("crates/y/Cargo.toml", &manifest("y")),
            ("crates/x/src/definitions.rs", "fn changed() {}\n"),
            ("crates/x/tests/t.rs", "#[test]\nfn t() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match plan(&files, &read, &read) {
            Plan::Separable(ref one) => {
                let pinned = String::from("crates/y/src/pinned.rs");
                assert_eq!(one.held_back, vec![pinned.clone()], "unrelated held file stays held");
                assert!(!one.revert.contains(&pinned), "not reverted: {:?}", one.revert);
                assert_eq!(one.revert, vec![String::from("crates/x/src/definitions.rs")]);
                assert_eq!(one.test_files, vec![String::from("crates/x/tests/t.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }
}
