//! No first-party `Deref` or `Borrow`. Both leak a newtype's invariant, in two different ways.
//!
//! The newtype guide this repo adopts as policy is explicit about each, and both said *review* in
//! `.agents/skills/engineering/rust/SKILL.md`:
//!
//! * **`Deref` re-exports the inner type's whole API**, and the invariant leaks out with it. A
//!   `Digest` that derefs to `String` hands every caller `String`'s methods, including the ones
//!   that would produce a value `parse` refused - and the guide's alternative is an inherent method,
//!   or `AsRef<T>` where a borrow is genuinely wanted.
//! * **`Borrow` is *"unofficially unsafe"*, which is the sharper of the two.** Implementing it
//!   PROMISES that the wrapper hashes, compares and orders identically to what it borrows, and the
//!   compiler checks nothing - so a newtype whose `parse` folds case turns a map lookup into a
//!   silent miss on an entry that is present. The guide's own instruction is to *"scrutinize any
//!   `Borrow` implementation you see in code review"*, and this gate is that instruction with a
//!   mechanism attached.
//!
//! **Measured before it was written: there is no first-party `impl Deref` and no `impl Borrow` in
//! this tree.** So the gate starts green and its whole job is to keep it that way - which makes it
//! the cheapest of the design-rule gates and the one most likely to earn its keep years from now.
//! `AsRef` is deliberately NOT here: it is the alternative the guide recommends, and the domain
//! uses it.
//!
//! The mutable pair is included for the same reason as the immutable one, plus one of its own: a
//! `DerefMut` or a `BorrowMut` hands out a `&mut` to the inner value, so a caller can move a parsed
//! value to one `parse` would have refused without constructing anything.
//!
//! # Scope, and the limits
//!
//! Every tracked Rust file with `vendor/` excluded - third-party code adapted here is upstream's
//! shape, and `VENDOR.md` is where a local change to it is argued rather than a lint.
//!
//! * **Comments are blanked first**, through the serde gate's [`code_lines`](crate::serde_parse::scan::code_lines),
//!   and that is load-bearing rather than tidy: this repo's own prose says *"there is deliberately
//!   no `Deref`"* in three doc comments, so a scan over raw text would fail on the sentences
//!   explaining the rule. The interiors of multi-line strings go with them, which is what keeps
//!   this module's fixtures out of its own scan.
//! * **It matches the trait's last path segment**, so `impl core::ops::Deref for X` and
//!   `impl Deref for X` are both found - and a first-party trait somebody happens to call `Deref`
//!   would be found too. That is the safe direction, and such a trait would be a bad name anyway.
//! * **A blanket impl in a dependency is not ours and is not read.** The rule is about first-party
//!   types; what a library does with its own is its business.
//!
//! # The sealed witness types
//!
//! Four types in `xtask` exist so that a caller **cannot hold the sequence they carry** - the whole
//! mechanism `github.com/telekom/sutura#414` rests on is that a gate which never receives an
//! iterator has nowhere to write `.take(n)`. That property was held by nobody. Measured on
//! `565ebaae`: four lines of `impl IntoIterator for Census` compiled, `.take(3)` at the real call
//! site then produced a verdict over three of 1177 subjects at **exit 0**, and this gate counted
//! the new impl - `244` trait impls to `245` - **without refusing**.
//!
//! So [`SEALED`] names those types and this gate refuses three shapes for each:
//!
//! * a **trait impl** that hands out the contents ([`SEQUENCE_TRAITS`], plus the two global
//!   entries above) - `IntoIterator` is the four-line defeat, and `AsRef` is on the list *here*
//!   while being the recommended alternative everywhere else, because a witness type's contents are
//!   exactly what must not be borrowable;
//! * an **inherent method** whose signature hands them out - by name (`iter`, `as_slice`, …) and,
//!   more usefully, by RETURN SHAPE, so `fn subjects(&self) -> &[String]` is refused under any name;
//! * a declared sealed type that is **not declared where this list says it is**, so the list cannot
//!   rot into naming types that no longer exist.
//!
//! One exception, declared: `Census::into_listing`, the transitional door, whose own bound is the
//! call-site count in `xtask/src/repo/census.rs`.

use crate::Verdict;
use crate::repo;
use crate::serde_parse::scan::{code_lines, matching_angle};

/// One trait that leaks the invariant, and what to write instead.
struct LeakyTrait {
    /// The trait's name, as the last segment of its path.
    name: &'static str,
    /// Why it leaks. Printed, because a rule whose reason is unstated gets reverted.
    why: &'static str,
    /// What to do instead. Printed, because a gate that only says "no" gets worked around.
    instead: &'static str,
}

/// **Adding or removing an entry here is an architecture decision. That is the point** - the
/// sentence `ALLOWED_IN_DOMAIN` and `FORBIDDEN_EDGES` both carry, for the same reason: the diff is
/// where the argument happens.
const LEAKY: &[LeakyTrait] = &[
    LeakyTrait {
        name: "Deref",
        why: "it re-exports the inner type's whole API, and the invariant leaks out with it: a \
              `Digest` that derefs to `String` hands every caller the methods that would produce a \
              value `parse` refused",
        instead: "an inherent method named for what it gives - `as_str`, `value` - or `AsRef<T>` \
                  where a borrow is genuinely wanted. `AsRef` is the guide's own alternative and is \
                  deliberately not on this list",
    },
    LeakyTrait {
        name: "DerefMut",
        why: "everything `Deref` costs, plus a `&mut` to the inner value - so a caller can move a \
              parsed value to one `parse` would have refused, without constructing anything",
        instead: "a named method that re-establishes the invariant, or no mutation at all. The \
                  guide's `NonEmptyVec::pop` returning `None` rather than emptying the vec is the \
                  worked example, and the payoff is that `last` becomes infallible",
    },
    LeakyTrait {
        name: "Borrow",
        why: "the guide calls it \"unofficially unsafe\", and this is the sharper of the two: \
              implementing it PROMISES the wrapper hashes, compares and orders identically to what \
              it borrows, and the compiler checks nothing. A newtype whose `parse` folds case then \
              turns a map lookup into a silent miss on an entry that IS present",
        instead: "`AsRef<T>`, which promises nothing about hashing or ordering, or an inherent \
                  accessor. If a lookup by the inner type is genuinely wanted, key the map on the \
                  newtype and parse at the call site - that is one `parse` rather than a promise \
                  nothing enforces",
    },
    LeakyTrait {
        name: "BorrowMut",
        why: "everything `Borrow` promises, and a `&mut` through which the borrowed value can be \
              changed after the promise was made",
        instead: "the same as `Borrow` above: `AsRef`, or an accessor named for what it hands out",
    },
];

/// A type whose whole mechanism is that a caller cannot hold what it carries.
struct Sealed {
    /// The type's name, as written.
    name: &'static str,
    /// The repo-relative file that must declare it. Checked, so the list cannot name a type that
    /// was renamed or deleted.
    declared_in: &'static str,
    /// What holding its contents would undo. Printed.
    why: &'static str,
}

/// **Adding or removing an entry here is an architecture decision**, exactly as for [`LEAKY`].
///
/// `check-warm-start` keeps the second, independent derivation of this argument on purpose, which
/// is why both trees of types are here rather than one.
const SEALED: &[Sealed] = &[
    Sealed {
        name: "Census",
        declared_in: "xtask/src/repo/census.rs",
        why: "a gate that receives the listing can narrow it, which is the whole class #414 \
              exists to close - `.take(100)` dropped 99.46% of one walk at exit 0. The loop lives \
              inside `inspect` and the read with it, so there is no sequence to hand out",
    },
    Sealed {
        name: "Inspected",
        declared_in: "xtask/src/repo/census.rs",
        why: "it is the verdict's numbers, and `judged + out_of_scope + absent == discovered` \
              holds because nothing outside `inspect` can set them. An accessor handing out the \
              judged paths would let a gate print a count of its own again",
    },
    Sealed {
        name: "Offered",
        declared_in: "xtask/src/repo/accounting.rs",
        why: "the same class one level in - a gate's own walk INSIDE what the census handed it. \
              `.take(100)` on one such line walk left 214500 of 294744 lines unread at exit 0, so \
              the subjects are private and `each` is the only way to see one",
    },
    Sealed {
        name: "Discovered",
        declared_in: "xtask/src/warm_start/pairing.rs",
        why: "the reference derivation of the same argument: `Swept::over` refuses a paired set \
              shorter than what was discovered, which only works while nobody else can shorten it",
    },
    Sealed {
        name: "Swept",
        declared_in: "xtask/src/warm_start/pairing.rs",
        why: "its verdict prints the witness's own length, so a borrow of the inner sequence is a \
              second source for a number that must have exactly one",
    },
];

/// Traits that hand out a sealed type's contents.
///
/// Refused for a [`SEALED`] type only, unlike [`LEAKY`], which is refused everywhere. `AsRef` is
/// the guide's recommended alternative and `sutura-domain` uses it - the rule is not that `AsRef`
/// is bad, it is that a witness type has nothing it may lend.
const SEQUENCE_TRAITS: &[&str] = &[
    "IntoIterator",
    "Iterator",
    "AsRef",
    "AsMut",
    "Index",
    "IndexMut",
    "Extend",
    "FromIterator",
];

/// Method names that hand out a collection whatever they return.
///
/// `into_listing` is HERE rather than merely exempt elsewhere, and that is the fix for a hole
/// measured on this branch: the exemption was by BARE NAME with no type and no shape scope, so a
/// second `fn into_listing` on `Inspected` handing out all five verdict numbers passed at exit 0
/// with the suite green. A by-name exemption is a hole with a nice name. Now the name is refused
/// everywhere and [`DECLARED_DOOR`] re-permits exactly one type, one name and one return shape.
const LEAKY_ACCESSORS: &[&str] = &[
    "iter",
    "iter_mut",
    "into_iter",
    "as_slice",
    "as_mut_slice",
    "into_vec",
    "into_inner",
    "into_listing",
];

/// Return-type spellings that hand out a sequence. Checked on the return type ALONE, so a
/// parameter of type `&[&str]` - which `Census::inspect` has - is not a leak.
const SEQUENCE_RETURNS: &[&str] = &["[", "Vec<", "Iterator", "Iter<", "IterMut<", "IntoIter", "slice::"];

/// The one declared way out of a sealed type: this TYPE, this NAME and this RETURN SHAPE, all
/// three. It has a bound of its own as well - the exact call-site count in
/// `xtask/src/repo/census.rs`, checked against the live tree.
///
/// Scoped on all three axes because each was measured escaping on its own: by name alone, a
/// `fn into_listing` on `Inspected` was exempt; without the shape, `Census::into_listing` could
/// change what it hands back and stay exempt.
const DECLARED_DOOR: (&str, &str, &str) = ("Census", "into_listing", "Result<Listing, Refusal>");

/// One violation, located.
struct Leak {
    /// Repo-relative path.
    path: String,
    /// 1-based line of the `impl`.
    line: usize,
    /// The trait, as this gate names it.
    leaked: &'static str,
    /// The type it was implemented for, as written.
    onto: String,
}

/// One way a sealed witness type's contents became reachable.
struct Opened {
    /// Repo-relative path.
    path: String,
    /// 1-based line of the `impl` or the method.
    line: usize,
    /// The sealed type, as [`SEALED`] names it.
    sealed: &'static str,
    /// What was written, as this gate reads it.
    how: String,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let (root, files) = match repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::NewtypeLeaks)) {
        Ok(listing) => listing,
        Err(why) => {
            eprintln!("xtask check-newtype-leaks: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    let mut leaks: Vec<Leak> = Vec::new();
    let mut opened: Vec<Opened> = Vec::new();
    let mut declared: Vec<&'static str> = Vec::new();
    let mut scanned = 0_usize;
    let mut impls = 0_usize;
    for rel in &files {
        if !in_scope(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        let code = code_lines(&text);
        let shadowed = shadowing(&code, rel);
        for (line, header) in trait_impls(&code) {
            impls = impls.saturating_add(1);
            if let Some((leaked, onto)) = leaked_by(&header) {
                leaks.push(Leak {
                    path: rel.clone(),
                    line,
                    leaked,
                    onto,
                });
            }
            if let Some((sealed, how)) = hands_out_contents(&header, &shadowed) {
                opened.push(Opened {
                    path: rel.clone(),
                    line,
                    sealed,
                    how,
                });
            }
        }
        for entry in SEALED {
            if entry.declared_in == rel.as_str() && declares(&code, entry.name) {
                declared.push(entry.name);
            }
        }
        exposing_methods(&code, rel, &shadowed, &mut opened);
    }

    if scanned == 0 {
        // A gate that silently checked nothing is the failure mode a gate exists to prevent.
        eprintln!("xtask check-newtype-leaks: no Rust source in scope - this gate would check nothing");
        return Verdict::Fail;
    }

    let undeclared = undeclared(&declared);

    if leaks.is_empty() && opened.is_empty() && undeclared.is_empty() {
        println!(
            "xtask check-newtype-leaks: ok - {impls} trait impl(s) in {scanned} file(s), no Deref and no Borrow, \
             and {} sealed witness type(s) still hold their contents",
            SEALED.len()
        );
        return Verdict::Pass;
    }

    if !leaks.is_empty() {
        eprintln!("xtask check-newtype-leaks: FAILED - a newtype's invariant is reachable around it:");
        for leak in &leaks {
            eprintln!("  {}:{}: `impl {} for {}`", leak.path, leak.line, leak.leaked, leak.onto);
        }
        eprintln!();
        explain(&leaks);
    }
    if !opened.is_empty() {
        eprintln!("xtask check-newtype-leaks: FAILED - a sealed witness type hands out what it carries:");
        for open in &opened {
            eprintln!("  {}:{}: {} on `{}`", open.path, open.line, open.how, open.sealed);
        }
        eprintln!();
        explain_sealed(&opened);
    }
    for entry in &undeclared {
        eprintln!(
            "xtask check-newtype-leaks: FAILED - `{}` is declared sealed but `{}` does not declare it, so this \
             gate is guarding a name rather than a type",
            entry.name, entry.declared_in
        );
    }
    Verdict::Fail
}

/// The [`SEALED`] entries the scan did not find declared anywhere.
///
/// The reverse direction, so the list cannot rot into naming a type that was renamed away: an entry
/// nothing declares is a rule guarding nothing, and `run` reports it as a FAILURE rather than as an
/// absence of findings. Its own function so the refusal is held by a test rather than by the whole
/// gate - a predicate with passing tests and an untested refusal above it is the shape this stack
/// keeps finding.
fn undeclared(declared: &[&'static str]) -> Vec<&'static Sealed> {
    SEALED.iter().filter(|entry| !declared.contains(&entry.name)).collect()
}

/// Printed on a sealed violation, and only for the types actually found.
fn explain_sealed(opened: &[Opened]) {
    for entry in SEALED {
        if !opened.iter().any(|open| open.sealed == entry.name) {
            continue;
        }
        eprintln!("  {}:", entry.name);
        eprintln!("    Why: {}", entry.why);
        eprintln!();
    }
    eprintln!("These types exist so a caller CANNOT hold the sequence they carry - that is the whole");
    eprintln!("mechanism, not a stylistic preference. Measured on 565ebaae: four lines of");
    eprintln!("`impl IntoIterator for Census` compiled, `.take(3)` at the real call site then produced a");
    eprintln!("verdict over 3 of 1177 subjects at exit 0, and this gate COUNTED the impl without refusing.");
    eprintln!("Pass a closure to the witness instead, so the loop stays inside it. If a sealed type");
    eprintln!("genuinely has to lend its contents, SEALED in xtask/src/newtype_leaks.rs is what changes,");
    eprintln!("and that is an architecture decision with a visible diff.");
}

/// The sealed type a trait `impl` header hands the contents of, and what was written.
fn hands_out_contents(header: &str, shadowed: &[&'static str]) -> Option<(&'static str, String)> {
    let (before, after) = header.rsplit_once(" for ")?;
    let path = trait_path(before)?;
    let trait_name = last_segment(path);
    let sealed = sealed_target(after, shadowed)?;
    // The two global entries are reported by `leaked_by` already; naming them twice for one line
    // is a wall of text for one violation.
    if !SEQUENCE_TRAITS.contains(&trait_name) {
        return None;
    }
    Some((sealed, format!("`impl {trait_name} for {}`", after.trim())))
}

/// The [`SEALED`] type this `impl` target names, ignoring a reference, a lifetime and any generic
/// arguments - so `&'a Census`, `&mut Census` and `Census<'a>` are all this type.
///
/// **`shadowed` is the names this file declares ITSELF, and it is not a nicety.** Matching is by
/// last segment, so it is blind to modules - and the moment the trait half of this rule landed it
/// reported `xtask/src/worktree_state.rs:205: fn missed returning &[&'static str] on Inspected`,
/// which is #417's OWN witness type of the same name and none of this rule's business. A file that
/// declares its own `Inspected` means that one, so the sealed name is not in scope there. The cost
/// is precise and worth stating: a file could shadow a sealed name AND implement a leaky trait for
/// the real one, and this would believe the shadow.
fn sealed_target(target: &str, shadowed: &[&'static str]) -> Option<&'static str> {
    let mut rest = target.trim();
    loop {
        let trimmed = rest
            .strip_prefix('&')
            .or_else(|| rest.strip_prefix("mut "))
            .or_else(|| rest.strip_prefix("dyn "))
            .map(str::trim_start);
        match trimmed {
            Some(shorter) => rest = shorter,
            None => break,
        }
        if let Some(after) = rest.strip_prefix('\'') {
            rest = after.split_once(' ').map_or("", |(_, tail)| tail).trim_start();
        }
    }
    let name = rest.get(..rest.find('<').unwrap_or(rest.len()))?.trim();
    SEALED
        .iter()
        .find(|entry| entry.name == name && !shadowed.contains(&entry.name))
        .map(|entry| entry.name)
}

/// The [`SEALED`] names `rel` declares itself while this list says they live somewhere else.
fn shadowing(code: &[String], rel: &str) -> Vec<&'static str> {
    SEALED
        .iter()
        .filter(|entry| entry.declared_in != rel && declares(code, entry.name))
        .map(|entry| entry.name)
        .collect()
}

/// Does `code` declare a type called `name`?
fn declares(code: &[String], name: &str) -> bool {
    code.iter().any(|line| {
        ["struct ", "enum ", "type "].iter().any(|keyword| {
            line.split_once(keyword).is_some_and(|(_, rest)| {
                rest.strip_prefix(name)
                    .is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            })
        })
    })
}

/// Every method on a [`SEALED`] type whose signature hands out the contents - **inherent OR
/// through a trait, including a trait this gate has never heard of.**
///
/// The trait half is the fourth shape, measured on this branch: `SEQUENCE_TRAITS` refuses the
/// traits it KNOWS and this function used to walk inherent impls only, so a **five-line custom
/// trait** restored `.take(3)` at the production call site with the gate reporting
/// `246 trait impl(s) … 4 sealed witness type(s) still hold their contents` at **exit 0**, the
/// suite green and clippy clean. A new trait was neither of the two things being checked. The rule
/// is now about the METHOD rather than about the trait: no method reachable through a sealed type
/// may hand out its contents, whoever declared the signature.
fn exposing_methods(code: &[String], rel: &str, shadowed: &[&'static str], out: &mut Vec<Opened>) {
    let reachable = inherent_impls(code).into_iter().chain(trait_impls(code));
    for (line, header) in reachable {
        // For a trait impl the target is after ` for `; for an inherent one it is after `impl`.
        let target = header
            .rsplit_once(" for ")
            .map_or_else(|| header.trim().strip_prefix("impl").unwrap_or(""), |(_, after)| after);
        let Some(sealed) = sealed_target(target, shadowed) else {
            continue;
        };
        for (offset, signature) in method_signatures(code, line.saturating_sub(1)) {
            let Some((name, returns)) = split_signature(&signature) else {
                continue;
            };
            if (sealed, name, returns.trim()) == DECLARED_DOOR {
                continue;
            }
            let by_name = LEAKY_ACCESSORS.contains(&name);
            let by_shape = SEQUENCE_RETURNS.iter().any(|shape| returns.contains(shape));
            if by_name || by_shape {
                out.push(Opened {
                    path: String::from(rel),
                    line: offset,
                    sealed,
                    how: format!("`fn {name}` returning `{}`", returns.trim()),
                });
            }
        }
    }
}

/// Printed on failure, and only for the traits actually found - a wall of four explanations for
/// one violation is worse than one.
fn explain(leaks: &[Leak]) {
    for entry in LEAKY {
        if !leaks.iter().any(|leak| leak.leaked == entry.name) {
            continue;
        }
        eprintln!("  {}:", entry.name);
        eprintln!("    Why: {}", entry.why);
        eprintln!("    Do:  {}", entry.instead);
        eprintln!();
    }
    eprintln!("This gate started GREEN: there was no first-party `Deref` and no `Borrow` in the tree");
    eprintln!("when it was written, so its whole job is to keep it that way. If one of these");
    eprintln!("genuinely belongs, the entry in xtask/src/newtype_leaks.rs is what has to change, and");
    eprintln!("that is an architecture decision: it should be a visible diff with the argument in it.");
}

/// Is this a Rust file this gate judges?
fn in_scope(rel: &str) -> bool {
    !rel.starts_with("vendor/")
        && std::path::Path::new(rel)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Every trait implementation in `code`, as a 1-based line number and the joined `impl` header.
///
/// Joined across lines because a generic list or a `where` clause pushes the brace down, and the
/// trait being implemented can end up on a line of its own.
fn trait_impls(code: &[String]) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in code.iter().enumerate() {
        let trimmed = line.trim();
        if !starts_an_impl(trimmed) {
            continue;
        }
        let header = header_at(code, index);
        if header.contains(" for ") {
            found.push((index.saturating_add(1), header));
        }
    }
    found
}

/// Every INHERENT implementation in `code`, as a 1-based line number and the joined header.
///
/// The complement of [`trait_impls`]: no ` for `, so `impl Census {` is one and
/// `impl IntoIterator for Census {` is not.
fn inherent_impls(code: &[String]) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in code.iter().enumerate() {
        if !starts_an_impl(line.trim()) {
            continue;
        }
        let header = header_at(code, index);
        if !header.contains(" for ") {
            found.push((index.saturating_add(1), header));
        }
    }
    found
}

/// Every method declared DIRECTLY in the `impl` block whose header begins at `at` (0-based), as a
/// 1-based line number and the signature up to its body.
///
/// Depth-tracked so a `fn` nested inside a method's body is not read as a method of the type - it
/// is not reachable through the type, so it cannot hand anything out.
fn method_signatures(code: &[String], at: usize) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut depth = 0_usize;
    let mut opened = false;
    for (index, line) in code.iter().enumerate().skip(at) {
        if opened && depth == 1 && starts_a_method(line.trim()) {
            found.push((index.saturating_add(1), header_at(code, index)));
        }
        for character in line.chars() {
            if character == '{' {
                depth = depth.saturating_add(1);
                opened = true;
            } else if character == '}' {
                depth = depth.saturating_sub(1);
                if opened && depth == 0 {
                    return found;
                }
            }
        }
    }
    found
}

/// Does this line begin a method item? Everything before `fn` has to be a qualifier, so a `fn`
/// inside an expression or a type - `scope: fn(&str) -> bool` - is not one.
fn starts_a_method(trimmed: &str) -> bool {
    let Some((before, _)) = trimmed.split_once("fn ") else {
        return false;
    };
    before
        .split_whitespace()
        .all(|word| word.starts_with("pub(") || matches!(word, "pub" | "const" | "async" | "unsafe" | "extern" | "default"))
}

/// A method signature's name and its RETURN type, or `None` when it is unreadable.
///
/// The return type alone, because a PARAMETER of type `&[&str]` is not a leak - `Census::inspect`
/// takes one - and because the parameter list is where a closure's own `->` lives.
fn split_signature(signature: &str) -> Option<(&str, &str)> {
    let after_fn = signature.split_once("fn ")?.1;
    let name_end = after_fn.find(['(', '<']).unwrap_or(after_fn.len());
    let name = after_fn.get(..name_end)?.trim();
    let rest = after_fn.get(name_end..)?;
    // Skip the method's own generic list first, so `fn f<F: Fn(&str)>(..)` does not open its
    // parameter list inside a bound.
    let rest = if rest.starts_with('<') {
        rest.get(matching_angle(rest)?.saturating_add(1)..)?
    } else {
        rest
    };
    let open = rest.find('(')?;
    let close = matching_paren(rest.get(open..)?)?;
    let tail = rest.get(open.saturating_add(close).saturating_add(1)..)?.trim();
    let returns = tail.strip_prefix("->").unwrap_or("");
    let returns = returns.get(..returns.find(" where ").unwrap_or(returns.len()))?;
    Some((name, returns))
}

/// The index of the `)` closing the `(` at the start of `text`.
fn matching_paren(text: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, character) in text.char_indices() {
        if character == '(' {
            depth = depth.saturating_add(1);
        } else if character == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(at);
            }
        }
    }
    None
}

/// Does this line begin an `impl` item? `impl` followed by a space or a generic list, so
/// `implementors_of(..)` is not one.
fn starts_an_impl(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("impl")
        .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with('<'))
}

/// The `impl` header starting at `at`, up to the `{` that opens its body.
fn header_at(code: &[String], at: usize) -> String {
    let mut text = String::new();
    for line in code.iter().skip(at) {
        for character in line.chars() {
            if character == '{' {
                return text;
            }
            text.push(character);
        }
        text.push(' ');
    }
    text
}

/// The leaky trait this header implements and the type it implements it for, or `None`.
fn leaked_by(header: &str) -> Option<(&'static str, String)> {
    // ` for ` with spaces, so a higher-ranked bound written `for<'a>` is not read as the split.
    let (before, after) = header.rsplit_once(" for ")?;
    let path = trait_path(before)?;
    let leaked = LEAKY.iter().find(|entry| entry.name == last_segment(path))?;
    Some((leaked.name, String::from(after.trim())))
}

/// The trait path in an `impl` header's left half, with any generic parameter list on `impl`
/// itself skipped first.
fn trait_path(before: &str) -> Option<&str> {
    let rest = before.trim().strip_prefix("impl")?;
    let rest = if rest.starts_with('<') {
        rest.get(matching_angle(rest)?.saturating_add(1)..)?
    } else {
        rest
    };
    // What is left is `Trait`, `Trait<Args>` or `path::to::Trait<Args>`; the arguments and any
    // `where` clause are not part of the name.
    let name = rest.trim();
    Some(name.get(..name.find('<').unwrap_or(name.len()))?.trim()).filter(|path| !path.is_empty())
}

/// The last `::`-separated segment of a path.
fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path).trim()
}

#[cfg(test)]
mod tests {
    use super::{LEAKY, SEALED, in_scope, last_segment, leaked_by, trait_impls, trait_path};
    use crate::serde_parse::scan::code_lines;

    /// The sealed types a source's TRAIT impls hand out, as the gate reads them.
    fn handed_out(source: &str) -> Vec<(&'static str, String)> {
        let code = code_lines(source);
        trait_impls(&code)
            .into_iter()
            .filter_map(|(_, header)| super::hands_out_contents(&header, &[]))
            .collect()
    }

    /// The sealed types a source's INHERENT methods hand out, as `(sealed, how)`.
    fn exposed(source: &str) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        super::exposing_methods(&code_lines(source), "probe.rs", &[], &mut out);
        out.into_iter().map(|open| (open.sealed, open.how)).collect()
    }

    /// The leaky traits a header names, as the gate reads them.
    fn leaks(source: &str) -> Vec<(&'static str, String)> {
        let code = code_lines(source);
        trait_impls(&code)
            .into_iter()
            .filter_map(|(_, header)| leaked_by(&header))
            .collect()
    }

    #[test]
    fn a_deref_on_a_newtype_is_found() {
        let source = "impl core::ops::Deref for Digest {\n    type Target = str;\n}\n";
        assert_eq!(leaks(source), vec![("Deref", String::from("Digest"))]);
    }

    #[test]
    fn an_unqualified_deref_is_found_too() {
        assert_eq!(leaks("impl Deref for Digest {\n}\n"), vec![("Deref", String::from("Digest"))]);
    }

    #[test]
    fn a_borrow_with_a_type_argument_is_found() {
        // The shape that makes `Borrow` worth gating: it promises equal hashing for `str`, and a
        // `parse` that folds case makes that promise false.
        let source = "impl std::borrow::Borrow<str> for Phrase {\n}\n";
        assert_eq!(leaks(source), vec![("Borrow", String::from("Phrase"))]);
    }

    #[test]
    fn the_mutable_pair_is_found() {
        let source = "impl DerefMut for Digest {\n}\nimpl BorrowMut<str> for Phrase {\n}\n";
        let found = leaks(source);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|(name, _)| *name == "DerefMut"), "{found:?}");
        assert!(found.iter().any(|(name, _)| *name == "BorrowMut"), "{found:?}");
    }

    #[test]
    fn a_generic_impl_is_read_past_its_own_parameters() {
        let source = "impl<'a, T: Clone> Deref for Holder<'a, T> {\n}\n";
        assert_eq!(leaks(source), vec![("Deref", String::from("Holder<'a, T>"))]);
    }

    #[test]
    fn a_where_clause_on_a_later_line_does_not_hide_the_trait() {
        let source = "impl<T> Deref for Holder<T>\nwhere\n    T: Clone,\n{\n}\n";
        assert_eq!(leaks(source).len(), 1);
    }

    #[test]
    fn as_ref_is_the_alternative_and_is_not_a_leak() {
        // Deliberately absent from `LEAKY`: it promises nothing about hashing or ordering, and the
        // guide recommends it wherever a borrow is genuinely wanted.
        assert!(leaks("impl AsRef<str> for Digest {\n}\n").is_empty());
    }

    #[test]
    fn an_ordinary_trait_impl_is_not_a_leak() {
        let source = "impl core::fmt::Display for Digest {\n}\nimpl TryFrom<String> for Digest {\n}\n";
        assert!(leaks(source).is_empty());
    }

    #[test]
    fn an_inherent_impl_is_not_a_trait_impl() {
        assert!(leaks("impl Digest {\n    pub fn as_str(&self) -> &str {\n        &self.0\n    }\n}\n").is_empty());
    }

    #[test]
    fn a_higher_ranked_bound_is_not_read_as_the_split() {
        // `for<'a>` is not the ` for ` that separates the trait from the type, and reading it as
        // one would make every such impl unclassifiable.
        let source = "impl<F: for<'a> Fn(&'a str) -> u8> Deref for Wrapper<F> {\n}\n";
        assert_eq!(leaks(source), vec![("Deref", String::from("Wrapper<F>"))]);
    }

    #[test]
    fn prose_saying_there_is_deliberately_no_deref_is_not_an_impl() {
        // Load-bearing rather than tidy: this repo says exactly that in three doc comments, so a
        // scan over raw text would fail on the sentences that explain the rule.
        let source = "/// There is deliberately no `Deref` here.\n// impl Deref for Digest {}\npub struct Digest(String);\n";
        assert!(leaks(source).is_empty());
    }

    #[test]
    fn an_impl_inside_a_multi_line_string_is_not_an_impl() {
        // Which is what keeps this module's own fixtures invisible to the gate that reads them.
        let source = "fn fixture() -> &'static str {\n    r#\"\nimpl Deref for Digest {\n}\n\"#\n}\n";
        assert!(leaks(source).is_empty());
    }

    #[test]
    fn a_trait_path_drops_its_arguments() {
        assert_eq!(trait_path("impl std::borrow::Borrow<str>"), Some("std::borrow::Borrow"));
        assert_eq!(trait_path("impl<T> Borrow<T>"), Some("Borrow"));
        assert_eq!(last_segment("std::borrow::Borrow"), "Borrow");
        assert_eq!(last_segment("Borrow"), "Borrow");
    }

    #[test]
    fn vendored_code_is_out_of_scope_and_rust_files_are_in_it() {
        // Upstream's shape is not ours to lint; `VENDOR.md` is where a local change is argued.
        assert!(in_scope("crates/sutura-domain/src/lib.rs"));
        assert!(!in_scope("vendor/mimalloc_rust/src/lib.rs"));
        assert!(!in_scope("AGENTS.md"));
    }

    #[test]
    fn the_four_line_defeat_of_the_whole_mechanism_is_refused() {
        // Measured on `565ebaae`, when this gate did not have this rule: these four lines compiled,
        // `.take(3)` at the real call site then produced a verdict over 3 of 1177 subjects at exit
        // 0, and the gate counted the impl - `244` to `245` - without refusing. So the headline
        // property of #419 was held by nobody adding four lines.
        let source = "impl IntoIterator for Census {\n    type Item = String;\n}\n";
        assert_eq!(handed_out(source).len(), 1, "{:?}", handed_out(source));
    }

    #[test]
    fn a_reference_a_lifetime_and_a_generic_argument_do_not_hide_the_sealed_type() {
        // `&Census` is the shape that would otherwise be the same four lines with one character
        // added, and `IntoIterator for &T` is the idiomatic spelling of exactly this leak.
        for target in ["&Census", "&'a Census", "&mut Census", "Census<'a>"] {
            let source = format!("impl<'a> IntoIterator for {target} {{\n}}\n");
            assert_eq!(handed_out(&source).len(), 1, "{target} hid the sealed type");
        }
    }

    #[test]
    fn as_ref_is_the_alternative_everywhere_except_on_a_sealed_type() {
        // The one place the guide's recommended alternative is still a leak: a witness type has
        // nothing it may lend. `sutura-domain`'s own `AsRef` impls are untouched by this rule,
        // which is why it is scoped to `SEALED` rather than added to `LEAKY`.
        assert_eq!(handed_out("impl AsRef<[String]> for Census {\n}\n").len(), 1);
        assert!(handed_out("impl AsRef<str> for Digest {\n}\n").is_empty());
        assert!(
            leaks("impl AsRef<[String]> for Census {\n}\n").is_empty(),
            "not a global rule"
        );
    }

    #[test]
    fn an_ordinary_trait_impl_on_a_sealed_type_is_not_a_leak() {
        // The rule is about handing out the contents, not about sealing the type off entirely.
        assert!(handed_out("impl core::fmt::Debug for Census {\n}\n").is_empty());
        assert!(handed_out("impl Drop for Swept {\n}\n").is_empty());
    }

    #[test]
    fn an_accessor_is_refused_by_its_return_shape_and_not_only_by_its_name() {
        // The name list alone would be defeated by a rename, which is a one-word diff. Measured
        // with the method kept ALIVE so `dead_code` could not take the credit:
        // `xtask/src/repo/census.rs:227: fn subjects returning &[String] on Census`, exit 1.
        let named = "impl Census {\n    fn iter(&self) -> Something {\n        todo!()\n    }\n}\n";
        assert_eq!(exposed(named).len(), 1, "{:?}", exposed(named));

        let unnamed = "impl Census {\n    pub(crate) fn subjects(&self) -> &[String] {\n        &self.of\n    }\n}\n";
        assert_eq!(
            exposed(unnamed).len(),
            1,
            "a name no list would guess: {:?}",
            exposed(unnamed)
        );

        let vector = "impl Swept {\n    fn taken(&self) -> Vec<String> {\n        Vec::new()\n    }\n}\n";
        assert_eq!(exposed(vector).len(), 1, "{:?}", exposed(vector));

        let iterator = "impl Census {\n    fn walk(&self) -> impl Iterator<Item = &str> {\n        None.into_iter()\n    }\n}\n";
        assert_eq!(exposed(iterator).len(), 1, "{:?}", exposed(iterator));
    }

    #[test]
    fn a_parameter_carrying_a_slice_is_not_a_return_value() {
        // `Census::inspect` takes `&[&str]` and a `fn(&str) -> bool`, so reading anything but the
        // return type would refuse the production signature this rule exists to protect.
        let real = "impl Census {\n    pub(crate) fn inspect(self, must_judge: &[&str], scope: Scope, judge: impl FnMut(&str, &[u8])) -> Result<Inspected, Refusal> {\n        todo!()\n    }\n}\n";
        assert!(exposed(real).is_empty(), "{:?}", exposed(real));
    }

    #[test]
    fn the_declared_transitional_door_is_the_one_exception_and_it_is_scoped_three_ways() {
        // `into_listing` hands out a plain `Vec` on purpose and its own bound is the call-site
        // count in `xtask/src/repo/census.rs`. It is exempt on THREE axes - type, name and return
        // shape - because each was measured escaping on its own. Built from parts so this source
        // does not itself read as a call site to that count: the first run of the count test
        // reported 45 against 44 real ones, and the extra was this fixture.
        let (sealed, name, returns) = super::DECLARED_DOOR;
        let door = format!(
            "impl {sealed} {{\n    pub(crate) fn {name}(self, _caller: Unmigrated) -> {returns} {{\n        todo!()\n    }}\n}}\n"
        );
        assert!(exposed(&door).is_empty(), "{:?}", exposed(&door));

        // The measured hole: the same NAME on another sealed type, handing out all five verdict
        // numbers. Exempt before this was scoped; refused now.
        let elsewhere = format!(
            "impl Inspected {{\n    pub(crate) fn {name}(self) -> (usize, usize, usize, usize, usize) {{\n        todo!()\n    }}\n}}\n"
        );
        assert_eq!(exposed(&elsewhere).len(), 1, "{:?}", exposed(&elsewhere));

        // And the same type and name handing back something else.
        let reshaped =
            format!("impl {sealed} {{\n    pub(crate) fn {name}(self) -> Vec<String> {{\n        todo!()\n    }}\n}}\n");
        assert_eq!(exposed(&reshaped).len(), 1, "{:?}", exposed(&reshaped));
    }

    #[test]
    fn a_trait_nobody_has_heard_of_cannot_hand_out_a_sealed_type_either() {
        // **The fourth shape.** `SEQUENCE_TRAITS` refuses the traits it knows and the method scan
        // used to walk inherent impls only, so five lines of a custom trait restored `.take(3)` at
        // the production call site: `246 trait impl(s) … 4 sealed witness type(s) still hold their
        // contents`, exit 0, suite green, clippy clean.
        let custom = "impl Subjects for Census {\n    fn subjects(self) -> Vec<String> {\n        self.of\n    }\n}\n";
        assert_eq!(exposed(custom).len(), 1, "{:?}", exposed(custom));

        // A reference receiver and an unknown trait, which is the same escape one character wider.
        let borrowed = "impl<'a> Lend for &'a Census {\n    fn all(&self) -> &[String] {\n        &self.of\n    }\n}\n";
        assert_eq!(exposed(borrowed).len(), 1, "{:?}", exposed(borrowed));

        // An ordinary trait impl on a sealed type is still nobody's business: the rule is about the
        // METHOD's signature, not about sealing the type off entirely.
        let debug = "impl core::fmt::Debug for Census {\n    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {\n        todo!()\n    }\n}\n";
        assert!(exposed(debug).is_empty(), "{:?}", exposed(debug));
        let dropped = "impl Drop for Swept {\n    fn drop(&mut self) {\n        todo!()\n    }\n}\n";
        assert!(exposed(dropped).is_empty(), "{:?}", exposed(dropped));
    }

    #[test]
    fn a_file_declaring_its_own_type_of_a_sealed_name_means_its_own() {
        // Matching is by LAST SEGMENT, so it is blind to modules - and the trait half of this rule
        // immediately reported `xtask/src/worktree_state.rs:205: fn missed returning
        // &[&'static str] on Inspected`, which is #417's own witness type of the same name. A file
        // that declares its own `Inspected` means that one.
        let local = "pub(crate) struct Inspected {\n    missed: Vec<String>,\n}\n\nimpl Inspected {\n    fn missed(&self) -> &[String] {\n        &self.missed\n    }\n}\n";
        let code = code_lines(local);
        let shadowed = super::shadowing(&code, "probe.rs");
        assert_eq!(shadowed, vec!["Inspected"], "the file's own declaration was not seen");
        let mut out = Vec::new();
        super::exposing_methods(&code, "probe.rs", &shadowed, &mut out);
        assert!(out.is_empty(), "{:?}", out.iter().map(|o| o.sealed).collect::<Vec<_>>());

        // And the same source WITHOUT the declaration is the real sealed type again, so the
        // exclusion is the declaration rather than the file name.
        assert_eq!(
            exposed("impl Inspected {\n    fn missed(&self) -> &[String] {\n        todo!()\n    }\n}\n").len(),
            1
        );

        // The file the list NAMES is never shadowed by its own declaration.
        assert!(
            super::shadowing(&code_lines("pub(crate) struct Census {\n}\n"), "xtask/src/repo/census.rs").is_empty(),
            "the declaring file cannot shadow its own entry"
        );
    }

    #[test]
    fn a_method_of_an_unsealed_type_is_nobodys_business() {
        let other = "impl Digest {\n    fn iter(&self) -> Vec<String> {\n        Vec::new()\n    }\n}\n";
        assert!(exposed(other).is_empty(), "{:?}", exposed(other));
    }

    #[test]
    fn a_fn_nested_inside_a_method_is_not_a_method_of_the_type() {
        // Depth-tracked, because a helper inside a body is not reachable through the type.
        let nested = "impl Census {\n    fn verdict(&self) -> String {\n        fn helper() -> Vec<String> {\n            Vec::new()\n        }\n        String::new()\n    }\n}\n";
        assert!(exposed(nested).is_empty(), "{:?}", exposed(nested));
    }

    #[test]
    fn a_sealed_type_the_scan_never_found_is_a_failure_rather_than_a_quiet_pass() {
        // The refusal, not just its predicate. Nothing was found declared, so every entry is
        // reported; find them all and none is.
        assert_eq!(super::undeclared(&[]).len(), SEALED.len(), "an empty scan reported nothing");
        let all: Vec<&'static str> = SEALED.iter().map(|entry| entry.name).collect();
        assert!(super::undeclared(&all).is_empty(), "a complete scan still reported something");
        let missing: Vec<&'static str> = all.iter().skip(1).copied().collect();
        let reported = super::undeclared(&missing);
        assert_eq!(reported.len(), 1, "{:?}", reported.iter().map(|e| e.name).collect::<Vec<_>>());
        assert_eq!(reported.first().map(|entry| entry.name), all.first().copied());
    }

    #[test]
    fn every_sealed_type_is_declared_where_this_list_says_it_is() {
        // The reverse direction, so the list cannot rot into guarding a name. `run` reports this as
        // a FAILURE rather than as an absence of findings; measured by pointing one entry at a file
        // that exists and does not declare it: `FAILED - `Discovered` is declared sealed but
        // `xtask/src/repo/census.rs` does not declare it`, exit 1.
        let Some(root) = crate::repo::root() else {
            panic!("the repo root is what this gate depends on");
        };
        for entry in SEALED {
            let text = std::fs::read_to_string(root.join(entry.declared_in))
                .unwrap_or_else(|why| panic!("{} declares {}: {why}", entry.declared_in, entry.name));
            assert!(
                super::declares(&code_lines(&text), entry.name),
                "{} does not declare `{}` - the rule is guarding a name",
                entry.declared_in,
                entry.name
            );
            assert!(!entry.why.is_empty(), "{} has no reason", entry.name);
        }
    }

    #[test]
    fn every_leaky_trait_says_what_to_do_instead() {
        for entry in LEAKY {
            assert!(!entry.name.is_empty(), "an entry names a trait");
            assert!(!entry.why.is_empty(), "{} has no reason", entry.name);
            assert!(!entry.instead.is_empty(), "{} has no fix", entry.name);
        }
    }
}
