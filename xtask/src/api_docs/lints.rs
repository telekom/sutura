//! Is the rustdoc run this gate is about to perform actually judging doc links?
//!
//! `[workspace.lints.rustdoc] broken_intra_doc_links = "forbid"` is the mechanism for
//! `github.com/telekom/sutura#360`, and it is a LINT rather than a scan: rustdoc decides, wherever
//! cargo invokes it, and nothing here re-implements a resolver. Reading the tool is the whole
//! design - a gate that decided for itself which `` [`Foo`] `` resolves would be a second answer
//! to rustdoc's question, and the sibling gate over the published pages already learnt what that
//! costs.
//!
//! **What a lint cannot do is say whether it was armed**, and that is this module's entire job.
//! Cargo passes a `[workspace.lints]` table to a crate only where that crate's own manifest says
//! `[lints]` / `workspace = true`. A member that omits it compiles, documents, and is exempt -
//! silently, with every gate green. So a new crate is exactly one missing three-line table away
//! from being the subject `check-api-docs` reports success over without having judged.
//!
//! # The venue is `cargo rustdoc` over LIBRARY packages, and that is the whole of it
//!
//! Measured, because the first version of this file also claimed the doctest lane and that claim
//! was false twice over:
//!
//! * `cargo test --doc` does hand rustdoc `--deny=rustdoc::broken_intra_doc_links`, but rustdoc's
//!   `--test` mode never runs the link-resolution pass. One crate, one unresolvable link, one real
//!   doctest: `cargo test --doc` is **exit 0 with no diagnostic** and `cargo doc` over the same
//!   crate is **exit 101**.
//! * It reaches no binary-only member at all - `cargo test --doc -p xtask` answers
//!   `error: no library targets found in package`, and so do `sutura-cli` and `sutura-serve`.
//!
//! **So the consequence has to be stated next to the claim: deny-level errors sit in this tree
//! with no venue judging them.** `cargo rustdoc` over the three binary-only members, on the
//! nightly pin this gate uses: `xtask` exit 101 (26 unresolved links plus one
//! `is both a function and a module`), `sutura-cli` exit 101 (1), `sutura-serve` exit 101 (2) - 30
//! in total. `cargo doc --workspace` went green-to-red on the commit that added the lint, and CI
//! stays green only because nothing runs it. Extending this gate to `cargo rustdoc` the
//! binary-only members is the fix; it needs those 30 repaired first, so it is its own change -
//! `github.com/telekom/sutura#418`.
//!
//! # The witness is a pair from two DERIVATIONS, not two counts of one walk
//!
//! The first version of this file got that wrong: both numbers were counted while iterating the
//! single vector `cargo metadata` produced, so truncating that walk to its first two entries
//! reported `2 of 2 workspace members` and stayed green. The two sources are now
//!
//! * the root manifest's own `members = [...]` list, read by [`crate::attribution::workspace_members`],
//!   which resolves each declared PATH to a package name out of that member's own manifest; and
//! * the package set `cargo metadata` resolved.
//!
//! They are compared as SETS rather than as counts, so a truncation on either side names the
//! members that went missing rather than merely disagreeing about a total. Three degenerate shapes
//! are errors rather than a clean sweep: an **empty member set** on either side, a **truncated
//! walk** on either side, and a member whose manifest the read did not reach.
//!
//! # The limits, next to the claim
//!
//! * It reads MANIFESTS, not rustdoc's invocation. It proves the table is declared and inherited;
//!   what proves the lint then fired is the rustdoc run in the parent module exiting non-zero.
//! * It says nothing about the other rustdoc warning classes. `private_intra_doc_links` and
//!   `redundant_explicit_links` are separate lints and separate judgements; the root manifest
//!   records the counts and why neither is denied here.
//! * SCOPE is every workspace member, which is wider than the library crates this gate documents.
//!   Deliberately, and now for one reason only: a binary-only crate that stops inheriting is the
//!   same hole one release later, on the day this gate learns to document it.

use std::collections::BTreeSet;
use std::path::Path;

/// The lint that decides whether an unresolvable `` [`Foo`] `` stops a rustdoc run.
const LINT: &str = "broken_intra_doc_links";

/// The workspace table that carries it, spelled as the root manifest spells it.
const WORKSPACE_TABLE: &str = "[workspace.lints.rustdoc]";

/// The table a member declares to inherit every workspace lint table.
const MEMBER_TABLE: &str = "[lints]";

/// The key a member sets inside [`MEMBER_TABLE`].
const MEMBER_KEY: &str = "workspace";

/// The levels at which the lint STOPS a rustdoc run.
///
/// An allowlist, so it fails closed: `warn` is what this tree had while 15 unresolved links sat
/// behind exit 0, and `allow` and an unparseable value have to be findings for the same reason.
///
/// **This tree sets `forbid`, and the difference from `deny` is not stylistic.** Measured on a
/// synthetic crate: a crate root carrying `#![allow(rustdoc::broken_intra_doc_links)]` beats
/// `deny` - **exit 0, no diagnostic at all** - and is refused under `forbid` with `E0453`,
/// *"overruled by previous forbid"*, exit 101. No such attribute exists in this tree, so it is a
/// hole rather than an instance, and closing a hole before it has an instance is the cheap
/// direction. `deny` stays in this list because it does stop a run; what it does not do is stop a
/// crate from lowering itself.
const STOPPING: &[&str] = &["deny", "forbid"];

/// One workspace member, as the arming check sees it.
///
/// The manifest is an `Option` and not a skip: a member the walk could not read has to reach
/// [`arm`] and be named there, which is half of the truncated-walk guard.
#[derive(Debug)]
struct Member {
    /// The name `cargo metadata` gave it.
    name: String,
    /// Its manifest text, where that could be read.
    manifest: Option<String>,
}

/// What the arming check saw, for the verdict to print.
#[derive(Debug)]
pub(crate) struct Arming {
    /// Members the ROOT MANIFEST's own `members = [...]` list declares.
    pub(crate) declared: usize,
    /// Members CARGO METADATA resolved. A different derivation of the same set, on purpose.
    pub(crate) resolved: usize,
    /// Manifests read off disk and found to inherit the workspace lint table.
    pub(crate) inheriting: usize,
    /// The level the root manifest sets the lint to.
    pub(crate) level: String,
}

/// Read the manifests and decide whether the rustdoc run about to happen is armed.
pub(crate) fn check(root: &Path, metadata: &serde_json::Value) -> Result<Arming, String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| format!("could not read the root Cargo.toml: {error}"))?;
    arm(root, &manifest, &members(metadata)?)
}

/// Every workspace member `cargo metadata` names, with its manifest text where that could be read.
fn members(metadata: &serde_json::Value) -> Result<Vec<Member>, String> {
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;

    let mut found = Vec::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| String::from("<unnamed>"), String::from);
        let manifest = package
            .get("manifest_path")
            .and_then(serde_json::Value::as_str)
            .and_then(|path| std::fs::read_to_string(path).ok());
        found.push(Member { name, manifest });
    }
    Ok(found)
}

/// The decision. `root` is read only for the SECOND derivation of the member set.
fn arm(root: &Path, root_manifest: &str, members: &[Member]) -> Result<Arming, String> {
    let Some(level) = workspace_level(root_manifest) else {
        return Err(format!(
            "the root Cargo.toml declares no `{LINT}` under `{WORKSPACE_TABLE}`.\n  \
             Without it rustdoc only WARNS about a link it cannot resolve, and this gate would \
             regenerate every page from a run that judged none of them."
        ));
    };
    if !STOPPING.contains(&level.as_str()) {
        return Err(format!(
            "`{LINT}` is `{level}` in the root Cargo.toml, which does not stop a rustdoc run.\n  \
             `deny` or `forbid` is what turns an unresolvable link into an error where the pages \
             are produced."
        ));
    }

    // THE SECOND DERIVATION. Not a second count of the vector above - that is what `.take(2)`
    // defeated - but the root manifest's own member list, resolved to package names off disk.
    let declared = crate::attribution::workspace_members(root, root_manifest).ok_or_else(|| {
        String::from(
            "the root Cargo.toml's `members = [...]` list resolved to no package name.\n  \
             The scan is broken, not the repo: without the declared set there is nothing for \
             cargo metadata's set to be compared against, and this gate would report the lint \
             armed for whatever it happened to walk.",
        )
    })?;

    let resolved: BTreeSet<String> = members.iter().map(|member| member.name.clone()).collect();
    if resolved.is_empty() {
        return Err(String::from(
            "cargo metadata named no workspace member.\n  \
             The scan is broken, not the repo: this gate would otherwise report the lint armed \
             having checked nothing.",
        ));
    }
    disagreement(&declared, &resolved)?;

    let mut inheriting = 0_usize;
    let mut unarmed = Vec::new();
    for Member { name, manifest } in members {
        match manifest {
            Some(text) if inherits_workspace_lints(text) => inheriting = inheriting.saturating_add(1),
            Some(_) => unarmed.push(format!(
                "  {name}: its Cargo.toml has no `{MEMBER_TABLE}` table saying `{MEMBER_KEY} = true`, \
                 so no workspace lint reaches it"
            )),
            None => unarmed.push(format!(
                "  {name}: its Cargo.toml could not be read, so the walk did not reach it"
            )),
        }
    }
    if inheriting != members.len() {
        return Err(format!(
            "the doc-link lint is armed for {inheriting} of {} workspace members:\n{}",
            members.len(),
            unarmed.join("\n")
        ));
    }

    Ok(Arming {
        declared: declared.len(),
        resolved: resolved.len(),
        inheriting,
        level,
    })
}

/// Where the two derivations of the member set disagree, named in both directions.
///
/// A set difference and not a count comparison: two sets of equal size can still be two different
/// sets, and the member that went missing is what a reader needs.
fn disagreement(declared: &BTreeSet<String>, resolved: &BTreeSet<String>) -> Result<(), String> {
    let missing: Vec<&String> = declared.difference(resolved).collect();
    let extra: Vec<&String> = resolved.difference(declared).collect();
    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }
    let mut lines: Vec<String> = missing
        .into_iter()
        .map(|name| format!("  declared in Cargo.toml, not resolved by cargo metadata: {name}"))
        .collect();
    lines.extend(
        extra
            .into_iter()
            .map(|name| format!("  resolved by cargo metadata, not declared in Cargo.toml: {name}")),
    );
    Err(format!(
        "the root Cargo.toml declares {} workspace member(s) and cargo metadata resolved {}, \
         and the two sets differ.\n  \
         One of the two walks is truncated, and a gate that armed only what it happened to walk \
         is the failure this comparison exists to catch.\n{}",
        declared.len(),
        resolved.len(),
        lines.join("\n")
    ))
}

/// The level [`LINT`] is set to under [`WORKSPACE_TABLE`], if it is set there at all.
///
/// Table-scoped rather than a substring search, so the same key under `[workspace.lints.rust]` -
/// or in a comment anywhere - cannot be read as this one. A hand-rolled read of one flat table
/// beats adding a TOML parser to a crate that has no other use for one; the shapes it accepts are
/// the tests below.
fn workspace_level(manifest: &str) -> Option<String> {
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == WORKSPACE_TABLE;
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim().trim_matches('"') == LINT {
            return Some(level_of(value));
        }
    }
    None
}

/// A lint's level, from either spelling cargo accepts.
///
/// The bare string is what this tree writes; the `{ level = "deny", priority = -1 }` form is what
/// the clippy table beside it writes, and a value this cannot read comes back empty rather than
/// as a guess - [`STOPPING`] is an allowlist, so empty is a finding.
fn level_of(value: &str) -> String {
    let value = value.trim();
    value.strip_prefix('{').and_then(|rest| rest.strip_suffix('}')).map_or_else(
        || String::from(value.trim_matches('"')),
        |table| {
            table
                .split(',')
                .filter_map(|entry| entry.split_once('='))
                .find(|(key, _)| key.trim() == "level")
                .map_or_else(String::new, |(_, level)| String::from(level.trim().trim_matches('"')))
        },
    )
}

/// Does this member's manifest inherit the workspace lint tables?
///
/// The key and the value are trimmed either side of the `=` rather than the line being matched
/// against a spelling: cargo honours `workspace=true`, and a gate that called that member exempt
/// would be a false positive - which is what gets a gate switched off.
fn inherits_workspace_lints(manifest: &str) -> bool {
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // `[lints.rustdoc]` is a DIFFERENT table and inherits nothing, so the match is exact.
            inside = trimmed == MEMBER_TABLE;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == MEMBER_KEY
            && value.trim() == "true"
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Arming, Member, arm, inherits_workspace_lints, workspace_level};

    /// A member manifest that inherits, for the fixture workspaces below.
    const INHERITING: &str = "[package]\nname = \"NAME\"\n\n[lints]\nworkspace = true\n";

    /// The root manifest, reduced to the two tables this module reads.
    ///
    /// `members` is a real list, because `arm` resolves it off disk now - so the fixtures write a
    /// throwaway workspace rather than a string.
    fn root_manifest(members: &[&str], level: &str) -> String {
        let quoted: Vec<String> = members.iter().map(|m| format!("  \"{m}\",")).collect();
        let list = quoted.join("\n");
        format!(
            "[workspace]\nmembers = [\n{list}\n]\n\n\
             [workspace.lints.rust]\nunsafe_code = \"forbid\"\n\n\
             [workspace.lints.rustdoc]\nbroken_intra_doc_links = \"{level}\"\n"
        )
    }

    /// A throwaway workspace on disk: one directory per member, each with a manifest.
    ///
    /// On disk because the declared half is READ off disk - that is the whole point of it being a
    /// second derivation, and a fixture that handed `arm` a ready-made set would test neither.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(label: &str, members: &[&str], level: &str) -> Self {
            let root = std::env::temp_dir().join(format!("sutura-lints-{}-{}", std::process::id(), label));
            drop(std::fs::remove_dir_all(&root));
            std::fs::create_dir_all(&root).expect("fixture root");
            std::fs::write(root.join("Cargo.toml"), root_manifest(members, level)).expect("root manifest");
            for member in members {
                let dir = root.join(member);
                std::fs::create_dir_all(&dir).expect("member dir");
                std::fs::write(dir.join("Cargo.toml"), INHERITING.replace("NAME", member)).expect("member manifest");
            }
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }

        fn manifest(&self) -> String {
            std::fs::read_to_string(self.root.join("Cargo.toml")).expect("root manifest")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.root));
        }
    }

    /// What cargo metadata would report for a fixture member that inherits.
    fn armed(name: &str) -> Member {
        Member {
            name: String::from(name),
            manifest: Some(String::from(INHERITING)),
        }
    }

    /// One member the walk could not read.
    fn unreadable(name: &str) -> Member {
        Member {
            name: String::from(name),
            manifest: None,
        }
    }

    /// One member whose manifest inherits nothing.
    fn exempt(name: &str) -> Member {
        Member {
            name: String::from(name),
            manifest: Some(String::from("[package]\nname = \"loner\"\n")),
        }
    }

    fn expect_armed(result: Result<Arming, String>) -> Arming {
        match result {
            Ok(arming) => arming,
            Err(error) => panic!("expected the lint to be armed: {error}"),
        }
    }

    fn expect_error(result: Result<Arming, String>) -> String {
        match result {
            Ok(arming) => panic!("expected a finding, got {arming:?}"),
            Err(error) => error,
        }
    }

    #[test]
    fn a_denied_lint_inherited_by_every_member_is_armed() {
        let fixture = Fixture::new("armed", &["one", "two"], "deny");
        let arming = expect_armed(arm(fixture.path(), &fixture.manifest(), &[armed("one"), armed("two")]));
        assert_eq!(arming.level, "deny");
        assert_eq!(
            (arming.declared, arming.resolved, arming.inheriting),
            (2, 2, 2),
            "three numbers, and the first two come from two different derivations of the member set"
        );
    }

    #[test]
    fn a_forbidden_lint_is_armed_too() {
        let fixture = Fixture::new("forbidden", &["one"], "forbid");
        let arming = expect_armed(arm(fixture.path(), &fixture.manifest(), &[armed("one")]));
        assert_eq!(arming.level, "forbid");
    }

    #[test]
    fn an_empty_member_set_is_an_error_not_an_armed_report() {
        // The vacuous pass. Point the walk at nothing and it must be red, or `ok` means only that
        // there was nothing to look at.
        let fixture = Fixture::new("empty", &["one"], "deny");
        let error = expect_error(arm(fixture.path(), &fixture.manifest(), &[]));
        assert!(error.contains("named no workspace member"), "{error}");
    }

    #[test]
    fn a_metadata_walk_that_stops_early_disagrees_with_the_root_manifest() {
        // THE MUTATION THAT USED TO PASS. Both numbers used to be counted off this one vector, so
        // truncating it reported `2 of 2` and stayed green. The declared set is the other side.
        let fixture = Fixture::new("truncated", &["one", "two", "three"], "deny");
        let error = expect_error(arm(fixture.path(), &fixture.manifest(), &[armed("one"), armed("two")]));
        assert!(
            error.contains("declares 3 workspace member(s) and cargo metadata resolved 2"),
            "{error}"
        );
        assert!(
            error.contains("declared in Cargo.toml, not resolved by cargo metadata: three"),
            "the member that went missing has to be named: {error}"
        );
    }

    #[test]
    fn a_root_manifest_that_stops_early_disagrees_with_cargo_metadata() {
        // The same guard from the other side, because a truncation can be in either walk.
        let fixture = Fixture::new("short-root", &["one"], "deny");
        let error = expect_error(arm(fixture.path(), &fixture.manifest(), &[armed("one"), armed("two")]));
        assert!(
            error.contains("resolved by cargo metadata, not declared in Cargo.toml: two"),
            "{error}"
        );
    }

    #[test]
    fn two_sets_of_equal_size_that_are_not_the_same_set_are_a_finding() {
        // Why the comparison is a set difference and not `declared == resolved` as counts.
        let fixture = Fixture::new("swapped", &["one", "two"], "deny");
        let error = expect_error(arm(fixture.path(), &fixture.manifest(), &[armed("one"), armed("three")]));
        assert!(error.contains("not resolved by cargo metadata: two"), "{error}");
        assert!(error.contains("not declared in Cargo.toml: three"), "{error}");
    }

    #[test]
    fn a_member_whose_manifest_cannot_be_read_leaves_the_pair_unequal() {
        let fixture = Fixture::new("unreadable", &["one", "two"], "deny");
        let error = expect_error(arm(fixture.path(), &fixture.manifest(), &[armed("one"), unreadable("two")]));
        assert!(error.contains("armed for 1 of 2"), "{error}");
        assert!(error.contains("two: its Cargo.toml could not be read"), "{error}");
    }

    #[test]
    fn a_member_that_does_not_inherit_is_named() {
        let fixture = Fixture::new("exempt", &["one", "loner"], "deny");
        let error = expect_error(arm(fixture.path(), &fixture.manifest(), &[armed("one"), exempt("loner")]));
        assert!(error.contains("armed for 1 of 2"), "{error}");
        assert!(error.contains("loner: its Cargo.toml has no `[lints]`"), "{error}");
    }

    #[test]
    fn a_warn_level_does_not_count_as_armed() {
        // What this tree had while 15 unresolved links sat behind exit 0.
        let fixture = Fixture::new("warned", &["one"], "warn");
        let error = expect_error(arm(fixture.path(), &fixture.manifest(), &[armed("one")]));
        assert!(error.contains("is `warn`"), "{error}");
    }

    #[test]
    fn a_missing_table_is_a_finding_rather_than_a_default() {
        let fixture = Fixture::new("no-table", &["one"], "deny");
        let stripped = fixture
            .manifest()
            .replace("[workspace.lints.rustdoc]", "[workspace.lints.other]");
        let error = expect_error(arm(fixture.path(), &stripped, &[armed("one")]));
        assert!(error.contains("declares no `broken_intra_doc_links`"), "{error}");
    }

    #[test]
    fn the_lint_is_read_only_out_of_its_own_table() {
        // A key of the same name under a neighbouring table is not this lint. Without the table
        // scope the root manifest's own prose would be enough to satisfy the gate.
        assert!(workspace_level("[workspace.lints.rust]\nbroken_intra_doc_links = \"deny\"\n").is_none());
        assert!(workspace_level("# broken_intra_doc_links = \"deny\" in a comment\n").is_none());
    }

    #[test]
    fn both_spellings_of_a_level_are_read_and_an_unreadable_one_is_empty() {
        let table = "[workspace.lints.rustdoc]\nbroken_intra_doc_links = { level = \"deny\", priority = -1 }\n";
        assert_eq!(workspace_level(table).as_deref(), Some("deny"));
        let opaque = "[workspace.lints.rustdoc]\nbroken_intra_doc_links = { priority = -1 }\n";
        assert_eq!(
            workspace_level(opaque).as_deref(),
            Some(""),
            "an unreadable level must fall outside the allowlist rather than be guessed"
        );
    }

    #[test]
    fn only_the_lints_table_inherits_and_the_spacing_is_not_the_rule() {
        assert!(inherits_workspace_lints("[lints]\nworkspace = true\n"));
        assert!(inherits_workspace_lints("[lints]\n  workspace   =   true\n"));
        assert!(
            inherits_workspace_lints("[lints]\nworkspace=true\n"),
            "cargo honours it, so calling that member exempt would be a false positive"
        );
        assert!(
            !inherits_workspace_lints("[lints.rustdoc]\nworkspace = true\n"),
            "a tool-scoped table inherits nothing"
        );
        assert!(
            !inherits_workspace_lints("[dependencies]\nworkspace = true\n"),
            "`workspace = true` means something else in every other table"
        );
        assert!(!inherits_workspace_lints("[lints]\nworkspace = false\n"));
        assert!(!inherits_workspace_lints("[package]\nname = \"x\"\n"));
    }

    #[test]
    fn this_workspace_arms_the_lint_for_every_member() {
        // The fixtures above are the SHAPES; this is the tree, read the way `check-api-docs`
        // reads it before it documents a single crate. It is the cheap venue for the same
        // invariant: a crate that arrives without `[lints]` fails here in seconds rather than in
        // the nix check that needs a nightly rustdoc.
        let root = crate::repo::root().expect("the repo root");
        let metadata = crate::cargo_metadata(&["--no-deps"]).expect("cargo metadata should succeed in-tree");
        let arming = super::check(&root, &metadata).expect("the doc-link lint is armed in this workspace");
        assert!(super::STOPPING.contains(&arming.level.as_str()), "level was {}", arming.level);
        // EQUALITY of the three, not a floor. `members > 1` was the old assertion and `.take(2)`
        // walked straight through it.
        assert_eq!(
            (arming.declared, arming.resolved),
            (arming.inheriting, arming.inheriting),
            "the root manifest's member list, cargo metadata's package set and the manifests that \
             inherit must be one set counted three ways"
        );
    }
}
