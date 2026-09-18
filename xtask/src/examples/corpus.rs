//! Which files are the corpus a reach may come from, and the witness that every path the listing
//! offered got a verdict.
//!
//! # Two questions
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
//! * **Did the scan read what the listing offered?** [`Census::inspect`](crate::repo::Census)
//!   holds this now: `judged + out_of_scope + absent == discovered` by construction, so the walk
//!   half of `github.com/telekom/sutura#414` cannot recur here the way it did before this file's
//!   own walk was moved onto it - **12 of 205 files, 69 of 1333 declarations, exit 0**, measured
//!   against this gate's own predecessor.
//!
//! # Migrated onto [`repo::Census::inspect`], and what changed
//!
//! **`Scope` is a bare `fn` pointer** ([`repo::Scope`]), so it cannot hold `Members` - parsed from
//! the root manifest at RUNTIME - or `crate_dir` as a decision made before the read. So [`in_scope`]
//! only answers "is this Rust, or under `examples/`", and the own-crate/workspace-member exclusion
//! that used to run BEFORE `fs::read_to_string` now runs AFTER `Census::inspect`'s own read, inside
//! the closure - see [`Owned`]. **The cost is stated rather than hidden**: a vendored or
//! this-gate's-own Rust file gets read and its bytes discarded, where the old `look()` never opened
//! it.
//!
//! **Three walks become one.** `variants` and `publishes` used to read the WHOLE git listing
//! directly; now they read the paths [`repo::Census::inspect`] actually visited and handed to the
//! closure, accumulated into one `Vec` as it runs - so a `Scope` that stops matching narrows what
//! they see exactly as it narrows the corpus, rather than the two disagreeing the way a caller
//! holding two different `Vec`s could.
//!
//! **The `must_judge` anchor is [`ANCHOR`]**, a Rust test file rather than an `examples/` file:
//! `in_scope` never reads inside a variant directory for its own sake, only for `variants` and
//! `publishes`, so an anchor under `examples/` would prove nothing about whether the CORPUS half of
//! this walk ran. `judged == 0` cannot see a `Scope` that quietly stopped covering test code while
//! still reading `examples/`'s own files, so the anchor is the one test file this repository's own
//! module doc already names as the SOLE reach for `examples/multi-player` - see [`ANCHOR`].
//!
//! # What the witness is, and what it is not
//!
//! The `must_judge` anchor and the census's own `judged + out_of_scope + absent == discovered`
//! equality hold the walk half of `#414`. What they do not hold is [`Corpus::barren`]'s own
//! question - a SET OF NAMES, the workspace's own member list against the members the scan
//! actually reached - because narrowing [`in_scope`] to something still Rust-shaped moves files
//! from read to `outside`(`NoMember`) and the census's own accounting stays balanced either way.
//! It is derived from the root manifest rather than from the listing, so a narrowing of either does
//! not move both.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::repo;

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

/// Is this path Rust, or does it sit under `examples/`?
///
/// The `Scope` [`Corpus::read`] hands to [`repo::Census::inspect`]. A bare `fn` pointer cannot
/// hold `Members` - parsed at runtime, from the root manifest - so it cannot decide the
/// finer-grained "whose code is this" question `Owned` answers; it can only decide whether the
/// census needs the BYTES at all. The union with `examples/` is what lets [`variants`] and
/// [`publishes`] see every kind of file a deployment example holds - a `README.md`, a fixture
/// `.json` - rather than only the Rust half.
fn in_scope(rel: &str) -> bool {
    is_rust(rel) || rel.starts_with(EXAMPLES)
}

/// This repository's own module doc names the ONE line that reaches `examples/multi-player`: a
/// helper in this test file, not a test body, which is why `#400`'s per-line rule alone could not
/// see it. A `Scope` that stops covering test code while still reading `examples/` itself would
/// leave `judged` non-zero and every OTHER variant satisfied - `judged == 0` cannot see that one
/// readable file is not the same as reading the tree this gate is about - so this is the anchor
/// [`repo::Census::inspect`] cannot discharge without opening it.
const ANCHOR: &str = "crates/sutura-catalog-datahub/tests/multi_player.rs";

/// Whose code a Rust file the census read turns out to be, decided AFTER the read rather than
/// before it - see [`in_scope`] for why. THREE ANSWERS AND NO FOURTH: [`Corpus::read`]'s closure
/// matches this exhaustively, so a new kind of answer is `error[E0004]` rather than a path that
/// quietly stops being counted, the discipline `github.com/telekom/sutura#414` asks for.
enum Owned<'a> {
    /// Rust in this gate's own crate. See [`gate_crate`].
    GateCrate,
    /// Rust under no workspace member - vendored source, or a crate the manifest does not build.
    /// Not a run in this venue.
    NoMember,
    /// Owned by this workspace member.
    Member(&'a str),
}

/// [`Owned`], for one Rust path.
fn owned<'a>(rel: &str, crate_dir: &str, members: &'a Members) -> Owned<'a> {
    if is_under(rel, crate_dir) {
        return Owned::GateCrate;
    }
    members.owner(rel).map_or(Owned::NoMember, Owned::Member)
}

/// The corpus, and the accounting that says the scan reached all of it.
///
/// Every field is private and [`Self::read`] is the only constructor that ships, so no caller can
/// state a number the walk did not reach - the shape `check-warm-start`'s `Swept` already uses,
/// generalised no further than this gate needs. `#[cfg(test)]` adds `Self::fixture` so the pure
/// verdict arms stay testable without a checkout; that is a test-only door, and the property this
/// type exists for is a property of every production caller.
#[derive(Debug)]
pub(crate) struct Corpus {
    /// The in-scope files that were read: repo-relative path to raw text, member-owned Rust only.
    sources: BTreeMap<String, String>,
    /// Deployment variant names, read off every path [`in_scope`] admitted.
    variants: BTreeSet<String>,
    /// Every path under `examples/` git publishes, read off the same admitted paths.
    published: BTreeSet<String>,
    /// Rust files under no workspace member.
    outside: usize,
    /// Rust files in this gate's own crate. Zero is a broken exclusion, not a clean tree.
    own: usize,
    /// Member-owned Rust files the census read whose bytes were not valid UTF-8, each with the
    /// error. A problem stated for the caller, never a silent drop - `fs::read` does not fail on
    /// this the way `fs::read_to_string` used to, so the check moves here instead of vanishing.
    unreadable: Vec<String>,
    /// Declared workspace members the scan reached no file of, this gate's own crate aside.
    barren: Vec<String>,
    /// This gate's own crate directory, for the messages that name it.
    crate_dir: &'static str,
    /// `Inspected::verdict`'s own sentence, so the pass line cannot state a total the census did
    /// not reach.
    witness: String,
}

impl Corpus {
    /// Read the corpus out of `census`, the discovery [`repo::all_files`] returned.
    ///
    /// The loop is [`repo::Census::inspect`]'s now, not this function's: a caller cannot narrow it
    /// with `.take(n)` because there is no `Vec` here to narrow, which is the same guarantee
    /// `repo::census` gives every gate that migrates onto it. What THIS closure adds is the
    /// classification `Scope` cannot make - see [`in_scope`] and [`Owned`] - at the cost this
    /// module's doc states: a vendored or this-gate's-own Rust file is read and its bytes
    /// discarded, where the walk this replaces never opened it.
    pub(crate) fn read(census: repo::Census, members: &Members, crate_dir: &'static str) -> Result<Self, repo::Refusal> {
        let mut sources = BTreeMap::new();
        let mut candidates: Vec<String> = Vec::new();
        let mut own = 0_usize;
        let mut outside = 0_usize;
        let mut unreadable: Vec<String> = Vec::new();
        let mut reached: BTreeSet<&str> = BTreeSet::new();

        let inspected = census.inspect(&[ANCHOR], in_scope, |rel, bytes| {
            candidates.push(String::from(rel));
            if !is_rust(rel) {
                return;
            }
            match owned(rel, crate_dir, members) {
                Owned::GateCrate => own = own.saturating_add(1),
                Owned::NoMember => outside = outside.saturating_add(1),
                Owned::Member(owner) => {
                    reached.insert(owner);
                    match std::str::from_utf8(bytes) {
                        Ok(text) => {
                            sources.insert(String::from(rel), String::from(text));
                        }
                        Err(why) => unreadable.push(format!("`{rel}`: {why}")),
                    }
                }
            }
        })?;

        let barren = members
            .dirs()
            .iter()
            .filter(|dir| dir.as_str() != crate_dir && !reached.contains(dir.as_str()))
            .cloned()
            .collect();

        Ok(Self {
            sources,
            variants: variants(&candidates),
            published: publishes(&candidates),
            outside,
            own,
            unreadable,
            barren,
            crate_dir,
            witness: inspected.verdict(),
        })
    }

    /// The files a reach may come from: repo-relative path to raw text.
    pub(crate) const fn sources(&self) -> &BTreeMap<String, String> {
        &self.sources
    }

    /// The deployment variants this scan found.
    pub(crate) const fn variants(&self) -> &BTreeSet<String> {
        &self.variants
    }

    /// Every path under `examples/` this scan resolves a reach against.
    pub(crate) const fn published(&self) -> &BTreeSet<String> {
        &self.published
    }

    /// This gate's own crate directory.
    pub(crate) const fn crate_dir(&self) -> &'static str {
        self.crate_dir
    }

    /// How many files the self-exclusion removed.
    pub(crate) const fn own(&self) -> usize {
        self.own
    }

    /// Member-owned Rust files the census read that were not valid UTF-8.
    pub(crate) fn unreachable(&self) -> &[String] {
        &self.unreadable
    }

    /// Declared members the scan reached no file of.
    pub(crate) fn barren(&self) -> &[String] {
        &self.barren
    }

    /// The sentence the verdict prints: the census's own witness, plus this gate's accounting.
    ///
    /// Every number here is read off a private field of a value only [`Self::read`] builds, so the
    /// line cannot state a total the walk did not reach.
    pub(crate) fn witness(&self) -> String {
        format!(
            "{} ({} Rust file(s) kept, {} outside the workspace, {} under `{}/`)",
            self.witness,
            self.sources.len(),
            self.outside,
            self.own,
            self.crate_dir
        )
    }

    /// A corpus with the fields the pure verdict arms vary, and a healthy remainder.
    #[cfg(test)]
    pub(crate) fn fixture(own: usize, unreadable: Vec<String>, barren: Vec<String>) -> Self {
        Self {
            sources: BTreeMap::new(),
            variants: BTreeSet::new(),
            published: BTreeSet::new(),
            outside: 0,
            own,
            unreadable,
            barren,
            crate_dir: "xtask",
            witness: String::from("0 of 0 subject(s) judged, 0 out of scope, 0 absent, 0 byte(s) read"),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ANCHOR, Corpus, Members, is_rust, is_under};
    use crate::repo::{self, Refusal};
    use crate::scratch_tree::Tree;

    /// The `[workspace]` table this repository writes, with the comments it writes between the
    /// entries - which is the half a naive `members = [..]` split gets wrong. Carries the
    /// `must_judge` anchor's own crate too, so a fixture tree that includes [`ANCHOR`] resolves it
    /// to a real member rather than to `NoMember`.
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
        "  \"crates/sutura-catalog-datahub\",\n",
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

    /// A census over `tree`, through the same production door [`repo::all_files`] uses -
    /// [`Corpus::read`] performs its own read now, so a listing over a root with nothing on disk
    /// no longer proves anything about the corpus.
    fn census_over(tree: &Tree, extensions: &[&str]) -> repo::Census {
        repo::collect_files(tree.root(), tree.root(), extensions)
    }

    #[test]
    fn the_member_list_comes_from_the_workspace_table_and_nothing_else() {
        // The `[package]` table above declares its own `members` key, which is not a workspace
        // member list. Reading the first `members =` in the file would take it, and then every
        // real crate is outside the workspace and the corpus is empty at exit 0.
        assert_eq!(
            members().dirs(),
            [
                "crates/sutura-domain",
                "crates/sutura-cli",
                "crates/sutura-catalog-datahub",
                "xtask"
            ],
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
    fn a_member_owned_rust_file_is_kept_and_everything_else_is_counted_and_discarded() {
        // THE MIGRATION'S OWN COST, stated: `in_scope` cannot ask `Members` before the read, so a
        // vendored file and this gate's own crate get read and their bytes discarded - `outside`
        // and `own` move, `sources` does not.
        let tree = Tree::of(
            "corpus-accounting",
            &[
                (ANCHOR, b"#[test]\nfn t() {}\n" as &[u8]),
                ("crates/sutura-cli/tests/example.rs", b"#[test]\nfn u() {}\n"),
                ("crates/sutura-cli/README.md", b"not rust\n"),
                ("vendor/mimalloc_rust/src/lib.rs", b"#[test]\nfn v() {}\n"),
                ("xtask/src/examples.rs", b"// this gate's own crate\n"),
            ],
        );
        let corpus = Corpus::read(census_over(&tree, &["rs", "md"]), &members(), "xtask").expect("every path is accounted for");
        assert_eq!(
            corpus.sources().keys().cloned().collect::<Vec<_>>(),
            vec![String::from(ANCHOR), String::from("crates/sutura-cli/tests/example.rs")],
            "{:?}",
            corpus.sources().keys().collect::<Vec<_>>()
        );
        assert_eq!(corpus.own(), 1, "this gate's own crate, read and discarded");
        assert!(corpus.witness().contains("1 outside the workspace"), "{}", corpus.witness());
        assert!(corpus.witness().contains("1 under `xtask/`"), "{}", corpus.witness());
        // THE SET OF NAMES, which is the half a pair of counts cannot hold: narrowing the scope
        // predicate moves a file from `read` to `outside` and leaves the census's own accounting
        // balanced either way, so the members the scan reached are compared against the members
        // the MANIFEST declares - two derivations, neither read off the other.
        assert_eq!(
            corpus.barren(),
            ["crates/sutura-domain"],
            "the member no fixture file belongs to, and not `xtask`, which is excluded by design"
        );
    }

    #[test]
    fn any_extension_under_examples_is_captured_for_variants_and_publishing() {
        // THE OTHER HALF OF THE WIDENED SCOPE: `variants`/`publishes` need every kind of file a
        // deployment example holds, not only Rust, so `in_scope` admits the whole `examples/`
        // subtree regardless of extension.
        let tree = Tree::of(
            "corpus-examples-any-extension",
            &[
                (ANCHOR, b"#[test]\nfn t() {}\n" as &[u8]),
                ("examples/x/README.md", b"prose\n"),
                ("examples/x/question.json", b"{}\n"),
            ],
        );
        let corpus =
            Corpus::read(census_over(&tree, &["rs", "md", "json"]), &members(), "xtask").expect("every path is accounted for");
        assert_eq!(corpus.variants(), &BTreeSet::from([String::from("x")]));
        assert!(
            corpus.published().contains("examples/x/question.json"),
            "{:?}",
            corpus.published()
        );
    }

    #[test]
    fn an_anchor_this_repository_names_as_the_sole_reach_cannot_be_discharged_by_a_narrower_scope() {
        // The shape a rename or a narrowed `in_scope` leaves behind: no file at `ANCHOR` at all,
        // with an otherwise healthy tree. `judged == 0` cannot see this - the fixture's other file
        // is read and kept - so this is exactly what `must_judge` is for.
        let tree = Tree::of(
            "corpus-anchor-missing",
            &[("crates/sutura-cli/tests/example.rs", b"#[test]\nfn u() {}\n" as &[u8])],
        );
        let refused = Corpus::read(census_over(&tree, &["rs"]), &members(), "xtask");
        let Err(Refusal::NotJudged { path, .. }) = refused else {
            panic!("a tree missing its own anchor produced a verdict");
        };
        assert_eq!(path, ANCHOR);
    }

    /// A sealed in-scope Rust file is a refusal the CENSUS itself reports, independently of
    /// [`ANCHOR`]: [`Refusal::Unreachable`] is returned before [`Corpus::read`]'s own
    /// classification ever runs, so a mutation that swallows `inspect`'s `Result` is caught here
    /// even when the anchor is present and satisfied.
    #[cfg(unix)]
    #[test]
    fn a_sealed_member_owned_file_is_a_refusal_the_census_itself_reports() {
        let mut tree = Tree::of(
            "corpus-sealed",
            &[
                (ANCHOR, b"#[test]\nfn t() {}\n" as &[u8]),
                ("crates/sutura-cli/tests/sealed.rs", b"#[test]\nfn u() {}\n"),
            ],
        );
        if !tree.seal("crates/sutura-cli/tests/sealed.rs") {
            // Mode bits ignored for this uid - asserting a refusal here would assert nothing.
            return;
        }
        let refused = Corpus::read(census_over(&tree, &["rs"]), &members(), "xtask");
        let Err(Refusal::Unreachable(subjects)) = refused else {
            panic!("a sealed member-owned file produced a verdict");
        };
        assert!(
            subjects
                .iter()
                .any(|entry| entry.starts_with("crates/sutura-cli/tests/sealed.rs")),
            "{subjects:?}"
        );
    }

    #[test]
    fn a_member_owned_file_that_is_not_valid_utf8_is_a_problem_rather_than_a_silent_drop() {
        // `fs::read` never fails on this the way `fs::read_to_string` used to - the census reads
        // the bytes successfully, so the UTF-8 check has to live in the closure now, and a mutation
        // that drops it silently would keep every other number healthy.
        let tree = Tree::of(
            "corpus-non-utf8",
            &[
                (ANCHOR, b"#[test]\nfn t() {}\n" as &[u8]),
                ("crates/sutura-cli/tests/broken.rs", &[0xff_u8, 0xfe, 0x00]),
            ],
        );
        let corpus = Corpus::read(census_over(&tree, &["rs"]), &members(), "xtask").expect("not a census-level refusal");
        assert!(
            corpus
                .unreachable()
                .iter()
                .any(|entry| entry.starts_with("`crates/sutura-cli/tests/broken.rs`")),
            "{:?}",
            corpus.unreachable()
        );
        assert!(
            !corpus.sources().contains_key("crates/sutura-cli/tests/broken.rs"),
            "a file that failed to decode is not a source"
        );
    }

    #[test]
    fn the_witness_is_read_off_the_walk_rather_than_recomputed() {
        // A corpus built by the test door states its own fields and nothing else; `read` is the
        // only constructor that ships, so a production caller cannot conjure a denominator.
        let fixture = Corpus::fixture(73, Vec::new(), Vec::new());
        assert_eq!(fixture.crate_dir(), "xtask");
        assert!(
            fixture.witness().contains("0 of 0 subject(s) judged"),
            "{}",
            fixture.witness()
        );
        assert!(fixture.witness().contains("73 under `xtask/`"), "{}", fixture.witness());
    }
}
