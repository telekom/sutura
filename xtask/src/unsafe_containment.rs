//! The unsafe-containment gate: every crate root re-asserts `forbid(unsafe_code)` but one.
//!
//! `telekom/sutura#929`'s sixth finding needed the workspace's `unsafe_code = "forbid"` relaxed to
//! `deny`, because there is no other way to except one crate: cargo refuses a member that both
//! inherits `[workspace.lints]` and overrides one entry (*"cannot override `workspace.lints` in
//! `lints`"*, measured), and an `#[expect(unsafe_code)]` under an inherited `forbid` is `E0453`.
//! A `deny` a crate can lower is exactly the silent widening this gate exists to refuse, so the
//! strictness moved to the crate roots and this holds it there.
//!
//! # The two rules
//!
//! 1. Every crate root of every workspace member carries `#![forbid(unsafe_code)]`, which the
//!    compiler then makes unlowerable for that whole target - and each integration test, each
//!    bench and each build script is its own target with its own root, which is why they are all
//!    read.
//! 2. Exactly one root is excepted, by name, and the `#[expect(unsafe_code)]` that exception
//!    buys appears in exactly the two files [`EXCEPTED_FILES`] names - the linked drivers' init
//!    declarations and the linked libpq's OAuth hook - and must APPEAR in each. A stale exception
//!    is a widening nobody revoked, so an unused one is a finding too.
//!
//! # What it reads, and what that cannot see
//!
//! Text, with comments blanked by [`crate::rust_source::blank_comments`] - this repository's
//! comments quote the attributes below, so an unblanked scan would read prose as a declaration.
//! Reading text rather than asking a compiler is deliberate and is the reason the gate exists at
//! all: the `unsafe` it contains lives behind `cfg(adbc_driver_linked)`, which only a build that
//! links the driver archive compiles, so `just lint` never judges it and a compiler-based rule
//! would be silent about the one region that matters.
//!
//! **The limits, next to the claim.** It does not parse Rust, so a root whose attribute sits
//! inside a macro or a string would read as present; it says nothing about `unsafe` inside a
//! DEPENDENCY, which is `cargo deny`'s question and not this one; and it judges the roots a
//! member's conventional layout produces (`src/lib.rs`, `src/main.rs`, `build.rs`, `src/bin/*`,
//! `tests/*`, `benches/*`, `examples/*`) rather than the target list cargo resolves - a target
//! declared with an explicit `path` in a manifest is outside its scope.

use std::path::{Path, PathBuf};

use crate::registry::Verdict;

/// The lint, spelled once.
///
/// **Assembled into the needles below rather than written out with them, and that is the rule
/// `crate::rust_source`'s header states**: an assertion gate has to write the shape down to
/// explain itself, and the shape is what it finds. Written whole, this file and its tests would be
/// the first two findings the gate reports - measured, before this was split.
const LINT: &str = "unsafe_code";

/// The attribute every root has to carry.
pub(super) fn reassertion() -> String {
    format!("#![forbid({LINT})]")
}

/// The attribute spellings that lower the lint where `deny` is the level.
///
/// All four, because `expect` and `allow` differ only in whether an unfulfilled one warns, and an
/// inner one at a root or a module lowers everything under it - none may appear outside
/// [`EXCEPTED_FILES`].
pub(super) fn lowerings() -> [String; 4] {
    [
        format!("#[expect({LINT}"),
        format!("#[allow({LINT}"),
        format!("#![allow({LINT}"),
        format!("#![expect({LINT}"),
    ]
}

/// The one crate root that does not re-assert the forbid.
const EXCEPTED_ROOT: &str = "crates/sutura-adbc/src/lib.rs";

/// The two files that may lower the lint, under [`EXCEPTED_ROOT`]'s crate - each must.
const EXCEPTED_FILES: [&str; 2] = ["crates/sutura-adbc/src/linked.rs", "crates/sutura-adbc/src/oauth.rs"];

/// Why the containment is not intact.
#[derive(Debug)]
enum Why {
    /// The root manifest could not be read, or declares no members.
    NoMembers,
    /// The walk found no crate root at all, so nothing was judged.
    NoRoots,
    /// Roots missing the re-assertion, other than the excepted one.
    Unguarded(Vec<String>),
    /// The excepted root carries the re-assertion, so the exception is stale.
    ExceptionUnused,
    /// Files lowering the lint outside the declared ones.
    Lowered(Vec<String>),
    /// Declared files that do not lower the lint, so that part of the exception buys nothing.
    ExceptionEmpty(Vec<String>),
}

impl Why {
    fn describe(&self) -> String {
        match self {
            Self::NoMembers => String::from("the root manifest declares no workspace member this gate could read"),
            Self::NoRoots => String::from("no crate root was found under any declared member, so nothing was judged"),
            Self::Unguarded(paths) => format!(
                "{} crate root(s) do not re-assert `{}`, so the workspace `deny` is lowerable \
                 there: {}",
                paths.len(),
                reassertion(),
                paths.join(", ")
            ),
            Self::ExceptionUnused => format!(
                "`{EXCEPTED_ROOT}` carries `{}` - the exception is stale and should be revoked here \
                 rather than left as a widening nobody uses",
                reassertion()
            ),
            Self::Lowered(paths) => format!(
                "{} file(s) lower the lint outside `{}`: {}",
                paths.len(),
                EXCEPTED_FILES.join("` and `"),
                paths.join(", ")
            ),
            Self::ExceptionEmpty(paths) => format!(
                "{} lower(s) the lint nowhere, so that file's exception buys nothing and should be \
                 revoked",
                paths.join(", ")
            ),
        }
    }
}

/// What the scan counted, for a verdict line that states its own scope.
#[derive(Debug)]
struct Scanned {
    /// Crate roots read.
    roots: usize,
    /// Members those roots came from.
    members: usize,
    /// Every `.rs` file read for a lowering attribute.
    files: usize,
}

/// The paths `members = [...]` declares, relative to the root.
///
/// **Paths and not package names**, which is why this is not
/// [`crate::attribution::workspace_members`]: that one resolves each path to the name in its
/// manifest, and what a root walk needs is the directory.
fn member_paths(manifest: &str) -> Vec<String> {
    let Some(block) = manifest
        .split_once("members = [")
        .and_then(|(_, rest)| rest.split_once("\n]").map(|(block, _)| block))
    else {
        return Vec::new();
    };
    block
        .lines()
        .map(str::trim)
        // Comment lines carry prose that quotes crate names, so a scan for quoted strings that read
        // them would take a name out of a sentence - `attribution::workspace_members` says the same.
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.strip_prefix('"')?.split('"').next())
        .map(String::from)
        .collect()
}

/// One member's conventional crate roots, in a stable order.
fn roots_of(root: &Path, member: &str) -> Vec<PathBuf> {
    let dir = root.join(member);
    let mut found: Vec<PathBuf> = ["src/lib.rs", "src/main.rs", "build.rs"]
        .iter()
        .map(|leaf| dir.join(leaf))
        .filter(|path| path.is_file())
        .collect();
    for sub in ["src/bin", "tests", "benches", "examples"] {
        let Ok(entries) = std::fs::read_dir(dir.join(sub)) else {
            continue;
        };
        let mut rust: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        rust.sort();
        found.extend(rust);
    }
    found
}

/// Every `.rs` file under a member, for the lowering rule.
fn rust_files(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            // A build directory holds generated and vendored `.rs`, which is nobody's declaration.
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            rust_files(&path, into);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            into.push(path);
        }
    }
}

/// Reads the tree and decides.
fn check(root: &Path) -> Result<Scanned, Why> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|_unreadable| Why::NoMembers)?;
    let members = member_paths(&manifest);
    if members.is_empty() {
        return Err(Why::NoMembers);
    }
    let carried = reassertion();
    let mut roots = 0usize;
    let mut unguarded: Vec<String> = Vec::new();
    let mut exception_seen = false;
    for member in &members {
        for path in roots_of(root, member) {
            roots += 1;
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let relative = relative(root, &path);
            let code = without_whitespace(&crate::rust_source::blank_comments(&text));
            let asserts = code.contains(&carried);
            if relative == EXCEPTED_ROOT {
                exception_seen = true;
                if asserts {
                    return Err(Why::ExceptionUnused);
                }
            } else if !asserts {
                unguarded.push(relative);
            }
        }
    }
    if roots == 0 {
        return Err(Why::NoRoots);
    }
    if !unguarded.is_empty() {
        unguarded.sort();
        return Err(Why::Unguarded(unguarded));
    }
    // An absent excepted root is not a finding: a tree that removed the crate has nothing to
    // except, and the `Lowered` rule below still refuses any file that lowers the lint.
    let spellings = lowerings();
    let mut files = 0usize;
    let mut lowered: Vec<String> = Vec::new();
    let mut spent: Vec<&str> = Vec::new();
    for member in &members {
        // From the member's own directory, so `build.rs` and every `tests/` file are read beside
        // `src/` - the lowering rule is about the whole crate and not only its library.
        let mut paths: Vec<PathBuf> = Vec::new();
        rust_files(&root.join(member), &mut paths);
        for path in paths {
            files += 1;
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let code = without_whitespace(&crate::rust_source::blank_comments(&text));
            if !spellings.iter().any(|spelling| code.contains(spelling)) {
                continue;
            }
            let relative = relative(root, &path);
            if let Some(&excepted) = EXCEPTED_FILES.iter().find(|&&excepted| excepted == relative) {
                spent.push(excepted);
            } else {
                lowered.push(relative);
            }
        }
    }
    if !lowered.is_empty() {
        lowered.sort();
        return Err(Why::Lowered(lowered));
    }
    let unspent: Vec<String> = EXCEPTED_FILES
        .iter()
        .filter(|excepted| !spent.contains(excepted))
        .map(|excepted| String::from(*excepted))
        .collect();
    if exception_seen && !unspent.is_empty() {
        return Err(Why::ExceptionEmpty(unspent));
    }
    Ok(Scanned {
        roots,
        members: members.len(),
        files,
    })
}

/// The blanked source with every ASCII whitespace byte removed.
///
/// **So a needle matches an attribute rustfmt wrapped.** `max_width` is 130, and an `#[expect(..,
/// reason = "..")]` long enough to wrap puts a newline between the attribute and the lint name -
/// which a line-shaped scan reads as absent, measured on this tree's one real instance. Both
/// needles this gate carries are whitespace-free, so comparing in this space costs nothing and
/// removes the dependency on how a formatter broke a line.
fn without_whitespace(code: &str) -> String {
    code.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A path as the declarations above spell it: relative to the root, with `/` separators.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The gate.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = crate::repo::root() else {
        eprintln!("xtask check-unsafe: FAILED - no repository root, so nothing was judged");
        return Verdict::Fail;
    };
    match check(&root) {
        Err(why) => {
            eprintln!("xtask check-unsafe: FAILED - {}", why.describe());
            eprintln!();
            eprintln!("The workspace denies this lint, which a crate can lower. What makes that as strong");
            eprintln!("as the `forbid` it replaced is `{}` at every crate root,", reassertion());
            eprintln!("with one declared exception ({EXCEPTED_ROOT}) for the ADBC drivers a static");
            eprintln!("musl artefact carries and its libpq's OAuth hook. telekom/sutura#929, #913.");
            Verdict::Fail
        }
        Ok(scanned) => {
            println!(
                "xtask check-unsafe: ok - {} crate root(s) across {} member(s) re-assert `{}`, one declared \
                 exception spent in its two files, and {} source file(s) hold no other lowering",
                scanned.roots,
                scanned.members,
                reassertion(),
                scanned.files
            );
            Verdict::Pass
        }
    }
}

#[cfg(test)]
mod tests;
