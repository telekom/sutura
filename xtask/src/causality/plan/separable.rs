//! What [`Separable`]'s lists compose into for each attempt, split out of `super` at the
//! unexemptable 1000-line cap.

use super::Separable;

impl Separable {
    /// Every file kept at HEAD on the first attempt whose own tests the proof does not measure.
    ///
    /// One list because the second attempt restores them together: that retry exists to put the
    /// tree coherently at base, and a held test helper calling a reverted neighbour is exactly a
    /// thing that does not compile until it goes too.
    pub(in crate::causality) fn held(&self) -> Vec<String> {
        self.held_back.iter().chain(self.test_only.iter()).cloned().collect()
    }

    /// What the first attempt keeps at HEAD: the held files PLUS the test files.
    ///
    /// Composition rather than recall: the membership's withdrawal decision (`membership::withdrawn`)
    /// asks whether an added crate still has ANY file at HEAD, and a crate whose only held file is a
    /// TEST one counts - so the test files must reach that question or a crate carrying a provable
    /// test would be withdrawn under a tree that then cannot run it.
    pub(in crate::causality) fn at_head_first_attempt(&self) -> Vec<String> {
        let mut at_head = self.held();
        at_head.extend(self.test_files.iter().cloned());
        at_head
    }

    /// Which build inputs the named attempt does NOT put at base - the held-at-HEAD remainder the
    /// output must name truthfully. Filtering rather than copying the list: a withdrawn manifest
    /// goes to base on the first attempt, and printing it as held at HEAD would be this gate
    /// describing a tree it did not build.
    pub(in crate::causality) fn unreverted_from(&self, reverting: &[String]) -> Vec<String> {
        self.build_inputs
            .iter()
            .filter(|path| !reverting.contains(path))
            .cloned()
            .collect()
    }
}
