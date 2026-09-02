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

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-newtype-leaks: could not determine the repo root");
        return Verdict::Fail;
    };

    let mut leaks: Vec<Leak> = Vec::new();
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
        }
    }

    if scanned == 0 {
        // A gate that silently checked nothing is the failure mode a gate exists to prevent.
        eprintln!("xtask check-newtype-leaks: no Rust source in scope - this gate would check nothing");
        return Verdict::Fail;
    }
    if leaks.is_empty() {
        println!("xtask check-newtype-leaks: ok - {impls} trait impl(s) in {scanned} file(s), no Deref and no Borrow");
        return Verdict::Pass;
    }

    eprintln!("xtask check-newtype-leaks: FAILED - a newtype's invariant is reachable around it:");
    for leak in &leaks {
        eprintln!("  {}:{}: `impl {} for {}`", leak.path, leak.line, leak.leaked, leak.onto);
    }
    eprintln!();
    explain(&leaks);
    Verdict::Fail
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
    use super::{LEAKY, in_scope, last_segment, leaked_by, trait_impls, trait_path};
    use crate::serde_parse::scan::code_lines;

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
    fn every_leaky_trait_says_what_to_do_instead() {
        for entry in LEAKY {
            assert!(!entry.name.is_empty(), "an entry names a trait");
            assert!(!entry.why.is_empty(), "{} has no reason", entry.name);
            assert!(!entry.instead.is_empty(), "{} has no fix", entry.name);
        }
    }
}
