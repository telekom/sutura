//! Whether the venue table records LEG 2 as proven - the one answer this gate exports.
//!
//! **Why it is here and its reader is in `check-guidance`.** That gate walks every prose file in
//! the repository and this one parses the claims matrix; a rule needing both would otherwise
//! duplicate one of them. So `guidance::leg_two` asks this question and holds the pages to the
//! answer, and its own header carries what the pair does not reach.
//!
//! **It carries its own tests, unlike every other split in this directory.** Those left their
//! assertions in `venues.rs` because the causality gate may revert a file that adds no `#[test]`,
//! which would orphan the declaration. A file that adds its own is not on that list -
//! `guidance/claims/anchors.rs` is the precedent - and `venues.rs` has no room left for three more.

use super::PAGE;
use super::page;

/// The claim rows that ARE leg 2 - a source executing as the asker - found by a substring of each
/// row's own claim text.
///
/// Two rows and not one, because leg 2 has two halves at two layers and either would prove it: the
/// account a subject's question runs as, and the rows a served caller sees. A claim that leg 2 is
/// proven is true the moment EITHER carries a citable verdict.
///
/// **Anchors and not whole cells**, so a reworded claim still resolves - and if an anchor stops
/// matching, [`citable_in`] answers `None` and its caller fails closed rather than reading an
/// absent row as *not proven*, which would be a rule passing for the wrong reason.
const LEG_TWO_ROWS: &[&str] = &[
    "resolve to two distinct",
    "executes as a verified human caller through the declared per-source map",
];

/// The verdicts that count as answering a claim, which is the page's own set.
///
/// `unrun` and `wired` are deliberately absent: the page's header says neither counts towards a
/// claim being answered, "which is not at all".
const CITABLE: &[&str] = &["yes", "can", "only here"];

/// Whether the venue table records leg 2 as proven, or `None` when the question cannot be answered
/// from the page.
///
/// `Some(true)` means some leg-2 row carries a verdict from [`CITABLE`]. `Some(false)` means none
/// does. `None` means the page is unreadable, the matrix will not parse, or one of
/// [`LEG_TWO_ROWS`] matches no row - all three being *this rule has lost its own input*, which a
/// caller reports rather than resolves.
pub(crate) fn leg_two_citable(root: &std::path::Path) -> Option<bool> {
    citable_in(&std::fs::read_to_string(root.join(PAGE)).ok()?)
}

/// The same answer over page text, so the three outcomes are assertable without a tree.
fn citable_in(text: &str) -> Option<bool> {
    let (_, rows) = page::claims(text).ok()?;
    let mut citable = false;
    for anchor in LEG_TWO_ROWS {
        let row = rows
            .iter()
            .find(|cells| cells.first().is_some_and(|claim| claim.contains(anchor)))?;
        citable |= row
            .iter()
            .skip(1)
            .filter_map(|cell| page::verdict(cell))
            .any(|verdict| CITABLE.contains(&verdict));
    }
    Some(citable)
}

#[cfg(test)]
mod tests {
    use super::citable_in;

    /// A claims matrix with the two leg-2 rows, each verdict as the caller chose.
    fn page(first: &str, second: &str) -> String {
        format!(
            "| Claim | Fake at the port |\n| --- | --- |\n\
             | Whether two distinct subjects resolve to two distinct principals | {first} |\n\
             | A served binary executes as a verified human caller through the declared per-source map | {second} |\n"
        )
    }

    #[test]
    fn a_matrix_recording_no_run_of_either_half_answers_not_proven() {
        // The state this tree is in, and the one the rule in `check-guidance` acts on. `wired` and
        // `unrun` are the two words a reader is most likely to mistake for evidence, so both are
        // here rather than only the `no`/`-` pair.
        assert_eq!(citable_in(&page("**wired** - no run observed", "-")), Some(false));
        assert_eq!(citable_in(&page("no", "**unrun** - the cell lives here")), Some(false));
    }

    #[test]
    fn either_half_going_citable_makes_leg_two_sayable_again() {
        // The self-retiring half: the rule this feeds stops applying the moment a run is recorded,
        // rather than having to be deleted by whoever records it. `can` counts for the reason the
        // page says it does - the standing test lives in another venue - and `only here` too.
        assert_eq!(citable_in(&page("**yes**, on the spawned binary", "-")), Some(true));
        assert_eq!(citable_in(&page("-", "can - the standing cell is elsewhere")), Some(true));
        assert_eq!(citable_in(&page("only here", "-")), Some(true));
    }

    #[test]
    fn a_matrix_missing_a_leg_two_row_answers_nothing_rather_than_not_proven() {
        // **FAIL CLOSED, and this is the cell that makes the anchors safe to keep as substrings.**
        // A row renamed past its anchor would otherwise read as *no citable verdict* - a rule
        // passing for the wrong reason, forever, with nothing to notice it. `None` is what the
        // caller turns into a refusal naming the anchors.
        assert!(citable_in(&page("**yes**", "-")).is_some());
        let one_row = "| Claim | Fake at the port |\n| --- | --- |\n| Whether two distinct subjects resolve to two distinct principals | no |\n";
        assert_eq!(citable_in(one_row), None);
        assert_eq!(citable_in("not a table at all"), None);
    }
}
