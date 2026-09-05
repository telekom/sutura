//! Which tests the diff added, and the nextest filter that runs exactly those.
//!
//! WHY THE RUN IS SCOPED AT ALL. An unfiltered `--workspace` base run makes the verdict a property
//! of the WHOLE suite rather than of the tests the diff added; `super::base` carries the run that
//! measured what that cost. Scoping is the half that stops an unrelated failure from happening at
//! all, and that module is the half that stops one being read as evidence if it happens anyway.
//!
//! A BARE FUNCTION NAME IS NOT A KEY IN THIS TREE, and `super::place` owns that key, the
//! measurement behind it and the collisions it still does not separate. Read it before believing
//! a filter here identifies one test.
//!
//! FAIL CLOSED ON AN EMPTY SCAN, and that is what [`Scoped`] is for. A scan naming no test may
//! not fall back to "no filter", because an unfiltered run makes the verdict a property of the
//! suite. So the non-empty set is a TYPE rather than a check somebody remembers to write at the
//! call site: [`Scan::of`] answers [`Scan::Unnamed`], and the gate refuses instead of measuring
//! the suite.
//!
//! FAIL CLOSED ON A PARTIAL ONE TOO, which took longer to see because the scan is AGGREGATE: it
//! answered `Runnable` the moment ONE provable file named a test, so a second provable file whose
//! added `#[test]` yielded no name rode along unmeasured and unmentioned - the common shape, not a
//! corner. [`Scan::Unreadable`] is that case and it refuses AHEAD of `Runnable`. It is counted
//! PER ATTRIBUTE rather than per file, which is the second half of the same finding: `named > 0`
//! used to end the file's inspection, so a second unnameable attribute BESIDE a nameable one was
//! invisible in both of `super::coverage`'s numbers. **The precision cost was measured before it
//! was taken** (2026-09-05, every `.rs` file under `crates/`, `xtask/` and `dev/` walked through
//! this extractor): **zero** test-declaring attributes fail to name a function, so the
//! per-attribute rule refuses nothing this tree writes. Only the zero is written down, because it
//! is the whole argument and a total would rot inside one PR. What it WOULD refuse is a
//! test-declaring attribute with no function under it, which does not compile, and a
//! `#[test]`-shaped line inside a raw string literal, of which there is none.
//!
//! WHAT A SILENT FILE IS ALLOWED TO CLAIM, and this one was asserted rather than checked. A
//! `#[cfg(test)] mod ..` declaration names nothing by design, so refusing per file would redden
//! the ordinary way a test module is added - it is [`Scoped::silent`] and the gate prints it. The
//! sentence printed beside it said *its own file names the tests*, and nothing read whether that
//! file was in the diff at all: a `#[cfg(test)] mod legacy;` added to `lib.rs` while `legacy.rs`
//! sits untouched puts a whole pre-existing module of tests into the build, with no added line
//! naming any of them and no run measuring any of them - and, because `lib.rs` is held at HEAD,
//! those tests are in both trees and cannot be red on base either. That is
//! [`Scan::Enabled`], it refuses, and `super::place::declared_module_files` is what resolves the
//! declaration instead of asserting it.
//!
//! AN `#[ignore]`d TEST IS NAMED AND DROPPED, because a filterset naming only ignored tests
//! matches nothing and nextest exits 4 with *error: no tests to run* - a false RED on legitimate
//! work, and the acceptance suites here are full of them
//! (`git grep -c -E '^[[:space:]]*#\[ignore' -- '*.rs'` counts the attributes; the same command
//! WITHOUT the anchor answers roughly twice as many across twice as many files, because most
//! `#[ignore` in this tree is a doc comment ABOUT one - no number is written down, because the
//! argument holds at any count above zero and a figure would rot within a PR).
//! Running them is the wrong direction for the reason a tier-backed cell is not required in the
//! reconstructed worktree (see [`super::runner::nextest`]): they are ignored because this venue lacks what
//! they need, so forcing them in a tree nothing provisioned fails CLOSED and reads as
//! red-on-base. So they leave the
//! scope, and a diff whose every added test is ignored gets [`Scan::OnlyIgnored`] - a statement
//! that this gate has not verified the change, not a claim that the extractor is broken.
//!
//! WHAT IT DOES NOT REACH. A name is read from an ADDED test attribute and the function under
//! it, so a body-only edit inside an existing `#[test]` names nothing here. `super::attributes`
//! also accepts an added `#[cfg(test)] mod ..` or `mod tests {` marker, which names no function -
//! so a diff whose ONLY test signal is such a marker is a file the plan calls a test file and this
//! cannot name. **A `#[cfg(test)]` item that is not a module is NOT such a marker**, and treating
//! it as one was a refusal no author could act on; that module's header carries the reasoning.
//! Both attribute lists live THERE, in one file, because a name this cannot extract from a marker
//! it accepts is exactly the disagreement that would reopen the unfiltered run.

use crate::causality::attributes::{attached, declares_a_test, item_below};
use crate::causality::diff::ChangedFile;
use crate::causality::names::Ident;
use crate::causality::place::{AddedTest, Declares, accounted_for, place};
use crate::causality::regions::{AddedLine, PostImage};

/// A provable file that named no test, and where its tests actually are.
///
/// The second field is the change: the printed sentence used to ASSERT that the declared module's
/// own file names the tests, and nothing read whether that file was in the diff. It is resolved
/// now, so a declaration this cannot account for is [`Scan::Enabled`] rather than a pass.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Silent {
    /// The provable file that named nothing.
    pub(crate) path: String,
    /// The declared module's own file, which IS in this diff and is where its tests are named -
    /// or `None` for an INLINE module, whose body is in this same file and whose lines were
    /// already compiled, so nothing arrived for this to name.
    pub(crate) module: Option<String>,
}

/// A provable file whose added declaration puts a module of tests into the build that this diff
/// does not contain.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Enabled {
    /// The file carrying the declaration.
    pub(crate) path: String,
    /// Where cargo will look for the module's source. Not in this diff, which is the finding.
    pub(crate) module: String,
}

/// The tests a diff added: at least one, by construction.
#[derive(Debug)]
pub(crate) struct Scoped {
    tests: Vec<AddedTest>,
    /// Provable files that named no test and whose reason for naming none is accounted for.
    ///
    /// Carried because this scan is AGGREGATE, so a file it could not name is otherwise invisible
    /// whenever a sibling could. Stated rather than refused, and the statement is now checked.
    silent: Vec<Silent>,
}

impl Scoped {
    /// The keys, for comparing a failure against the set under test.
    pub(crate) fn tests(&self) -> &[AddedTest] {
        &self.tests
    }

    /// Provable files this named no test in, with what accounts for each.
    pub(crate) fn silent(&self) -> &[Silent] {
        &self.silent
    }

    /// The nextest filter expression that runs exactly these tests.
    pub(crate) fn filterset(&self) -> String {
        self.tests.iter().map(AddedTest::term).collect::<Vec<String>>().join(" + ")
    }
}

/// What scanning the diff's test files found.
#[derive(Debug)]
pub(crate) enum Scan {
    /// Tests this venue can run, so there is something to measure.
    Runnable(Scoped),
    /// A provable file added an attribute that DECLARES a test and no name came out of it, or its
    /// post-image could not be read at all. Refuses, and refuses ahead of every other answer,
    /// which is the point: this scan is aggregate, so one nameable test elsewhere in the diff
    /// used to mask the file completely and the run measured a subset with nothing saying so. The
    /// fix is the extractor, which is why it comes first: any other verdict over the same diff is
    /// a verdict this gate cannot read its own inputs for.
    Unreadable(Vec<String>),
    /// A provable file declares a test module whose own file is not in this diff, so a module of
    /// pre-existing tests becomes compiled and no added line names any of them. Refuses - the
    /// remedy is stated evidence, not an extractor fix, which is why it is its own answer.
    ///
    /// **ITS INVERSE IS UNREPORTED, and the asymmetry is strict.** This refusal reads a `.rs`
    /// diff. A pre-existing `#[cfg(feature = "x")] mod tests;` whose `x` a **`Cargo.toml`-only**
    /// diff turns on compiles the same whole module of tests into the build with ZERO added `.rs`
    /// lines - so `super::plan` finds no changed test file at all, answers `Plan::NotRequired`, and
    /// the gate passes with *no changed tests - nothing to prove*. Reproduced through `plan`, and
    /// recorded rather than fixed: nothing here reads a manifest, and resolving which `cfg`
    /// declarations a feature change activates is a different scan from this one.
    /// `github.com/telekom/sutura#343` carries the three candidate shapes; the honest statement
    /// until then is that a manifest-only diff is outside this gate's reach, the same way a
    /// markdown-only one is.
    Enabled(Vec<Enabled>),
    /// Every test the diff added is `#[ignore]`d. Named, and unreachable by any run here.
    OnlyIgnored(Vec<Ident>),
    /// No test could be named at all.
    Unnamed,
}

impl Scan {
    /// The tests the `provable` files added.
    ///
    /// `provable` is the plan's own list - a file carrying both an implementation change and its
    /// tests is excluded there, and its tests are not part of the proof, so they are not part of
    /// the run either.
    pub(crate) fn of(files: &[ChangedFile], provable: &[String], read: &PostImage<'_>) -> Self {
        let mut runnable: Vec<AddedTest> = Vec::new();
        let mut ignored: Vec<Ident> = Vec::new();
        let mut silent: Vec<Silent> = Vec::new();
        let mut enabled: Vec<Enabled> = Vec::new();
        let mut unreadable: Vec<String> = Vec::new();
        for file in files.iter().filter(|file| provable.contains(&file.path)) {
            // PER ATTRIBUTE, not per file: the count is what `named` is compared against, so an
            // unnameable attribute beside a nameable one is refused rather than dropped.
            let declared = file.added.iter().filter(|line| declares_a_test(line.text.trim())).count();
            let mut named = 0_usize;
            let Some((text, at)) = read(&file.path).zip(place(&file.path, read)) else {
                // ONE fail-closed arm for *this gate cannot say where this file's tests would
                // land*, and nothing may be claimed about such a file - including that a test
                // module arrived in it. `attributes::adds` answers `TestModule` for a post-image it
                // cannot read, which is the fail-closed direction, and THIS is where that direction
                // is delivered: it used to land on the passing `silent` arm, whose printed sentence
                // said a test module arrived. A path no `Cargo.toml` owns is the same answer for
                // the same reason - cargo compiles nothing from it, so no sentence about its tests
                // can be true.
                unreadable.push(file.path.clone());
                continue;
            };
            let lines: Vec<&str> = text.lines().collect();
            for declaration in file.added.iter().filter_map(|added| declared_under(&lines, added)) {
                named += 1;
                match declaration {
                    Declared::Ignored(name) => {
                        if !ignored.contains(&name) {
                            ignored.push(name);
                        }
                    }
                    Declared::Runs(name) => {
                        let one = AddedTest::at(&at, name);
                        if !runnable.contains(&one) {
                            runnable.push(one);
                        }
                    }
                }
            }
            if named > 0 && named == declared {
                continue;
            }
            // WHICH kind of unnameable, because they ask for different things - and each file
            // carries its own cause, so a printed remedy cannot offer one this input cannot have.
            if declared > 0 {
                // An added attribute that declares a test and yielded no name: the extractor is
                // the fix.
                unreadable.push(file.path.clone());
                continue;
            }
            match accounted_for(file, &lines) {
                Some(Declares::Inline) => silent.push(Silent {
                    path: file.path.clone(),
                    module: None,
                }),
                Some(Declares::OutOfLine(candidates)) => {
                    match candidates.iter().find(|one| files.iter().any(|f| f.path == **one)) {
                        Some(present) => silent.push(Silent {
                            path: file.path.clone(),
                            module: Some(present.clone()),
                        }),
                        None => enabled.push(Enabled {
                            path: file.path.clone(),
                            module: candidates.into_iter().next().unwrap_or_default(),
                        }),
                    }
                }
                // Nothing in the added lines declares a module, so what made this a test file
                // names nothing and cannot be accounted for: a dangling attribute, or a form this
                // gate does not read. Same remedy as an unreadable name - fix the extractor.
                None => unreadable.push(file.path.clone()),
            }
        }
        // Ahead of everything: a subset the caller cannot see is the defect, and one sibling that
        // names a test is exactly what used to hide it.
        if !unreadable.is_empty() {
            return Self::Unreadable(unreadable);
        }
        if !enabled.is_empty() {
            return Self::Enabled(enabled);
        }
        if !runnable.is_empty() {
            return Self::Runnable(Scoped { tests: runnable, silent });
        }
        if ignored.is_empty() {
            Self::Unnamed
        } else {
            Self::OnlyIgnored(ignored)
        }
    }
}

/// A test an added attribute declares, and whether a run in this venue reaches it.
enum Declared {
    /// A test that runs here.
    Runs(Ident),
    /// `#[ignore]`d, so no filter can make it run and naming it in one matches nothing.
    Ignored(Ident),
}

/// The test an added attribute line declares, read out of the post-image below it.
///
/// The post-image rather than the added set, because the attribute and its function are two
/// lines and only one of them has to be new: appending `#[test]` above an existing helper, or
/// adding the attribute and the signature in one hunk, both have to name the same test.
fn declared_under(lines: &[&str], added: &AddedLine) -> Option<Declared> {
    if !declares_a_test(added.text.trim()) {
        return None;
    }
    // `number` is 1-based, so it IS the 0-based index of the line after the attribute.
    let (index, declaration) = item_below(lines, added.number)?;
    let name = function_name(declaration)?;
    Some(if is_ignored(lines, index) {
        Declared::Ignored(name)
    } else {
        Declared::Runs(name)
    })
}

/// Does the attribute block attached to the function at `index` carry an `#[ignore]`?
///
/// `attributes::attached` owns the block, because `#[ignore]` is legal on either side of `#[test]`
/// and a WRAPPED `#[ignore = ".."]` is one attribute over several lines - the shape that used to
/// put an ignored test into the filterset and cost the loud `OnlyIgnored` pass.
/// `#[cfg_attr(.., ignore)]` is not recognised - no such spelling exists in this tree, and the
/// direction of missing one is the `no tests to run` failure this dropping exists to prevent,
/// which is loud.
fn is_ignored(lines: &[&str], index: usize) -> bool {
    attached(lines, index).iter().any(|opening| opening.starts_with("#[ignore"))
}

/// The name in `fn NAME(`, if this line declares a function.
///
/// One line rather than a parser, and it reaches further than "a test's signature is written on one
/// line in this tree" - which is what this said, and what `super::attributes` overstated into *a
/// wrapped signature names nothing*. The name sits before the `(`, and rustfmt breaks a signature
/// too long for one line AFTER that `(`, so the FIRST line of a wrapped `async fn very_long_...(`
/// still carries `fn <name>(` and is still named. The shapes that occur are `fn`, `async fn` and a
/// visibility in front of either. What is genuinely out of reach is an added line that is not a
/// signature's first line: a body-only or def-interior edit.
fn function_name(line: &str) -> Option<Ident> {
    let declared = line.split_whitespace().skip_while(|word| *word != "fn").nth(1)?;
    Ident::parse(declared.split(['(', '<', ':']).next()?)
}

#[cfg(test)]
mod tests {
    use super::{AddedTest, Scan, Silent};
    use crate::causality::diff::ChangedFile;
    use crate::causality::fixtures::{changed, manifest, tree};
    use crate::causality::names::Ident;
    use crate::causality::regions::PostImage;

    /// The names `Scan::of` found runnable, as plain strings.
    fn runnable(files: &[ChangedFile], provable: &[&str], read: &PostImage<'_>) -> Option<Vec<String>> {
        let owned: Vec<String> = provable.iter().map(|path| String::from(*path)).collect();
        match Scan::of(files, &owned, read) {
            Scan::Runnable(scoped) => Some(scoped.tests().iter().map(|one| String::from(one.name())).collect()),
            _ => None,
        }
    }

    /// The filterset for the tests `provable` added.
    fn filterset(files: &[ChangedFile], provable: &[&str], read: &PostImage<'_>) -> String {
        let owned: Vec<String> = provable.iter().map(|path| String::from(*path)).collect();
        match Scan::of(files, &owned, read) {
            Scan::Runnable(scoped) => scoped.filterset(),
            other => panic!("expected runnable tests, got {other:?}"),
        }
    }

    #[test]
    fn the_base_run_is_scoped_to_the_tests_the_diff_added() {
        // The property the whole module exists for: the run names the ADDED test and nothing
        // else. `existing` is unchanged context in the same module and must not be scoped in,
        // because a verdict about it is a verdict about the suite rather than the change.
        let file = concat!(
            "pub fn open() -> u8 {\n",       // 1
            "    1\n",                       // 2
            "}\n",                           // 3
            "#[cfg(test)]\n",                // 4
            "mod tests {\n",                 // 5
            "    #[test]\n",                 // 6
            "    fn existing() {}\n",        // 7
            "    #[test]\n",                 // 8
            "    fn added_one() {}\n",       // 9
            "    #[tokio::test]\n",          // 10
            "    async fn added_two() {}\n", // 11
            "}\n",                           // 12
        );
        let files = vec![changed(
            "crates/x/src/a.rs",
            8,
            &[
                "    #[test]",
                "    fn added_one() {}",
                "    #[tokio::test]",
                "    async fn added_two() {}",
            ],
        )];
        let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(
            runnable(&files, &["crates/x/src/a.rs"], &read),
            Some(vec![String::from("added_one"), String::from("added_two")])
        );
    }

    #[test]
    fn a_marker_whose_module_file_is_in_the_diff_names_nothing_and_refuses() {
        // The fail-closed shape. `#[cfg(test)]` makes the file a test file and names no
        // function, so the scan comes back empty - and empty has to be unrepresentable rather
        // than "run everything", which is the unfiltered run this module replaced. `tests.rs` is
        // in the diff, so the declaration IS accounted for; nothing named a test all the same.
        let files = vec![
            changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]"]),
            changed("crates/x/src/tests.rs", 1, &["use super::f;"]),
        ];
        let read = tree(&[
            ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
            ("crates/x/src/tests.rs", "use super::f;\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        assert!(matches!(
            Scan::of(&files, &[String::from("crates/x/src/lib.rs")], &read),
            Scan::Unnamed
        ));
    }

    #[test]
    fn a_declaration_whose_module_file_is_absent_from_the_diff_refuses() {
        // THE FINDING. `lib.rs` gains `#[cfg(test)] mod legacy;` while `legacy.rs` already exists
        // and is untouched, so a whole module of pre-existing tests becomes compiled: no added
        // line names any of them, and `lib.rs` is held at HEAD, so they are in BOTH trees and
        // cannot be red on base either. One nameable test elsewhere used to be enough to land
        // this on the passing `silent` arm, whose printed sentence claimed *its own file names
        // the tests* - a fact nothing checked.
        let files = vec![
            changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]", "mod legacy;"]),
            changed("crates/x/src/other.rs", 1, &["#[test]", "fn added() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod legacy;\n"),
            (
                "crates/x/src/legacy.rs",
                "#[test]\nfn old_one() {}\n#[test]\nfn old_two() {}\n",
            ),
            ("crates/x/src/other.rs", "#[test]\nfn added() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let provable = vec![String::from("crates/x/src/lib.rs"), String::from("crates/x/src/other.rs")];
        match Scan::of(&files, &provable, &read) {
            Scan::Enabled(ref refused) => {
                assert_eq!(refused.len(), 1, "one declaration this diff cannot account for");
                assert_eq!(refused.first().map(|one| one.path.as_str()), Some("crates/x/src/lib.rs"));
                assert_eq!(refused.first().map(|one| one.module.as_str()), Some("crates/x/src/legacy.rs"));
            }
            other => panic!("expected Enabled ahead of Runnable, got {other:?}"),
        }
    }

    #[test]
    fn an_unreadable_post_image_is_refused_rather_than_called_a_test_module() {
        // The other half of the same finding. `attributes::adds` answers `TestModule` for a file
        // it cannot read, which is the fail-closed direction - but this scan could not read it
        // either, so it landed on `silent` and printed *a test module arrived here*. Nothing knew
        // that. Delivering the fail-closed direction is what this arm is.
        let files = vec![
            changed("crates/x/src/gone.rs", 1, &["#[cfg(test)]"]),
            changed("crates/x/src/other.rs", 1, &["#[test]", "fn added() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/other.rs", "#[test]\nfn added() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let provable = vec![String::from("crates/x/src/gone.rs"), String::from("crates/x/src/other.rs")];
        match Scan::of(&files, &provable, &read) {
            Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("crates/x/src/gone.rs")]),
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    #[test]
    fn a_file_not_in_the_proof_is_not_scanned() {
        // A file carrying both an implementation change and its tests is excluded from the proof
        // by the plan. Its tests must not reach the run either: they cannot be red on base,
        // because their own implementation is never reverted.
        let held = concat!(
            "fn fixed() -> u8 { 2 }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn held() {}\n",
            "}\n"
        );
        let files = vec![changed("crates/x/src/held.rs", 4, &["    #[test]", "    fn held() {}"])];
        let read = tree(&[("crates/x/src/held.rs", held), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(runnable(&files, &[], &read), None);
    }

    #[test]
    fn a_relocated_declaration_is_accounted_for_by_the_file_it_actually_names() {
        // The `#[path]` half of the resolution, at the level that refuses, and the fixture has to
        // DIVERGE from the layout or it proves nothing: `#[path = "shared/cells.rs"] mod support;`
        // in `tests/golden.rs` names `tests/shared/cells.rs`, while the layout would look for
        // `tests/golden/support.rs`. Not in the diff is a REFUSAL now, so a resolver that read the
        // layout instead of the attribute would redden a correct change - and the first version of
        // this test used `#[path = "golden/catalogs.rs"]`, where the two agree by coincidence and
        // ignoring the attribute reddened nothing.
        let target = concat!(
            "#[cfg(test)]\n",                  // 1
            "#[path = \"shared/cells.rs\"]\n", // 2
            "mod support;\n",                  // 3
        );
        let files = vec![
            changed(
                "crates/x/tests/golden.rs",
                1,
                &["#[cfg(test)]", "#[path = \"shared/cells.rs\"]", "mod support;"],
            ),
            changed("crates/x/tests/shared/cells.rs", 1, &["#[test]", "fn sums() {}"]),
        ];
        let read = tree(&[
            ("crates/x/tests/golden.rs", target),
            ("crates/x/tests/shared/cells.rs", "#[test]\nfn sums() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let provable = vec![
            String::from("crates/x/tests/golden.rs"),
            String::from("crates/x/tests/shared/cells.rs"),
        ];
        match Scan::of(&files, &provable, &read) {
            Scan::Runnable(ref scoped) => assert_eq!(
                scoped.silent(),
                [Silent {
                    path: String::from("crates/x/tests/golden.rs"),
                    module: Some(String::from("crates/x/tests/shared/cells.rs")),
                }]
            ),
            other => panic!("expected the relocated declaration to be stated, got {other:?}"),
        }
    }

    #[test]
    fn a_path_no_package_owns_is_refused_rather_than_skipped() {
        // Nothing compiles it, so it has no test to run - and inventing a package name for it
        // would put a name nextest does not know into the filter. A REFUSAL rather than a skip,
        // which is the change: the file is part of the proof, and skipping it measures a subset.
        let files = vec![changed("stray/a.rs", 1, &["#[test]", "fn sums() {}"])];
        let read = tree(&[("stray/a.rs", "#[test]\nfn sums() {}\n")]);
        match Scan::of(&files, &[String::from("stray/a.rs")], &read) {
            Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("stray/a.rs")]),
            other => panic!("expected Unreadable, got {other:?}"),
        }
        // And whatever it added, not only a `#[test]`: a declaration in a file cargo compiles
        // nothing from cannot enable tests, so *its own file names them* is not sayable either.
        let declaring = vec![changed("stray/lib.rs", 2, &["#[cfg(test)]", "mod tests;"])];
        let read = tree(&[
            ("stray/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
            ("stray/tests.rs", "#[test]\nfn sums() {}\n"),
        ]);
        match Scan::of(&declaring, &[String::from("stray/lib.rs")], &read) {
            Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("stray/lib.rs")]),
            other => panic!("expected Unreadable for a path no package owns, got {other:?}"),
        }
    }

    #[test]
    fn one_nameable_test_does_not_mask_a_file_this_could_not_name() {
        // THE MASKING. This scan is aggregate - one nameable test anywhere made the whole answer
        // `Runnable` - so a provable file whose added `#[test]` yielded no name rode along
        // unmeasured, unmentioned. `b.rs` adds the attribute over a line no function name comes
        // out of, and `a.rs` naming its test fine is what used to hide it.
        let files = vec![
            changed("crates/x/src/a.rs", 1, &["#[test]", "fn reads_fine() {}"]),
            changed("crates/x/src/b.rs", 1, &["#[test]", "let _ = 1;"]),
        ];
        let read = tree(&[
            ("crates/x/src/a.rs", "#[test]\nfn reads_fine() {}\n"),
            ("crates/x/src/b.rs", "#[test]\nlet _ = 1;\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let provable = vec![String::from("crates/x/src/a.rs"), String::from("crates/x/src/b.rs")];
        match Scan::of(&files, &provable, &read) {
            Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("crates/x/src/b.rs")]),
            other => panic!("expected Unreadable ahead of Runnable, got {other:?}"),
        }
    }

    #[test]
    fn one_nameable_attribute_does_not_mask_another_in_the_same_file() {
        // The masking one level down, which `named > 0` left open: the file's inspection ended on
        // the first name, so the second attribute was dropped from the filter with nothing naming
        // it - and `super::coverage` could not surface it either, because both of its numbers come
        // from this extractor. Counting per ATTRIBUTE closes it, and the precision cost was
        // measured at zero over this tree before it was taken (this module's header).
        let file = concat!(
            "#[test]\n",       // 1
            "fn plain() {}\n", // 2
            "#[test]\n",       // 3
            "let _ = 1;\n",    // 4
        );
        let files = vec![changed(
            "crates/x/src/a.rs",
            1,
            &["#[test]", "fn plain() {}", "#[test]", "let _ = 1;"],
        )];
        let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        match Scan::of(&files, &[String::from("crates/x/src/a.rs")], &read) {
            Scan::Unreadable(ref refused) => assert_eq!(*refused, vec![String::from("crates/x/src/a.rs")]),
            other => panic!("expected Unreadable for the unnameable second attribute, got {other:?}"),
        }
    }

    #[test]
    fn a_test_module_that_names_nothing_is_stated_rather_than_refused() {
        // The other unnameable shape, and it must NOT refuse: `lib.rs` gains
        // `#[cfg(test)] mod tests;` and the module's own file arrives in the same diff naming the
        // tests. That is the ordinary way a test module is added, so refusing per file would
        // redden a correct change - the declaration is carried as `silent`, RESOLVED to the file
        // that names them, and printed instead.
        let files = vec![
            changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]", "mod tests;"]),
            changed("crates/x/src/tests.rs", 1, &["#[test]", "fn added() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
            ("crates/x/src/tests.rs", "#[test]\nfn added() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let provable = vec![String::from("crates/x/src/lib.rs"), String::from("crates/x/src/tests.rs")];
        match Scan::of(&files, &provable, &read) {
            Scan::Runnable(ref scoped) => {
                assert_eq!(
                    scoped.tests().iter().map(AddedTest::name).collect::<Vec<&str>>(),
                    vec!["added"]
                );
                assert_eq!(
                    scoped.silent(),
                    [Silent {
                        path: String::from("crates/x/src/lib.rs"),
                        module: Some(String::from("crates/x/src/tests.rs")),
                    }]
                );
            }
            other => panic!("expected Runnable with the declaration stated, got {other:?}"),
        }
    }

    #[test]
    fn an_inline_module_added_around_existing_tests_is_stated_with_no_second_file() {
        // The third silent shape, and it must not refuse either: an added `mod tests {` whose
        // body is unchanged context enables nothing - those lines were already compiled. There is
        // no second file to look for, so the sentence beside it may not claim one.
        let file = concat!(
            "fn f() {}\n",            // 1
            "#[cfg(test)]\n",         // 2
            "mod tests {\n",          // 3
            "    #[test]\n",          // 4
            "    fn existing() {}\n", // 5
            "}\n",                    // 6
        );
        let files = vec![
            changed("crates/x/src/a.rs", 2, &["#[cfg(test)]", "mod tests {"]),
            changed("crates/x/src/b.rs", 1, &["#[test]", "fn added() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/a.rs", file),
            ("crates/x/src/b.rs", "#[test]\nfn added() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let provable = vec![String::from("crates/x/src/a.rs"), String::from("crates/x/src/b.rs")];
        match Scan::of(&files, &provable, &read) {
            Scan::Runnable(ref scoped) => assert_eq!(
                scoped.silent(),
                [Silent {
                    path: String::from("crates/x/src/a.rs"),
                    module: None,
                }]
            ),
            other => panic!("expected the inline module to be stated, got {other:?}"),
        }
    }

    #[test]
    fn two_tests_are_one_expression() {
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &["#[test]", "fn one() {}", "#[test]", "fn two() {}"],
        )];
        let read = tree(&[
            ("crates/x/tests/t.rs", "#[test]\nfn one() {}\n#[test]\nfn two() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        assert_eq!(
            filterset(&files, &["crates/x/tests/t.rs"], &read),
            concat!(
                "(binary_id(=x::t) & test(/^(?:.*::)?one(?:::|$)/))",
                " + (binary_id(=x::t) & test(/^(?:.*::)?two(?:::|$)/))"
            )
        );
    }

    #[test]
    fn an_ignored_test_leaves_the_scope_rather_than_emptying_the_run() {
        // Measured on nextest 0.9.143: a filterset naming only `#[ignore]`d tests matches
        // nothing and exits 4 with `error: no tests to run`, which the gate read as a failure.
        // `#[ignore]` is legal on either side of `#[test]`, so both orders are dropped, and the
        // runnable neighbour is still proven.
        let file = concat!(
            "#[test]\n",                      // 1
            "#[ignore = \"needs a tier\"]\n", // 2
            "fn below() {}\n",                // 3
            "#[ignore]\n",                    // 4
            "#[test]\n",                      // 5
            "fn above() {}\n",                // 6
            "#[test]\n",                      // 7
            "fn runs() {}\n",                 // 8
        );
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &[
                "#[test]",
                "#[ignore = \"needs a tier\"]",
                "fn below() {}",
                "#[ignore]",
                "#[test]",
                "fn above() {}",
                "#[test]",
                "fn runs() {}",
            ],
        )];
        let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(
            runnable(&files, &["crates/x/tests/t.rs"], &read),
            Some(vec![String::from("runs")])
        );
    }

    #[test]
    fn a_test_under_an_attribute_the_formatter_wrapped_is_still_named() {
        // THE DEFECT. `#[test]` over a wrapped `#[expect(..)]` is how eight tests in this tree are
        // written, and the downward search stopped on `clippy::disallowed_methods,` - so
        // `function_name` got that instead of a signature and no name came out. Survivable while
        // the scan was aggregate and silently skipped the file; after `Scan::Unreadable` refuses
        // ahead of `Runnable` it is a hard red on a correct change, which is the failure mode
        // `report_unreadable`'s own doc argues against.
        let file = concat!(
            "#[test]\n",                                  // 1
            "#[expect(\n",                                // 2
            "    clippy::disallowed_methods,\n",          // 3
            "    reason = \"exposing it IS the test\"\n", // 4
            ")]\n",                                       // 5
            "fn expose_secret_returns_the_value() {}\n",  // 6
            "#[test]\n",                                  // 7
            "fn reads_fine() {}\n",                       // 8
        );
        let files = vec![changed(
            "crates/x/src/a.rs",
            1,
            &[
                "#[test]",
                "#[expect(",
                "    clippy::disallowed_methods,",
                "    reason = \"exposing it IS the test\"",
                ")]",
                "fn expose_secret_returns_the_value() {}",
                "#[test]",
                "fn reads_fine() {}",
            ],
        )];
        let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(
            runnable(&files, &["crates/x/src/a.rs"], &read),
            Some(vec![
                String::from("expose_secret_returns_the_value"),
                String::from("reads_fine")
            ])
        );
    }

    #[test]
    fn a_signature_the_formatter_wrapped_is_named_off_its_first_line() {
        // THE LIMIT THAT WAS OVERSTATED. `super::attributes` said the item under an attribute is
        // read as one line, "so a `fn` signature the formatter had to wrap names nothing" -
        // measured FALSE in `github.com/telekom/sutura#319` and asserted here, because the next
        // extractor change would otherwise be judged against a limit that is wider than the code.
        // rustfmt breaks a long signature AFTER the `(`, and the name is before it, so the first
        // line still carries `fn <name>(`. `async` and a return type are in here because those are
        // the shapes that actually wrap in this tree.
        let file = concat!(
            "#[test]\n",                                                    // 1
            "async fn a_question_that_names_more_than_one_dimension_is(\n", // 2
            "    refused_before_it_reaches_the_source: bool,\n",            // 3
            ") -> Result<(), Box<dyn std::error::Error>> {\n",              // 4
            "    Ok(())\n",                                                 // 5
            "}\n",                                                          // 6
        );
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &[
                "#[test]",
                "async fn a_question_that_names_more_than_one_dimension_is(",
                "    refused_before_it_reaches_the_source: bool,",
                ") -> Result<(), Box<dyn std::error::Error>> {",
                "    Ok(())",
                "}",
            ],
        )];
        let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(
            runnable(&files, &["crates/x/tests/t.rs"], &read),
            Some(vec![String::from("a_question_that_names_more_than_one_dimension_is")]),
            "the name is read off the first signature line, which the formatter does not break"
        );
    }

    #[test]
    fn an_ignore_the_formatter_wrapped_still_leaves_the_scope() {
        // The worse half of the same defect, because it costs a PASS rather than a name.
        // `#[ignore = ".."]` continued with a trailing `\` is one attribute over two lines, and
        // reading the second as an item detached the `#[ignore]` from the test - so the only
        // added test entered the filterset, nextest matched nothing, and `Scan::OnlyIgnored`'s
        // loud pass was unreachable. Two tests in `crates/sutura-catalog-datahub/tests` are
        // written exactly this way.
        let file = concat!(
            "#[test]\n",                                                    // 1
            "#[ignore = \"needs `just dev-up-datahub`; run that task \\\n", // 2
            "            instead\"]\n",                                     // 3
            "fn the_provisioned_surface_answers() {}\n",                    // 4
        );
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &[
                "#[test]",
                "#[ignore = \"needs `just dev-up-datahub`; run that task \\",
                "            instead\"]",
                "fn the_provisioned_surface_answers() {}",
            ],
        )];
        let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        match Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read) {
            Scan::OnlyIgnored(ref names) => {
                assert_eq!(
                    names.iter().map(Ident::as_str).collect::<Vec<&str>>(),
                    vec!["the_provisioned_surface_answers"]
                );
            }
            other => panic!("a wrapped `#[ignore]` still leaves the scope, got {other:?}"),
        }
    }

    #[test]
    fn a_diff_whose_every_added_test_is_ignored_is_named_not_refused() {
        // The other half of the same measurement, and the reason it is a third answer rather
        // than the empty scan: an all-`#[ignore]`d diff is not an extractor bug, so it must not
        // print one. The names come back so the report can say what it could not measure.
        let file = "#[test]\n#[ignore]\nfn acceptance() {}\n";
        let files = vec![changed(
            "crates/x/tests/t.rs",
            1,
            &["#[test]", "#[ignore]", "fn acceptance() {}"],
        )];
        let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        match Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read) {
            Scan::OnlyIgnored(ref names) => {
                assert_eq!(names.iter().map(Ident::as_str).collect::<Vec<&str>>(), vec!["acceptance"]);
            }
            other => panic!("an ignored test is named, got {other:?}"),
        }
    }

    #[test]
    fn a_stray_attribute_does_not_reach_down_the_file() {
        // The attribute is the last line of its module, so there is no function under it. Naming
        // the next test in the file would scope in something the diff did not add - so it is
        // refused instead, which is what a test-declaring attribute yielding no name now means.
        let file = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "}\n",
            "#[test]\n",
            "fn elsewhere() {}\n"
        );
        let files = vec![changed("crates/x/src/a.rs", 3, &["    #[test]"])];
        let read = tree(&[("crates/x/src/a.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
        assert_eq!(runnable(&files, &["crates/x/src/a.rs"], &read), None);
    }
}
