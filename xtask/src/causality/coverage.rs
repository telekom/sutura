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
//! WHAT IT COSTS TO PRINT A NUMBER BEFORE THE OUTCOME IS KNOWN, and it was the same defect one
//! line earlier. `super::prove` printed this ratio BEFORE the head run - a `measured:` line the
//! reader meets first - and then an inconclusive verdict twenty lines below corrected it to
//! `0 of N`. Two numerators, one sentence, in one output: reviewed and reproduced on the head that
//! introduced [`super::base::earned`], whose whole purpose was to stop exactly that wording being
//! printed by an arm that measured nothing. The wording fix does not reach a caller that never
//! consults an outcome.
//!
//! **So the sentence a verdict cites is not reachable without the outcome.** [`Attributed`] is the
//! evidence [`Coverage::measured`] requires, `super::base::earned` is the only place that produces
//! [`Attributed::PerTest`], and it produces it from two of six [`super::base::BaseOutcome`]
//! variants - both of which exist only after both runs reported per-test results. What a caller
//! before the runs has instead is [`Coverage::named`], which carries [`NAMED`] and never [`MEASURED`]:
//! the two keys are single constants here, so the grep a handoff uses finds exactly one measurement
//! per run and a pre-run line cannot be read as one. A non-earned numerator is now a compile error
//! rather than a wording somebody has to notice.
//!
//! A BARE TEST NAME IS NOT A KEY HERE - `super::place` carries the measurement - and these names
//! are not used as one. They are a POINTER for a person reading the output; every filter and every
//! comparison still goes through the package-plus-module-plus-name key. Two added tests sharing a
//! name collapse to one line, which under-reports the list and never the ratio: the counts come
//! from the keys.
//!
//! WHAT NEITHER NUMBER COUNTS, AND WHY THERE IS A THIRD LIST. `#[ignore]`d added tests are in
//! neither number, since no run in this venue reaches one, and putting them in the DENOMINATOR
//! instead would make every acceptance-heavy branch read as partially measured forever - the
//! *a gate that reddens correct work gets disabled* shape applied to a number. So the numbers stay
//! as they are and the names are carried beside them: [`Coverage::not_runnable`]. Without that,
//! `report_only_ignored` named them on its own arm and every OTHER arm printed
//! `0 of 0 added tests measured` with the names appearing nowhere - and `0 of 0` reads as *this
//! diff added no tests*, which is the sentence this module exists to remove. Reachable from an
//! inseparable file whose added tests sit behind a provisioned tier, which is the ordinary shape of
//! an acceptance change here. `github.com/telekom/sutura#314`, and the numerator that arm used to
//! format for itself is `Coverage`'s now, so one place formats every number this gate prints.

use crate::causality::attributes::{Adds, adds};
use crate::causality::base::PerTestResults;
use crate::causality::diff::ChangedFile;
use crate::causality::names::Ident;
use crate::causality::place::AddedTest;
use crate::causality::provenance::Moved;
use crate::causality::regions::PostImage;
use crate::causality::reverted::Attempts;
use crate::causality::scoped::Scan;

/// The grep key for a MEASUREMENT, in one place.
///
/// Both measured wordings carry it and nothing else in this gate does, so one grep over a log
/// finds every claim about what ran and no line that merely names a scope.
const MEASURED: &str = "added tests measured";

/// The grep key for what the filterset NAMED, in one place.
///
/// Deliberately a different sentence from [`MEASURED`]: the pre-run line and the verdict line both
/// carry a numerator, and the only thing that stops a reader citing the wrong one is that they are
/// not the same claim.
const NAMED: &str = "added tests named";

/// Whether a run produced per-test results, and therefore whether the filterset's names are also
/// what was measured.
///
/// THE EVIDENCE [`Coverage::measured`] REQUIRES, and the reason it is a type rather than a rule.
/// #307 moved the choice of wording into one pure function; the line an operator reads FIRST never
/// consulted it, because `super::prove` prints before either run has happened and had a `ratio()`
/// in reach. The [`PerTestResults`] this carries is **sealed in `super::base`** - its field is
/// private there - so a caller with no classified base run cannot spell a non-zero numerator
/// carrying [`MEASURED`] at all, whatever it wants to say. The two numerators in one output cannot
/// become the same claim again without a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attributed {
    /// Both runs happened and reported per-test results, so the filterset's names are also what
    /// was measured. Only `super::base` can produce the witness.
    PerTest(PerTestResults),
    /// No run attributed anything to a test this diff added: a tree that did not build ran none of
    /// them, a filter that matched none ran none of them, and a run no line attributes attributes
    /// nothing. Available to every arm, because every arm can honestly say this much.
    Nothing,
}

/// What the proof measured, against every runnable test the diff added.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Coverage {
    /// Both numbers came from ONE scan of the whole diff, so the ratio is a ratio.
    Measured {
        /// How many tests the filterset names.
        measured: usize,
        /// Added, runnable, and NOT in the filterset, by name.
        unmeasured: Vec<String>,
        /// Added and `#[ignore]`d, so in neither number. See the field on the other variant.
        not_runnable: Vec<String>,
    },
    /// The denominator could not be established: a changed file added something that declares a
    /// test, or a module of tests, and this gate could not read what. How many tests the diff
    /// added is therefore not known, and a ratio would be a claim.
    Unknown {
        /// How many tests the filterset names. Still true, and still worth printing.
        measured: usize,
        /// The changed files that stopped the total being established.
        unestablished: Vec<String>,
        /// Added and `#[ignore]`d, so in neither number.
        ///
        /// **THE THIRD LIST, AND IT IS ON BOTH VARIANTS ON PURPOSE.** An ignored test is counted by
        /// neither number for the reason the header gives, and the consequence was that an
        /// all-`#[ignore]`d diff printed `0 of 0 added tests measured` with the names appearing
        /// nowhere - the wording a reader takes to mean *this diff added no tests*, which is the
        /// sentence this module exists to remove, one arm over. `github.com/telekom/sutura#314`.
        /// A total this gate could not establish still has ignored tests it read, so leaving the
        /// list off this variant would reopen the same misreading on the arm that says least.
        not_runnable: Vec<String>,
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
                not_runnable: named(all.ignored()),
            },
            Scan::Unreadable(paths) => Self::Unknown {
                measured: measured_count,
                unestablished: paths,
                // The scan refused before it finished reading, so what it did see is not a list
                // this may present as the ignored tests of the diff.
                not_runnable: Vec::new(),
            },
            Scan::Enabled(refused) => Self::Unknown {
                measured: measured_count,
                unestablished: refused.into_iter().map(|one| one.path).collect(),
                not_runnable: Vec::new(),
            },
            // Every added test is out of reach of any run here, which the header records as
            // counted by neither number - so the NAMES are the whole of what this arm can say,
            // and `0 of 0` beside nothing at all was #314.
            Scan::OnlyIgnored(ref names) => Self::Measured {
                measured: measured_count,
                unmeasured: Vec::new(),
                not_runnable: named(names),
            },
            // No added line named a test at all: nothing to count and nothing to name.
            Scan::Unnamed => Self::Measured {
                measured: measured_count,
                unmeasured: Vec::new(),
                not_runnable: Vec::new(),
            },
        }
    }

    /// The sentence a verdict cites, for the evidence `attributed` presents.
    ///
    /// **THE ONLY ROUTE TO A LINE CARRYING [`MEASURED`]**, and that is the mechanism rather than
    /// the wording. The two wordings below existed already and one pure function chose between
    /// them; what was not held is that a caller had to consult an OUTCOME to reach either. So
    /// `super::prove` printed the ratio before the head run and an inconclusive verdict corrected
    /// it twenty lines later - one output, one sentence, two numerators. A caller with no
    /// [`Attributed`] to hand over now has only [`Coverage::named`], which is a different claim.
    pub(crate) fn measured(&self, attributed: Attributed) -> String {
        match attributed {
            Attributed::PerTest(_) => self.ratio(),
            Attributed::Nothing => self.nothing_measured(),
        }
    }

    /// What the filterset NAMED, for a line printed before either run has happened.
    ///
    /// Carries [`NAMED`] and never [`MEASURED`], because a numerator printed before a run is a
    /// SCOPE and reads as a measurement only if it is spelled like one. The count is the same
    /// count; the claim is not, and the reader who cites a number out of a log is the reason the
    /// two sentences may not be interchangeable.
    pub(crate) fn named(&self) -> String {
        match *self {
            Self::Measured {
                measured,
                ref unmeasured,
                ..
            } => format!("{measured} of {} {NAMED}", measured + unmeasured.len()),
            Self::Unknown { measured, .. } => {
                format!("{measured} {NAMED}, of a total this gate could not establish")
            }
        }
    }

    /// The ratio, for an outcome that produced per-test results.
    ///
    /// Private: reachable only through [`Coverage::measured`], so producing it takes an
    /// [`Attributed::PerTest`] that only `super::base::earned` mints.
    fn ratio(&self) -> String {
        match *self {
            Self::Measured {
                measured,
                ref unmeasured,
                ..
            } => format!("{measured} of {} {MEASURED}", measured + unmeasured.len()),
            Self::Unknown { measured, .. } => {
                format!("{measured} {MEASURED}, of a total this gate could not establish")
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
    fn nothing_measured(&self) -> String {
        match *self {
            Self::Measured {
                measured,
                ref unmeasured,
                ..
            } => format!("0 of {} {MEASURED}", measured + unmeasured.len()),
            Self::Unknown { .. } => format!("0 {MEASURED}, of a total this gate could not establish"),
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

    /// The added tests no run in this venue reaches, by name.
    ///
    /// In NEITHER number, which is the decision the header argues for and this list is what makes
    /// it readable: the ratio says what was measured out of what could be, and these say what
    /// could not be. Without them `0 of 0 added tests measured` was the only thing an
    /// acceptance-only branch was told.
    ///
    /// **THE MECHANISM UNDER IT IS DEAD-CODE ANALYSIS, and that holds only while this has exactly
    /// one production caller.** Deleting the reader today is `error: method 'not_runnable' is never
    /// used` under `-D warnings` - `super::remedies::unmeasured_lines` is the caller - so the
    /// plumbing from `Scan::of` cannot be quietly dropped. A second caller would remove that
    /// property, leaving only
    /// `an_ignored_test_beside_a_runnable_one_is_named_rather_than_dropped`, which is a real test
    /// and is why the property is worth naming rather than relying on.
    pub(crate) fn not_runnable(&self) -> &[String] {
        match *self {
            Self::Measured { ref not_runnable, .. } | Self::Unknown { ref not_runnable, .. } => not_runnable,
        }
    }
}

/// Test names as plain strings, for a line a PERSON reads.
///
/// A pointer and not a key, the same limit `AddedTest::name` states: two ignored tests sharing a
/// name collapse to one line here, and no filter is built from any of this.
fn named(names: &[Ident]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let one = String::from(name.as_str());
        if !out.contains(&one) {
            out.push(one);
        }
    }
    out
}

/// What the two runs are scoped to, and how much of the diff that leaves unmeasured.
///
/// One value rather than three parameters, so the filter, its own limit and what the base tree
/// already had reach the reconstruction together. **What that buys is narrower than it sounds, and
/// it is one step wider than it was:** `super::base::report_base` used to take the sentence as a
/// `&str` chosen by this caller, so which wording each of its arms printed was decided here and
/// pinned by review. It takes the `Coverage` now and `super::base::earned` maps outcome to wording
/// in one place, with a test on the mapping. What is still NOT held is that any arm printed it: that
/// is prose, and the review of these modules is what holds it.
pub(crate) struct Scope<'s> {
    pub(crate) only: &'s str,
    pub(crate) coverage: &'s Coverage,
    /// Which of the tests the filter names the base tree already had.
    ///
    /// Here rather than beside it because it is part of what the scope IS: the same filterset over
    /// tests that were MOVED into this diff is a different question from one over tests it added,
    /// and `super::base::classify_base` needs both to say what a green run means.
    pub(crate) moved: &'s Moved,
    /// Whether the files put back at base restore anything these tests could execute - once per
    /// ATTEMPT, because the retry reverts the held-back files too.
    ///
    /// The same argument as `moved` on the other side of the premise. That one asks whether the
    /// tests were ADDED here; this one asks whether what was reverted is BEHAVIOUR they can reach,
    /// and a green run is the defect this gate names only when both answers are yes.
    pub(crate) reverted: &'s Attempts,
}

#[cfg(test)]
mod tests {
    use super::{Attributed, Coverage};
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
    fn a_scope_and_a_measurement_are_not_the_same_sentence() {
        // THE GREP KEY IS THE WHOLE POINT. A handoff pastes this output and a reviewer cites a
        // number out of it, so a line printed before any run may carry the same COUNT as the
        // verdict and must not carry the same CLAIM - that is how `1 of 1 added tests measured`
        // came to sit twenty lines above `0 of 1 added tests measured` in one output.
        let seven_of_eight = Coverage::Measured {
            measured: 7,
            unmeasured: vec![String::from("held")],
            not_runnable: Vec::new(),
        };
        assert_eq!(seven_of_eight.named(), "7 of 8 added tests named");
        assert_eq!(seven_of_eight.measured(Attributed::Nothing), "0 of 8 added tests measured");
        // The ratio itself is asserted where the witness can be minted:
        // `base::an_outcome_that_ran_nothing_has_earned_no_numerator` reads both directions off the
        // real outcomes. Constructing `Attributed::PerTest` here is a compile error, which is the
        // property this module is for.

        // Both halves of the unknown denominator too: the scope wording has to exist for every
        // state the ratio has, or a caller before the runs has nothing honest to print.
        let unknown = Coverage::Unknown {
            measured: 2,
            unestablished: vec![String::from("crates/x/src/odd.rs")],
            not_runnable: Vec::new(),
        };
        assert_eq!(
            unknown.named(),
            "2 added tests named, of a total this gate could not establish"
        );
        assert_eq!(
            unknown.measured(Attributed::Nothing),
            "0 added tests measured, of a total this gate could not establish"
        );

        // And the key itself: no scope wording, in any state, is greppable as a measurement.
        for scope in [seven_of_eight.named(), unknown.named()] {
            assert!(!scope.contains(super::MEASURED), "a scope is not a measurement: {scope}");
            assert!(scope.contains(super::NAMED), "a scope says what was named: {scope}");
        }
    }

    #[test]
    fn an_ignored_test_beside_a_runnable_one_is_named_rather_than_dropped() {
        // THE DEFECT, through the scan rather than at a synthetic value - which matters here,
        // because a runnable NEIGHBOUR is exactly what used to lose the ignored names: the scan
        // answered `Runnable` and the ignored list went with the answer that was thrown away. Then
        // an all-`#[ignore]`d file in an otherwise ordinary diff was in neither number and named
        // nowhere, and `0 of 1` was everything the reader got about it.
        let acceptance = concat!("#[test]\n", "#[ignore]\n", "fn needs_a_tier() {}\n");
        let files = vec![
            changed("crates/x/src/proved.rs", 1, &["#[test]", "fn proved() {}"]),
            changed(
                "crates/x/tests/provisioned.rs",
                1,
                &["#[test]", "#[ignore]", "fn needs_a_tier() {}"],
            ),
        ];
        let read = tree(&[
            ("crates/x/src/proved.rs", &one_test("proved")),
            ("crates/x/tests/provisioned.rs", acceptance),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let Scan::Runnable(measured) = Scan::of(&files, &[String::from("crates/x/src/proved.rs")], &read) else {
            panic!("the provable file names a test");
        };
        let coverage = Coverage::of(measured.tests(), &files, &read);
        // The two numbers are unchanged - an ignored test is in NEITHER, which is what stops every
        // acceptance-heavy branch reading as partially measured forever.
        assert_eq!(coverage.ratio(), "1 of 1 added tests measured");
        // And the third list is what removes the misreading.
        assert_eq!(coverage.not_runnable(), [String::from("needs_a_tier")]);
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
