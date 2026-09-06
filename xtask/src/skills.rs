//! The skills gate: keep `skill-router.json` and the skill tree in agreement.
//!
//! Tree-shaped discovery only saves tokens if the router is trustworthy. Two failure modes,
//! both silent without a check:
//!
//! * a router entry naming a skill that was renamed or deleted - an agent follows the route
//!   and finds nothing;
//! * a `SKILL.md` no route reaches - dead weight that still costs review, and a rule nobody
//!   will read while believing it is in force.
//!
//! So this checks BOTH directions, plus that each skill's frontmatter `name` matches its
//! directory, since the name is what a router entry and an agent refer to it by.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::Verdict;
use crate::repo;

const SKILLS_DIR: &str = ".agents/skills";
const ROUTER: &str = ".agents/skills/skill-router.json";
/// The non-discoverable tier. Not routed by design, so `check-skills` cannot demand a route -
/// but it can demand the one thing that makes an import maintainable.
const LIBRARY_DIR: &str = ".agents/skill-library";
/// Written by `pixi run skills-refresh`. Records what each imported skill was, and its hash.
const LOCK: &str = ".agents/skills.lock.json";

/// `name` and `description` from a `SKILL.md` YAML frontmatter block.
///
/// Hand-parsed: the shape is two keys in a `---` fenced block at the top of a file this repo
/// controls, and a YAML dependency to read that would be a poor trade.
fn frontmatter(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return out;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            out.insert(String::from(key.trim()), String::from(value.trim()));
        }
    }
    out
}

/// Every `SKILL.md` under the skills tree, as `group/name` keyed by its declared name.
fn discovered(root: &Path) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let dir = root.join(SKILLS_DIR);
    let Ok(groups) = std::fs::read_dir(&dir) else {
        return found;
    };
    for group in groups.flatten() {
        if !group.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(skills) = std::fs::read_dir(group.path()) else {
            continue;
        };
        for skill in skills.flatten() {
            if !skill.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let manifest = skill.path().join("SKILL.md");
            if !manifest.is_file() {
                continue;
            }
            let group_name = group.file_name().to_string_lossy().into_owned();
            let skill_name = skill.file_name().to_string_lossy().into_owned();
            found.insert(
                format!("{group_name}/{skill_name}"),
                format!("{group_name}/{skill_name}/SKILL.md"),
            );
        }
    }
    found
}

/// Every `group/skill` the router claims exists, from the `groups.*.skills` maps.
///
/// Parsed with `serde_json`, which xtask already depends on.
fn routed(text: &str) -> Result<BTreeSet<String>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("skill-router.json is not valid JSON: {e}"))?;
    let groups = value
        .get("groups")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| String::from("skill-router.json has no `groups` object"))?;

    let mut out = BTreeSet::new();
    for (group, body) in groups {
        let skills = body
            .get("skills")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| format!("group `{group}` has no `skills` object"))?;
        for name in skills.keys() {
            out.insert(format!("{group}/{name}"));
        }
    }
    Ok(out)
}

/// Intents must resolve to a routed skill, or the top-level router sends an agent nowhere.
fn intent_targets(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    value
        .get("intents")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|i| {
                    let g = i.get("group")?.as_str()?;
                    let s = i.get("skill")?.as_str()?;
                    Some(format!("{g}/{s}"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every locked skill whose file no longer hashes to what the lock recorded.
///
/// This is what makes `mirror` mean something. Without it, an edit to an imported skill leaves
/// it claiming to be a mirror while being a fork - and the next refresh silently reverts the
/// edit, or silently keeps it, depending on which side moved.
///
/// Hashed with LF normalised, so a checkout on a CRLF platform is not a false positive.
fn lock_mismatches(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(LOCK)) else {
        // No lock is not a failure: a repo need not import anything. An import WITHOUT a lock
        // is caught by the provenance check instead.
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return vec![format!("{LOCK} is not valid JSON")];
    };
    let Some(skills) = value.get("skills").and_then(serde_json::Value::as_array) else {
        return vec![format!("{LOCK} has no `skills` array")];
    };

    let mut problems = Vec::new();
    for entry in skills {
        let Some(local) = entry.get("local_path").and_then(serde_json::Value::as_str) else {
            problems.push(format!("{LOCK}: an entry has no `local_path`"));
            continue;
        };
        let Some(expected) = entry.get("sha256").and_then(serde_json::Value::as_str) else {
            problems.push(format!("{LOCK}: `{local}` has no `sha256`"));
            continue;
        };
        let path = root.join(".agents").join(local).join("SKILL.md");
        let Ok(bytes) = std::fs::read(&path) else {
            problems.push(format!("{LOCK}: `{local}/SKILL.md` is locked but missing"));
            continue;
        };
        if sha256_lf(&bytes) != expected {
            let status = entry.get("status").and_then(serde_json::Value::as_str).unwrap_or("imported");
            problems.push(format!("`{local}/SKILL.md` no longer matches the lock (status `{status}`)"));
        }
    }
    problems
}

/// SHA-256 with CRLF normalised to LF, matching what the sync script records.
///
/// Normalised so a checkout on a CRLF platform is not a false positive: the lock is about
/// content, and a line ending is not content.
fn sha256_lf(bytes: &[u8]) -> String {
    use sha2::Digest as _;

    // Numeric, not char literals: this is a byte-level normaliser, and the numbers say so.
    const CR: u8 = 13;
    const LF: u8 = 10;

    let mut normalised = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter().copied().peekable();
    while let Some(byte) = iter.next() {
        // Drop the CR of a CRLF pair; the LF arrives next iteration. A lone CR is content.
        if byte == CR && iter.peek() == Some(&LF) {
            continue;
        }
        normalised.push(byte);
    }

    let mut hex = String::with_capacity(64);
    for byte in sha2::Sha256::digest(&normalised) {
        use std::fmt::Write as _;
        if write!(hex, "{byte:02x}").is_err() {
            return String::new();
        }
    }
    hex
}

/// How many library skills there are, for the verdict line.
fn library_count(root: &Path) -> usize {
    let Ok(groups) = std::fs::read_dir(root.join(LIBRARY_DIR)) else {
        return 0;
    };
    groups
        .flatten()
        .filter_map(|g| std::fs::read_dir(g.path()).ok())
        .map(|skills| skills.flatten().filter(|s| s.path().join("SKILL.md").is_file()).count())
        .sum()
}

/// Each agent's conventional skills directory, which must be a symlink to the one tree.
///
/// One canonical set, three products. Claimed in `.agents/skills/README.md` and in
/// `.claude/settings.json`, and it stopped being true: the entries were added to the index and
/// a later `git add -A` on a checkout with `core.symlinks=false` replaced them. Nothing
/// noticed, because a claim in a README enforces nothing.
// The same list `text-hygiene` skips, so the set that is exempt from the newline rule and
// the set whose symlink mode is verified cannot drift apart.
use crate::repo::INDEX_SYMLINKS as AGENT_SKILL_LINKS;

/// The symlink target the entries must have.
const LINK_TARGET: &str = "../.agents/skills";

/// Verify each agent directory still points at the canonical tree.
///
/// Checks the INDEX, not the working tree: on a platform without symlink support git stores
/// mode 120000 and writes a pointer file, so the working tree looks like a regular file and
/// only the index says what it is.
fn link_problems(root: &Path) -> Vec<String> {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args(["ls-files", "--stage", "--"])
        .args(AGENT_SKILL_LINKS)
        .output();
    // `Ok` means git RAN, not that it succeeded. A missing binary is `Err`; a present binary
    // outside a repository is `Ok` with a non-zero status and empty stdout - and reading that
    // empty listing as "the index has no such entry" reported all three links missing in
    // exactly the environment the doc comment above says cannot be judged. That is what broke
    // the Dockerfile's `build` target, where `COPY . .` brings the tree and no `.git`.
    //
    // The same distinction as the supply-chain gate: a check that could not run must not
    // report a finding.
    let Ok(out) = out else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let listing = String::from_utf8_lossy(&out.stdout);
    problems_in_listing(root, &listing)
}

/// Judge an index listing that git DID produce.
///
/// Split out so the two failure modes can be told apart in a test: an empty listing from a
/// successful `git ls-files` genuinely means the entries are gone, while git being unable to
/// answer at all means nothing - and conflating them is the bug this split exists to prevent.
fn problems_in_listing(root: &Path, listing: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for link in AGENT_SKILL_LINKS {
        let entry = listing.lines().find(|l| l.ends_with(link));
        match entry {
            None => problems.push(format!(
                "`{link}` is missing - it must be a symlink to `{LINK_TARGET}` so every agent reads one tree"
            )),
            Some(line) if !line.starts_with("120000") => problems.push(format!(
                "`{link}` is a regular file, not a symlink - `git add -A` on a checkout without symlink support does this"
            )),
            Some(_) => {
                // `read_link` FIRST, because the link points at a DIRECTORY: reading it as a file
                // fails once the OS has followed it, and `unwrap_or_default` turned that into
                // "points at ``" - this gate failing on a checkout that was correct, since
                // `4378d2e`. `read_to_string` stays as the FALLBACK, for the pointer-file shape the
                // mode check above describes: git can record 120000 while the working tree holds a
                // regular file whose content is the target path.
                let target = std::fs::read_link(root.join(link))
                    .map(|p| p.to_string_lossy().into_owned())
                    .or_else(|_| std::fs::read_to_string(root.join(link)))
                    .unwrap_or_default();
                if target.trim() != LINK_TARGET {
                    problems.push(format!("`{link}` points at `{}`, not `{LINK_TARGET}`", target.trim()));
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod link_tests {
    use std::path::Path;

    #[test]
    fn an_empty_listing_from_a_successful_run_means_the_links_are_gone() {
        // This is the CORRECT reading when git answered. The bug was reaching this conclusion
        // after git had exited non-zero with nothing to say.
        let problems = super::problems_in_listing(Path::new("."), "");
        assert_eq!(problems.len(), super::AGENT_SKILL_LINKS.len());
        assert!(problems.iter().all(|p| p.contains("is missing")));
    }

    #[test]
    fn a_regular_file_is_named_as_such_rather_than_missing() {
        // Mode 100644 instead of 120000: the shape `git add -A` produces on a checkout with no
        // symlink support. Worth its own message, because the fix is different.
        let listing = super::AGENT_SKILL_LINKS
            .iter()
            .map(|l| format!("100644 0000000000000000000000000000000000000000 0	{l}"))
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        let problems = super::problems_in_listing(Path::new("."), &listing);
        assert_eq!(problems.len(), super::AGENT_SKILL_LINKS.len());
        assert!(problems.iter().all(|p| p.contains("not a symlink")));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_directory_reads_as_a_link_rather_than_as_a_file() {
        // THE bug this closes: each of these links points at a DIRECTORY, so reading it as a
        // FILE fails once the OS has followed it, and `unwrap_or_default` turned that failure
        // into `points at ``` - the gate failing on a checkout that was correct. Nothing caught
        // it, and the reason is the same blind spot the mode check documents: the nix sandbox
        // has no `.git`, so `check-skills` skips itself in CI and only ever ran on a real tree.
        let root = std::env::temp_dir().join(format!("sutura-skills-{}", std::process::id()));
        // Bound rather than `let _`: `let_underscore_must_use` is on, and a scratch directory
        // that may not exist yet - or may be left behind by a killed run - is the one case where
        // the result genuinely carries nothing.
        let _cleanup = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(super::LINK_TARGET.trim_start_matches("../"))).expect("a scratch tree");
        for link in super::AGENT_SKILL_LINKS {
            let path = root.join(link);
            std::fs::create_dir_all(path.parent().expect("a link has a parent")).expect("the link's directory");
            std::os::unix::fs::symlink(super::LINK_TARGET, &path).expect("a symlink to the one tree");
        }
        let listing = super::AGENT_SKILL_LINKS
            .iter()
            .map(|link| format!("120000 2b7a412b8fa0fb7e985b0793321bd4e698f2b6cd 0\t{link}"))
            .collect::<Vec<String>>()
            .join("\n");

        let problems = super::problems_in_listing(&root, &listing);
        // Bound rather than `let _`: `let_underscore_must_use` is on, and a scratch directory
        // that may not exist yet - or may be left behind by a killed run - is the one case where
        // the result genuinely carries nothing.
        let _cleanup = std::fs::remove_dir_all(&root);
        assert!(problems.is_empty(), "a correct checkout reports nothing, got {problems:?}");
    }
}

/// Library skills are exempt from routing but not from provenance. An imported skill with no
/// upstream recorded cannot be updated, audited for licence, or compared against upstream
/// later - which is how a mirror silently becomes a fork nobody can reconcile.
fn library_problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let dir = root.join(LIBRARY_DIR);
    let Ok(groups) = std::fs::read_dir(&dir) else {
        return problems;
    };
    for group in groups.flatten() {
        if !group.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(skills) = std::fs::read_dir(group.path()) else {
            continue;
        };
        for skill in skills.flatten() {
            let manifest = skill.path().join("SKILL.md");
            if !manifest.is_file() {
                continue;
            }
            let rel = format!(
                "{}/{}",
                group.file_name().to_string_lossy(),
                skill.file_name().to_string_lossy()
            );
            let Ok(body) = std::fs::read_to_string(&manifest) else {
                problems.push(format!("could not read `{LIBRARY_DIR}/{rel}/SKILL.md`"));
                continue;
            };
            if !body.contains("## Provenance") {
                problems.push(format!(
                    "`{LIBRARY_DIR}/{rel}/SKILL.md` has no `## Provenance` section - record the upstream, licence and local status"
                ));
            }
            if !frontmatter(&body).contains_key("name") {
                problems.push(format!("`{LIBRARY_DIR}/{rel}/SKILL.md` has no `name` in its frontmatter"));
            }
        }
    }
    problems
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-skills: could not determine the repo root");
        return Verdict::Fail;
    };

    let router_path = root.join(ROUTER);
    if !router_path.is_file() {
        // FAIL CLOSED, BOTH WAYS. This arm used to answer `ok - no skills tree` on the argument
        // that a repo need not have one - but this gate is registered in THIS repository's
        // sweep, where `AGENTS.md` routes every session through that tree and declares a skill
        // absent from the router non-discoverable by policy. So the tree going missing is the
        // loudest thing this gate can be asked about, and it announced success instead:
        // `github.com/telekom/sutura#371`'s class, a gate answering `ok` before reading its
        // subject.
        eprintln!("xtask check-skills: FAILED - {ROUTER} does not exist");
        if !discovered(&root).is_empty() {
            eprintln!("  and SKILL.md files do - an unrouted skill is non-discoverable by policy.");
        }
        eprintln!("  The skills tree is how AGENTS.md routes a session; without the router there is");
        eprintln!("  nothing to hold it against, so this is a refusal rather than a clean bill.");
        return Verdict::Fail;
    }

    let Ok(text) = std::fs::read_to_string(&router_path) else {
        eprintln!("xtask check-skills: could not read {ROUTER}");
        return Verdict::Fail;
    };

    let claimed = match routed(&text) {
        Ok(set) => set,
        Err(e) => {
            eprintln!("xtask check-skills: FAILED - {e}");
            return Verdict::Fail;
        }
    };
    let present = discovered(&root);
    let present_keys: BTreeSet<String> = present.keys().cloned().collect();

    let mut problems = Vec::new();

    for missing in claimed.difference(&present_keys) {
        problems.push(format!("router names `{missing}`, but no such SKILL.md exists"));
    }
    for orphan in present_keys.difference(&claimed) {
        problems.push(format!(
            "`{orphan}/SKILL.md` is in no route - unlisted skills are non-discoverable, so add it to {ROUTER} or delete it"
        ));
    }
    for target in intent_targets(&text) {
        if !claimed.contains(&target) {
            problems.push(format!("an intent routes to `{target}`, which no group lists"));
        }
    }
    problems.extend(library_problems(&root));
    problems.extend(lock_mismatches(&root));
    problems.extend(link_problems(&root));

    // The frontmatter `name` is the identifier a route and an agent use; a mismatch makes the
    // route look right and read wrong.
    for (key, rel) in &present {
        let Ok(body) = std::fs::read_to_string(root.join(SKILLS_DIR).join(rel)) else {
            problems.push(format!("could not read `{rel}`"));
            continue;
        };
        let fm = frontmatter(&body);
        let dir_name = key.rsplit('/').next().unwrap_or(key);
        match fm.get("name") {
            None => problems.push(format!("`{rel}` has no `name` in its frontmatter")),
            Some(name) if name != dir_name => {
                problems.push(format!("`{rel}` declares name `{name}` but lives in `{dir_name}/`"));
            }
            Some(_) => {}
        }
        if !fm.contains_key("description") {
            problems.push(format!(
                "`{rel}` has no `description` - that line is what a router uses to decide relevance"
            ));
        }
    }

    if problems.is_empty() {
        println!(
            "xtask check-skills: ok - {} routed, {} in the library, {} agent link(s)",
            present.len(),
            library_count(&root),
            AGENT_SKILL_LINKS.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-skills: FAILED");
    for p in &problems {
        eprintln!("  {p}");
    }
    if problems.iter().any(|p| p.contains("no longer matches the lock")) {
        eprintln!();
        eprintln!("Edited an import on purpose? `pixi run skills-relock` records the new hash.");
        eprintln!("Otherwise it is a local fork: `pixi run skills-refresh` restores upstream.");
    }
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{frontmatter, intent_targets, routed};

    #[test]
    fn reads_frontmatter() {
        let fm = frontmatter("---\nname: rust\ndescription: A thing.\n---\n\n# Body\n");
        assert_eq!(fm.get("name").map(String::as_str), Some("rust"));
        assert_eq!(fm.get("description").map(String::as_str), Some("A thing."));
    }

    #[test]
    fn a_file_without_frontmatter_yields_nothing() {
        assert!(frontmatter("# Just a heading\n").is_empty());
        // A `---` that is not the first line is a horizontal rule, not frontmatter.
        assert!(frontmatter("text\n---\nname: x\n---\n").is_empty());
    }

    #[test]
    fn collects_routed_skills() {
        let json = r#"{"groups":{"engineering":{"skills":{"rust":"a","oauth":"b"}},
                                  "reasoning":{"skills":{"autoreason":"c"}}}}"#;
        let set = routed(json).expect("valid");
        assert!(set.contains("engineering/rust"));
        assert!(set.contains("engineering/oauth"));
        assert!(set.contains("reasoning/autoreason"));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn rejects_a_malformed_router() {
        drop(routed("not json").unwrap_err());
        drop(routed(r#"{"no_groups":1}"#).unwrap_err());
        // A group with no `skills` map is a route to nowhere.
        drop(routed(r#"{"groups":{"g":{}}}"#).unwrap_err());
    }

    #[test]
    fn collects_intent_targets() {
        let json = r#"{"intents":[{"intent":"x","group":"engineering","skill":"rust"}]}"#;
        assert_eq!(intent_targets(json), vec![String::from("engineering/rust")]);
        assert!(intent_targets("{}").is_empty());
    }

    // NOTE: there is deliberately no test here that reads the real router and the real
    // tree. That property is enforced by `cargo xtask check-skills`, which runs in the
    // hooks and in the `hygiene` flake check - and `hygiene` is the check given the whole
    // repository as its source. A unit test cannot do it: the `nextest` check gets
    // crane's Cargo-only source filter, so `.agents/` is not there, and it failed in CI
    // for exactly that reason. Widening the test derivation to the whole repo would make
    // every documentation edit invalidate the test build.
}
