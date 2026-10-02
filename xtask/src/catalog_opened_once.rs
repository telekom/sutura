//! Each catalog document is opened once, via a rustix `O_NOFOLLOW|O_NONBLOCK` descriptor that is
//! `fstat`'d and read through that one handle. A second, unguarded `std::fs::read_to_string(&path)`
//! right after the safe open is the defect this gate refuses - and until it landed, the property
//! "opened once" was held by review alone, because a swap-timing test cannot land in the
//! sub-microsecond window between two back-to-back opens.
//!
//! This is a STATIC mechanism: a text scan over the non-test source of every `crates/sutura-catalog-*`
//! crate and of `sutura-bounded-read`, the crate whose walk and open they share, that refuses any
//! path-based read outside a committed list of registered call sites. What is walked is decided by
//! [`walked_crate`] off the directory name, so a newly added catalog is walked the day it lands;
//! every `sutura-catalog-*` workspace member from `cargo metadata` adds its `src/lib.rs` as an
//! anchor ([`mandatory_files`]), so a member the walk does not reach refuses instead of passing. It
//! is the same shape as `check-answer-path-caches` and `check-bounded-wait`: a rule the code cannot
//! state about itself, read as text, starting from a tree that already obeys it.
//!
//! # The rule
//!
//! Over the non-test `.rs` files of every `crates/sutura-catalog-*` crate and of
//! `sutura-bounded-read` - the whole crate directory, not only `src/` - every occurrence of one of
//! [`NEEDLES`] must match a [`Registered`] entry - a file and a needle - or it is a violation naming
//! the file and line. [`REGISTERED`] is the committed list, and adding or removing an entry is an
//! architecture decision, exactly as the tables in `check-newtype-leaks` and
//! `check-answer-path-caches` are.
//!
//! # What it deliberately does NOT cover
//!
//! **Membership is a name prefix.** A crate is walked when its directory under `crates/` is named
//! `sutura-catalog-*` (or is `sutura-bounded-read`); a crate that loads catalog files under any other
//! name is outside this gate. A `sutura-catalog-*` workspace member that is not at
//! `crates/<its name>/`, or has no `src/lib.rs`, refuses rather than passes - its anchor is never
//! judged.
//!
//! **A read through an alias or a helper in another crate escapes.** The scan reads text in these
//! crates; `use std::fs as disk;` followed by `disk::read_to_string` is not found, and neither
//! is a function in a third crate that reads a path, called from here. `use std::fs;` (or
//! `use std::{fs, io};`) followed by `fs::read_to_string` IS found: [`qualify`] reads a bare `fs::`
//! as `std::fs::` before matching, which also reads any other `fs::` path that way - a
//! `tokio::fs::read_to_string(` is refused too. Renaming on import is the same blind spot
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
/// prefixed needles also see `fs::read_to_string` after `use std::fs;`, because [`qualify`] runs
/// first; the bare `File::open(` and `OpenOptions::` needles are reached through
/// `use std::fs::File;` / `use std::fs::OpenOptions;`.
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
/// The `std::fs::read_dir` in `sutura-bounded-read`'s `walk`, which every file catalog's
/// `documents()` calls; and `sutura-catalog-wren`'s two reads, which are an offline importer's
/// (`sutura import wren`) - it reads one source manifest and checks that its destination is empty,
/// and is never a catalog loaded at boot. The `rustix::fs::open` in `read_document` is the safe
/// open this gate exists to protect, and it is not a `std::fs` call - so any other path-based read
/// in a walked crate's non-test source is a violation.
const REGISTERED: &[Registered] = &[
    Registered {
        file: "crates/sutura-bounded-read/src/walk.rs",
        needle: "std::fs::read_dir(",
    },
    Registered {
        file: "crates/sutura-catalog-wren/src/lib.rs",
        needle: "std::fs::read_to_string(",
    },
    Registered {
        file: "crates/sutura-catalog-wren/src/lib.rs",
        needle: "std::fs::read_dir(",
    },
];

/// Is `rel` the source of a crate this gate walks - any `crates/sutura-catalog-*` crate, or the one
/// crate whose walk and open the catalogs share? This, not the workspace member list, decides what
/// is judged.
///
/// A name prefix on the directory rather than a declared list, so a newly added catalog is walked
/// the day its `crates/` directory lands; the header's first limit is what that costs. A bare `fn`
/// because [`repo::Scope`] is one, and taking the first path segment keeps a sibling elsewhere whose
/// name merely starts with the same prefix from matching.
fn walked_crate(rel: &str) -> bool {
    let Some(name) = rel.strip_prefix("crates/").and_then(|rest| rest.split('/').next()) else {
        return false;
    };
    name.starts_with("sutura-catalog-") || name == "sutura-bounded-read"
}

/// The `sutura-catalog-*` workspace members in `cargo metadata --no-deps` output (`packages` under
/// `--no-deps` is exactly the member set), each as the `crates/NAME/src/` it is expected at. An
/// empty set refuses: it would anchor nothing beyond [`REGISTERED`].
fn scope_from_metadata(metadata: &serde_json::Value) -> Result<Vec<String>, String> {
    let packages = metadata
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;
    let mut scope: Vec<String> = packages
        .iter()
        .filter_map(|p| p.get("name").and_then(serde_json::Value::as_str))
        .filter(|name| name.starts_with("sutura-catalog-"))
        .map(|name| format!("crates/{name}/src/"))
        .collect();
    scope.sort();
    scope.dedup();
    if scope.is_empty() {
        return Err(String::from("cargo metadata named no sutura-catalog-* workspace members"));
    }
    Ok(scope)
}

/// The paths a verdict cannot be had without: each [`REGISTERED`] file, and every member in
/// `scope`'s `src/lib.rs`. The second half is the fail-closed part - a member [`walked_crate`] does
/// not reach refuses ([`repo::Refusal::NotJudged`]) rather than passing on a crate never read.
fn mandatory_files(scope: &[String]) -> Vec<String> {
    let mut must_judge: Vec<String> = anchors().into_iter().map(String::from).collect();
    must_judge.extend(scope.iter().map(|prefix| format!("{prefix}lib.rs")));
    must_judge.sort_unstable();
    must_judge.dedup();
    must_judge
}

/// The files [`REGISTERED`] names - this gate cannot have a verdict without reading each of them,
/// so an entry whose file moved refuses here rather than silently declaring nothing.
fn anchors() -> Vec<&'static str> {
    let mut paths: Vec<&'static str> = REGISTERED.iter().map(|entry| entry.file).collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// Is `rel` a non-test `.rs` file in a walked crate that this gate should judge?
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
    if !walked_crate(rel) {
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
            let dense = qualify(&strip_whitespace(line));
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
    verdict(crate::cargo_metadata(&["--no-deps"]), repo::all_files())
}

/// [`run`] over the member list and the census it is handed, so a test reaches every refusal arm -
/// a metadata error, an empty member list, an anchor never judged - with a scratch tree.
fn verdict(metadata: Result<serde_json::Value, String>, census: Result<repo::Census, repo::Refusal>) -> Verdict {
    let scope = match metadata.and_then(|metadata| scope_from_metadata(&metadata)) {
        Ok(scope) => scope,
        Err(message) => {
            eprintln!("xtask check-catalog-opened-once: FAILED - {message}");
            return Verdict::Fail;
        }
    };
    let mandatory = mandatory_files(&scope);
    let must_judge: Vec<&str> = mandatory.iter().map(String::as_str).collect();
    match census.and_then(|census| scan(census, &must_judge)) {
        Ok(found) => decide(&found),
        Err(why) => {
            eprintln!("xtask check-catalog-opened-once: FAILED - {}", why.describe());
            Verdict::Fail
        }
    }
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

/// Read every `fs::` path as `std::fs::`, so `fs::read_to_string(` after `use std::fs;` meets the
/// same needle - and the same registration - as its qualified spelling. Collapsing first keeps an
/// already-qualified call from becoming `std::std::fs::`.
fn qualify(dense: &str) -> String {
    dense.replace("std::fs::", "fs::").replace("fs::", "std::fs::")
}

#[cfg(test)]
mod tests {
    use super::{NEEDLES, REGISTERED, Scanned, is_registered, scan, strip_whitespace, verdict};
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

    /// The shared walk's `read_dir` is registered, and a catalog's own is not.
    #[test]
    fn the_walk_is_registered() {
        assert!(is_registered("crates/sutura-bounded-read/src/walk.rs", "std::fs::read_dir("));
        assert!(!is_registered("crates/sutura-catalog-local/src/lib.rs", "std::fs::read_dir("));
    }

    /// A `read_to_string` in a registered file is still a violation: only `read_dir` is registered
    /// there, and the whole point is that no other path-based read appears.
    #[test]
    fn read_to_string_in_a_registered_file_is_still_a_violation() {
        assert!(!is_registered(
            "crates/sutura-bounded-read/src/walk.rs",
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
                "crates/sutura-bounded-read/src/walk.rs",
                b"fn walk() {\n    let entries = std::fs::read_dir(&dir);\n}\n",
            )],
        );
        let found = scan_over(&tree, &["crates/sutura-bounded-read/src/walk.rs"]).expect("a readable tree scans");
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

    /// One spelling after `use std::fs;` - the bare `fs::NAME(` - in a walked catalog crate is
    /// refused as its qualified needle.
    fn refuses_the_imported_spelling(call: &str, needle: &str) {
        let source = format!("use std::fs;\nfn f(path: &Path) {{\n    let _ = {call}(&path);\n}}\n");
        let tree = crate::scratch_tree::Tree::of(
            "catalog-opened-once-imported-spelling",
            &[("crates/sutura-catalog-local/src/lib.rs", source.as_bytes())],
        );
        let found = scan_over(&tree, &["crates/sutura-catalog-local/src/lib.rs"]).expect("a readable tree scans");
        assert_eq!(found.violations.len(), 1, "{:?}", found.violations);
        assert_eq!(found.violations[0].needle, needle);
        assert_eq!(found.violations[0].line, 3);
        assert_eq!(super::decide(&found), crate::Verdict::Fail);
    }

    #[test]
    fn an_imported_fs_read_to_string_is_refused() {
        refuses_the_imported_spelling("fs::read_to_string", "std::fs::read_to_string(");
    }

    #[test]
    fn an_imported_fs_read_is_refused() {
        refuses_the_imported_spelling("fs::read", "std::fs::read(");
    }

    #[test]
    fn an_imported_fs_read_dir_is_refused() {
        refuses_the_imported_spelling("fs::read_dir", "std::fs::read_dir(");
    }

    /// A previously-unlisted `sutura-catalog-*` crate is walked: the crate set is read off the
    /// directory name rather than a declared list, so a newly added catalog's unregistered read is
    /// caught the day the crate directory lands.
    #[test]
    fn a_new_unregistered_catalog_crate_is_caught() {
        let tree = crate::scratch_tree::Tree::of(
            "catalog-opened-once-new-crate",
            &[(
                "crates/sutura-catalog-newcrate/src/lib.rs",
                b"fn f(path: &Path) {\n    let _ = std::fs::read_to_string(&path);\n}\n",
            )],
        );
        let found = scan_over(&tree, &["crates/sutura-catalog-newcrate/src/lib.rs"])
            .expect("the directory-derived predicate walks a new catalog crate");
        assert_eq!(found.violations.len(), 1, "{:?}", found.violations);
        assert_eq!(super::decide(&found), crate::Verdict::Fail);
    }

    /// `cargo metadata --no-deps` naming these packages.
    fn members(names: &[&str]) -> serde_json::Value {
        serde_json::json!({ "packages": names.iter().map(|name| serde_json::json!({ "name": name })).collect::<Vec<_>>() })
    }

    /// A scratch-tree file: its path and its bytes.
    type Fixture = (&'static str, &'static [u8]);

    /// The files of a tree that holds every [`REGISTERED`] file and one clean catalog.
    const CLEAN: [Fixture; 3] = [
        (
            "crates/sutura-bounded-read/src/walk.rs",
            b"fn walk() {\n    let _ = std::fs::read_dir(&dir);\n}\n",
        ),
        ("crates/sutura-catalog-wren/src/lib.rs", b"fn f() {}\n"),
        ("crates/sutura-catalog-local/src/lib.rs", b"fn f() {}\n"),
    ];

    fn census_of(tree: &crate::scratch_tree::Tree) -> repo::Census {
        repo::collect_files(tree.root(), tree.root(), &["rs"])
    }

    /// **The derived anchors fail closed.** A `sutura-catalog-*` member whose `lib.rs` sits where
    /// [`super::walked_crate`] does not reach refuses on that anchor instead of passing on a tree
    /// it judged only partly.
    #[test]
    fn a_member_the_walk_does_not_reach_refuses_on_its_anchor() {
        let [walk, wren, local] = CLEAN;
        let tree = crate::scratch_tree::Tree::of(
            "catalog-opened-once-unwalked-member",
            &[
                walk,
                wren,
                local,
                ("crates/catalogs/sutura-catalog-x/src/lib.rs", b"fn f() {}\n"),
            ],
        );
        let scope = vec![String::from("crates/sutura-catalog-x/src/")];
        let mandatory = super::mandatory_files(&scope);
        let must_judge: Vec<&str> = mandatory.iter().map(String::as_str).collect();
        let Err(why) = scan(census_of(&tree), &must_judge) else {
            panic!("a member never judged produced a verdict");
        };
        assert!(
            matches!(&why, repo::Refusal::NotJudged { path, .. } if path == "crates/sutura-catalog-x/src/lib.rs"),
            "{why:?}"
        );
        assert_eq!(
            verdict(Ok(members(&["sutura-catalog-x"])), Ok(census_of(&tree))),
            crate::Verdict::Fail
        );
    }

    /// A metadata error refuses rather than running with no derived anchors. The same tree passes
    /// with its members named, so the refusal is the error's.
    #[test]
    fn a_metadata_error_refuses() {
        let tree = crate::scratch_tree::Tree::of("catalog-opened-once-metadata-error", &CLEAN);
        let named = members(&["sutura-catalog-local", "sutura-catalog-wren"]);
        assert_eq!(verdict(Ok(named), Ok(census_of(&tree))), crate::Verdict::Pass);
        assert_eq!(
            verdict(Err(String::from("no cargo")), Ok(census_of(&tree))),
            crate::Verdict::Fail
        );
    }

    /// A workspace naming no `sutura-catalog-*` member refuses rather than reading as "no catalogs
    /// to guard". The same tree passes with its members named, so the refusal is the empty list's.
    #[test]
    fn an_empty_member_list_refuses() {
        let tree = crate::scratch_tree::Tree::of("catalog-opened-once-no-members", &CLEAN);
        let named = members(&["sutura-catalog-local", "sutura-catalog-wren"]);
        assert_eq!(verdict(Ok(named), Ok(census_of(&tree))), crate::Verdict::Pass);
        assert_eq!(
            verdict(Ok(members(&["sutura-app"])), Ok(census_of(&tree))),
            crate::Verdict::Fail
        );
    }

    /// The members `cargo metadata` names are exactly the `sutura-catalog-*` directories under
    /// `crates/`, so every one of them is anchored.
    #[test]
    fn the_derived_scope_names_every_catalog_directory() {
        let metadata = crate::cargo_metadata(&["--no-deps"]).expect("cargo metadata resolves in-tree");
        let scope = super::scope_from_metadata(&metadata).expect("the workspace has catalog members");
        let mut real: Vec<String> = std::fs::read_dir(crate::repo::root().expect("the repo root").join("crates"))
            .expect("crates/ is readable")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str().map(String::from))
            .filter(|name| name.starts_with("sutura-catalog-"))
            .map(|name| format!("crates/{name}/src/"))
            .collect();
        real.sort();
        assert_eq!(scope, real);
    }

    /// The real tree passes the whole gate - its derived anchors included, not only [`REGISTERED`]'s.
    #[test]
    fn the_real_tree_passes_against_its_derived_anchors() {
        assert_eq!(
            verdict(crate::cargo_metadata(&["--no-deps"]), repo::all_files()),
            crate::Verdict::Pass
        );
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
