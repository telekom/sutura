//! Is the rustdoc run this gate is about to perform actually judging doc links?
//!
//! `[workspace.lints.rustdoc] broken_intra_doc_links = "deny"` is the mechanism for
//! `github.com/telekom/sutura#360`, and it is a LINT rather than a scan: rustdoc decides, wherever
//! cargo invokes it, and nothing here re-implements a resolver. Reading the tool is the whole
//! design - a gate that decided for itself which `` [`Foo`] `` resolves would be a second answer to
//! rustdoc's question, and the sibling gate over the published pages already learnt what that
//! costs.
//!
//! **What a lint cannot do is say whether it was armed**, and that is this module's entire job.
//! Cargo passes a `[workspace.lints]` table to a crate only where that crate's own manifest says
//! `[lints]` / `workspace = true`. A member that omits it compiles, documents, and is exempt -
//! silently, with every gate green. So a new crate is exactly one missing three-line table away
//! from being the subject `check-api-docs` reports success over without having judged.
//!
//! **The witness is a pair of numbers from two different places**, printed in the verdict: the
//! members `cargo metadata` names, and the manifests read off disk and found to inherit. A count
//! on its own says nothing - the two agreeing is what says the walk reached every member. Both
//! degenerate shapes are errors rather than a clean sweep:
//!
//!   * **the empty set** - metadata naming no member at all, which would otherwise pass having
//!     armed nothing;
//!   * **a truncated walk** - a member whose manifest could not be read, which leaves the pair
//!     unequal and cannot be reported as armed.
//!
//! **The limits, next to the claim.**
//!
//! * It reads MANIFESTS, not rustdoc's invocation. It proves the table is declared and inherited;
//!   what proves the lint then fired is the rustdoc run in the parent module exiting non-zero,
//!   which is the same run whose output the byte-compare judges.
//! * It says nothing about the other rustdoc warning classes. `private_intra_doc_links` and
//!   `redundant_explicit_links` are separate lints and separate judgements; the root manifest
//!   records the counts and why neither is denied here.
//! * SCOPE is every workspace member, which is wider than the library crates this gate documents.
//!   Deliberately: the lint's other venue is the doctest lane, which reaches all of them, and a
//!   binary-only crate that stops inheriting is the same hole one release later.

use std::path::Path;

/// The lint that decides whether an unresolvable `` [`Foo`] `` stops a rustdoc run.
const LINT: &str = "broken_intra_doc_links";

/// The workspace table that carries it, spelled as the root manifest spells it.
const WORKSPACE_TABLE: &str = "[workspace.lints.rustdoc]";

/// The table a member declares to inherit every workspace lint table.
const MEMBER_TABLE: &str = "[lints]";

/// The key a member sets inside [`MEMBER_TABLE`], with its whitespace normalised.
const MEMBER_KEY: &str = "workspace = true";

/// The levels at which the lint STOPS a rustdoc run.
///
/// An allowlist, so it fails closed: `warn` is what this tree had while 15 unresolved links sat
/// behind exit 0, and `allow` and an unparseable value have to be findings for the same reason.
const STOPPING: &[&str] = &["deny", "forbid"];

/// One workspace member, as the arming check sees it.
///
/// The manifest is an `Option` and not a skip: a member the walk could not read has to reach
/// [`arm`] and unbalance the pair there, which is the whole of the truncated-walk guard.
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
    /// Members `cargo metadata` named.
    pub(crate) members: usize,
    /// Manifests read off disk and found to inherit the workspace lint table.
    pub(crate) inheriting: usize,
    /// The level the root manifest sets the lint to.
    pub(crate) level: String,
}

/// Read the manifests and decide whether the rustdoc run about to happen is armed.
pub(crate) fn check(root: &Path, metadata: &serde_json::Value) -> Result<Arming, String> {
    let workspace = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| format!("could not read the root Cargo.toml: {error}"))?;
    arm(&workspace, &members(metadata)?)
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

/// The decision, over text rather than over the filesystem, so every shape below is a test.
fn arm(workspace_manifest: &str, members: &[Member]) -> Result<Arming, String> {
    let Some(level) = workspace_level(workspace_manifest) else {
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

    if members.is_empty() {
        return Err(String::from(
            "cargo metadata named no workspace member.\n  \
             The scan is broken, not the repo: this gate would otherwise report the lint armed \
             having checked nothing.",
        ));
    }

    let mut inheriting = 0_usize;
    let mut unarmed = Vec::new();
    for Member { name, manifest } in members {
        match manifest {
            Some(text) if inherits_workspace_lints(text) => inheriting = inheriting.saturating_add(1),
            Some(_) => unarmed.push(format!(
                "  {name}: its Cargo.toml has no `{MEMBER_TABLE}` table saying `{MEMBER_KEY}`, \
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
        members: members.len(),
        inheriting,
        level,
    })
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
fn inherits_workspace_lints(manifest: &str) -> bool {
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // `[lints.rustdoc]` is a DIFFERENT table and inherits nothing, so the match is exact.
            inside = trimmed == MEMBER_TABLE;
            continue;
        }
        if inside && trimmed.split_whitespace().collect::<Vec<_>>().join(" ") == MEMBER_KEY {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{Arming, Member, arm, inherits_workspace_lints, workspace_level};

    /// One member that inherits, named.
    fn armed(name: &str) -> Member {
        Member {
            name: String::from(name),
            manifest: Some(String::from("[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n")),
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

    /// The root manifest, reduced to the table this module reads.
    const ROOT: &str = "[workspace.lints.rust]\nunsafe_code = \"forbid\"\n\n\
                        [workspace.lints.rustdoc]\nbroken_intra_doc_links = \"deny\"\n";

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
        let arming = expect_armed(arm(ROOT, &[armed("one"), armed("two")]));
        assert_eq!(arming.level, "deny");
        assert_eq!(
            (arming.members, arming.inheriting),
            (2, 2),
            "the pair is the witness: metadata's count and the manifests' count"
        );
    }

    #[test]
    fn an_empty_member_set_is_an_error_not_an_armed_report() {
        // The vacuous pass. Point the walk at nothing and it must be red, or `ok` means only that
        // there was nothing to look at.
        let error = expect_error(arm(ROOT, &[]));
        assert!(error.contains("named no workspace member"), "{error}");
    }

    #[test]
    fn a_member_whose_manifest_cannot_be_read_leaves_the_pair_unequal() {
        // The truncated walk. A member that drops out must not be silently absent from the count.
        let error = expect_error(arm(ROOT, &[armed("one"), unreadable("two")]));
        assert!(error.contains("armed for 1 of 2"), "{error}");
        assert!(error.contains("two: its Cargo.toml could not be read"), "{error}");
    }

    #[test]
    fn a_member_that_does_not_inherit_is_named() {
        let error = expect_error(arm(ROOT, &[armed("one"), exempt("loner")]));
        assert!(error.contains("armed for 1 of 2"), "{error}");
        assert!(error.contains("loner: its Cargo.toml has no `[lints]`"), "{error}");
    }

    #[test]
    fn a_warn_level_does_not_count_as_armed() {
        // What this tree had while 15 unresolved links sat behind exit 0.
        let warned = ROOT.replace("\"deny\"", "\"warn\"");
        let error = expect_error(arm(&warned, &[armed("one")]));
        assert!(error.contains("is `warn`"), "{error}");
    }

    #[test]
    fn a_missing_table_is_a_finding_rather_than_a_default() {
        let error = expect_error(arm("[workspace.lints.rust]\nunsafe_code = \"forbid\"\n", &[armed("one")]));
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
    fn this_workspace_arms_the_lint_for_every_member() {
        // The fixtures above are the SHAPES; this is the tree, read the way `check-api-docs`
        // reads it before it documents a single crate. It is the cheap venue for the same
        // invariant: a crate that arrives without `[lints]` fails here in seconds rather than
        // in the nix check that needs a nightly rustdoc.
        let root = crate::repo::root().expect("the repo root");
        let metadata = crate::cargo_metadata(&["--no-deps"]).expect("cargo metadata should succeed in-tree");
        let arming = super::check(&root, &metadata).expect("the doc-link lint is armed in this workspace");
        assert_eq!(arming.level, "deny");
        assert_eq!(
            arming.members, arming.inheriting,
            "every workspace member must inherit the lint table"
        );
        assert!(arming.members > 1, "cargo metadata named {} member(s)", arming.members);
    }

    #[test]
    fn only_the_lints_table_inherits() {
        assert!(inherits_workspace_lints("[lints]\nworkspace = true\n"));
        assert!(inherits_workspace_lints("[lints]\n  workspace   =   true\n"));
        assert!(
            !inherits_workspace_lints("[lints.rustdoc]\nworkspace = true\n"),
            "a tool-scoped table inherits nothing"
        );
        assert!(
            !inherits_workspace_lints("[dependencies]\nworkspace = true\n"),
            "`workspace = true` means something else in every other table"
        );
        assert!(!inherits_workspace_lints("[package]\nname = \"x\"\n"));
    }
}
