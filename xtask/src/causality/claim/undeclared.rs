//! A claim mutation this range ADDED that no `Claim-Cell:` in the range declares.
//!
//! Without the trailer nothing in this gate applies the patch, so the test it was written for
//! reaches the ordinary proof alone - measured on `github.com/telekom/sutura#970`: exit 0 with
//! *NO BASE BEHAVIOUR TO COMPARE AGAINST*, `0 of 1` measured, the mutation never run. An added
//! patch is its author's claim that a mutation proves the cell, so the undeclared shape is refused
//! rather than ignored. A patch the base already has is left alone: re-anchoring one is not a new
//! claim, and `super::super::rot` re-proves every committed patch on its own schedule.

use super::{Claim, MUTATIONS_DIR};
use crate::Verdict;

/// The cells whose patch `touched` adds - absent at base - and no declaration in `claim` names.
pub(in crate::causality) fn undeclared<'a>(
    touched: &'a [String],
    claim: Option<&Claim>,
    at_base: impl Fn(&str) -> bool,
) -> Vec<&'a str> {
    touched
        .iter()
        .filter_map(|path| {
            let cell = path.strip_prefix(MUTATIONS_DIR)?.strip_prefix('/')?.strip_suffix(".patch")?;
            let declared = claim.is_some_and(|claim| claim.cells().iter().any(|named| named == cell));
            (!declared && !at_base(path)).then_some(cell)
        })
        .collect()
}

/// Refuse `cells`, naming each patch and the two ways out.
pub(in crate::causality) fn report(cells: &[&str]) -> Verdict {
    eprintln!("xtask test-causality: FAILED - a claim mutation this range adds is declared by no `Claim-Cell:`");
    for cell in cells {
        eprintln!("  undeclared:  {MUTATIONS_DIR}/{cell}.patch");
    }
    eprintln!();
    eprintln!("Without the trailer this gate never applies the patch, so it proves nothing about the cell.");
    eprintln!("Declare `Claim-Cell: <name>` on the commit that adds the test, or drop the patch.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{Claim, undeclared};

    #[test]
    fn only_an_added_patch_no_trailer_names_is_undeclared() {
        let touched: Vec<String> = [
            "devco/claim-mutations/added_alone.patch",
            "devco/claim-mutations/added_and_declared.patch",
            "devco/claim-mutations/already_at_base.patch",
            "devco/claim-mutations/nested/not_a_cell.txt",
            "crates/x/src/lib.rs",
        ]
        .map(String::from)
        .to_vec();
        let claim = Claim::synthetic(["added_and_declared"].into_iter());
        let at_base = |path: &str| path.contains("already_at_base");

        assert_eq!(undeclared(&touched, claim.as_ref(), at_base), vec!["added_alone"]);
        assert_eq!(undeclared(&touched, None, at_base), vec!["added_alone", "added_and_declared"]);
    }
}
