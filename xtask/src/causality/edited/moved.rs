//! A PURE MOVE of a test is neither a deletion nor an addition.
//!
//! [`super::deletion_in`] compares each file only with its own base, so a test deleted from one
//! file and added unchanged in another reads as deleted and the range is refused with
//! `Plan::DeletedTests` - the wrong answer over the split this repository asks for when a file hits
//! the line cap. [`unmoved`] takes out of a file's removed lines those inside a `fn` item that the
//! range re-added, unchanged, in another file, and `super::super::plan` asks the deletion question
//! of what is left. [`carried`] is the same match on the added side: the range's added tests are
//! scanned without the lines of an item it moved. Nothing else is excused, so every other refusal
//! still fires.
//!
//! **THE MATCH IS ONE-TO-ONE, ON NAME AND TOKENS.** An item is its attached attributes through its
//! closing brace, lexed as Rust tokens, so comments (doc comments too), indentation and line breaks
//! do not matter, and neither does a visibility on the item's OWN signature - a helper moved into a
//! sibling module gains `pub(super)`. One changed literal or path, a removed assertion, a dropped
//! `#[test]`, an added `#[ignore]` or a visibility anywhere in the body does. Each re-added
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
//! - **A move and a rename** is still a deletion, and so is an item this lexer cannot read (an
//!   unbalanced slice, a stray delimiter): it has no key, so it matches nothing. A reflow that adds
//!   or drops a trailing comma changes the key too, because `(a,)` and `(a)` differ. A string's
//!   `\`-newline continuation is keyed by value, so re-indenting it is layout as well.
//! - **Equal tokens are not equal resolution.** The destination may import a different item under
//!   a name the test uses; only `fn` items are compared, never a `use`, `const` or fixture file - the
//!   same reach `deletion_in` has, which names nothing for those either.
//! - **A carried test is measured by no run**, because [`carried`] takes its added lines out of the
//!   scan: a range that only moves tests has nothing to prove and passes. What the move did not
//!   change in a test's own body is not re-checked either, which is what the previous bullet says.
//! - **A destination counts as built when a target root or a `mod` declaration reaches it**
//!   ([`is_built`]), read without evaluating `cfg`. A test moved anywhere else keeps its added lines
//!   in the scan and is refused as an added test.
//! - **A moved file that also holds a new test** is reverted to base with the other files that add
//!   no test, so a base tree that still declares it may not build: INCONCLUSIVE, not a pass.

use std::ops::RangeInclusive;

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

use crate::causality::attributes::attached;
use crate::causality::diff::{ChangedFile, RemovedLine};
use crate::causality::names::Ident;
use crate::causality::place::is_built;
use crate::causality::plan::{Plan, plan_with_deleted};
use crate::causality::provenance::Reach;
use crate::causality::regions::{PostImage, item_end};
use crate::causality::scoped::{Code, function_name};

/// One outermost `fn` item of an image: the key a move is matched on, and where it sat.
#[derive(Debug)]
struct Item {
    name: Ident,
    /// `None` when the slice does not lex, which matches nothing.
    tokens: Option<Vec<String>>,
    /// 1-based, attached attributes through closing brace - the numbering `RemovedLine` uses.
    span: RangeInclusive<usize>,
}

impl Item {
    fn is(&self, other: &Self) -> bool {
        self.name == other.name && self.tokens.is_some() && self.tokens == other.tokens
    }
}

/// Every `fn` item in `text` not nested in another, so a move is matched as the unit it moved in.
fn items(text: Option<String>) -> Vec<Item> {
    let text = text.unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let code = Code::of(&text);
    let mut out = Vec::new();
    let mut index = 0_usize;
    while index < lines.len() {
        let declared = function_name(&code, index);
        let Some(name) = declared else {
            index += 1;
            continue;
        };
        let start = attached(&lines, index).first().map_or(index, |(at, _)| *at);
        let last = item_end(&lines, index);
        let tokens = key(&lines.get(start..=last).unwrap_or_default().join("\n"));
        out.push(Item {
            name,
            tokens,
            span: start.saturating_add(1)..=last.saturating_add(1),
        });
        index = last.saturating_add(1);
    }
    out
}

/// `source`'s tokens with comments and layout gone and its own leading visibility dropped, or `None`
/// if it does not lex.
fn key(source: &str) -> Option<Vec<String>> {
    let trees: Vec<TokenTree> = source.parse::<TokenStream>().ok()?.into_iter().collect();
    let mut signature = 0_usize;
    while let [TokenTree::Punct(hash), TokenTree::Group(attribute), ..] = trees.get(signature..).unwrap_or_default()
        && hash.as_char() == '#'
        && attribute.delimiter() == Delimiter::Bracket
    {
        signature = signature.saturating_add(2);
    }
    let visibility = match trees.get(signature..).unwrap_or_default() {
        [TokenTree::Ident(vis), TokenTree::Group(scope), ..] if vis == "pub" && scope.delimiter() == Delimiter::Parenthesis => 2,
        [TokenTree::Ident(vis), ..] if vis == "pub" => 1,
        _ => 0,
    };
    let own = trees.get(..signature).unwrap_or_default().iter();
    let rest = trees.get(signature.saturating_add(visibility)..).unwrap_or_default().iter();
    let mut out = Vec::new();
    flatten(own.chain(rest).cloned().collect(), &mut out);
    Some(out)
}

/// Every token of `stream`, delimiters included, less `#[doc = ..]` - the form a doc comment lexes to.
fn flatten(stream: TokenStream, out: &mut Vec<String>) {
    let mut trees = stream.into_iter().peekable();
    while let Some(tree) = trees.next() {
        match tree {
            TokenTree::Punct(hash) if hash.as_char() == '#' && trees.peek().is_some_and(is_doc) => {
                trees.next();
            }
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                out.push(String::from(open));
                flatten(group.stream(), out);
                out.push(String::from(close));
            }
            // Joint keeps `&&` apart from `& &`. Not after `?`, `,` or `;`, which start no longer
            // operator: rustfmt breaks `x?.y()` into `x?` and `.y()` at a different depth.
            TokenTree::Punct(punct) => {
                let joint = punct.spacing() == Spacing::Joint && !matches!(punct.as_char(), '?' | ',' | ';');
                out.push(format!("{}{}", punct.as_char(), if joint { "~" } else { "" }));
            }
            TokenTree::Ident(ident) => out.push(ident.to_string()),
            TokenTree::Literal(literal) => out.push(continued(&literal.to_string())),
        }
    }
}

/// `literal` with each `\`-newline continuation's following whitespace dropped, as the compiler
/// drops it: re-indenting a moved string changes its source, never its value. A raw string has no
/// escapes and is kept whole.
fn continued(literal: &str) -> String {
    if !literal.trim_start_matches(['b', 'c']).starts_with('"') {
        return String::from(literal);
    }
    let mut out = String::with_capacity(literal.len());
    let mut chars = literal.chars().peekable();
    while let Some(one) = chars.next() {
        out.push(one);
        if one != '\\' {
            continue;
        }
        match chars.next() {
            Some('\n') => {
                out.push('\n');
                while chars.next_if(|next| matches!(next, ' ' | '\t' | '\n' | '\r')).is_some() {}
            }
            Some(escaped) => out.push(escaped),
            None => {}
        }
    }
    out
}

/// `[doc = ..]` only: `#[doc(hidden)]` and `#[doc(alias = ..)]` are attributes, not comments.
fn is_doc(tree: &TokenTree) -> bool {
    let TokenTree::Group(group) = tree else {
        return false;
    };
    let mut inner = group.stream().into_iter();
    group.delimiter() == Delimiter::Bracket
        && matches!(inner.next(), Some(TokenTree::Ident(doc)) if doc == "doc")
        && matches!(inner.next(), Some(TokenTree::Punct(equals)) if equals.as_char() == '=')
}

/// Each file's removed lines, less those inside an item the range moved unchanged to another file.
///
/// Index-aligned with `files` followed by `gone`.
pub(crate) fn unmoved(
    files: &[ChangedFile],
    gone: &[ChangedFile],
    base: &PostImage<'_>,
    read: &PostImage<'_>,
) -> Vec<Vec<RemovedLine>> {
    let moved = moves(files, gone, base, read);
    files
        .iter()
        .chain(gone)
        .enumerate()
        .map(|(at, file)| {
            file.removed
                .iter()
                .filter(|line| !moved.iter().any(|one| one.from == at && one.left.contains(&line.before)))
                .cloned()
                .collect()
        })
        .collect()
}

/// One item the range moved unchanged: its pre-image span in the file it left, and its post-image
/// span in the file it landed in, each an index into `files` followed by `gone`.
struct Move {
    from: usize,
    left: RangeInclusive<usize>,
    to: usize,
    landed: RangeInclusive<usize>,
}

/// Every item the range deleted from one file and re-added, unchanged, in another.
///
/// Index-aligned with `files` followed by `gone`, the files the range deleted whole: one of those
/// can be the place a test moved FROM, and has no post-image to be a place it moved to. A file cargo
/// does not compile keeps every line: nothing it held can be a test, and nothing it gained can be a
/// test's new home.
fn moves(files: &[ChangedFile], gone: &[ChangedFile], base: &PostImage<'_>, read: &PostImage<'_>) -> Vec<Move> {
    let mut deleted: Vec<(usize, Item)> = Vec::new();
    let mut added: Vec<(usize, Item)> = Vec::new();
    for (at, file) in files.iter().chain(gone).enumerate() {
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
        added.extend(after.into_iter().map(|item| (at, item)));
    }
    let mut moved = Vec::new();
    for (from, item) in deleted {
        if let Some(copy) = added.iter().position(|(_, one)| one.is(&item)) {
            let (to, landed) = added.swap_remove(copy);
            moved.push(Move {
                from,
                left: item.span,
                to,
                landed: landed.span,
            });
        }
    }
    moved
}

/// `files` less the added lines inside an item the range moved unchanged from another file, so a
/// moved test is neither added nor deleted: [`unmoved`] is the deletion side of the same match.
///
/// Only into a file a cargo target builds ([`is_built`]): a test moved where no target runs it is
/// measured as added, and refused as one.
pub(crate) fn carried(
    mut files: Vec<ChangedFile>,
    gone: &[ChangedFile],
    base: &PostImage<'_>,
    read: &PostImage<'_>,
) -> Vec<ChangedFile> {
    let moved = moves(&files, gone, base, read);
    for (at, file) in files.iter_mut().enumerate() {
        if !is_built(&file.path, read) {
            continue;
        }
        file.added
            .retain(|line| !moved.iter().any(|one| one.to == at && one.landed.contains(&line.number)));
    }
    files
}

/// [`plan_with_deleted`] over `files` less the tests the range moved unchanged, and those files:
/// what a scan of the plan's tests has to read.
pub(crate) fn plan_carrying(
    files: Vec<ChangedFile>,
    gone: &[ChangedFile],
    read: &PostImage<'_>,
    base: &PostImage<'_>,
) -> (Vec<ChangedFile>, Plan) {
    let files = carried(files, gone, base, read);
    let plan = plan_with_deleted(&files, gone, read, base);
    (files, plan)
}
