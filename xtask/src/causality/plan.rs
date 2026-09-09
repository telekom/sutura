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
//! reverted it and the gate blamed the author for a partition it chose.

use std::collections::BTreeSet;

use super::attributes::{Adds, adds};
use super::diff::ChangedFile;
use super::place;
use super::provenance::Reach;
use super::regions::{PostImage, has_non_test_additions, scope};

/// What the gate concluded, so the shape is testable without git or cargo.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// No changed tests: nothing to prove.
    NotRequired,
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
    /// tree, and requiring it to name a test is the refusal [`attributes`] records.
    pub(crate) test_only: Vec<String>,
    /// Changed, and NOT reverted: a manifest, a lockfile or a cargo configuration. Reverting one
    /// changes what cargo resolves rather than what the tests measure, and would take a dependency
    /// this branch added away from a test file kept at HEAD. Carried so the output can NAME them -
    /// a diff whose only implementation change is a manifest gets no verdict from this gate, and
    /// [`provenance::Reach::BuildInput`] is where that argument lives.
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
}

/// Split the changed files into "added tests", "the old behaviour", and what may not be touched.
///
/// `read` returns a file's POST-IMAGE by repo-relative path - the working tree in the gate, a
/// fixed map in the tests. It is what makes the test-region question answerable at all.
///
/// **A FILE CARGO DOES NOT COMPILE IS STILL AN IMPLEMENTATION**, and reading `.rs` only is what
/// made a documentation-driven suite unprovable: the pages a test asserts on stayed at HEAD, the
/// test was green against "base", and the gate answered *tests changed but no implementation did* -
/// a pass, over a diff whose implementation was prose. [`provenance::Reach`] sorts a changed path
/// into what the reconstruction may do with it, so a page, a recipe or a nix file is reverted like
/// any other implementation and a build input is held back and NAMED.
pub(crate) fn plan(files: &[ChangedFile], read: &PostImage<'_>) -> Plan {
    let mut test_files = Vec::new();
    let mut test_only = Vec::new();
    let mut impl_only = Vec::new();
    let mut build_inputs = Vec::new();

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
                Adds::Nothing => impl_only.push(file.path.clone()),
            },
        }
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
    // implementation sits in the same file, and that file is not reverted precisely because it
    // holds the tests. So those tests are excluded from the proof rather than counted in it.
    //
    // The condition here was `!inseparable.is_empty() && impl_only.is_empty()`, which was wrong
    // in a way that made the gate lie. With even one separable impl file present it took the
    // Separable path, reverted that file, ran the inseparable tests anyway - and they passed,
    // because nothing they exercise had been reverted. It then reported "green against base
    // behaviour" and blamed the author for a partition the gate itself had chosen. Adding two
    // new gate modules, each impl and tests in one new file, alongside an edit to `main.rs` is
    // exactly that shape.
    let provable: Vec<String> = test_files.iter().filter(|p| !inseparable.contains(p)).cloned().collect();

    if provable.is_empty() {
        return Plan::NotSeparable {
            files: inseparable,
            build_inputs,
        };
    }

    // A held-back file can be the implementation of a test this proof MEASURES. A file is held
    // back because it gained an implementation change and a test in one file - so reverting it
    // would remove its own tests along with the fix, and its own tests are excluded from the
    // proof on that account. But a SEPARABLE test in the SAME package, added in its own file, may
    // exercise that very implementation: the proof's base run reverts only `impl_only`, leaving
    // the held-back file at HEAD, so such a test stays green on "base" and the gate blames the
    // author for a partition it chose. That is [`reverted`]'s own sentence - a green base run is
    // first a statement about WHICH files were put back - applied to the classes it already calls
    // excusable.
    //
    // So a held-back file whose package is one the provable tests compile into joins the revert:
    // its absence is then genuinely measured, and a test that stays green without it is a genuine
    // tautology rather than an artefact of the partition. Reverting it is sound where the same-
    // package relationship holds because `place::package` is the existing scope the run's own
    // excusing logic keys on (`reverted::packages`). A held-back file in a package NO provable
    // test lives in stays held - reaching it would require the cross-package dependency graph
    // `github.com/telekom/sutura#358` left open.
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
    use super::{Plan, plan};
    use crate::causality::coverage::{Attributed, Coverage};
    use crate::causality::fixtures::{changed, manifest, tree};
    use crate::causality::scoped::Scan;

    #[test]
    fn no_changed_tests_means_nothing_to_prove() {
        let files = vec![changed("src/a.rs", 1, &["fn f() {}"])];
        assert_eq!(plan(&files, &tree(&[])), Plan::NotRequired);
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
        match plan(&files, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/a.rs")]);
                assert_eq!(one.test_files, vec![String::from("crates/x/tests/t.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn one_file_with_both_is_reported_not_guessed() {
        // The honest case: a fix and its test in one file cannot be split by reverting the
        // file, so the gate must say so rather than pass or fail arbitrarily. Line 1 is the fix
        // and sits outside the test module that lines 2..6 hold.
        let files = vec![changed(
            "crates/x/src/a.rs",
            1,
            &["fn fixed() -> u8 { 2 }", "#[cfg(test)]", "mod tests {", "    #[test]"],
        )];
        let read = tree(&[(
            "crates/x/src/a.rs",
            "fn fixed() -> u8 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
        )]);
        match plan(&files, &read) {
            Plan::NotSeparable { files, .. } => {
                assert_eq!(files, vec![String::from("crates/x/src/a.rs")]);
            }
            other => panic!("expected NotSeparable, got {other:?}"),
        }
    }

    #[test]
    fn an_unrelated_impl_file_does_not_make_an_inseparable_one_provable() {
        // The bug this replaced: a change that adds a new module (impl and tests in one file)
        // AND edits an unrelated impl file took the Separable path, reverted only the unrelated
        // file, then failed the author because the new module's tests still passed - which they
        // could not help doing, since their own implementation was never reverted.
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
        match plan(&files, &read) {
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
        match plan(&files, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/b.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn tests_appended_to_existing_test_modules_are_not_an_implementation_change() {
        // THE DEFECT, at the level a caller sees. Two files gain tests INSIDE `#[cfg(test)] mod
        // tests` blocks that already existed, and no production line is touched. The markers are
        // unchanged diff context, so they never appear among the added lines - and the old
        // classifier, which read only those, called both files "changes behaviour and adds tests
        // in one file" and sent the author to the stated-evidence path. The correct answer is a
        // Separable plan with NOTHING to revert, which is `run`'s own tests-only branch: a
        // different message asking for a different thing.
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
        match plan(&files, &read) {
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
        // A page is not compiled, so no line of it can declare a test - a prose page whose line
        // begins `#[test]` was scanned as a test file once, and three pages under
        // `.agents/skills/` carry such a line. It is still the IMPLEMENTATION of any test that
        // reads it; the case below is that half.
        let files = vec![changed("README.md", 1, &["#[test]"])];
        assert_eq!(plan(&files, &tree(&[])), Plan::NotRequired);
    }

    #[test]
    fn a_changed_page_is_the_implementation_of_the_test_that_reads_it() {
        // THE DEFECT. A test whose subject is a markdown page - it runs the commands the page
        // prints and holds the output - had NOTHING this gate could revert: every `.rs` file was
        // classified and every other path dropped, so the test was green against "base" and the
        // gate reported *tests changed but no implementation did*. A pass, over a suite whose
        // implementation is prose, and indistinguishable from a genuinely tests-only branch.
        //
        // Reverting a file cargo never reads cannot break the build, and it is exactly what makes
        // such a test red on base. The same arm covers a justfile recipe and a nix file, which two
        // of this repository's own gates assert on.
        let files = vec![
            changed("crates/x/tests/documented.rs", 1, &["#[test]", "fn the_page_runs() {}"]),
            changed("docs/getting-started.md", 4, &["    sutura ask 'a question'"]),
            changed("justfile", 9, &["    cargo run -q -p xtask -- something"]),
        ];
        let read = tree(&[
            ("crates/x/tests/documented.rs", "#[test]\nfn the_page_runs() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match plan(&files, &read) {
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
        // THE SHAPE #343 IS ABOUT, and the one the first version of this PR could not name: a new
        // dependency in `Cargo.toml` plus the test that uses it, and NO other implementation file.
        // `impl_only` is empty, so `run` takes the *tests changed but no implementation did* arm -
        // which returns before the proof block, where the only `not reverted:` line used to be
        // printed. `plan` has to carry the manifest for that arm to be able to name it, and
        // `remedies::a_manifest_only_change_is_named_on_the_arm_it_lands_on` is the other half.
        let files = vec![
            changed("crates/x/tests/t.rs", 1, &["#[test]", "fn uses_the_new_dependency() {}"]),
            changed("crates/x/Cargo.toml", 9, &["serde = \"1\""]),
        ];
        let read = tree(&[
            ("crates/x/tests/t.rs", "#[test]\nfn uses_the_new_dependency() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match plan(&files, &read) {
            Plan::Separable(ref one) => {
                assert!(one.revert.is_empty(), "a manifest is not revertible: {:?}", one.revert);
                assert_eq!(one.build_inputs, vec![String::from("crates/x/Cargo.toml")]);
            }
            other => panic!("expected Separable with nothing to revert, got {other:?}"),
        }
    }

    #[test]
    fn an_inseparable_diff_carries_its_build_inputs_too() {
        // The third arm that returns before the proof block. One file with an implementation change
        // and its tests, plus a manifest: `NOT MECHANICALLY SEPARABLE` is a PASS, and a pass that
        // does not name the changed file it could not revert is the same defect one arm over.
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
            plan(&files, &read),
            Plan::NotSeparable {
                files: vec![String::from("crates/x/src/a.rs")],
                build_inputs: vec![String::from("Cargo.lock")],
            }
        );
    }

    #[test]
    fn a_manifest_is_not_reverted_and_is_named_instead() {
        // The other half, and the reason the class is not just "everything that is not Rust".
        // Reverting a manifest changes what cargo RESOLVES: a dependency this branch added would
        // be gone from the base tree while the test file needing it is held at HEAD, so the tree
        // stops compiling and the answer is INCONCLUSIVE about a change with nothing wrong in it.
        // They are held at HEAD and NAMED - which is also the honest statement of #343, where a
        // manifest-only diff can enable a whole test module and nothing here reads a feature table.
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
        match plan(&files, &read) {
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
        // `vendor/mimalloc_rust` is `exclude`d from the root manifest and carries its own
        // `#[test]`s, so `--workspace` never builds it. Counted as a changed test, its package
        // reaches the filterset - and nextest REFUSES an unknown `package(=..)` rather than
        // matching nothing, so a vendor bump touching a `#[test]` line would redden a correct
        // change. `changes::is_non_member` is the same rule the compile-check gate uses.
        let files = vec![changed(
            "vendor/mimalloc_rust/src/lib.rs",
            1,
            &["    #[test]", "    fn allocates() {}"],
        )];
        assert_eq!(plan(&files, &tree(&[])), Plan::NotRequired);
    }

    #[test]
    fn a_file_carrying_its_own_tests_is_held_back_not_forgotten() {
        // The shape that used to end as INCONCLUSIVE in CI: one file holding an implementation
        // change AND its tests, one impl-only file to revert, and one dedicated test target to
        // prove. The plan is Separable - there is something to prove - and the file it cannot
        // prove is NAMED rather than dropped, because reconstructing a tree that compiles needs
        // it. Dropping it is what left base and HEAD versions of one API in the same tree.
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
        match plan(&files, &read) {
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
        // THE DEFECT, at the level `run` branches on. A file gains a `#[cfg(test)]` HELPER and no
        // `#[test]` anywhere in its hunk. The old classifier called it a test file on the bare
        // attribute, put it in the proof, then refused the whole diff with *the added tests could
        // not be NAMED* - which no author could act on, because the item can never name a test. A
        // helper is not a test, so there is nothing to prove: the answer this gate already gives
        // any implementation change that adds no test.
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
        assert_eq!(plan(&files, &read), Plan::NotRequired);
    }

    #[test]
    fn a_cfg_test_helper_is_held_at_head_rather_than_reverted() {
        // The direction that matters more than the refusal it replaces, and the one a narrower
        // `adds_test` would have lost. The helper is never in `revert`: reverting it takes it out
        // of the base tree while the tests calling it are held there - `E0425`, `DidNotCompile`,
        // then a pass that proves nothing, over the COMMON case, since a new helper usually
        // exists because a new test needed it.
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
        match plan(&files, &read) {
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
        // THE EDGE, which was sharper than the failure it caused. With the helper counted as a
        // test file the diff had one "provable" file that could name nothing and the gate FAILED;
        // without the helper the only remaining test file is inseparable, `provable` is empty and
        // the gate PASSED. One `#[cfg(test)]` attribute was the whole difference between a hard
        // refusal and a silent pass, which is why stopping the failure alone would have traded a
        // loud wrong answer for a quiet one. It moves no partition now, and the pass that remains
        // states what it did not measure.
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
            plan(&with_helper, &read),
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
            plan(&[added(&hunk)], &read),
            Plan::NotSeparable {
                files: inseparable,
                build_inputs: Vec::new(),
            }
        );
    }

    #[test]
    fn a_test_module_declaration_still_refuses_when_it_names_no_test() {
        // The arm that must STILL fire, and the reason the split is on `mod` rather than on
        // `#[cfg(test)]`. A file gaining `#[cfg(test)] mod tests;` gained a test MODULE, so a test
        // the extractor could not read is plausible there: the file stays in the proof and the
        // scan's refusal stays reachable. The helper case widens nothing.
        //
        // WHICH refusal moved, and it is the whole point of the second finding. `tests.rs` is not
        // in this diff, so the declaration compiles a module of tests that arrived with no added
        // line naming any of them - `Scan::Enabled`, whose remedy is stated evidence rather than
        // an extractor fix. It used to be `Unnamed` here and the passing `silent` arm as soon as
        // any sibling named a test, which is the one input that had two remedies.
        let files = vec![
            changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]", "mod tests;"]),
            changed("crates/x/src/other.rs", 1, &["fn changed() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
            ("crates/x/src/other.rs", "fn changed() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let planned = plan(&files, &read);
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
        // THE DEFECT THIS CHANGE REMOVES, at the level `plan` decides. A file gains an
        // implementation change AND its own `#[cfg(test)]` tests, so it is inseparable and held
        // back - but a SEPARABLE integration test, added in its own `tests/` file in the SAME
        // package, exercises that implementation. The proof's base run reverted only `shipped.rs`,
        // left `default_features.rs` at HEAD, the integration test stayed green on "base", and the
        // gate reported `FAILED - green against base behaviour` - blaming the author for a
        // partition the gate itself chose. Reverting the held-back implementation too gives that
        // test a real red-before-green proof, or a genuine tautology when it stays green without
        // it - either way a verdict about the test rather than about the partition.
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
        match plan(&files, &read) {
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
        // The other direction, and the reason the rule is "a scoped test's package reaches it"
        // rather than "revert every held-back file". A held-back implementation in a package where
        // NO provable test lives cannot be the thing a scoped test measures - reaching it would
        // need the cross-package dependency graph `github.com/telekom/sutura#358` left open - so
        // it stays held for the tree that compiles, and the retry machinery can still name it.
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
        match plan(&files, &read) {
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
