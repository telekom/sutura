//! A changed test file whose only change is a call-site argument a signature change forced, or a
//! match arm widened to a variant an enum gained, is EDITED, not added.
//!
//! The defect this closes. Change `render(&defs)` to take a wrapper and every existing test calling
//! it gains `render(&Wrapper::of(&defs))`. [`super::touches`] names each such test - an added line
//! lands inside its span - so its file was kept at HEAD for the base run, where it calls the NEW
//! signature against the reverted implementation: the base tree does not compile with nothing held
//! back, and a change with no defect in it was refused whatever else it proved. Add a variant to an
//! enum and every exhaustive `match` on it in a test gains an alternative, with the same result.
//!
//! **THE RULE.** Such a file is put back to its BASE content for the base run, like any
//! implementation file: every test in it existed at base under the same path and name, so that
//! version compiles against the reverted implementation. Its tests are out of the proof - none of
//! them is evidence FOR the change. **Whenever any file is classed edited, the run names each one and
//! the tests in it, and the verdict is at best INCONCLUSIVE ([`cap`]), never a pass** - a range of
//! edited files alone is [`Plan::EditedTests`], and one beside a genuinely added test runs that test's
//! proof and then caps its pass. So a wrap this criterion wrongly admits is at worst a disclosed exit
//! 3, never a silent exit 0. A file mixing an edited test with an added one is still ADDED as a
//! whole, so the base run keeps it at HEAD as before this rule and may still fail to build.
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
//! - **An alternative a match arm gained for a name the base never spells.** `| Path`,
//!   `| Path { .. }` or `| Path(_, ..)` - fields binding nothing - directly before the arm's `=>`,
//!   the path's last segment an identifier the base file never spells.
//!   `A { .. } => panic!()` -> `A { .. } | New { .. } => panic!()` qualifies; a new arm, a guard, a
//!   binding, a name the base spells or an alternative anywhere but last does not.
//! - **An arm body the formatter put in a block.** `=> E,` and `=> { E }` compare equal where `E`
//!   holds no top-level `;`: a block holding only its tail expression is that expression, and a
//!   gained alternative pushes an arm past the line width.
//! - **An added `use` item whose every bound name the base never spells and some accepted wrapper
//!   does** - the import exists for the wrap. A glob, a `self` leaf, a visibility or attribute in front,
//!   or a REMOVED import is never such an item.
//!
//! So a changed assertion, literal, expected value, statement, attribute, doc comment, test name or
//! helper body - anywhere in the file - leaves it ADDED, and the ordinary proof runs over it.
//!
//! **THE LIMIT.** The wrapper's own behaviour is not read: `f(x)` -> `f(weaken(x))` passes the
//! criterion exactly as `f(x)` -> `f(Wrapper::of(x))` does, because both are a call around the old
//! argument and no token comparison can tell an adaptor from a transformation. An import a wrapper
//! names is in scope for the whole file too, so a TRAIT it brings in can change what an unchanged
//! `x.method()` elsewhere resolves to. A gained alternative's name is unspelled at base, yet a
//! variant it names may have existed there and reached another arm, so the test may now take a
//! different arm for it. All three are bounded by the verdict, not the criterion: an edited
//! file is named and caps the run at INCONCLUSIVE. Where a file is judged is also a limit: a helper
//! in ANOTHER file is judged there, and a revert of an edited file under a declarer kept at HEAD is
//! taken whatever the declarer's `use` lines expect.

use std::collections::BTreeSet;

use proc_macro2::{Delimiter, Group, Punct, Spacing, TokenStream, TokenTree};

use crate::Verdict;
use crate::causality::declared;
use crate::causality::diff::ChangedFile;
use crate::causality::plan::{Plan, partition};
use crate::causality::regions::{PostImage, scope};

/// Identifiers that open an expression rather than name a callee: `match (a, b)` is no call.
const KEYWORDS: &[&str] = &[
    "as", "break", "else", "for", "if", "in", "let", "loop", "match", "move", "mut", "return", "while", "yield",
];

/// One test file classed edited, and the tests in it whose span or helper the diff touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Edited {
    pub(crate) path: String,
    pub(crate) tests: Vec<String>,
}

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
    let edited: Vec<Edited> = files
        .iter()
        .filter(|file| separable.test_files.contains(&file.path) || separable.held_back.contains(&file.path))
        .filter(|file| {
            base(&file.path)
                .zip(read(&file.path))
                .is_some_and(|(before, after)| wrapped(&before, &after))
        })
        .map(|file| Edited {
            path: file.path.clone(),
            tests: touched(file, read),
        })
        .collect();
    let paths: Vec<String> = edited.iter().map(|one| one.path.clone()).collect();
    for list in [
        &mut separable.test_files,
        &mut separable.held_back,
        &mut separable.inseparable,
    ] {
        list.retain(|path| !paths.contains(path));
    }
    separable.revert.extend(paths.iter().cloned());
    match declared::keep(Plan::Separable(separable), read, base) {
        Plan::Separable(mut kept) => {
            for path in &paths {
                if !kept.revert.contains(path) {
                    kept.revert.push(path.clone());
                }
            }
            if kept.test_files.is_empty() {
                Plan::EditedTests(edited)
            } else {
                kept.edited = edited;
                Plan::Separable(kept)
            }
        }
        other => other,
    }
}

/// The tests in `file` an added line reached, directly or through a helper they call.
fn touched(file: &ChangedFile, read: &PostImage<'_>) -> Vec<String> {
    let text = read(&file.path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let direct = super::touched_in(&lines, &file.added);
    let callers = super::edited_helper_caller(&lines, &file.added, &scope(&file.path, read));
    let names: BTreeSet<String> = direct
        .iter()
        .map(|test| String::from(test.name().as_str()))
        .chain(callers.iter().map(|caller| String::from(caller.name().as_str())))
        .collect();
    names.into_iter().collect()
}

/// Name every edited file and its tests; nothing when there are none.
pub(crate) fn name(edited: &[Edited]) {
    for one in edited {
        println!(
            "  edited, not measured: {}  (only call-site arguments wrapped or match arms widened; the base run takes its base version)",
            one.path
        );
        for test in &one.tests {
            println!("    edited test: {test}");
        }
    }
}

/// `verdict`, never better than INCONCLUSIVE when any file was classed edited: the edited tests were
/// not run against base, and a wrap this criterion admits wrongly would otherwise pass silently.
pub(crate) fn cap(edited: &[Edited], verdict: Verdict) -> Verdict {
    if edited.is_empty() || verdict != Verdict::Pass {
        return verdict;
    }
    println!();
    println!("xtask test-causality: INCONCLUSIVE - the proof above passed, but the edited test file(s) named");
    println!("above were reverted unmeasured and prove nothing about this change. State why each wrap is");
    println!("an adaptor rather than a transformation in the handoff. Exit code 3, not a pass.");
    Verdict::Inconclusive
}

/// The verdict for a range whose every test file in scope only wrapped call-site arguments or widened
/// a match arm.
pub(crate) fn report(edited: &[Edited]) -> Verdict {
    name(edited);
    println!();
    println!("xtask test-causality: INCONCLUSIVE - every test file in scope only wrapped call-site arguments");
    println!("or widened a match arm, so no test in scope is new and none was run. Add a test that is red on base, or prove the");
    println!("change by MUTATION and state it in the handoff. Exit code 3, not a pass.");
    Verdict::Inconclusive
}

/// Is `after` `before` with nothing changed but wrapped call-site arguments, the imports the
/// wrappers need and alternatives match arms gained?
pub(crate) fn wrapped(before: &str, after: &str) -> bool {
    let (Ok(before), Ok(after)) = (before.parse::<TokenStream>(), after.parse::<TokenStream>()) else {
        return false;
    };
    let (before, after) = (trees(before), trees(after));
    let spelled = idents(&before);
    flat(&before) != flat(&after) && same(&before, &after, &spelled).is_some_and(|seen| seen.imported.is_subset(&seen.wrappers))
}

/// What an accepted comparison relied on: the names its wrappers spell, and the names its added
/// imports bind.
#[derive(Default)]
struct Seen {
    wrappers: BTreeSet<String>,
    imported: BTreeSet<String>,
}

impl Seen {
    fn and(mut self, other: Self) -> Self {
        self.wrappers.extend(other.wrappers);
        self.imported.extend(other.imported);
        self
    }
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
    names(&flat(trees))
}

fn names(tokens: &[Flat]) -> BTreeSet<String> {
    tokens
        .iter()
        .filter_map(|one| match one {
            Flat::Ident(name) => Some(name.clone()),
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

/// The two sequences, the head's added imports and gained alternatives and both sides' arm blocks
/// aside, equal up to wrapped call arguments - and what that relied on - or `None`.
fn same(before: &[TokenTree], after: &[TokenTree], spelled: &BTreeSet<String>) -> Option<Seen> {
    let (after, imported) = unimported(after, spelled);
    let (before, after) = (unblocked(before), unblocked(&unarmed(&after, spelled)));
    if before.len() != after.len() {
        return None;
    }
    let mut seen = Seen {
        imported,
        ..Seen::default()
    };
    for (at, pair) in before.iter().zip(&after).enumerate() {
        let more = match pair {
            (TokenTree::Group(old), TokenTree::Group(new)) if old.delimiter() == new.delimiter() => {
                let (old_inner, new_inner) = (trees(old.stream()), trees(new.stream()));
                if old.delimiter() == Delimiter::Parenthesis && is_call(before.get(..at).unwrap_or_default()) {
                    arguments(&old_inner, &new_inner, spelled)?
                } else {
                    same(&old_inner, &new_inner, spelled)?
                }
            }
            (TokenTree::Group(_), _) | (_, TokenTree::Group(_)) => return None,
            (old, new) if flat(std::slice::from_ref(old)) == flat(std::slice::from_ref(new)) => Seen::default(),
            _ => return None,
        };
        seen = seen.and(more);
    }
    Some(seen)
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
fn arguments(before: &[TokenTree], after: &[TokenTree], spelled: &BTreeSet<String>) -> Option<Seen> {
    let (before, after) = (split(before), split(after));
    if before.len() != after.len() {
        return None;
    }
    let mut seen = Seen::default();
    for (old, new) in before.iter().zip(&after) {
        seen = seen.and(same(old, new, spelled).or_else(|| wraps(old, new))?);
    }
    Some(seen)
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

/// `after` as `before` behind a prefix of identifiers, `&`, `:` and `(` and a suffix of `)`, with
/// the prefix's names, or `None`.
fn wraps(before: &[TokenTree], after: &[TokenTree]) -> Option<Seen> {
    let (old, new) = (flat(before), flat(after));
    if old.is_empty() {
        return None;
    }
    (1..=new.len().saturating_sub(old.len())).find_map(|at| {
        let (prefix, rest) = new.split_at_checked(at)?;
        let (inner, suffix) = rest.split_at_checked(old.len())?;
        (inner == old.as_slice()
            && prefix
                .iter()
                .all(|one| matches!(one, Flat::Ident(_) | Flat::Punct('&' | ':') | Flat::Open('(')))
            && suffix.iter().all(|one| *one == Flat::Close(')')))
        .then(|| Seen {
            wrappers: names(prefix),
            ..Seen::default()
        })
    })
}

/// A sequence with its added imports taken out, and the names they bind.
type Unimported = (Vec<TokenTree>, BTreeSet<String>);

/// `trees` less every `use` item at this depth that binds only names `spelled` lacks, and the names
/// those items bind.
fn unimported(trees: &[TokenTree], spelled: &BTreeSet<String>) -> Unimported {
    let mut out = Vec::new();
    let mut imported = BTreeSet::new();
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
            && let Some(leaves) = leaves(trees.get(at..at.saturating_add(end)).unwrap_or_default())
            && !leaves.is_empty()
            && leaves.iter().all(|name| !spelled.contains(name))
        {
            imported.extend(leaves);
            at = at.saturating_add(end).saturating_add(1);
            continue;
        }
        out.push(tree.clone());
        at = at.saturating_add(1);
    }
    (out, imported)
}

/// `seq` with every arm body `=> { E }` at this depth read as `=> E,`, where `E` holds no top-level `;`.
fn unblocked(seq: &[TokenTree]) -> Vec<TokenTree> {
    let mut out: Vec<TokenTree> = Vec::with_capacity(seq.len());
    let mut at = 0_usize;
    while let Some(tree) = seq.get(at) {
        at = at.saturating_add(1);
        let arrow = matches!(out.as_slice(), [.., TokenTree::Punct(eq), TokenTree::Punct(gt)] if eq.as_char() == '=' && gt.as_char() == '>');
        let body = match tree {
            TokenTree::Group(block) if arrow && block.delimiter() == Delimiter::Brace => Some(trees(block.stream())),
            _ => None,
        };
        let Some(body) = body.filter(|inner| {
            !inner
                .iter()
                .any(|one| matches!(one, TokenTree::Punct(semi) if semi.as_char() == ';'))
        }) else {
            out.push(tree.clone());
            continue;
        };
        out.extend(body);
        out.push(TokenTree::Punct(Punct::new(',', Spacing::Alone)));
        if matches!(seq.get(at), Some(TokenTree::Punct(comma)) if comma.as_char() == ',') {
            at = at.saturating_add(1);
        }
    }
    out
}

/// `seq` less every alternative at this depth that a match arm gained for a name `spelled` lacks.
fn unarmed(seq: &[TokenTree], spelled: &BTreeSet<String>) -> Vec<TokenTree> {
    let mut out = Vec::with_capacity(seq.len());
    let mut at = 0_usize;
    while let Some(tree) = seq.get(at) {
        if let Some(len) = gained(seq.get(at..).unwrap_or_default(), spelled) {
            at = at.saturating_add(len);
            continue;
        }
        out.push(tree.clone());
        at = at.saturating_add(1);
    }
    out
}

/// The length of the `| Path`, `| Path { .. }` or `| Path(_, ..)` `seq` opens with, directly before
/// a `=>`, whose last segment `spelled` lacks, or `None`.
fn gained(seq: &[TokenTree], spelled: &BTreeSet<String>) -> Option<usize> {
    let [TokenTree::Punct(bar), rest @ ..] = seq else {
        return None;
    };
    let path = rest
        .iter()
        .take_while(|one| match one {
            TokenTree::Ident(_) => true,
            TokenTree::Punct(colon) => colon.as_char() == ':',
            _ => false,
        })
        .count();
    let (segments, tail) = rest.split_at_checked(path)?;
    let Some(TokenTree::Ident(last)) = segments.last() else {
        return None;
    };
    // `New if ready` and `ref new` are two identifiers in a row, never one path.
    if segments
        .windows(2)
        .any(|pair| matches!(pair, [TokenTree::Ident(_), TokenTree::Ident(_)]))
    {
        return None;
    }
    let fields = usize::from(matches!(tail.first(), Some(TokenTree::Group(group)) if binds_nothing(group)));
    let arrow = flat(tail.get(fields..fields.saturating_add(2)).unwrap_or_default());
    let name = last.to_string();
    (bar.as_char() == '|' && arrow == [Flat::Punct('='), Flat::Punct('>')] && name != "_" && !spelled.contains(&name))
        .then(|| path.saturating_add(fields).saturating_add(1))
}

/// Is `group` a `{ .. }` or `(_, ..)` binding no field?
fn binds_nothing(group: &Group) -> bool {
    matches!(group.delimiter(), Delimiter::Brace | Delimiter::Parenthesis)
        && flat(&trees(group.stream())).iter().all(|one| match one {
            Flat::Punct(dot) => matches!(dot, '.' | ','),
            Flat::Ident(name) => name == "_",
            _ => false,
        })
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

#[cfg(test)]
mod tests {
    use super::Edited;

    #[test]
    fn a_range_of_only_wrapped_call_sites_reports_inconclusive() {
        let edited = vec![Edited {
            path: String::from("crates/x/tests/a.rs"),
            tests: vec![String::from("renders")],
        }];
        assert_eq!(
            super::report(&edited),
            crate::Verdict::Inconclusive,
            "an all-edited range must report INCONCLUSIVE, never a pass"
        );
    }
}
