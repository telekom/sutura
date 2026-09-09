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
mod tests;
