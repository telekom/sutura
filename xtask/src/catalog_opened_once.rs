//! Each catalog document is opened once, via a rustix `O_NOFOLLOW|O_NONBLOCK` descriptor that is
//! `fstat`'d and read through that one handle. A second, unguarded `std::fs::read_to_string(&path)`
//! right after the safe open is the defect this gate refuses - and until it landed, the property
//! "opened once" was held by review alone, because a swap-timing test cannot land in the
//! sub-microsecond window between two back-to-back opens.
//!
//! This is a STATIC mechanism: a text scan over the three file catalogs' non-test source that
//! refuses any path-based read outside a committed list of registered call sites. It is the same
//! shape as `check-answer-path-caches` and `check-bounded-wait`: a rule the code cannot state about
//! itself, read as text, starting from a tree that already obeys it.
//!
//! # The rule
//!
//! Over the non-test `.rs` files of `sutura-catalog-local`, `sutura-catalog-okf` and
//! `sutura-catalog-datacontract`, every occurrence of one of [`NEEDLES`] must match a [`Registered`] entry - a file and a needle - or it is a
//! violation naming the file and line. [`REGISTERED`] is the committed list, and adding or removing
//! an entry is an architecture decision, exactly as the tables in `check-newtype-leaks` and
//! `check-answer-path-caches` are.
//!
//! # What it deliberately does NOT cover
//!
//! **A read through an alias or a helper in another crate escapes.** The scan reads text in these
//! crates; `use std::fs as disk;` followed by `disk::read_to_string` is not found, and neither
//! is a function in a third crate that reads a path, called from here. `use std::fs;` followed by
//! `fs::read_to_string` is also not found - the bare `fs::` spelling was removed to avoid
//! double-matching with the `std::fs::` prefixed needles (the former is a substring of the latter),
//! and neither catalog crate uses `use std::fs;` today. Renaming on import is the same blind spot
//! `check-bounded-wait`'s header records for `Stdio::piped()`.
//!
//! **Test code is not scanned.** A file named `tests.rs` or one under any `tests/` directory is out
//! of scope - the catalog tests legitimately use `std::fs::read_to_string` and `std::fs::write` to
//! build fixtures. Test code is not in a shipped binary, and the property is about what a boot-time
//! load does.
//!
//! **A second `rustix::fs::open` of the same path is not a needle.** The safe open is itself a
//! rustix call, so a needle on it would fire on the site this gate protects; a second one beside it
//! is held by review.
//!
//! **It holds "no second `std::fs` open was written", not "the one open is safe".** Whether the registered
//! `read_dir` walk and the `rustix::fs::open` that follows it are correct is argued by those files'
//! own documentation and by their tests; this gate makes sure no path-based `std::fs` read was
//! written beside them.
//!
//! Comments and the interiors of multi-line strings are blanked first, through the shared lexer
//! [`code_lines`](crate::serde_parse::scan::code_lines) - this repository's own prose quotes the
//! banned `std::fs::read_to_string(&path)` in the comments this gate exists to replace, so a raw
//! scan would report the documentation that argues the rule.

use crate::Verdict;
use crate::repo;
use crate::serde_parse::scan::code_lines;

/// A path-based read this gate refuses, as it is written in source.
///
/// Each needle is the qualified spelling, so `use std::fs as disk; disk::read_to_string` is not
/// found - the same limit `check-bounded-wait`'s header records, and for the same reason: a bare
/// `read_to_string` needle would fire on every method of that name on any type. The `std::fs::`
/// prefixed needles are kept alongside the bare `File::open(` and `OpenOptions::` needles because
/// the latter are reached through `use std::fs::File;` / `use std::fs::OpenOptions;`, while the
/// former are how this workspace spells `read_dir` and `read_to_string` today - with the full path
/// and no `use std::fs;` import in either catalog crate.
const NEEDLES: &[&str] = &[
    "std::fs::read(",
    "std::fs::read_to_string(",
    "std::fs::read_dir(",
    "File::open(",
    "OpenOptions::",
];

/// One registered call site: a file where one of [`NEEDLES`] may appear, and which needle.
///
/// The key is line-independent: the needle itself, not a line number, so the registered call
/// survives reformatting and reordering within the file. **Adding or removing an entry here is an
/// architecture decision**, the sentence the tables in `check-newtype-leaks` and
/// `check-answer-path-caches` both carry.
struct Registered {
    file: &'static str,
    needle: &'static str,
}

/// The committed list of call sites where a path-based read is the registered one.
///
/// One entry per file catalog: the `std::fs::read_dir` that walks the catalog root in
/// each adapter's `documents()` function. The `rustix::fs::open` that follows the walk is the safe
/// open this gate exists to protect, and it is not a `std::fs` call - so no other needle is
/// registered, and any `std::fs::read_to_string`, `File::open` or `OpenOptions` in either crate's
/// non-test source is a violation.
const REGISTERED: &[Registered] = &[
    Registered {
        file: "crates/sutura-catalog-local/src/lib.rs",
        needle: "std::fs::read_dir(",
    },
    Registered {
        file: "crates/sutura-catalog-okf/src/lib.rs",
        needle: "std::fs::read_dir(",
    },
    Registered {
        file: "crates/sutura-catalog-datacontract/src/lib.rs",
        needle: "std::fs::read_dir(",
    },
];

/// The file catalogs this gate walks. Trailing `/` so a sibling whose name merely starts with the
/// same prefix cannot match. A LIST, not a prefix rule: a new catalog that opens files is outside
/// this gate until it is added here.
const SCOPE: &[&str] = &[
    "crates/sutura-catalog-local/src/",
    "crates/sutura-catalog-okf/src/",
    "crates/sutura-catalog-datacontract/src/",
];

/// The files [`REGISTERED`] names - this gate cannot have a verdict without reading each of them,
/// so an entry whose file moved refuses here rather than silently declaring nothing.
fn anchors() -> Vec<&'static str> {
    let mut paths: Vec<&'static str> = REGISTERED.iter().map(|entry| entry.file).collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// Is `rel` a non-test `.rs` file inside [`SCOPE`] that this gate should judge?
///
/// A file named `tests.rs` or one under any `tests/` directory is out of scope: the catalog tests
/// use `std::fs::read_to_string` and `std::fs::write` to build fixtures, and test code is not in a
/// shipped binary.
fn judged(rel: &str) -> bool {
    if std::path::Path::new(rel)
        .extension()
        .is_none_or(|ext| !ext.eq_ignore_ascii_case("rs"))
    {
        return false;
    }
    if !SCOPE.iter().any(|prefix| rel.starts_with(prefix)) {
        return false;
    }
    let segments = std::path::Path::new(rel);
    !segments.components().any(|c| c.as_os_str() == "tests") && segments.file_name().is_none_or(|name| name != "tests.rs")
}

/// One unregistered path-based read, located.
#[derive(Debug)]
struct Violation {
    path: String,
    line: usize,
    needle: &'static str,
}

/// What one walk over the tree found.
struct Scanned {
    violations: Vec<Violation>,
    read: usize,
    witness: String,
}

/// The scan, over a census it does not mint - the same split `check-answer-path-caches` uses so a
/// test can hand it a scratch tree instead of asserting on this file's own source text.
fn scan(census: repo::Census, must_judge: &[&str]) -> Result<Scanned, repo::Refusal> {
    let mut violations = Vec::new();
    let mut read = 0_usize;

    let scope: repo::Scope = judged;
    let inspected = census.inspect(must_judge, scope, |rel, bytes| {
        let text = String::from_utf8_lossy(bytes);
        read = read.saturating_add(1);
        let code = code_lines(&text);
        for (index, line) in code.iter().enumerate() {
            let dense = strip_whitespace(line);
            for needle in NEEDLES {
                if !dense.contains(needle) {
                    continue;
                }
                if is_registered(rel, needle) {
                    continue;
                }
                violations.push(Violation {
                    path: String::from(rel),
                    line: index + 1,
                    needle,
                });
            }
        }
    })?;

    Ok(Scanned {
        violations,
        read,
        witness: inspected.verdict(),
    })
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    if NEEDLES.is_empty() || REGISTERED.is_empty() {
        eprintln!(
            "xtask check-catalog-opened-once: FAILED - a declared list is empty (NEEDLES, \
             REGISTERED); this gate would guard nothing"
        );
        return Verdict::Fail;
    }

    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask check-catalog-opened-once: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let found = match scan(census, &anchors()) {
        Ok(found) => found,
        Err(why) => {
            eprintln!("xtask check-catalog-opened-once: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    decide(&found)
}

/// What to say about a scan. Separated from [`run`] so both verdicts are reachable from a test.
fn decide(found: &Scanned) -> Verdict {
    if found.violations.is_empty() {
        println!(
            "xtask check-catalog-opened-once: ok - {} file(s) in the catalog crates, no \
             unregistered path-based read; {}",
            found.read, found.witness
        );
        return Verdict::Pass;
    }

    for violation in &found.violations {
        eprintln!(
            "xtask check-catalog-opened-once: FAILED - {}:{}: `{}` is a path-based read outside \
             the registered call sites",
            violation.path, violation.line, violation.needle
        );
    }
    eprintln!();
    eprintln!(
        "A catalog document is opened once through a rustix descriptor that is fstat'd and read \
         through that one handle. A second std::fs read of the same path - read_to_string, \
         File::open, OpenOptions - re-opens it and defeats the O_NOFOLLOW|O_NONBLOCK open: a \
         document swapped after the walk is read rather than refused."
    );
    Verdict::Fail
}

/// Is this `needle` in this `file` a registered call site?
fn is_registered(rel: &str, needle: &str) -> bool {
    REGISTERED.iter().any(|entry| entry.file == rel && entry.needle == needle)
}

/// Remove ASCII whitespace so a call broken by rustfmt's `max_width` still matches.
fn strip_whitespace(line: &str) -> String {
    line.chars().filter(|c| !c.is_ascii_whitespace()).collect()
}

#[cfg(test)]
mod tests {
    use super::{NEEDLES, REGISTERED, Scanned, is_registered, scan, strip_whitespace};
    use crate::repo;

    /// A `std::fs::read_to_string` on a line of code is found as a needle.
    #[test]
    fn read_to_string_is_a_needle() {
        assert!(NEEDLES.contains(&"std::fs::read_to_string("));
    }

    /// `File::open` and `OpenOptions` are needles.
    #[test]
    fn file_open_and_open_options_are_needles() {
        assert!(NEEDLES.contains(&"File::open("));
        assert!(NEEDLES.contains(&"OpenOptions::"));
    }

    /// The registered `read_dir` in each catalog crate is registered.
    #[test]
    fn the_walk_is_registered() {
        assert!(is_registered("crates/sutura-catalog-local/src/lib.rs", "std::fs::read_dir("));
        assert!(is_registered("crates/sutura-catalog-okf/src/lib.rs", "std::fs::read_dir("));
    }

    /// A `read_to_string` in a registered file is still a violation: only `read_dir` is registered
    /// there, and the whole point is that no other path-based read appears.
    #[test]
    fn read_to_string_in_a_registered_file_is_still_a_violation() {
        assert!(!is_registered(
            "crates/sutura-catalog-local/src/lib.rs",
            "std::fs::read_to_string("
        ));
    }

    /// Whitespace inside a line is removed before matching, so `std :: fs :: read ( ` is found.
    #[test]
    fn whitespace_is_stripped() {
        assert!(strip_whitespace("std :: fs :: read ( & path )").contains("std::fs::read("));
    }

    /// The gate's own scan, over a scratch tree rather than over the repo.
    fn scan_over(tree: &crate::scratch_tree::Tree, anchors: &[&str]) -> Result<Scanned, repo::Refusal> {
        scan(repo::collect_files(tree.root(), tree.root(), &["rs"]), anchors)
    }

    /// **The refusal itself.** A `std::fs::read_to_string(&path)` added to a registered file is
    /// refused at `decide`'s own level - the shape the falsifier in `task_table::architecture`
    /// seeds into the shared sweep.
    #[test]
    fn an_unregistered_read_to_string_is_refused() {
        let tree = crate::scratch_tree::Tree::of(
            "catalog-opened-once-violation",
            &[(
                "crates/sutura-catalog-local/src/lib.rs",
                b"fn f(path: &Path) {\n    let _ = std::fs::read_to_string(&path);\n}\n",
            )],
        );
        let found = scan_over(&tree, &["crates/sutura-catalog-local/src/lib.rs"]).expect("a readable tree scans");
        assert_eq!(found.violations.len(), 1, "{:?}", found.violations);
        assert_eq!(found.violations[0].needle, "std::fs::read_to_string(");
        assert_eq!(found.violations[0].line, 2);
        assert_eq!(super::decide(&found), crate::Verdict::Fail);
    }

    /// The registered `read_dir` in its registered file passes.
    #[test]
    fn the_registered_read_dir_passes() {
        let tree = crate::scratch_tree::Tree::of(
            "catalog-opened-once-registered",
            &[(
                "crates/sutura-catalog-local/src/lib.rs",
                b"fn documents() {\n    let entries = std::fs::read_dir(&dir);\n}\n",
            )],
        );
        let found = scan_over(&tree, &["crates/sutura-catalog-local/src/lib.rs"]).expect("a readable tree scans");
        assert!(found.violations.is_empty(), "{:?}", found.violations);
    }

    /// A `read_to_string` in a test file (`tests.rs`) is out of scope and not scanned.
    #[test]
    fn a_read_in_a_test_file_is_not_scanned() {
        let tree = crate::scratch_tree::Tree::of(
            "catalog-opened-once-test-file",
            &[
                ("crates/sutura-catalog-local/src/lib.rs", b"fn f() {}\n".as_slice()),
                (
                    "crates/sutura-catalog-local/src/tests.rs",
                    b"fn test() {\n    let s = std::fs::read_to_string(path).unwrap();\n}\n".as_slice(),
                ),
            ],
        );
        // The judged `lib.rs` is there so the scan has a subject: a tree whose only file is test
        // code is refused as judging nothing, which is not the question this cell asks.
        let found = scan_over(&tree, &[]).expect("a readable tree scans");
        assert_eq!(found.read, 1, "only the library file is judged");
        assert!(found.violations.is_empty(), "{:?}", found.violations);
    }

    /// A `read_to_string` under a `tests/` directory is out of scope.
    #[test]
    fn a_read_under_tests_dir_is_not_scanned() {
        let tree = crate::scratch_tree::Tree::of(
            "catalog-opened-once-tests-dir",
            &[
                ("crates/sutura-catalog-local/src/lib.rs", b"fn f() {}\n".as_slice()),
                (
                    "crates/sutura-catalog-local/src/tests/helper.rs",
                    b"fn helper() {\n    let s = std::fs::read_to_string(path).unwrap();\n}\n".as_slice(),
                ),
            ],
        );
        // The judged `lib.rs` is there so the scan has a subject: a tree whose only file is test
        // code is refused as judging nothing, which is not the question this cell asks.
        let found = scan_over(&tree, &[]).expect("a readable tree scans");
        assert_eq!(found.read, 1, "only the library file is judged");
        assert!(found.violations.is_empty(), "{:?}", found.violations);
    }

    /// A path-based read outside the catalog crates is not in scope.
    #[test]
    fn a_read_outside_the_catalog_crates_is_not_scanned() {
        let tree = crate::scratch_tree::Tree::of(
            "catalog-opened-once-out-of-scope",
            &[(
                "crates/sutura-domain/src/lib.rs",
                b"fn f() {\n    let _ = std::fs::read_to_string(path);\n}\n",
            )],
        );
        let Err(why) = scan_over(&tree, &[]) else {
            panic!("a tree with nothing in scope produced a verdict");
        };
        assert!(matches!(why, repo::Refusal::Empty | repo::Refusal::NothingJudged { .. }));
    }

    /// **Measured before it was written: today's real tree carries no unregistered path-based read
    /// in the catalog crates.** This is the "starts green" half, run as a test so a regression
    /// here fails the suite and not only a manual read.
    #[test]
    fn the_real_tree_has_no_unregistered_path_based_read() {
        let Ok(census) = repo::all_files() else {
            panic!("the repo root is what this gate depends on");
        };
        let found = super::scan(census, &super::anchors()).expect("the real tree scans");
        assert_eq!(
            super::decide(&found),
            crate::Verdict::Pass,
            "{:?}",
            found.violations.iter().map(|v| &v.path).collect::<Vec<_>>()
        );
    }

    /// Every registered file is a real file in this workspace, checked against the repo root - so
    /// an entry pointed at a moved file is a rule guarding nothing rather than a silent pass.
    #[test]
    fn every_registered_file_exists() {
        let Some(root) = crate::repo::root() else {
            panic!("the repo root is what this gate depends on");
        };
        for entry in REGISTERED {
            assert!(
                root.join(entry.file).exists(),
                "`{}` is registered but does not exist",
                entry.file
            );
        }
    }
}
