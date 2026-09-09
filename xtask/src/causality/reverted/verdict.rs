//! What a run SAYS about a revert nothing in scope could reach.
//!
//! `super` decides; this speaks. The seam is the one `super::super::plan` and
//! `super::super::remedies` already draw, and the reason to keep it inside `reverted` rather than
//! beside the other remedies is that these sentences read `Excused`'s private fields: an excuse and
//! the words explaining it are one thing to keep true.
//!
//! **THE LINES ARE PURE AND THE PRINTING IS NOT**, which is `remedies`' own arrangement and its
//! reason: nothing in this venue captures stdout, so what a verdict may CLAIM has to be readable by
//! an assertion. What that does NOT cover is the `println!` itself; a mutation deleting the loop in
//! `explain` reddens nothing, and the same is true of every printed line in this gate.

use crate::Verdict;
use crate::causality::base::BaseOutcome;

use super::{Excused, Reverted, Why};

impl Excused {
    /// The line the verdict prints for it.
    pub(crate) fn line(&self) -> String {
        match self.why {
            Why::SameProgram => format!("    out of reach: {}  (blank lines and comments only)", self.path),
            Why::TestCodeElsewhere(ref package) => format!(
                "    out of reach: {}  (`{}` test code, and no test in scope is in `{}`)",
                self.path,
                package.as_str(),
                package.as_str()
            ),
        }
    }
}

/// Say that a green base run measured the partition, and how each reverted file is out of reach.
///
/// **The sentence lives beside the rule that decides it.** Every other verdict arm's words live in
/// `super::base`; this one's whole content is which excuse applied to which file, so an excuse and
/// the words explaining it are one thing to keep true rather than two. It also kept that module
/// under the unexemptable 1000-line cap, which is the honest second reason.
pub(crate) fn explain(excused: &[Excused], measured: &str, out: &mut Emit<'_>) -> Verdict {
    for line in unreachable_lines(excused, measured) {
        out(&line);
    }
    Verdict::Inconclusive
}

/// Where a verdict's lines go: `println!` in the gate, a `Vec` in a test.
///
/// **PURE LINES WERE NOT ENOUGH, and review measured the gap rather than arguing it.** Replacing
/// either emission with `let _unused = <the same pure call>;` left the suite at 1034 passed,
/// exit 0: the decision was asserted, the CLAIM was asserted, and whether either ever reached a
/// reader was not. That matters more here than for an ordinary remedy, because the contradiction
/// line is the only mechanism able to reveal a reach rule that is wrong - a gate that detects one
/// and says nothing is the failure this whole module exists to avoid, one level up.
pub(crate) type Emit<'a> = dyn FnMut(&str) + 'a;

/// Print to standard output, the gate's own sink.
pub(crate) fn to_stdout(line: &str) {
    println!("{line}");
}

/// Every line [`explain`] prints, in order.
///
/// PURE for the reason `super::remedies::scope_lines` is: nothing in this venue captures stdout,
/// and what a verdict may CLAIM is exactly the thing an assertion has to be able to read. The claim
/// that matters here is that each excused file is NAMED - a verdict saying *nothing was in reach*
/// over a list nobody printed is the shape this gate has already been wrong in twice.
fn unreachable_lines(excused: &[Excused], measured: &str) -> Vec<String> {
    let mut lines = vec![String::from(
        "  base: green - and nothing that was reverted is in reach of the tests in scope",
    )];
    lines.extend(excused.iter().map(Excused::line));
    lines.push(String::new());
    lines.extend(
        [
            "xtask test-causality: INCONCLUSIVE - the revert could not have reddened these tests.",
            "Every file this run put back at base restores no line a test in scope can execute,",
            "so they passed there for a reason that is a fact about the PARTITION rather than",
            "about them: nothing that could have made them red was reverted. That is the argument",
            "a build input already gets - a file that cannot be the thing under test proves",
            "nothing when it is reverted - reaching a file that WAS reverted.",
            "It is not a pass: this run carries NO red-before-green evidence. Prove each added",
            "test by MUTATION - break what it claims, one at a time, scoped to a whole test",
            "binary and never to a name pattern - or, if the change really is test-only, say",
            "plainly that it is not a regression test.",
        ]
        .map(String::from),
    );
    lines.push(format!(
        "This exit is INCONCLUSIVE (code 3) rather than a pass, and {measured}."
    ));
    lines
}

/// The falsifier's lines: a RED run over a revert `super` excused.
///
/// **A FALSIFIER, from two different places.** This module reads the diff and says the revert
/// restores nothing a scoped test can execute; nextest says a scoped test failed once it was
/// restored. Both cannot be right, and the pairing is the only evidence available that the reach
/// rule has a hole in it. Empty for [`Reverted::Behaviour`], because there is no claim to
/// contradict - so the ordinary passing run prints nothing new.
pub(crate) fn contradiction(outcome: &BaseOutcome, reverted: &Reverted) -> Vec<String> {
    if !matches!(
        *outcome,
        BaseOutcome::RedByAssertion { .. } | BaseOutcome::RedOutsideTheDiff { .. }
    ) {
        return Vec::new();
    }
    reverted
        .out_of_reach()
        .iter()
        .map(|one| format!("    CONTRADICTED: {} was classified as out of reach of these tests", one.path))
        .collect()
}

/// Emit the falsifier's lines, if this outcome has any.
///
/// Separate from [`contradiction`] so the DECISION and the EMISSION are each reachable by an
/// assertion: deleting this body used to redden nothing.
pub(crate) fn emit_contradiction(outcome: &BaseOutcome, reverted: &Reverted, out: &mut Emit<'_>) {
    for line in contradiction(outcome, reverted) {
        out(&line);
    }
}

/// #397's excused partition, for `super::base`'s tests.
///
/// `#[cfg(test)]` and built through the real classifier: `Excused`'s fields are private to this
/// module, so a sibling asserting on the falsifier cannot spell one, and a hand-built stand-in
/// would let it pin a shape the classifier never produces.
#[cfg(test)]
pub(crate) fn excused_for_tests() -> Reverted {
    let head = crate::causality::fixtures::tree(&[
        ("crates/other/Cargo.toml", &crate::causality::fixtures::manifest("other")),
        ("crates/other/src/prompt.rs", "#[cfg(test)]\nmod tests;\n"),
        ("crates/other/src/prompt/tests.rs", "// a whole file of tests\n"),
        ("crates/x/Cargo.toml", &crate::causality::fixtures::manifest("x")),
        ("crates/x/src/parse/tests.rs", "#[test]\nfn a_new_cell() {}\n"),
    ]);
    let files = vec![
        crate::causality::fixtures::changed("crates/other/src/prompt/tests.rs", 4, &["    assert_eq!(text, rendered);"]),
        crate::causality::fixtures::changed("crates/other/src/a.rs", 2, &["// only a comment"]),
        crate::causality::fixtures::changed("crates/other/src/b.rs", 2, &["// and another"]),
        crate::causality::fixtures::changed("crates/x/src/parse/tests.rs", 2, &["#[test]", "fn a_new_cell() {}"]),
    ];
    Reverted::of(
        &[
            String::from("crates/other/src/prompt/tests.rs"),
            String::from("crates/other/src/a.rs"),
            String::from("crates/other/src/b.rs"),
        ],
        &[String::from("crates/x/src/parse/tests.rs")],
        &files,
        &head,
        &head,
    )
}

#[cfg(test)]
mod tests {
    use super::excused_for_tests;
    use super::{contradiction, emit_contradiction, explain, unreachable_lines};
    use crate::Verdict;
    use crate::causality::base::BaseOutcome;
    use crate::causality::reverted::Reverted;

    /// A red-by-assertion outcome, the ordinary case the falsifier rides on.
    fn red() -> BaseOutcome {
        BaseOutcome::RedByAssertion {
            failed: vec![String::from("x tests::one")],
        }
    }

    #[test]
    fn the_inconclusive_verdict_names_every_file_it_excused_and_claims_no_proof() {
        // WHAT THE SENTENCE MAY SAY. A verdict asserting *nothing reverted was in reach* over a
        // list nobody printed is a claim with no witness, which is the defect class this gate has
        // already carried twice. So every excused file is named, and the same paste says in words
        // that the run proved nothing - the exit code alone has been misread here before.
        let excused = excused_for_tests();
        let lines = unreachable_lines(excused.out_of_reach(), "1 of 1 added tests measured");
        for path in [
            "crates/other/src/prompt/tests.rs",
            "crates/other/src/a.rs",
            "crates/other/src/b.rs",
        ] {
            assert!(lines.iter().any(|line| line.contains(path)), "{path} is not named: {lines:?}");
        }
        assert!(
            lines
                .iter()
                .any(|line| line.contains("INCONCLUSIVE (code 3) rather than a pass")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("NO red-before-green evidence")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_red_run_contradicts_this_rule_per_file_and_an_ordinary_pass_prints_nothing() {
        // The falsifier's own shape: it says which file the contradiction is about, and it is
        // silent on every run where no excuse was given - so a passing verdict does not grow a
        // line for a claim nobody made.
        assert!(contradiction(&red(), &Reverted::Behaviour).is_empty(), "a red run with no excuse contradicts nothing");
        let lines = contradiction(&red(), &excused_for_tests());
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines.iter().all(|line| line.contains("CONTRADICTED")), "{lines:?}");
    }

    /// Every line a call emitted, so "it reached a reader" is a thing an assertion can read.
    fn captured(run: impl FnOnce(&mut super::Emit<'_>)) -> Vec<String> {
        let mut lines = Vec::new();
        run(&mut |line: &str| lines.push(String::from(line)));
        lines
    }

    #[test]
    fn the_verdict_and_the_falsifier_are_emitted_and_not_merely_computed() {
        // MEASURED BY REVIEW: with the lines pure and the printing a bare loop, replacing either
        // emission with `let _unused = <the same pure call>;` left the suite green. The decision
        // was held, the wording was held, and whether either ever reached a reader was not - which
        // matters most for the contradiction line, the only mechanism able to surface a reach rule
        // that is wrong.
        let excused = excused_for_tests();
        let said = captured(|out| {
            let verdict = explain(excused.out_of_reach(), "1 of 1 added tests measured", out);
            assert_eq!(verdict, Verdict::Inconclusive);
        });
        assert!(
            said.iter().any(|line| line.contains("crates/other/src/prompt/tests.rs")),
            "the verdict names its excused files where somebody can read them: {said:?}"
        );
        assert!(said.iter().any(|line| line.contains("INCONCLUSIVE")), "{said:?}");

        let shouted = captured(|out| emit_contradiction(&red(), &excused, out));
        assert_eq!(shouted.len(), 3, "one per excused file: {shouted:?}");
        assert!(shouted.iter().all(|line| line.contains("CONTRADICTED")), "{shouted:?}");

        // And an outcome with nothing to contradict emits nothing at all.
        assert!(captured(|out| emit_contradiction(&BaseOutcome::Green, &excused, out)).is_empty(), "a green outcome emits nothing to contradict");
    }

    #[test]
    fn the_falsifier_fires_on_both_red_arms_and_on_nothing_else() {
        // MEASURED BY REVIEW rather than argued: with a loop in each red arm of `report_base`,
        // deleting the one in `RedOutsideTheDiff` reddened no test at all. The choice is one
        // function now, and this is what holds it.
        let excused = excused_for_tests();
        let outside = BaseOutcome::RedOutsideTheDiff {
            failed: vec![String::from("y tests::other")],
        };
        assert_eq!(contradiction(&red(), &excused).len(), 3, "a scoped failure contradicts it");
        assert_eq!(
            contradiction(&outside, &excused).len(),
            3,
            "so does one outside the filterset: the revert reddened SOMETHING"
        );
        // Nothing else does: a run that is not red made no claim to contradict, and neither does a
        // red one over a revert this module never excused.
        assert!(contradiction(&BaseOutcome::Green, &excused).is_empty(), "a green run has nothing to contradict");
        assert!(contradiction(&BaseOutcome::DidNotCompile, &excused).is_empty(), "a did-not-compile run made no claim to contradict");
        assert!(contradiction(&red(), &Reverted::Behaviour).is_empty(), "an unexplained red run has nothing to contradict");
        assert!(contradiction(&outside, &Reverted::Behaviour).is_empty(), "an unexplained red run contradicts nothing outside the diff");
    }
}
