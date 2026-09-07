//! Which files are the corpus a reach may come from, and the witness that every path the listing
//! offered got a verdict.
//!
//! # Two questions, and both were answered by the same loop before this file existed
//!
//! * **Whose code is "a run here"?** `cargo nextest run --workspace` compiles workspace MEMBERS,
//!   and `[workspace] exclude` keeps `vendor/mimalloc_rust` out of that set - so a `#[test]` in
//!   vendored source is not a run in this venue. Measured on merged `main`
//!   (`github.com/telekom/sutura#400`): a variant whose ONLY reach was
//!   `vendor/mimalloc_rust/src/lib.rs:73` passed at exit 0. Vendored code was 10 of the 1333
//!   declarations and 2 of the 205 files, so no count moved enough to notice. `crate::fmt` already
//!   derives its scope from membership for the neighbouring reason - reformatting vendored source
//!   makes it stop matching the release it claims to be - and this is the same authority read the
//!   cheap way, off the root manifest rather than out of `cargo metadata`, because this gate runs
//!   in a pre-commit hook and in the git-derived nix sandbox where a subprocess is a liability.
//! * **Did the scan read what the listing offered?** The count in the old verdict line was
//!   `sources.len()`, taken off the same filter as the loop, which is the shape
//!   `github.com/telekom/sutura#414` collects five measured instances of: narrowing the loop
//!   narrows the witness with it. Measured here before this file: a truncated walk gave
//!   **12 of 205 files, 69 of 1333 declarations, exit 0**.
//!
//! # What the witness is, and what it is not
//!
//! The denominator is the WHOLE listing - every path git publishes, counted before any predicate
//! of this gate's - and every one of them leaves the loop as exactly one [`Looked`]. So the
//! numerator cannot be narrowed without the equality breaking, and the denominator is counted by
//! nothing this gate's parser could reject: `#414`'s correction of its own issue is that
//! `check-guidance`'s pages half counts `offered` and `read` off ONE slice, which defends a
//! narrowed loop and not a narrowed discovery.
//!
//! **A pair of counts is still not enough, and this file states it rather than implying it.** An
//! item the walk never counted cannot be caught by a floor over the count, so narrowing the SCOPE
//! predicate - `is_rust` to something narrower - moves files from [`Looked::Read`] to
//! [`Looked::Outside`] and the equality holds. That is what [`Corpus::barren`] is for: a SET OF
//! NAMES, the workspace's own member list against the members the scan actually reached, which is
//! the instrument `sutura/invariants` records as beating a pair of counts. It is derived from the
//! root manifest rather than from the listing, so a narrowing of either does not move both.
//!
//! **What neither reaches:** `repo::all_files` itself, and the boundary is worth stating exactly.
//! In a git checkout `offered` is `git ls-files`' own answer, so a directory the process cannot
//! enter is git's problem and not a silent narrowing. In the git-derived nix sandbox there is no
//! `.git`, `all_files` falls back to its own walk, and that walk still drops a `read_dir` error
//! and a `DirEntry` error in silence - verified at `repo.rs:88`, `:194` and `:236` on `110591d5`,
//! AFTER `#373` merged, so that PR did not close them. `github.com/telekom/sutura#414` owns the
//! shape; a sibling measured a base walk falling from 373 to 371 files at exit 0. So in that venue
//! `offered` is a floor over what the walk reached, not over what the tree holds - which is why
//! the member anchor below is the load-bearing half rather than the accounting.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The directory whose children are the deployment variants, with the separator that makes it a
/// path prefix. One constant: a scan for the segment and a message naming the directory must not
/// drift apart.
pub(crate) const EXAMPLES: &str = "examples/";

/// Is this path Rust? Case-insensitive, for the reason `docs::is_markdown` gives: half of this
/// repo is developed on a filesystem that does not distinguish `.RS` from `.rs`.
pub(crate) fn is_rust(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Is this path inside `dir`, as a whole leading path segment?
pub(crate) fn is_under(rel: &str, dir: &str) -> bool {
    rel.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// The crate this gate lives in, whose files are excluded from its own scan.
///
/// Not tidiness - it is the difference between a gate and a mirror, and the mirror was live one
/// file over. That module's own fixtures hold paths under `examples/`, and `changes.rs` holds
/// `examples/single-player/...` as a classification fixture; both are test code by every rule the
/// gate uses. Measured: repointing EVERY `crates/**/*.rs` mention of `examples/single-player` left
/// the verdict green on those two fixtures alone. No figure is written here - the count moved from
/// 23 to 40 between that measurement and this sentence;
/// `git grep -c "examples/single-player" -- "crates/**/*.rs"` answers it. No gate's test runs a
/// deployment example, so the crate is the honest scope rather than one file.
///
/// DERIVED rather than written down, because a path constant is held by recall and fails OPEN when
/// it stops matching: `mv xtask/src/examples.rs xtask/src/examples/mod.rs` still compiles, and it
/// silently re-admitted this file's fixtures with the file count as the only tell. The manifest
/// directory's own last segment cannot disagree with where this file lives, and the caller fails
/// closed if it excludes nothing at all.
pub(crate) fn gate_crate() -> Option<&'static str> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
}

/// The workspace's member directories, read off the root manifest.
///
/// The field is private and [`Self::parse`] is the only constructor, so a caller cannot narrow the
/// scope by handing over a shorter list than the manifest declares.
///
/// A GLOB IS A REFUSAL rather than a best effort. `members = ["crates/*"]` is legal and this
/// parser cannot expand it; treating an unexpanded pattern as a directory name would put every
/// crate outside the workspace and take the whole corpus with it, which is the silent-pass
/// direction. Nothing in this workspace writes one today, and "no such spelling exists yet" is not
/// a mechanism, so the refusal is here instead.
#[derive(Debug)]
pub(crate) struct Members {
    dirs: Vec<String>,
}

impl Members {
    /// The member directories declared under `[workspace]` in `text`.
    ///
    /// Line-based, in the same spirit as `changes::package_name`: the TOML shape read here is one
    /// key of one table, and the alternative - `cargo metadata` - is a subprocess this gate cannot
    /// count on, since it runs as a pre-commit hook and inside the git-derived nix sandbox.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut in_workspace = false;
        let mut collecting = false;
        let mut dirs = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if !collecting && trimmed.starts_with('[') {
                in_workspace = trimmed == "[workspace]";
                continue;
            }
            if !in_workspace {
                continue;
            }
            if !collecting {
                let Some(rest) = trimmed.strip_prefix("members") else {
                    continue;
                };
                let Some(rest) = rest.trim_start().strip_prefix('=') else {
                    continue;
                };
                collecting = true;
                if !push_quoted(rest, &mut dirs)? {
                    continue;
                }
                break;
            }
            if push_quoted(trimmed, &mut dirs)? {
                break;
            }
        }
        if dirs.is_empty() {
            return Err(String::from(
                "the root manifest declares no `[workspace] members`, so this scan cannot say whose code a run here compiles - a corpus of everything would count a vendored test as a run in this venue, which is the state this reads the manifest to leave",
            ));
        }
        Ok(Self { dirs })
    }

    /// The member directory `rel` sits in, or `None` for a path no member owns.
    fn owner(&self, rel: &str) -> Option<&str> {
        self.dirs.iter().find(|dir| is_under(rel, dir)).map(String::as_str)
    }

    /// Every declared member directory.
    fn dirs(&self) -> &[String] {
        &self.dirs
    }
}

/// Collect the quoted entries in `rest`, returning whether the array's `]` was reached.
///
/// A `*` anywhere in an entry is the glob refusal [`Members`] documents.
fn push_quoted(rest: &str, dirs: &mut Vec<String>) -> Result<bool, String> {
    let body = rest.split('#').next().unwrap_or_default();
    let mut at = body;
    while let Some((_, after)) = at.split_once('"') {
        let Some((entry, tail)) = after.split_once('"') else {
            break;
        };
        if entry.contains('*') {
            return Err(format!(
                "the root manifest declares the workspace member `{entry}`, and this scan cannot expand a glob - an unexpanded pattern owns no directory, so every crate would read as outside the workspace and the corpus would be empty"
            ));
        }
        dirs.push(String::from(entry.trim_end_matches('/')));
        at = tail;
    }
    Ok(body.contains(']'))
}

/// What one offered path turned out to be.
///
/// THREE ANSWERS AND NO FOURTH. [`Corpus::read`] matches this exhaustively, so a new kind of
/// answer is `error[E0004]` rather than a path that quietly stops being counted -
/// `github.com/telekom/sutura#414`'s fourth property, and the reason it is an `enum` rather than
/// a `continue` with a comment. `Unreachable` is a value for the same reason: a `continue` there
/// is how `text-hygiene` came to hide an unreadable in-scope file inside its own count.
enum Looked {
    /// In scope, and read: the file's text as written.
    Read(String),
    /// Deliberately outside the corpus, by the rule that put it there.
    Outside(Outside),
    /// In scope and NOT read, with the error. A problem at the caller, never a drop.
    Unreachable(String),
}

/// Which rule put a path outside the corpus.
///
/// Three rules, each with its own count in the verdict, because they fail differently: a scope
/// that stops matching Rust, a workspace that stops declaring members and a self-exclusion that
/// stops matching all read as "fewer files" from one number.
enum Outside {
    /// Not Rust source. The bulk of the listing, and the reason the denominator is the whole
    /// listing rather than the Rust half of it: this is what the loop's parser rejects, so
    /// counting it is what makes the denominator a different derivation from the numerator.
    NotRust,
    /// Rust under no workspace member - vendored source, or a crate the manifest does not build.
    /// Not a run in this venue.
    NoMember,
    /// Rust in this gate's own crate. See [`gate_crate`].
    GateCrate,
}

/// The corpus, and the accounting that says the scan reached all of it.
///
/// Every field is private and [`Self::read`] is the only constructor that ships, so no caller can
/// state a number the walk did not reach - the shape `check-warm-start`'s `Swept` already uses,
/// generalised no further than this gate needs. `#[cfg(test)]` adds [`Self::fixture`] so the pure
/// verdict arms stay testable without a checkout; that is a test-only door, and the property this
/// type exists for is a property of every production caller.
#[derive(Debug)]
pub(crate) struct Corpus {
    /// Paths the listing offered - the whole of it, counted before any predicate of this gate's.
    offered: usize,
    /// The in-scope files that were read: repo-relative path to raw text.
    sources: BTreeMap<String, String>,
    /// Paths that were not Rust.
    not_rust: usize,
    /// Rust files under no workspace member.
    outside: usize,
    /// Rust files in this gate's own crate. Zero is a broken exclusion, not a clean tree.
    own: usize,
    /// In-scope files the scan could not read, each with the error.
    unreachable: Vec<String>,
    /// Declared workspace members the scan reached no file of, this gate's own crate aside.
    barren: Vec<String>,
    /// This gate's own crate directory, for the messages that name it.
    crate_dir: &'static str,
}

impl Corpus {
    /// Read the corpus out of `files`, the listing rooted at `root`.
    ///
    /// The loop is here rather than at the call site on purpose: the gate hands over a listing and
    /// receives a witness, so the narrowing `#414` measured five times - `.take(n)` between the
    /// discovery and the count that reports it - has nowhere to be written in a gate. That is as
    /// far as this goes, and the limit is worth stating: `repo::all_files`' return type is still
    /// `Vec<String>`, so nothing stops a gate walking it directly. Changing that is `#414`'s own
    /// shared change across 25 call sites, not this one.
    pub(crate) fn read(root: &Path, files: &[String], members: &Members, crate_dir: &'static str) -> Result<Self, String> {
        let offered = files.len();
        let mut found = Self {
            offered,
            sources: BTreeMap::new(),
            not_rust: 0,
            outside: 0,
            own: 0,
            unreachable: Vec::new(),
            barren: Vec::new(),
            crate_dir,
        };
        let mut reached: BTreeSet<&str> = BTreeSet::new();
        for rel in files {
            match look(root, rel, members, crate_dir) {
                Looked::Read(text) => {
                    if let Some(owner) = members.owner(rel) {
                        reached.insert(owner);
                    }
                    found.sources.insert(rel.clone(), text);
                }
                Looked::Outside(Outside::NotRust) => found.not_rust = found.not_rust.saturating_add(1),
                Looked::Outside(Outside::NoMember) => found.outside = found.outside.saturating_add(1),
                Looked::Outside(Outside::GateCrate) => found.own = found.own.saturating_add(1),
                Looked::Unreachable(entry) => found.unreachable.push(entry),
            }
        }
        found.barren = members
            .dirs()
            .iter()
            .filter(|dir| dir.as_str() != crate_dir && !reached.contains(dir.as_str()))
            .cloned()
            .collect();
        let verdicts = found
            .sources
            .len()
            .saturating_add(found.not_rust)
            .saturating_add(found.outside)
            .saturating_add(found.own)
            .saturating_add(found.unreachable.len());
        if verdicts != offered {
            return Err(format!(
                "gave a verdict to {verdicts} of the {offered} path(s) the listing offered - the rest left the walk uncounted, so every number this gate prints would be about a subset it chose silently"
            ));
        }
        Ok(found)
    }

    /// The files a reach may come from: repo-relative path to raw text.
    pub(crate) const fn sources(&self) -> &BTreeMap<String, String> {
        &self.sources
    }

    /// This gate's own crate directory.
    pub(crate) const fn crate_dir(&self) -> &'static str {
        self.crate_dir
    }

    /// How many files the self-exclusion removed.
    pub(crate) const fn own(&self) -> usize {
        self.own
    }

    /// In-scope files the scan could not read.
    pub(crate) fn unreachable(&self) -> &[String] {
        &self.unreachable
    }

    /// Declared members the scan reached no file of.
    pub(crate) fn barren(&self) -> &[String] {
        &self.barren
    }

    /// The sentence the verdict prints: the accounting, from this witness's own fields.
    ///
    /// Every number here is read off a private field of a value only [`Self::read`] builds, so the
    /// line cannot state a total the walk did not reach.
    pub(crate) fn witness(&self) -> String {
        format!(
            "every one of {} path(s) git publishes got a verdict ({} read, {} not Rust, {} outside the workspace, {} under `{}/`)",
            self.offered,
            self.sources.len(),
            self.not_rust,
            self.outside,
            self.own,
            self.crate_dir
        )
    }

    /// A corpus with the fields the pure verdict arms vary, and a healthy remainder.
    #[cfg(test)]
    pub(crate) const fn fixture(own: usize, unreachable: Vec<String>, barren: Vec<String>) -> Self {
        Self {
            offered: 0,
            sources: BTreeMap::new(),
            not_rust: 0,
            outside: 0,
            own,
            unreachable,
            barren,
            crate_dir: "xtask",
        }
    }
}

/// The variant a reach is evidence for, or `None` if the path it names is not one git publishes.
///
/// The scan reads a LITERAL and the literal is repo-relative only under an assumption about the
/// root it is joined to - `github.com/telekom/sutura#308`'s second limit. Requiring the path to
/// resolve against the published listing does not close that (measured: the issue's own
/// `tmp.join("examples/multi-player")` names a directory this repository publishes, so it still
/// counts); what it removes is the shape a synthetic corpus actually takes, a FABRICATED path at
/// the same anchor - and, in the same move, a stale one, which used to hold a variant green while
/// naming a file nothing could open.
///
/// A directory counts through the files under it, because git publishes no directory: one
/// authority for this and for [`variants`], which is what stops the two halves disagreeing.
pub(crate) fn resolved(reach: &str, published: &BTreeSet<String>) -> Option<String> {
    if !published.contains(reach) {
        return None;
    }
    reach
        .strip_prefix(EXAMPLES)
        .map(|rest| rest.split('/').next().unwrap_or_default())
        .filter(|name| !name.is_empty())
        .map(String::from)
}

/// Every path under `examples/` this line of code reaches for.
///
/// `find` in a loop rather than once, because a second path on the same line used to be invisible.
/// The whole path rather than the variant name alone, so [`resolved`] can ask the listing about
/// what the line actually names: `examples/x` and `examples/x/corpus-with-two-metrics.json` are
/// the same variant and are not the same claim.
pub(crate) fn reaches_for(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = 0_usize;
    while let Some(offset) = line.get(at..).and_then(|rest| rest.find(EXAMPLES)) {
        let start = at.saturating_add(offset);
        at = start.saturating_add(EXAMPLES.len());
        if !anchored(line.get(..start).unwrap_or_default()) {
            continue;
        }
        let rest: String = line
            .get(at..)
            .unwrap_or_default()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(*c, '-' | '_' | '.' | '/'))
            .collect();
        let named = rest.trim_end_matches('/');
        if !named.is_empty() {
            found.push(format!("{EXAMPLES}{named}"));
        }
    }
    found
}

/// Is a path boundary in front of the match, of one of the two shapes a repo-relative reach takes?
///
/// The start of a string literal, or a `../` walking up out of `CARGO_MANIFEST_DIR`. Measured over
/// this workspace: those two cover every reach in it, and nothing else does. `#` is deliberately
/// not a comment marker anywhere here - it opens one in a shell and an ATTRIBUTE in Rust, and
/// `#[path = "../../examples/x/mod.rs"]` is a real reach.
pub(crate) fn anchored(before: &str) -> bool {
    before.is_empty() || before.ends_with('"') || before.ends_with("../")
}

/// The deployment variants, read off the same listing the evidence is read from.
///
/// One authority for both halves of the gate. `read_dir` was the other candidate and disagreed
/// with the scan in two directions, both reproduced: `mkdir examples/x` was a local FAILURE that
/// the git-derived nix sandbox and CI could not see, because git tracks no empty directory - a red
/// no venue reproduces; and a gitignored or symlinked child was a variant that can never be
/// published, while `hygiene` is a pre-commit hook, so it blocked every commit over a path git
/// will never publish. A directory git would not publish is not one a reader can find, which is
/// the failure this gate exists for.
pub(crate) fn variants(files: &[String]) -> BTreeSet<String> {
    files
        .iter()
        .filter_map(|rel| rel.strip_prefix(EXAMPLES))
        .filter_map(|rest| rest.split_once('/'))
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, _)| String::from(name))
        .collect()
}

/// Every path under `examples/` git publishes, each directory included through the files in it.
///
/// The same listing [`variants`] reads, for the same reason: a reach resolved against the
/// filesystem and a variant read off git would disagree exactly where that module's doc says they
/// did. A directory is in no listing, so each ancestor of a published file is added here - which
/// is what lets `join("../../examples/single-player")` resolve while
/// `join("../../examples/single-player/gone.yaml")` does not.
pub(crate) fn publishes(files: &[String]) -> BTreeSet<String> {
    let mut published = BTreeSet::new();
    for rel in files.iter().filter(|rel| rel.starts_with(EXAMPLES)) {
        let mut at = 0_usize;
        while let Some(offset) = rel.get(at..).and_then(|rest| rest.find('/')) {
            at = at.saturating_add(offset).saturating_add(1);
            published.insert(String::from(rel.get(..at.saturating_sub(1)).unwrap_or_default()));
        }
        published.insert(rel.clone());
    }
    published
}

/// What one path is, in one place, so the exhaustive `match` above has one thing to be exhaustive
/// over.
fn look(root: &Path, rel: &str, members: &Members, crate_dir: &str) -> Looked {
    if !is_rust(rel) {
        return Looked::Outside(Outside::NotRust);
    }
    if is_under(rel, crate_dir) {
        return Looked::Outside(Outside::GateCrate);
    }
    if members.owner(rel).is_none() {
        return Looked::Outside(Outside::NoMember);
    }
    match std::fs::read_to_string(root.join(rel)) {
        Ok(text) => Looked::Read(text),
        Err(error) => Looked::Unreachable(format!("`{rel}`: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Corpus, Members, is_rust, is_under};

    /// The `[workspace]` table this repository writes, with the comments it writes between the
    /// entries - which is the half a naive `members = [..]` split gets wrong.
    const MANIFEST: &str = concat!(
        "[package]\n",
        "name = \"root\"\n",
        "members = [\"not-the-workspace-table\"]\n",
        "\n",
        "[workspace]\n",
        "resolver = \"3\"\n",
        "members = [\n",
        "  \"crates/sutura-domain\",\n",
        "  # A comment between the entries, which this manifest writes.\n",
        "  \"crates/sutura-cli\",\n",
        "  \"xtask\",\n",
        "]\n",
        "exclude = [\"vendor/mimalloc_rust\"]\n",
        "\n",
        "[workspace.package]\n",
        "version = \"0.4.1\"\n",
    );

    fn members() -> Members {
        Members::parse(MANIFEST).expect("the fixture manifest declares members")
    }

    #[test]
    fn the_member_list_comes_from_the_workspace_table_and_nothing_else() {
        // The `[package]` table above declares its own `members` key, which is not a workspace
        // member list. Reading the first `members =` in the file would take it, and then every
        // real crate is outside the workspace and the corpus is empty at exit 0.
        assert_eq!(
            members().dirs(),
            ["crates/sutura-domain", "crates/sutura-cli", "xtask"],
            "the entries under `[workspace]`, past the comment between them"
        );
        assert_eq!(
            Members::parse("[workspace]\nmembers = [\"a\", \"b\"]\n")
                .expect("one line is a list too")
                .dirs(),
            ["a", "b"],
            "the inline spelling"
        );
    }

    #[test]
    fn a_manifest_that_declares_no_members_is_a_refusal_rather_than_an_empty_scope() {
        // FAIL CLOSED. An empty member list makes every path outside the workspace, which reads as
        // a clean tree with nothing in it - and the `barren` set cannot see it either, because a
        // list with no names is satisfied by reaching none of them.
        let said = Members::parse("[workspace]\nresolver = \"3\"\n").expect_err("no members");
        assert!(said.contains("declares no `[workspace] members`"), "{said}");
        let elsewhere = Members::parse("[package]\nmembers = [\"a\"]\n").expect_err("not the workspace table");
        assert!(elsewhere.contains("declares no `[workspace] members`"), "{elsewhere}");
    }

    #[test]
    fn a_glob_member_is_a_refusal_because_this_cannot_expand_one() {
        let said = Members::parse("[workspace]\nmembers = [\"crates/*\"]\n").expect_err("a glob");
        assert!(said.contains("cannot expand a glob"), "{said}");
    }

    #[test]
    fn a_path_under_no_member_is_outside_the_workspace() {
        let scope = members();
        assert_eq!(scope.owner("crates/sutura-cli/tests/example.rs"), Some("crates/sutura-cli"));
        assert_eq!(
            scope.owner("vendor/mimalloc_rust/src/lib.rs"),
            None,
            "`[workspace] exclude` keeps it out of the member list, so a `#[test]` in it is not a run here"
        );
        assert_eq!(
            scope.owner("crates/sutura-cliish/src/lib.rs"),
            None,
            "a whole segment, not a prefix"
        );
        assert!(is_rust("crates/x/src/A.RS"), "case-insensitive, like `docs::is_markdown`");
        assert!(!is_rust("crates/x/src/lib.rss"));
        assert!(is_under("xtask/src/examples/corpus.rs", "xtask"));
    }

    #[test]
    fn every_offered_path_gets_exactly_one_verdict_and_the_witness_states_the_whole_listing() {
        // THE ACCOUNTING, and the denominator is the WHOLE listing rather than the Rust half of
        // it: `not Rust` is what this gate's own parser rejects, so counting it is what makes the
        // denominator a different derivation from the numerator. A walk narrowed between the
        // listing and the count cannot leave this equal - which is the state `#414` measured five
        // times, here as `12 of 205 files, 69 of 1333 declarations, exit 0`.
        let dir = std::env::temp_dir().join(format!("sutura-corpus-{}", std::process::id()));
        let member = dir.join("crates/sutura-cli/src");
        std::fs::create_dir_all(&member).expect("the fixture tree is creatable");
        std::fs::write(member.join("lib.rs"), "#[test]\nfn t() {}\n").expect("the fixture file is writable");
        let offered = [
            String::from("crates/sutura-cli/src/lib.rs"),
            String::from("crates/sutura-cli/README.md"),
            String::from("vendor/mimalloc_rust/src/lib.rs"),
            String::from("xtask/src/examples.rs"),
            String::from("crates/sutura-domain/src/gone.rs"),
        ];
        let corpus = Corpus::read(&dir, &offered, &members(), "xtask").expect("every path is accounted for");
        assert_eq!(corpus.sources().len(), 1, "{:?}", corpus.sources().keys().collect::<Vec<_>>());
        assert_eq!(corpus.own(), 1, "the gate's own crate");
        assert_eq!(corpus.unreachable().len(), 1, "the member file that is not on disk");
        assert!(
            corpus
                .witness()
                .contains("every one of 5 path(s) git publishes got a verdict"),
            "{}",
            corpus.witness()
        );
        assert!(corpus.witness().contains("1 outside the workspace"), "{}", corpus.witness());
        assert!(corpus.witness().contains("1 not Rust"), "{}", corpus.witness());
        // THE SET OF NAMES, which is the half a pair of counts cannot hold: narrowing the scope
        // predicate moves a file from `read` to `outside` and leaves the accounting equal, so the
        // members the scan reached are compared against the members the MANIFEST declares - two
        // derivations, neither read off the other.
        assert_eq!(
            corpus.barren(),
            ["crates/sutura-domain"],
            "the member whose only file could not be read, and not `xtask`, which is excluded by design"
        );
        std::fs::remove_dir_all(&dir).expect("the fixture tree is removable");
    }

    #[test]
    fn the_witness_is_read_off_the_walk_rather_than_recomputed() {
        // A corpus built by the test door states its own fields and nothing else; `read` is the
        // only constructor that ships, so a production caller cannot conjure a denominator.
        let fixture = Corpus::fixture(73, Vec::new(), Vec::new());
        assert_eq!(fixture.crate_dir(), "xtask");
        assert!(fixture.witness().contains("every one of 0 path(s)"), "{}", fixture.witness());
        assert_eq!(
            Path::new("xtask").file_name().and_then(std::ffi::OsStr::to_str),
            Some("xtask")
        );
    }
}
