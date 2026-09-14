//! The two tables are one table, paired by position - and what that pairing may rest on.
//!
//! The fifth seam `venues.rs` has been split on, and the unexemptable 1000-line gate forced this
//! one as well. Split HERE because the rule had outgrown its `for` loop: what it CLAIMS is that
//! the Nth claims column names the Nth venue, and what it HELD was that every alphanumeric word of
//! the column is a SUBSTRING of that venue's name. Two escapes, both measured on the merged tree,
//! one edit at a time with the page restored after each:
//!
//! * **A column that names nothing passed.** Emptying the two-keys venue's header cell left the
//!   verdict byte-identical to the clean run at exit 0 - the walk found no word, so it found no
//!   unmatched word, and a rule whose whole subject is *does this column name that venue* passed by
//!   finding nothing. One file over, this gate's own [`super::sources::invoked`] and
//!   [`super::sources::test_tasks`] fail closed on exactly that state.
//! * **A substring is not a name.** Spelling columns 3 and 4 both `Real dataset key` - `key` sits
//!   inside `keys` - passed at exit 0 with the two venues' limit columns indistinguishable. The
//!   swap that then moves every limit was refused by three SIBLING rules and not by this one, so
//!   the reorder this rule exists for was caught by accident rather than by the rule.
//!
//! So a column word has to be a WORD of the venue's name, and a column spelling no word at all is
//! a column that names no venue.
//!
//! **What it still does not reach, stated where the claim is.** Two venues whose names share every
//! word a short column spells stay interchangeable here: `key` against `keys` is closed, a column
//! reading only `Real dataset` over either dataset venue is not. So the pairing is narrowed rather
//! than proven, and which venue a row DESCRIBES remains review's - both tables are prose on one
//! page, editable in one diff.
//!
//! And the error it can make in the OTHER direction, which is the one that gets a gate deleted: a
//! word is compared whole, so a column spelling `two-keys` over the venue named `two keys` is
//! refused though the page is right. No column on the page does that, and widening the comparison to
//! a word's parts would re-open the substring escape above - so the strict reading is deliberate and
//! this is where it is written down.
//!
//! **The split moved no assertion**, which is what `.agents/skills/sutura/gates` prescribes: the
//! end-to-end reorder test stayed in `venues.rs` and reaches this rule through `page_problems`, so
//! nothing is orphaned. This file carries implementation and its own tests together, so the plan is
//! `NotSeparable`, nothing is reverted and no base build is attempted - the verdict is `NOT
//! MECHANICALLY SEPARABLE` and a mutation stands in for the proof.

use super::PAGE;
use super::page::Venue;

/// One word with the sentence's own punctuation removed - a table pipe, a comma, the emphasis a
/// heading is written in. **The ends only.** `trim_matches` does not reach inside a word, so
/// `two-keys` stays `two-keys` - this sentence claimed the opposite until it was measured, which is
/// the defect this whole file is about, one scope down.
fn bare(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_ascii_alphanumeric())
}

/// The words a cell spells, with the empty ones dropped.
fn words(cell: &str) -> Vec<&str> {
    cell.split_whitespace().map(bare).filter(|word| !word.is_empty()).collect()
}

/// Everything wrong with the pairing of the claims matrix's columns onto the venues table's rows.
///
/// The caller has already refused a column count that differs from the row count, so this reads
/// two lists of equal length and asks of each pair only what the printed message claims: does this
/// column name that venue.
pub(super) fn problems(listed: &[Venue], columns: &[&String]) -> Vec<String> {
    let mut problems = Vec::new();
    for (venue, column) in listed.iter().zip(columns) {
        let spelled = words(&venue.name);
        let cited = words(column);
        if cited.is_empty() {
            problems.push(format!(
                "{PAGE}: the claims column standing over `{}` spells no word at all, so there is \
                 nothing to compare against the row in the same position - and a rule with nothing \
                 to compare would pass by finding nothing. The two tables are paired by ORDER, and \
                 a column that names no venue is a set of limits nothing can be read against",
                venue.name
            ));
            continue;
        }
        let unmatched: Vec<&str> = cited
            .into_iter()
            .filter(|word| !spelled.iter().any(|word_of_name| word_of_name.eq_ignore_ascii_case(word)))
            .collect();
        if !unmatched.is_empty() {
            problems.push(format!(
                "{PAGE}: the claims column `{column}` does not name the venue in the same position \
                 (`{}`) - {unmatched:?} is no WORD of that name, and a substring inside a longer \
                 word is not a name. The two tables are paired by ORDER, so a reorder in one of \
                 them silently moves every limit",
                venue.name
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::super::page::Venue;

    /// A row, with only the cell this rule reads spelled out.
    fn venue(name: &str) -> Venue {
        Venue {
            name: name.to_owned(),
            runs: String::from("in process, every run"),
            reached: String::from("`just test`"),
        }
    }

    #[test]
    fn a_column_the_venue_s_own_words_spell_is_the_pairing_this_rule_accepts() {
        // The direction that gets a gate deleted: a correct page reddening. Every shape the real
        // page uses is here - a comma inside the column, an article the venue name carries and the
        // column drops, and a column shorter than the name it names.
        let listed = [
            venue("A fake at the port"),
            venue("A real dataset under two keys"),
            venue("A real token exchange, and two grants"),
        ];
        let columns = [
            String::from("Fake at the port"),
            String::from("Real dataset, two keys"),
            String::from("Real exchange"),
        ];
        let columns: Vec<&String> = columns.iter().collect();
        assert_eq!(super::problems(&listed, &columns), Vec::<String>::new());
    }

    #[test]
    fn a_column_that_spells_no_word_is_refused_rather_than_passing_by_finding_nothing() {
        // Measured on the merged tree before this rule existed: emptying this header cell left
        // `check-venues` byte-identical to the clean run at exit 0.
        let listed = [venue("A real dataset under two keys")];
        let empty = String::new();
        let found = super::problems(&listed, &[&empty]);
        assert!(
            found.iter().any(|problem| problem.contains("spells no word at all")),
            "{found:?}"
        );
        // And a cell holding only what a table puts around a word is the same state one spelling
        // further on, which is why the words are compared bare.
        let punctuation = String::from("--");
        let found = super::problems(&listed, &[&punctuation]);
        assert!(
            found.iter().any(|problem| problem.contains("spells no word at all")),
            "{found:?}"
        );
    }

    #[test]
    fn a_column_word_inside_a_longer_word_of_the_name_does_not_name_the_venue() {
        // `key` is inside `keys`, which is what made the two dataset venues' columns
        // interchangeable at exit 0 - and the reorder that then moves every limit was refused by
        // three sibling rules rather than by the rule that claims to catch a reorder.
        let listed = [venue("A real dataset under two keys")];
        let substring = String::from("Real dataset key");
        let found = super::problems(&listed, &[&substring]);
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("does not name the venue in the same position")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|problem| problem.contains("is no WORD of that name")),
            "{found:?}"
        );
    }
}
