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
//! WHY IT STATES RATHER THAN FAILS, which is the decision worth recording. Failing when the two
//! numbers differ would fail nearly every branch in this workspace, because a unit test beside the
//! code it tests IS the held-back shape and `report_not_separable` exists to say so. A gate that
//! reddens correct work gets disabled, and a disabled gate holds nothing - so the ratio is printed
//! and the unmeasured tests are NAMED, which is what a reader needs to decide whether the
//! remainder wants a mutation run by hand.
//!
//! A BARE TEST NAME IS NOT A KEY HERE - `super::scoped` carries the measurement - and these names
//! are not used as one. They are a POINTER for a person reading the output; every filter and every
//! comparison still goes through the package-plus-module-plus-name key. Two added tests sharing a
//! name collapse to one line, which under-reports the list and never the ratio: the counts come
//! from the keys.

use crate::causality::diff::ChangedFile;
use crate::causality::regions::PostImage;
use crate::causality::scoped::{AddedTest, Scan};

/// What the proof measured, against every runnable test the diff added.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Coverage {
    /// How many tests the filterset names.
    measured: usize,
    /// Added, runnable, and NOT in the filterset, by name.
    unmeasured: Vec<String>,
}

impl Coverage {
    /// What `measured` covers of the tests `files` added, read through `read`.
    ///
    /// The denominator comes from the same [`Scan::of`] the numerator does, over every changed
    /// file rather than the provable ones - one extractor for both numbers, so the ratio cannot
    /// drift from the filter. `#[ignore]`d tests are in neither: no run in this venue reaches one,
    /// and `report_only_ignored` is the answer when they are all there is.
    pub(crate) fn of(measured: &[AddedTest], files: &[ChangedFile], read: &PostImage<'_>) -> Self {
        let every: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
        let scan = Scan::of(files, &every, read);
        let added: &[AddedTest] = match scan {
            Scan::Runnable(ref all) => all.tests(),
            Scan::OnlyIgnored(_) | Scan::Unnamed => &[],
        };
        Self {
            measured: measured.len(),
            unmeasured: added
                .iter()
                .filter(|one| !measured.contains(one))
                .map(|one| String::from(one.name()))
                .collect(),
        }
    }

    /// The ratio, for the verdict line a reader cites.
    ///
    /// Always both numbers, never "all of them": one wording is greppable across a log and a
    /// branch cannot change which sentence it produced.
    pub(crate) fn ratio(&self) -> String {
        format!(
            "{} of {} added tests measured",
            self.measured,
            self.measured + self.unmeasured.len()
        )
    }

    /// The tests the proof left out, by name.
    pub(crate) fn unmeasured(&self) -> &[String] {
        &self.unmeasured
    }
}

/// What the two runs are scoped to, and how much of the diff that leaves unmeasured.
///
/// One value rather than two parameters, so the filter and its own limit reach the reconstruction
/// together. **What that buys is narrower than it sounds:** `super::base::report_base` takes the
/// sentence as a `&str`, so nothing stops a caller passing an empty one. What is held is that the
/// ratio is in REACH at every call site - not that it was printed, which is prose and is held by
/// the review of these four modules.
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
