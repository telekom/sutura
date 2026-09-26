//! A PURE MOVE of a test is not a deletion.
//!
//! [`super::deletion_in`] compares each file only with its own base, so a test deleted from one
//! file and added unchanged in another reads as deleted and the range is refused with
//! `Plan::DeletedTests` - the wrong answer over the split this repository asks for when a file hits
//! the line cap. [`unmoved`] takes out of a file's removed lines those inside a `fn` item that the
//! range re-added, unchanged, in another file, and `super::super::plan` asks the deletion question
//! of what is left. Nothing else is excused, so every other refusal still fires.
//!
//! **THE MATCH IS ONE-TO-ONE, ON NAME AND TOKENS.** An item is its attached attributes through its
//! closing brace, split on whitespace, so indentation and line breaks do not matter and one changed
//! literal, a removed assertion, a dropped `#[test]` or an added `#[ignore]` does. Each re-added
//! item excuses ONE deleted item and is spent: a move beside a same-named test deleted or weakened
//! elsewhere excuses only the move. A name that ALSO has a new version in the file it left is not a
//! move candidate there - that file rewrote it, which is an edit, not a move.
//!
//! **A HELPER IS AN ITEM TOO**, because `deletion_in` names a test from its called helper's removed
//! lines. A helper moved unchanged is excused with its callers; one changed on the way is not, so
//! the test calling it stays named even though the test itself moved unchanged.
//!
//! What this does NOT cover, stated next to the claim:
//! - **A move within one file** is still refused: an item whose identical copy stays in its own file
//!   cancels there and excuses nothing.
//! - **A move and a rename**, or a move rustfmt reflowed differently at its new depth, is still a
//!   deletion - both change the key.
//! - **Equal tokens are not equal resolution.** The destination may import a different item under
//!   a name the test uses; only `fn` items are compared, never a `use`, `const` or fixture file - the
//!   same reach `deletion_in` has, which names nothing for those either.
//! - **The re-added test is then an ADDED test** and is measured as one: a wholly moved set is
//!   `BaseOutcome::GreenAfterAMove`, INCONCLUSIVE, not a pass.

use std::ops::RangeInclusive;

use crate::causality::attributes::attached;
use crate::causality::diff::{ChangedFile, RemovedLine};
use crate::causality::names::Ident;
use crate::causality::provenance::Reach;
use crate::causality::regions::{PostImage, item_end};
use crate::causality::scoped::function_name;

/// One outermost `fn` item of an image: the key a move is matched on, and where it sat.
#[derive(Debug)]
struct Item {
    name: Ident,
    tokens: Vec<String>,
    /// 1-based, attached attributes through closing brace - the numbering `RemovedLine` uses.
    span: RangeInclusive<usize>,
}

impl Item {
    fn is(&self, other: &Self) -> bool {
        self.name == other.name && self.tokens == other.tokens
    }
}

/// Every `fn` item in `text` not nested in another, so a move is matched as the unit it moved in.
fn items(text: Option<String>) -> Vec<Item> {
    let text = text.unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut index = 0_usize;
    while let Some(line) = lines.get(index) {
        let trimmed = line.trim();
        let Some(name) = function_name(trimmed).filter(|_| !trimmed.starts_with("//")) else {
            index += 1;
            continue;
        };
        let start = attached(&lines, index).first().map_or(index, |(at, _)| *at);
        let last = item_end(&lines, index);
        let tokens = lines
            .get(start..=last)
            .unwrap_or_default()
            .iter()
            .flat_map(|one| one.split_whitespace())
            .map(String::from)
            .collect();
        out.push(Item {
            name,
            tokens,
            span: start.saturating_add(1)..=last.saturating_add(1),
        });
        index = last.saturating_add(1);
    }
    out
}

/// Each file's removed lines, less those inside an item the range moved unchanged to another file.
///
/// Index-aligned with `files`. A file cargo does not compile keeps every removed line: nothing it
/// held can be a test, and nothing it gained can be a test's new home.
pub(crate) fn unmoved(files: &[ChangedFile], base: &PostImage<'_>, read: &PostImage<'_>) -> Vec<Vec<RemovedLine>> {
    let mut deleted: Vec<(usize, Item)> = Vec::new();
    let mut added: Vec<Item> = Vec::new();
    for (at, file) in files.iter().enumerate() {
        if !matches!(Reach::of(&file.path), Reach::Compiled) {
            continue;
        }
        let mut after = items(read(&file.path));
        let mut lost = Vec::new();
        for item in items(base(&file.before)) {
            match after.iter().position(|one| one.is(&item)) {
                Some(kept) => {
                    after.swap_remove(kept);
                }
                None => lost.push(item),
            }
        }
        lost.retain(|item| !after.iter().any(|one| one.name == item.name));
        deleted.extend(lost.into_iter().map(|item| (at, item)));
        added.append(&mut after);
    }
    let mut moved: Vec<(usize, Item)> = Vec::new();
    for (at, item) in deleted {
        if let Some(copy) = added.iter().position(|one| one.is(&item)) {
            added.swap_remove(copy);
            moved.push((at, item));
        }
    }
    files
        .iter()
        .enumerate()
        .map(|(at, file)| {
            file.removed
                .iter()
                .filter(|line| {
                    !moved
                        .iter()
                        .any(|(from, item)| *from == at && item.span.contains(&line.before))
                })
                .cloned()
                .collect()
        })
        .collect()
}
