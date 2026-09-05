//! How much of what the diff added the proof actually measured, and why the gate now says it.
//!
//! THE DEFECT. `super::plan` holds a file back whenever it carries an implementation change and a
//! test together - which in Rust is most of them - and the tests in a held file are deliberately
//! not part of the proof. Nothing said so. The verdict was `ok - red on base, green on head` over
//! a SUBSET of the tests the branch added, and a reader concludes the change is covered. Measured
//! on a real branch: eight added tests, seven in the filterset, the eighth in a held-back file and
//! genuinely causal - a mutation reddened it - and the verdict named no ratio at all.
//!
//! `AGENTS.md` is explicit that a test which passes both ways *"is worse than none, because it
//! looks like coverage"*. A gate that measures a subset while reporting on the whole has that same
//! shape one level up, so the fix is the same shape too: state the limit next to the claim.
//!
//! AND THE FIRST VERSION OF THAT FIX HAD THE DEFECT ONE LEVEL UP AGAIN, which is why the state is
//! an enum. When the whole-diff scan could not read its own inputs the added set collapsed to
//! empty, so the ratio printed `N of N added tests measured` - the one wording that asserts
//! nothing was left out - manufactured out of something unreadable, and `0 of 0` beside
//! `NOT MECHANICALLY SEPARABLE`, which reads as *the diff added no tests*. Two live routes,
//! measured: a changed PROSE file whose line begins `#[test]` (three pages in `.agents/skills/`
//! do), because `files` was passed as the provable list with no [`super::is_compiled_rust`]
//! filter; and a held-back file whose added `#[test]` this gate could not name, which is the
//! shape a real open branch had. **Collapsing to the numerator is the one outcome that has to be
//! unreachable**, so an unreadable denominator is [`Coverage::Unknown`] and says so.
//!
//! WHY IT STATES RATHER THAN FAILS, which is the decision worth recording. Failing when the two
//! numbers differ would fail nearly every branch in this workspace - and *nearly* understates it:
//! replayed over thirteen recent branch diffs, twelve reached a verdict and **all twelve had
//! `measured < M`**, seven of them at zero. A unit test beside the code it tests IS the held-back
//! shape and `report_not_separable` exists to say so. A gate that reddens correct work gets
//! disabled, and a disabled gate holds nothing - so the ratio is printed and the unmeasured tests
//! are NAMED, which is what a reader needs to decide whether the remainder wants a mutation run
//! by hand.
//!
//! A BARE TEST NAME IS NOT A KEY HERE - `super::place` carries the measurement - and these names
//! are not used as one. They are a POINTER for a person reading the output; every filter and every
//! comparison still goes through the package-plus-module-plus-name key. Two added tests sharing a
//! name collapse to one line, which under-reports the list and never the ratio: the counts come
//! from the keys.
//!
//! WHAT NEITHER NUMBER COUNTS, stated because `0 of 0` is otherwise read as *no tests*:
//! `#[ignore]`d added tests are in neither, since no run in this venue reaches one, and
//! `report_only_ignored` is the answer when they are all there is - putting them in the denominator
//! instead makes every acceptance-heavy branch read as partially measured forever. On any OTHER arm
//! an all-`#[ignore]`d diff therefore prints `0 of 0` with the names appearing nowhere, which is
//! this module's own defect one arm over. Filed as `github.com/telekom/sutura#314` with the three
//! candidate shapes, rather than closed here: it wants its own test and its own mutation.

use crate::causality::attributes::{Adds, adds};
use crate::causality::diff::ChangedFile;
use crate::causality::place::AddedTest;
use crate::causality::regions::PostImage;
use crate::causality::scoped::Scan;

/// What the proof measured, against every runnable test the diff added.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Coverage {
    /// Both numbers came from ONE scan of the whole diff, so the ratio is a ratio.
    Measured {
        /// How many tests the filterset names.
        measured: usize,
        /// Added, runnable, and NOT in the filterset, by name.
        unmeasured: Vec<String>,
    },
    /// The denominator could not be established: a changed file added something that declares a
    /// test, or a module of tests, and this gate could not read what. How many tests the diff
    /// added is therefore not known, and a ratio would be a claim.
    Unknown {
        /// How many tests the filterset names. Still true, and still worth printing.
        measured: usize,
        /// The changed files that stopped the total being established.
        unestablished: Vec<String>,
    },
}

impl Coverage {
    /// What `measured` covers of the tests `files` added, read through `read`.
    ///
    /// The denominator comes from the same [`Scan::of`] the numerator does, over every changed
    /// TEST file rather than the provable ones - one extractor for both numbers, so the ratio
    /// cannot drift from the filter, and every file `plan` held back is still counted.
    ///
    /// TWO FILTERS, and both were missing. [`super::is_compiled_rust`] is the rule `plan` applies
    /// and the reason `plan`'s own `a_vendored_test_is_not_a_changed_test_this_gate_can_measure`
    /// exists: a path this workspace does not compile has no test that may reach a filter, and
    /// none that may reach this count - without it a PROSE page whose line begins `#[test]` was
    /// scanned as a test file. [`Adds`] is the second, and it is the same classifier `plan`
    /// branches on: a file that added a `#[cfg(test)]` HELPER and no test is not a test file, and
    /// handing it to a scan that requires one is how the ordinary helper shape reached a refusal.
    pub(crate) fn of(measured: &[AddedTest], files: &[ChangedFile], read: &PostImage<'_>) -> Self {
        let every: Vec<String> = files
            .iter()
            .filter(|file| super::is_compiled_rust(&file.path))
            .filter(|file| matches!(adds(&file.added, &file.path, read), Adds::NamedTest | Adds::TestModule))
            .map(|file| file.path.clone())
            .collect();
        let measured_count = measured.len();
        match Scan::of(files, &every, read) {
            Scan::Runnable(ref all) => Self::Measured {
                measured: measured_count,
                unmeasured: all
                    .tests()
                    .iter()
                    .filter(|one| !measured.contains(one))
                    .map(|one| String::from(one.name()))
                    .collect(),
            },
            Scan::Unreadable(paths) => Self::Unknown {
                measured: measured_count,
                unestablished: paths,
            },
            Scan::Enabled(refused) => Self::Unknown {
                measured: measured_count,
                unestablished: refused.into_iter().map(|one| one.path).collect(),
            },
            // Neither of these hides a number. `OnlyIgnored` means every added test is out of
            // reach of any run here, which the module header records as counted by neither;
            // `Unnamed` means no added line named a test at all.
            Scan::OnlyIgnored(_) | Scan::Unnamed => Self::Measured {
                measured: measured_count,
                unmeasured: Vec::new(),
            },
        }
    }

    /// The ratio, for the verdict line a reader cites.
    ///
    /// Both wordings carry `added tests measured`, so one grep finds either across a log - and
    /// they are different sentences on purpose: only one of them is a ratio, and a branch cannot
    /// change which it produced.
    pub(crate) fn ratio(&self) -> String {
        match *self {
            Self::Measured {
                measured,
                ref unmeasured,
            } => format!("{measured} of {} added tests measured", measured + unmeasured.len()),
            Self::Unknown { measured, .. } => {
                format!("{measured} added tests measured, of a total this gate could not establish")
            }
        }
    }

    /// The same sentence with a ZERO numerator, for an arm whose base run produced no result.
    ///
    /// The numerator is what the filterset NAMES, which equals what was measured only once both
    /// runs have happened and reported per-test results. Two passing arms return before either
    /// run (nothing to revert, and no base behaviour to compare against), and two INCONCLUSIVE
    /// ones reach a base run that produced nothing to attribute: a tree that did not build ran no
    /// test at all. Printing what the filter named at any of the four is a claim about runs that
    /// did not happen, which is what `6 of 6 added tests measured` said beside *the base tree does
    /// not build*. `super::base::earned` is the mapping from outcome to wording.
    pub(crate) fn nothing_measured(&self) -> String {
        match *self {
            Self::Measured {
                measured,
                ref unmeasured,
            } => format!("0 of {} added tests measured", measured + unmeasured.len()),
            Self::Unknown { .. } => String::from("0 added tests measured, of a total this gate could not establish"),
        }
    }

    /// The tests the proof left out, by name.
    ///
    /// Empty while the denominator is unknown: a partial list of an unknown set reads as the
    /// whole of it, which is the sentence this module exists to remove.
    pub(crate) fn unmeasured(&self) -> &[String] {
        match *self {
            Self::Measured { ref unmeasured, .. } => unmeasured,
            Self::Unknown { .. } => &[],
        }
    }

    /// The changed files that stopped the total being established.
    pub(crate) fn unestablished(&self) -> &[String] {
        match *self {
            Self::Measured { .. } => &[],
            Self::Unknown { ref unestablished, .. } => unestablished,
        }
    }
}

/// What the two runs are scoped to, and how much of the diff that leaves unmeasured.
///
/// One value rather than two parameters, so the filter and its own limit reach the reconstruction
/// together. **What that buys is narrower than it sounds, and it is one step wider than it was:**
/// `super::base::report_base` used to take the sentence as a `&str` chosen by this caller, so which
/// wording each of its six arms printed was decided here and pinned by review. It takes the
/// `Coverage` now and `super::base::earned` maps outcome to wording in one place, with a test on the
/// mapping. What is still NOT held is that any arm printed it: that is prose, and the review of
/// these four modules is what holds it.
pub(crate) struct Scope<'s> {
    pub(crate) only: &'s str,
    pub(crate) coverage: &'s Coverage,
}

#[cfg(test)]
mod tests {
    use super::Coverage;
    use crate::causality::fixtures::{changed, manifest, tree};
    use crate::causality::scoped::Scan;

    /// A file whose whole content is one test, added at line 1.
    fn one_test(name: &str) -> String {
        format!("#[test]\nfn {name}() {{}}\n")
    }

    #[test]
    fn a_held_back_files_tests_are_counted_and_named() {
        // THE DEFECT, as the verdict now reports it: two files each add a test, one of them is
        // held back, and the ratio says one of two rather than nothing at all.
        let files = vec![
            changed("crates/x/src/proved.rs", 1, &["#[test]", "fn proved() {}"]),
            changed("crates/x/src/held.rs", 1, &["#[test]", "fn held() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/proved.rs", &one_test("proved")),
            ("crates/x/src/held.rs", &one_test("held")),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let Scan::Runnable(measured) = Scan::of(&files, &[String::from("crates/x/src/proved.rs")], &read) else {
            panic!("the provable file names a test");
        };
        let coverage = Coverage::of(measured.tests(), &files, &read);
        assert_eq!(coverage.ratio(), "1 of 2 added tests measured");
        assert_eq!(coverage.unmeasured(), [String::from("held")]);
    }

    #[test]
    fn a_fully_measured_diff_still_states_both_numbers() {
        // One wording whatever the answer: a reader greps for the sentence, not for its absence.
        let files = vec![changed("crates/x/src/proved.rs", 1, &["#[test]", "fn proved() {}"])];
        let read = tree(&[
            ("crates/x/src/proved.rs", &one_test("proved")),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let Scan::Runnable(measured) = Scan::of(&files, &[String::from("crates/x/src/proved.rs")], &read) else {
            panic!("the provable file names a test");
        };
        let coverage = Coverage::of(measured.tests(), &files, &read);
        assert_eq!(coverage.ratio(), "1 of 1 added tests measured");
        assert!(coverage.unmeasured().is_empty());
    }

    #[test]
    fn a_changed_prose_file_reaches_neither_number() {
        // THE FIRST LIVE ROUTE to a manufactured `N of N`. `files` was the provable list with no
        // `is_compiled_rust` filter, so a page whose line begins `#[test]` was scanned, owned by
        // no `Cargo.toml`, refused - and the refusal collapsed the added set to empty, so the
        // ratio read `1 of 1` over a diff that added two tests. Three pages under
        // `.agents/skills/engineering/` carry such a line today.
        let files = vec![
            changed("crates/x/src/proved.rs", 1, &["#[test]", "fn proved() {}"]),
            changed("crates/x/src/held.rs", 1, &["#[test]", "fn held() {}"]),
            changed(".agents/skills/engineering/guide.md", 1, &["#[test]", "fn it_works() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/proved.rs", &one_test("proved")),
            ("crates/x/src/held.rs", &one_test("held")),
            (".agents/skills/engineering/guide.md", &one_test("it_works")),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let Scan::Runnable(measured) = Scan::of(&files, &[String::from("crates/x/src/proved.rs")], &read) else {
            panic!("the provable file names a test");
        };
        let coverage = Coverage::of(measured.tests(), &files, &read);
        // The page is in neither number, and the held-back test is still named.
        assert_eq!(coverage.ratio(), "1 of 2 added tests measured");
        assert_eq!(coverage.unmeasured(), [String::from("held")]);
    }

    #[test]
    fn an_unreadable_denominator_is_said_rather_than_collapsed_onto_the_numerator() {
        // THE SECOND LIVE ROUTE, and the one that needs no prose: a CHANGED file whose added
        // `#[test]` yields no name is in the whole-diff scan too, so the scan refuses - and the
        // empty added set made `ratio()` print `N of N added tests measured`, the one wording
        // that asserts nothing was left out. Never that: the sentence says the total is not
        // established, and names the file that stopped it.
        let files = vec![
            changed("crates/x/src/proved.rs", 1, &["#[test]", "fn proved() {}"]),
            changed("crates/x/src/odd.rs", 1, &["#[test]", "let _ = 1;"]),
        ];
        let read = tree(&[
            ("crates/x/src/proved.rs", &one_test("proved")),
            ("crates/x/src/odd.rs", "#[test]\nlet _ = 1;\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let Scan::Runnable(measured) = Scan::of(&files, &[String::from("crates/x/src/proved.rs")], &read) else {
            panic!("the provable file names a test");
        };
        let coverage = Coverage::of(measured.tests(), &files, &read);
        assert_eq!(
            coverage.ratio(),
            "1 added tests measured, of a total this gate could not establish"
        );
        assert_eq!(coverage.unestablished(), [String::from("crates/x/src/odd.rs")]);
        // And the `NOT MECHANICALLY SEPARABLE` shape, where the collapse printed `0 of 0` - which
        // reads as a diff that added no tests rather than one that added some and measured none.
        assert_eq!(
            Coverage::of(&[], &files, &read).ratio(),
            "0 added tests measured, of a total this gate could not establish"
        );
    }

    #[test]
    fn a_test_only_helper_is_in_neither_number() {
        // The other half of this change: a `#[cfg(test)]` helper is not a test, so it may not
        // inflate the denominator and read as a test the proof declined to measure.
        let helper = concat!(
            "#[cfg(test)]\n",
            "pub(crate) fn helper() -> u8 { 1 }\n",
            "#[test]\n",
            "fn proved() {}\n",
        );
        let files = vec![changed(
            "crates/x/src/a.rs",
            1,
            &["#[cfg(test)]", "pub(crate) fn helper() -> u8 { 1 }"],
        )];
        let read = tree(&[("crates/x/src/a.rs", helper), ("crates/x/Cargo.toml", &manifest("x"))]);
        let coverage = Coverage::of(&[], &files, &read);
        assert_eq!(coverage.ratio(), "0 of 0 added tests measured");
        assert!(coverage.unmeasured().is_empty());
    }
}
