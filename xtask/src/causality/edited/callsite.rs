//! A changed test file whose only change is a call-site argument a signature change forced is
//! EDITED, not added.
//!
//! The defect this closes. Change `render(&defs)` to take a wrapper and every existing test calling
//! it gains `render(&Wrapper::of(&defs))`. [`super::touches`] names each such test - an added line
//! lands inside its span - so its file was kept at HEAD for the base run, where it calls the NEW
//! signature against the reverted implementation: the base tree does not compile with nothing held
//! back, and a change with no defect in it was refused whatever else it proved.
//!
//! **THE RULE.** Such a file is put back to its BASE content for the base run, like any
//! implementation file: every test in it existed at base under the same path and name, so that
//! version compiles against the reverted implementation. Its tests are out of the proof - none of
//! them is evidence FOR the change - and a range where every test file in scope is edited, with none
//! added, is [`super::super::plan::Plan::EditedTests`]: INCONCLUSIVE, never a pass. A file mixing an
//! edited test with an added one is still ADDED as a whole, so the base run keeps it at HEAD as
//! before this rule and may still fail to build.
//!
//! **THE CRITERION, EXACTLY.** Both images lex as `proc_macro2` token streams, compared tree by tree
//! with punctuation spacing ignored. They must differ, and every difference must be one of:
//!
//! - **An argument of a call, wrapped.** Inside a `(..)` group directly after an identifier that is
//!   not a keyword and not a `fn` or `struct` name - a function, method or tuple-constructor call,
//!   never a macro - the arguments split at top-level commas (a trailing comma dropped) must pair up
//!   one to one, and each head argument is its base argument or contains it as one contiguous token
//!   run, with only identifiers, `&`, `:` and `(` before it and only `)` after it.
//!   `&defs` -> `&Wrapper::of(&defs)` qualifies; `x` -> `f(x, 2)`, `x` -> `-x`, `x` -> `x.into()` and
//!   `1` -> `2` do not.
//! - **A `use` item whose every imported name the OTHER image never mentions**, added or removed at
//!   any depth. An import no token of the other side names cannot change what that side calls. A
//!   glob, a `self` leaf or a visibility or attribute in front is never such an item.
//!
//! So a changed assertion, literal, expected value, statement, attribute, doc comment, test name or
//! helper body - anywhere in the file - leaves it ADDED, and the ordinary proof runs over it.
//!
//! **THE LIMIT.** The wrapper's own behaviour is not read: `f(x)` -> `f(weaken(x))` passes the
//! criterion exactly as `f(x)` -> `f(Wrapper::of(x))` does, because both are a call around the old
//! argument and no token comparison can tell an adaptor from a transformation. What bounds that is
//! the verdict, not the criterion - an edited file proves nothing and is never a pass on its own.
//! Where a file is judged is also a limit: a helper in ANOTHER file is judged there, and a revert of
//! an edited file under a declarer kept at HEAD is taken whatever the declarer's `use` lines expect.

use std::collections::BTreeSet;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use crate::causality::declared;
use crate::causality::diff::ChangedFile;
use crate::causality::plan::{Plan, partition};
use crate::causality::regions::PostImage;

/// Identifiers that open an expression rather than name a callee: `match (a, b)` is no call.
const KEYWORDS: &[&str] = &[
    "as", "break", "else", "for", "if", "in", "let", "loop", "match", "move", "mut", "return", "while", "yield",
];

/// [`partition`], with every EDITED test file put back to base, then [`declared::keep`].
///
/// Edited files are judged BEFORE `keep` so a file this reverts declares nothing `keep` holds at
/// HEAD, and put back AFTER it because `keep` would otherwise hold an edited test module at HEAD for
/// its declarer - the one tree in which it cannot compile.
pub(crate) fn separate(files: &[ChangedFile], read: &PostImage<'_>, base: &PostImage<'_>) -> Plan {
    let plan = partition(files, read);
    let Plan::Separable(mut separable) = plan else {
        return plan;
    };
    let edited: Vec<String> = separable
        .test_files
        .iter()
        .chain(&separable.held_back)
        .filter(|path| {
            base(path)
                .zip(read(path))
                .is_some_and(|(before, after)| wrapped(&before, &after))
        })
        .cloned()
        .collect();
    for list in [
        &mut separable.test_files,
        &mut separable.held_back,
        &mut separable.inseparable,
    ] {
        list.retain(|path| !edited.contains(path));
    }
    separable.revert.extend(edited.iter().cloned());
    match declared::keep(Plan::Separable(separable), read, base) {
        Plan::Separable(mut kept) => {
            for path in &edited {
                if !kept.revert.contains(path) {
                    kept.revert.push(path.clone());
                }
            }
            if kept.test_files.is_empty() {
                Plan::EditedTests(edited)
            } else {
                Plan::Separable(kept)
            }
        }
        other => other,
    }
}

/// Is `after` `before` with nothing changed but wrapped call-site arguments and unused imports?
pub(crate) fn wrapped(before: &str, after: &str) -> bool {
    let (Ok(before), Ok(after)) = (before.parse::<TokenStream>(), after.parse::<TokenStream>()) else {
        return false;
    };
    let (before, after) = (trees(before), trees(after));
    let names = Names {
        before: idents(&before),
        after: idents(&after),
    };
    flat(&before) != flat(&after) && same(&before, &after, &names)
}

/// Every identifier each image spells, at any depth.
struct Names {
    before: BTreeSet<String>,
    after: BTreeSet<String>,
}

/// One token, delimiters as their own entries, spacing dropped.
#[derive(PartialEq, Eq)]
enum Flat {
    Open(char),
    Close(char),
    Ident(String),
    Punct(char),
    Literal(String),
}

fn trees(stream: TokenStream) -> Vec<TokenTree> {
    stream.into_iter().collect()
}

fn idents(trees: &[TokenTree]) -> BTreeSet<String> {
    flat(trees)
        .into_iter()
        .filter_map(|one| match one {
            Flat::Ident(name) => Some(name),
            _ => None,
        })
        .collect()
}

fn flat(input: &[TokenTree]) -> Vec<Flat> {
    let mut out = Vec::new();
    for tree in input {
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ('(', ')'),
                    Delimiter::Brace => ('{', '}'),
                    Delimiter::Bracket => ('[', ']'),
                    Delimiter::None => (' ', ' '),
                };
                out.push(Flat::Open(open));
                out.extend(flat(&trees(group.stream())));
                out.push(Flat::Close(close));
            }
            TokenTree::Ident(ident) => out.push(Flat::Ident(ident.to_string())),
            TokenTree::Punct(punct) => out.push(Flat::Punct(punct.as_char())),
            TokenTree::Literal(literal) => out.push(Flat::Literal(literal.to_string())),
        }
    }
    out
}

/// The two sequences, each less its imports the other image never names, equal up to wrapped call
/// arguments.
fn same(before: &[TokenTree], after: &[TokenTree], names: &Names) -> bool {
    let before = unimported(before, &names.after);
    let after = unimported(after, &names.before);
    before.len() == after.len()
        && before.iter().zip(&after).enumerate().all(|(at, pair)| match pair {
            (TokenTree::Group(old), TokenTree::Group(new)) if old.delimiter() == new.delimiter() => {
                let (old_inner, new_inner) = (trees(old.stream()), trees(new.stream()));
                if old.delimiter() == Delimiter::Parenthesis && is_call(before.get(..at).unwrap_or_default()) {
                    arguments(&old_inner, &new_inner, names)
                } else {
                    same(&old_inner, &new_inner, names)
                }
            }
            (TokenTree::Group(_), _) | (_, TokenTree::Group(_)) => false,
            (old, new) => flat(std::slice::from_ref(old)) == flat(std::slice::from_ref(new)),
        })
}

/// Does a `(..)` group after `before` hold a call's arguments?
fn is_call(before: &[TokenTree]) -> bool {
    match before {
        [.., TokenTree::Ident(item), TokenTree::Ident(_)] if item == "fn" || item == "struct" => false,
        [.., TokenTree::Ident(callee)] => !KEYWORDS.contains(&callee.to_string().as_str()),
        _ => false,
    }
}

/// Paired one to one, each head argument its base argument or that argument wrapped.
fn arguments(before: &[TokenTree], after: &[TokenTree], names: &Names) -> bool {
    let (before, after) = (split(before), split(after));
    before.len() == after.len()
        && before
            .iter()
            .zip(&after)
            .all(|(old, new)| same(old, new, names) || wraps(old, new))
}

/// `trees` split at top-level commas, a trailing comma dropped.
fn split(trees: &[TokenTree]) -> Vec<Vec<TokenTree>> {
    let mut out: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tree in trees {
        match (tree, out.last_mut()) {
            (TokenTree::Punct(comma), _) if comma.as_char() == ',' => out.push(Vec::new()),
            (_, Some(last)) => last.push(tree.clone()),
            (_, None) => {}
        }
    }
    if out.last().is_some_and(Vec::is_empty) {
        out.pop();
    }
    out
}

/// Is `after` `before` behind a prefix of identifiers, `&`, `:` and `(` and a suffix of `)`?
fn wraps(before: &[TokenTree], after: &[TokenTree]) -> bool {
    let (old, new) = (flat(before), flat(after));
    !old.is_empty()
        && (1..=new.len().saturating_sub(old.len())).any(|at| {
            let Some((prefix, rest)) = new.split_at_checked(at) else {
                return false;
            };
            let Some((inner, suffix)) = rest.split_at_checked(old.len()) else {
                return false;
            };
            inner == old.as_slice()
                && prefix
                    .iter()
                    .all(|one| matches!(one, Flat::Ident(_) | Flat::Punct('&' | ':') | Flat::Open('(')))
                && suffix.iter().all(|one| *one == Flat::Close(')'))
        })
}

/// `trees` less every `use` item at this depth that imports only names `other` never spells.
fn unimported(trees: &[TokenTree], other: &BTreeSet<String>) -> Vec<TokenTree> {
    let mut out = Vec::new();
    let mut at = 0_usize;
    while let Some(tree) = trees.get(at) {
        let starts = match at.checked_sub(1).and_then(|previous| trees.get(previous)) {
            None => true,
            Some(TokenTree::Punct(semi)) => semi.as_char() == ';',
            Some(TokenTree::Group(block)) => block.delimiter() == Delimiter::Brace,
            Some(_) => false,
        };
        let end = trees
            .get(at..)
            .unwrap_or_default()
            .iter()
            .position(|one| matches!(one, TokenTree::Punct(semi) if semi.as_char() == ';'));
        if starts
            && matches!(tree, TokenTree::Ident(keyword) if keyword == "use")
            && let Some(end) = end
            && leaves(trees.get(at..at.saturating_add(end)).unwrap_or_default())
                .is_some_and(|leaves| !leaves.is_empty() && leaves.iter().all(|name| !other.contains(name)))
        {
            at = at.saturating_add(end).saturating_add(1);
            continue;
        }
        out.push(tree.clone());
        at = at.saturating_add(1);
    }
    out
}

/// The names a `use` item binds, or `None` for a glob or a `self` leaf.
fn leaves(item: &[TokenTree]) -> Option<Vec<String>> {
    let tokens = flat(item);
    let mut out = Vec::new();
    for (at, one) in tokens.iter().enumerate() {
        match (one, tokens.get(at.saturating_add(1))) {
            (Flat::Punct('*'), _) => return None,
            (Flat::Ident(name), None | Some(Flat::Punct(',') | Flat::Close('}'))) => {
                if name == "self" {
                    return None;
                }
                out.push(name.clone());
            }
            _ => {}
        }
    }
    Some(out)
}
