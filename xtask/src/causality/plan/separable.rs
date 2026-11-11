//! What [`Separable`]'s lists compose into for each attempt, split out of `super` at the
//! unexemptable 1000-line cap.

use std::collections::BTreeSet;

use crate::causality::attributes::attached;
use crate::causality::names::Ident;
use crate::causality::regions::{PostImage, item_end};
use crate::causality::scoped::{Code, Scoped, function_name};

use super::Separable;

impl Separable {
    /// Accepted claim-only body edits may follow held files to base on a compile-failure retry.
    pub(in crate::causality) fn after_claims(
        &self,
        scoped: &Scoped,
        accepted: &BTreeSet<String>,
        head: &PostImage<'_>,
        base: &PostImage<'_>,
    ) -> Self {
        let mut plan = self.clone();
        plan.test_files.retain(|path| {
            let cells: Vec<&str> = scoped
                .tests()
                .iter()
                .filter(|cell| cell.file() == path.as_str())
                .map(crate::causality::place::AddedTest::name)
                .collect();
            let eligible = !cells.is_empty()
                && cells.iter().all(|name| accepted.contains(*name))
                && head(path).zip(base(path)).is_some_and(|(head, base)| {
                    outside_claim_bodies(&head, &cells)
                        .zip(outside_claim_bodies(&base, &cells))
                        .is_some_and(|(head, base)| head == base)
                });
            if eligible {
                plan.held_back.push(path.clone());
            }
            !eligible
        });
        plan
    }

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

/// Keep the exact file outside existing private, zero-argument, bare-test bodies.
///
/// ponytail: this uses the gate's lexer, not Rust name resolution or macro expansion. Visible
/// references and external modules refuse; macro-generated references remain outside its reach.
fn outside_claim_bodies(text: &str, names: &[&str]) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let code = Code::of(text);
    let written = code.span(0, lines.len().checked_sub(1)?);
    // A private test can still be called by a child module in another file. Admit only inline
    // module headers in the same narrow one-line shape this reader can account for.
    for line in written.lines() {
        let words: Vec<&str> = line
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|word| !word.is_empty())
            .collect();
        if words.contains(&"include") || words.contains(&"macro_rules") {
            return None;
        }
        if words.contains(&"mod") {
            let name = line.trim().strip_prefix("mod ")?.strip_suffix(" {")?;
            Ident::parse(name)?;
        }
    }
    let mut bodies = Vec::new();
    for name in names {
        // Exactly one identifier occurrence: no duplicate declaration, call, alias, or use as
        // a helper anywhere this lexer sees, including another accepted body's interior.
        if written
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|word| word == name)
            .count()
            != 1
        {
            return None;
        }
        let first = (0..lines.len()).find(|index| function_name(&code, *index).is_some_and(|found| found.as_str() == *name))?;
        if lines.get(first)?.trim() != format!("fn {name}() {{")
            || attached(&lines, first)
                .iter()
                .map(|(_, attribute)| *attribute)
                .collect::<Vec<_>>()
                != ["#[test]"]
        {
            return None;
        }
        let last = item_end(&lines, first);
        if last <= first || lines.get(last)?.trim() != "}" {
            return None;
        }
        bodies.push(first + 1..last);
    }
    let mut outside = String::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if !bodies.iter().any(|body| body.contains(&index)) {
            outside.push_str(line);
        }
    }
    Some(outside)
}
