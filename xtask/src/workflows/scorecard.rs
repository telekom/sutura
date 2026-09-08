//! A badge is a public claim, so it is held against the mechanism it claims.
//!
//! `README.md` carries two badges that assert something about this repository to anybody who reads
//! it, and each is served by a third party from data this repository sends. That makes them
//! different from the shields beside them: `rust-2024` and `workflows-zizmor` assert a property of
//! the tree, where these two report a scan. **An overstated control is itself the defect here**, so
//! a badge whose mechanism is gone has to be a failing check rather than a discovery made by
//! whoever believed it.
//!
//! **A RULE OF `check-workflows` rather than a gate of its own.** Partly because
//! `xtask/src/main.rs` stands at 999 lines against a 1000-line cap that `crates/` and `xtask/`
//! cannot be exempted from - so a new task-table entry is not available - and mostly because the
//! question is already this gate's: it reads `.github/**` and `flake.nix` and walks every place CI
//! invokes something from. A second gate over the same walk would be a second answer.
//!
//! # What it holds
//!
//! * **The Scorecard badge and the publication move together, in BOTH directions.** The badge is
//!   served from `api.scorecard.dev` and renders nothing unless the workflow sets
//!   `publish_results: true`; and that input sends this repository's score, every check's reason
//!   and the commit to that public endpoint. So a badge with publication off is a dead image, and
//!   publication with no badge is a disclosure no reader of the README can see. Either alone
//!   fails.
//! * **A publication is a decision somebody wrote down.** `publish_results: true` requires
//!   `devco/scorecard-publication` to exist and to carry content. An empty or missing record fails,
//!   because the alternative is a disclosure justified by a line of YAML.
//! * **The REUSE badge needs a live licence check.** It may only be claimed while `flake.nix`
//!   declares `checks.reuse` AND some file CI invokes something from actually builds it. A badge
//!   asserting compliance while nothing measures it is the exact defect above.
//! * **No copied tree can arrive attributed to us.** `REUSE.toml`'s catch-all makes an unnarrowed
//!   file OURS by default, and that file's own header says so - *a newly vendored third-party file
//!   is silently attributed to the sutura authors*, with *adding a vendored tree means appending a
//!   block* as the remedy. That remedy was held by recall. Now every immediate child of `vendor/`
//!   must be narrowed and recorded in `VENDOR.md`, and every `vendor/` block must name a child that
//!   exists. See [`vendored_tree_problems`], including why this is not the mechanism
//!   `nix/reuse.nix` weighs and rejects.
//! * **Both badges name THIS repository.** The slug comes from `REUSE.toml`'s
//!   `SPDX-PackageDownloadLocation`, so a badge copied from another project - which would render
//!   somebody else's score under our name - is a failure rather than a puzzle.
//!
//! # What it does NOT hold, and each is real
//!
//! * **Not that publishing is a good idea.** It holds that the decision is written down and
//!   visible, never that it is right. `docs/adr/0024` is the argument.
//! * **Not that the badge renders.** The repository is private as read on 2026-09-07, so
//!   `api.reuse.software` cannot clone it and the REUSE badge answers `unregistered`. Nothing here
//!   reaches a network; that is a fact about today, recorded in the ADR rather than checked.
//! * **Not that the REUSE lint is INFORMATIVE.** It holds that the check exists and is invoked.
//!   `REUSE.toml`'s `path = "**"` catch-all means an unheadered file PASSES that lint - measured,
//!   and stated in `nix/reuse.nix` - so the badge is a true statement about REUSE compliance and
//!   not a statement that the declarations are correct. **What the vendored-tree rule adds is
//!   narrow and worth stating exactly:** no immediate child of `vendor/` can lack its own covering
//!   `[[annotations]]` block, or be missing from `VENDOR.md`. It says nothing about a file DEEPER
//!   inside such a child - a block for the child's subtree covers it, and this reader does not
//!   open it - and nothing about a copy placed outside `vendor/`.
//!   It says nothing about a FILE inside a narrowed tree, nothing about a first-party file with no
//!   header, and nothing about a copy placed outside `vendor/`.
//! * **Not that the workflow ever runs, or that anyone looks.** It reports no status context on a
//!   pull request and the only required context is `ci`. A falling score blocks nothing.
//! * **Not what the third party does with what it receives.** The record is a dated reading of the
//!   action's documentation, not a trace.
//! * **It reads the FILESYSTEM, not git.** So a gitignored entry under `vendor/` is a child as far
//!   as this rule is concerned: on darwin one Finder visit leaves `vendor/.DS_Store`, which reddens
//!   `just hygiene` locally while CI stays green, because the git-derived sandbox never has it.
//!   That is a false local red rather than a finding - delete the file. Filtering it by name would
//!   be a special case, and filtering every dotfile would open a hole a copied `.hidden` tree walks
//!   through.

use std::path::Path;

use super::sources;

/// The workflow that runs and publishes the scan.
pub(super) const WORKFLOW: &str = ".github/workflows/scorecard.yml";

/// The dated decision behind the publication.
const RECORD: &str = "devco/scorecard-publication";

/// The page the badges are on.
const README: &str = "README.md";

/// Where the repository's own identity is declared, so a badge slug has one authority.
const PACKAGE: &str = "REUSE.toml";

/// The key that declares it there.
const DOWNLOAD_LOCATION: &str = "SPDX-PackageDownloadLocation";

/// The action input that sends a score to `api.scorecard.dev`, as a KEY.
///
/// The value is parsed rather than matched, and the difference is a bypass that was measured: this
/// used to be the literal `publish_results: true`, so `'true'`, `"true"` and `True` all read as
/// *not publishing* while the action published anyway - GitHub serialises a `with:` value to a
/// string. Both the badge rule and the record requirement were bypassed by a quoting choice, and
/// the end state was a live score at `api.scorecard.dev` with no badge and no dated record. Same
/// shape as this file's own `apps.reuse` lesson: textual presence is not the property.
const PUBLISH: &str = "publish_results";

/// The badge image host and path, without the slug.
const SCORECARD_BADGE: &str = "https://api.scorecard.dev/projects/";

/// The REUSE badge image host and path, without the slug.
const REUSE_BADGE: &str = "https://api.reuse.software/badge/";

/// The flake check the REUSE badge stands on, as `flake.nix` declares it.
const CHECK: &str = "reuse";

/// Where a COPIED third-party tree lands. The catch-all in `REUSE.toml` attributes anything
/// unnarrowed to us, so an unnarrowed tree here is the one way that file can state a falsehood.
const VENDOR: &str = "vendor";

/// Where a copied tree is recorded in prose - `AGENTS.md`: *vendor only to move fast and record it
/// in `VENDOR.md`*.
const VENDOR_RECORD: &str = "VENDOR.md";

/// And as a workflow, action or CI script has to build it.
const REUSE_CHECK: &str = "checks.x86_64-linux.reuse";

/// Every way a badge in `README.md` could be claiming more than this tree holds.
///
/// A `Vec` rather than a `Verdict`, like [`super::contexts::problems`]: one gate prints one
/// verdict, and a rule that owns its own exit code is a rule whose venue nobody can see.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();

    // THE SLUG, first, because two rules below are about naming THIS repository and a guess would
    // make both of them nonsense. An unreadable declaration is a failure rather than a smaller
    // scan - the shape `crate::arrow_major` draws between configuration and a fault.
    let package = match std::fs::read_to_string(root.join(PACKAGE)) {
        Ok(text) => text,
        Err(error) => {
            problems.push(format!(
                "could not read {PACKAGE}: {error} - it is where this repository's own name is declared"
            ));
            return problems;
        }
    };
    let Some(slug) = slug(&package) else {
        problems.push(format!(
            "{PACKAGE} declares no `{DOWNLOAD_LOCATION}` this reader can turn into an `<owner>/<repo>` slug - so whether a badge names this repository is unchecked rather than answered wrongly"
        ));
        return problems;
    };

    let readme = match std::fs::read_to_string(root.join(README)) {
        Ok(text) => text,
        Err(error) => {
            problems.push(format!("could not read {README}: {error} - it is where the badges are"));
            return problems;
        }
    };
    let workflow = std::fs::read_to_string(root.join(WORKFLOW)).unwrap_or_default();

    // Comment lines are skipped on the workflow side, so the file may EXPLAIN the input at length
    // without the explanation counting as setting it - the distinction
    // `workflows::collect`'s `a_comment_is_not_a_reference` draws.
    let publishes = publishes(&workflow);
    let badged = readme.contains(SCORECARD_BADGE);

    // BOTH DIRECTIONS, and only one of them is obvious. A badge with publication off is a dead
    // image; publication with no badge is a disclosure no reader of the README can see.
    if badged && !publishes {
        problems.push(format!(
            "{README} shows the OpenSSF Scorecard badge and {WORKFLOW} does not set `{PUBLISH}` to a true value - the badge is served from published results, so it would render nothing while asserting a score"
        ));
    }
    if publishes && !badged {
        problems.push(format!(
            "{WORKFLOW} sets `{PUBLISH}` to a value that publishes, which sends this repository's score to the public api.scorecard.dev, and {README} shows no badge - the disclosure would be visible only in YAML"
        ));
    }
    if badged && !readme.contains(&format!("{SCORECARD_BADGE}github.com/{slug}/badge")) {
        problems.push(format!(
            "the Scorecard badge in {README} does not name `{slug}` - a badge copied from another project renders somebody else's score under this name"
        ));
    }
    // A DECISION SOMEBODY WROTE DOWN. Emptiness and absence are the same finding: a disclosure
    // whose only justification is a line of YAML.
    if publishes {
        match std::fs::read_to_string(root.join(RECORD)) {
            Ok(text) if text.trim().is_empty() => problems.push(format!(
                "{RECORD} is empty and {WORKFLOW} publishes - a disclosure needs a dated reason, not a blank file"
            )),
            Ok(_) => {}
            Err(error) => problems.push(format!(
                "{WORKFLOW} publishes to api.scorecard.dev and {RECORD} could not be read: {error} - that record is where the decision and what it sends are written down"
            )),
        }
    }

    if readme.contains(REUSE_BADGE) {
        problems.extend(reuse_badge_problems(root, &readme, &slug, &package));
    }

    problems
}

/// What the REUSE badge stands on: a declared check, and a CI file that actually builds it.
///
/// Two reads rather than one, because a declaration nothing invokes is the failure this rule
/// exists for - the same distinction `crate::venues` draws between a venue existing and CI
/// reaching it.
fn reuse_badge_problems(root: &Path, readme: &str, slug: &str, package: &str) -> Vec<String> {
    let mut problems = Vec::new();

    if !readme.contains(&format!("{REUSE_BADGE}github.com/{slug}")) {
        problems.push(format!(
            "the REUSE badge in {README} does not name `{slug}` - it would report another project's compliance under this name"
        ));
    }

    // THE DECLARED CHECK SET, through this gate's own lexer rather than a substring search over
    // the file. `flake.nix` also declares `apps.reuse`, so *does the text mention reuse* is true
    // of a tree whose CHECK has been renamed away - which is precisely the state this rule exists
    // to catch, and the first version of it passed over exactly that mutation.
    let flake = std::fs::read_to_string(root.join("flake.nix")).unwrap_or_default();
    match super::declared_block(&flake, "checks = {") {
        Some(checks) if checks.contains(CHECK) => {}
        Some(_) => problems.push(format!(
            "{README} claims REUSE compliance and flake.nix's `checks` block declares no `{CHECK}` - the badge would assert a licence property nothing measures"
        )),
        None => problems.push(String::from(
            "flake.nix's `checks` block does not close, so which checks exist is unread - the scan is broken rather than the tree",
        )),
    }

    match sources::ci_sources(root) {
        Some(found) => {
            if !found.iter().any(|source| source.text.contains(REUSE_CHECK)) {
                problems.push(format!(
                    "{README} claims REUSE compliance and no workflow, action or CI script builds `{REUSE_CHECK}` - a declared check nothing invokes measures nothing"
                ));
            }
        }
        None => problems.push(String::from(
            "the CI sources could not be walked, so whether anything builds the reuse check is unread",
        )),
    }

    problems.extend(vendored_tree_problems(root, package));

    problems
}

/// Can `REUSE.toml` still be TRUE about the copied trees, or only complete?
///
/// **This is the one way that file can state a falsehood, and its own header says so** - the
/// catch-all resolves anything unnarrowed to Apache-2.0 and the sutura authors, so *a newly
/// vendored third-party file is silently attributed to the sutura authors* until a narrowing block
/// is appended. That sentence is in `REUSE.toml`, next to *adding a vendored tree means appending a
/// block* - the hazard named, the remedy named, and until now the remedy held by recall.
///
/// **NOT the mechanism `nix/reuse.nix` weighs and rejects, and the difference decides whether this
/// is sound.** That one was *every path `VENDOR.md` names must have its own block*, which is wrong
/// because most of what that file lists is recorded *"Rewritten, not copied"* and is therefore
/// correctly ours - a gate demanding a foreign licence would fail on correct code. This reads the
/// other direction and a narrower subject: the `vendor/` DIRECTORY, which is where a true copy
/// lands, so every subject of the rule is a copy by construction and no rewritten tree is in scope.
///
/// **Its limit, and it is the reason the rejected variant was tempting:** `vendor/` is a
/// convention, not a type. `.agents/skill-library/` is also a copy and is narrowed by hand; nothing
/// here would notice a copy placed somewhere new. Telling a copy from a rewrite in general remains
/// the review question `nix/reuse.nix` describes. What this closes is the case that recurs - a tree
/// dropped into `vendor/` and the block forgotten.
///
/// Both directions, on `check-skills`' precedent: a child with no block, and a block naming no
/// child. One-way, this would go green the day a vendored tree is removed and its block left
/// behind, still permitting nothing.
fn vendored_tree_problems(root: &Path, package: &str) -> Vec<String> {
    let mut problems = Vec::new();

    let narrowing = narrowing_paths(package);
    if narrowing.is_empty() {
        // FAIL CLOSED: this file always carries narrowing blocks, so reading none is the scanner
        // broken rather than the tree clean - and a scan that finds nothing must not be how the
        // rule goes unchecked again.
        problems.push(format!(
            "{PACKAGE} yielded no narrowing `path =` annotation - the reader stopped matching it, so which trees are narrowed is unknown rather than answered"
        ));
        return problems;
    }

    let children = match std::fs::read_dir(root.join(VENDOR)) {
        Ok(entries) => {
            // EVERY IMMEDIATE CHILD, never only the directories. The claim is about a COPY, and a
            // copied file is attributed by the same catch-all as a copied tree: measured on this
            // branch, one header-less `.rs` dropped straight into `vendor/` left the gate at
            // `ok - 33 gate(s)`, exit 0, while byte-identical content one directory deeper
            // refused - so `is_dir()` made the rule's set smaller than the sentence over it. It
            // also answered `false` for any child whose metadata could not be read, which is an
            // absence standing in for a fault; both are gone with the filter.
            let mut found = Vec::new();
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        problems.push(format!(
                            "{VENDOR}/ holds an entry that could not be read: {error} - so whether a copied tree is attributed to us is unread rather than clean"
                        ));
                        return problems;
                    }
                };
                found.push(entry.file_name().to_string_lossy().into_owned());
            }
            found.sort();
            found
        }
        // NO VENDORED TREE AT ALL is a legitimate state, and only `NotFound` says so. Every other
        // error is a fault - the same seam `crate::arrow_major` draws between configuration and an
        // input it could not read. A stale block is still caught below, because the loop over the
        // annotations runs against an empty child set.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            problems.push(format!(
                "{VENDOR}/ could not be read: {error} - so whether a copied tree is attributed to us is unread rather than clean"
            ));
            return problems;
        }
    };

    // ONE: every copied thing is narrowed BY A BLOCK THAT COVERS ALL OF IT.
    //
    // **`starts_with` was the bug, and it shipped a false public claim end to end.** A block for
    // `vendor/newtree/a.c` sits UNDER `vendor/newtree`, so the child read as narrowed while its
    // sibling `b.c` fell through to the catch-all - and that state was green at every stage:
    // `check-workflows` exit 0, `reuse lint` *"Congratulations! Your project is compliant"* exit 0,
    // and `b.c` resolving to `Apache-2.0` (c) the sutura authors. A copied third-party file
    // declared ours, with the badge asserting compliance over it. Found in review, not by this
    // gate. So a covering block is the child ITSELF or the child's whole subtree, and nothing
    // narrower: `vendor/mimalloc_rust/**` satisfies it, which is why the real tree stays green.
    for child in &children {
        let under = format!("{VENDOR}/{child}");
        let subtree = format!("{under}/**");
        let narrowed = narrowing.iter().any(|(_, path)| path == &under || path == &subtree);
        if !narrowed {
            problems.push(format!(
                "{VENDOR}/{child} is a copied path and {PACKAGE} has no block COVERING it, so the `path = \"**\"` catch-all attributes it to the sutura authors - a block for something inside it does not count, because its siblings still fall through. Append an `[[annotations]]` block whose path is exactly `{under}` - or `{under}/**` to cover a directory's whole subtree - after the catch-all, and record the copy in {VENDOR_RECORD}"
            ));
        }
    }

    // TWO: every narrowing block about a copied tree names one that exists.
    for (line, path) in &narrowing {
        let Some(rest) = path.strip_prefix(&format!("{VENDOR}/")) else {
            continue;
        };
        let Some(named) = rest.split('/').next().filter(|part| !part.is_empty() && *part != "**") else {
            continue;
        };
        if !children.iter().any(|child| child == named) {
            problems.push(format!(
                "{PACKAGE}:{line} narrows `{path}` and {VENDOR}/{named} does not exist - a block permitting nothing keeps its dated paragraph, which is the reasoning a reviewer reads to decide the exception is still earned"
            ));
        }
    }

    // THREE: every copied tree is recorded in prose, which is `AGENTS.md`'s own rule -
    // *vendor only to move fast and record it in `VENDOR.md`*. Cheap here because the child
    // listing is already in hand; unreadable is a fault, for the reason above.
    match std::fs::read_to_string(root.join(VENDOR_RECORD)) {
        Ok(record) => {
            for child in &children {
                if !record.contains(&format!("{VENDOR}/{child}")) {
                    problems.push(format!(
                        "{VENDOR}/{child} is a copied path and {VENDOR_RECORD} does not name it - a copy makes us its security response permanently, and that is the file where somebody can find out"
                    ));
                }
            }
        }
        Err(error) => problems.push(format!(
            "{VENDOR_RECORD} could not be read: {error} - so whether each copied tree is recorded is unread rather than clean"
        )),
    }

    problems
}

/// Every `path = "..."` in `REUSE.toml` that narrows rather than catches all, with its line.
///
/// The line is carried because a refusal a reader cannot open is a refusal they have to search for.
/// The catch-all is excluded by value: it is the thing the narrowing blocks exist to override, so
/// counting it as one would make every tree look narrowed.
fn narrowing_paths(package: &str) -> Vec<(usize, String)> {
    package
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with('#'))
        .filter_map(|(index, line)| {
            let rest = line.trim().strip_prefix("path")?.trim_start().strip_prefix('=')?;
            let value = rest.trim().trim_matches('"');
            (value != "**" && !value.is_empty()).then(|| (index + 1, String::from(value)))
        })
        .collect()
}

/// `<owner>/<repo>`, out of `REUSE.toml`'s declared download location.
///
/// One authority for this repository's own name, so the two badge rules cannot disagree about
/// which project they are checking.
fn slug(package: &str) -> Option<String> {
    let line = package
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find(|line| line.starts_with(DOWNLOAD_LOCATION))?;
    let url = line.split('"').nth(1)?;
    let rest = url.trim_end_matches('/').strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let owner = parts.next().filter(|part| !part.is_empty())?;
    let repo = parts.next().filter(|part| !part.is_empty())?;
    // Exactly two segments: anything longer is a path INTO the repository rather than the
    // repository, and a slug taken from one would name something no badge host serves.
    if parts.next().is_some() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

/// Does this workflow publish - by reading the input's VALUE, never its spelling?
///
/// **Fail closed on the value, and that direction is the whole point.** An ABSENT key is not
/// publishing: the action's own default is `false`, read from its `action.yaml`. A PRESENT key
/// publishes unless its value is unambiguously false, so every quoting and casing of true counts,
/// and so does anything this reader cannot classify - an expression, a typo, a YAML scalar nobody
/// expected. The alternative direction would let one unrecognised spelling publish a score with no
/// badge and no record, which is exactly the bypass this replaced.
pub(super) fn publishes(workflow: &str) -> bool {
    uncommented(workflow).any(|line| {
        let Some(rest) = line.trim().strip_prefix(PUBLISH) else {
            return false;
        };
        let Some(value) = rest.strip_prefix(':') else {
            // `publish_results_something:` is a different key, and `publish_results` with no colon
            // is not a mapping entry at all.
            return false;
        };
        let value = value.trim().trim_matches(['"', '\'']).to_ascii_lowercase();
        !value.is_empty() && value != "false"
    })
}

/// Every line that is not wholly a comment.
///
/// Whole lines only, deliberately. Stripping from the first `#` would also cut the `# v7.0.1`
/// that names the version behind every pinned action SHA, and the needle here is a key.
pub(super) fn uncommented(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.trim_start().starts_with('#'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slug_comes_from_the_declared_download_location() {
        let package = concat!(
            "# a comment naming SPDX-PackageDownloadLocation = \"https://github.com/someone/else\"\n",
            "SPDX-PackageName = \"sutura\"\n",
            "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
        );
        assert_eq!(slug(package).as_deref(), Some("telekom/sutura"));
    }

    #[test]
    fn a_location_this_reader_cannot_turn_into_a_slug_is_refused_rather_than_guessed() {
        // Each of these used to produce a plausible-looking wrong answer, and a wrong slug makes
        // both badge rules assertions about another project.
        for location in [
            "SPDX-PackageDownloadLocation = \"https://example.com/telekom/sutura\"",
            "SPDX-PackageDownloadLocation = \"https://github.com/telekom\"",
            "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura/tree/main\"",
            "SPDX-PackageName = \"sutura\"",
        ] {
            assert!(slug(location).is_none(), "{location} was turned into a slug");
        }
    }

    #[test]
    fn the_committed_tree_satisfies_every_rule_through_the_entry_point() {
        // THROUGH `problems`, not through its helpers: a test that re-implements the call site
        // passes over a `problems` that stopped making one of those calls. A falsifier has to call
        // the production entry point.
        use crate::repo;
        let root = repo::root().expect("repo root");
        let found = problems(&root);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn every_rule_is_reached_through_the_entry_point() {
        // One scratch tree, mutated one rule at a time, each read back through `problems`. This is
        // what makes a dropped call a failing test rather than an invisible hole.
        let scratch = std::env::temp_dir().join(format!("sutura-scorecard-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::create_dir_all(scratch.join("devco")).expect("the devco directory");
        let write = |path: &std::path::Path, text: &str| std::fs::write(path, text).expect("the scratch file");

        let readme_both = concat!(
            "<img src=\"https://api.reuse.software/badge/github.com/telekom/sutura\">\n",
            "<img src=\"https://api.scorecard.dev/projects/github.com/telekom/sutura/badge\">\n",
        );
        let publishing = "    with:\n      publish_results: true\n";

        // A REUSE.toml with the catch-all AND a narrowing block, a vendored tree under it, and a
        // VENDOR.md naming that tree - the shape `vendored_tree_problems` needs to be silent, so
        // every mutation below is the only difference. Its own rules have their own test.
        write(
            &scratch.join(PACKAGE),
            concat!(
                "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
                "[[annotations]]\npath = \"**\"\n",
                "[[annotations]]\npath = \"vendor/tree/**\"\n",
            ),
        );
        std::fs::create_dir_all(scratch.join("vendor/tree")).expect("the vendored tree");
        write(&scratch.join(VENDOR_RECORD), "| `vendor/tree/**` | upstream | MIT |\n");
        write(&scratch.join(RECORD), "# decided 2026-09-07\n");
        write(
            &scratch.join("flake.nix"),
            "        checks = {\n          reuse = licensing.check;\n        };\n",
        );
        write(
            &workflows.join("ci.yml"),
            "        run: nix build .#checks.x86_64-linux.reuse -L\n",
        );
        write(&scratch.join(README), readme_both);
        write(&scratch.join(WORKFLOW), publishing);
        assert!(problems(&scratch).is_empty(), "{:?}", problems(&scratch));

        // 1. The badge stays and the publication stops - a dead image asserting a score.
        write(&scratch.join(WORKFLOW), "    with:\n      publish_results: false\n");
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("would render nothing")), "{found:?}");

        // 2. The publication stays and the badge goes - a disclosure only YAML shows.
        write(&scratch.join(WORKFLOW), publishing);
        write(
            &scratch.join(README),
            "<img src=\"https://api.reuse.software/badge/github.com/telekom/sutura\">\n",
        );
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("visible only in YAML")), "{found:?}");

        // 3. A badge naming another project.
        write(
            &scratch.join(README),
            "<img src=\"https://api.scorecard.dev/projects/github.com/someone/else/badge\">\n",
        );
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("somebody else's score")), "{found:?}");

        // 4. Publishing with the decision unwritten.
        write(&scratch.join(README), readme_both);
        write(&scratch.join(RECORD), "   \n");
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("dated reason")), "{found:?}");
        std::fs::remove_file(scratch.join(RECORD)).expect("the record");
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("could not be read")), "{found:?}");
        write(&scratch.join(RECORD), "# decided 2026-09-07\n");

        // 5. The REUSE badge with no check declared.
        write(
            &scratch.join("flake.nix"),
            "        apps.reuse = { };\n        checks = {\n          fmt = f;\n        };\n",
        );
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("declares no `reuse`")), "{found:?}");

        // 6. The check declared and no CI file building it.
        write(
            &scratch.join("flake.nix"),
            "        checks = {\n          reuse = licensing.check;\n        };\n",
        );
        write(
            &workflows.join("ci.yml"),
            "        run: echo nothing builds the licence check\n",
        );
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("measures nothing")), "{found:?}");

        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn a_copied_tree_cannot_arrive_attributed_to_us() {
        // THROUGH `problems`, not through `vendored_tree_problems`: a falsifier has to call the
        // production entry point, or a `reuse_badge_problems` that stopped making the call would
        // pass this test.
        let scratch = std::env::temp_dir().join(format!("sutura-vendor-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::create_dir_all(scratch.join("vendor/tree")).expect("the vendored tree");
        let write = |path: &std::path::Path, text: &str| std::fs::write(path, text).expect("the scratch file");

        let catch_all_and_one_block = concat!(
            "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
            "[[annotations]]\npath = \"**\"\n",
            "[[annotations]]\npath = \"vendor/tree/**\"\n",
        );
        write(&scratch.join(PACKAGE), catch_all_and_one_block);
        write(&scratch.join(VENDOR_RECORD), "| `vendor/tree/**` | upstream | MIT |\n");
        write(
            &scratch.join(README),
            "<img src=\"https://api.reuse.software/badge/github.com/telekom/sutura\">\n",
        );
        write(
            &scratch.join("flake.nix"),
            "        checks = {\n          reuse = licensing.check;\n        };\n",
        );
        write(
            &workflows.join("ci.yml"),
            "        run: nix build .#checks.x86_64-linux.reuse -L\n",
        );
        assert!(problems(&scratch).is_empty(), "{:?}", problems(&scratch));

        // 1. A new copied tree with nothing narrowing it - the case `REUSE.toml`'s own header
        //    names, and the one it says is held by appending a block and nothing else.
        std::fs::create_dir_all(scratch.join("vendor/unnarrowed")).expect("the second tree");
        let found = problems(&scratch);
        assert!(
            found
                .iter()
                .any(|p| p.contains("vendor/unnarrowed") && p.contains("has no block COVERING it")),
            "{found:?}"
        );

        // 2. Recorded in prose and STILL unnarrowed: the two rules are independent, and only the
        //    licence one is about attribution.
        write(
            &scratch.join(VENDOR_RECORD),
            "| `vendor/tree/**` | upstream | MIT |\n| `vendor/unnarrowed/**` | upstream | MIT |\n",
        );
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("has no block COVERING it")), "{found:?}");
        assert!(!found.iter().any(|p| p.contains("does not name it")), "{found:?}");
        std::fs::remove_dir_all(scratch.join("vendor/unnarrowed")).expect("the second tree");
        write(&scratch.join(VENDOR_RECORD), "| `vendor/tree/**` | upstream | MIT |\n");

        // 3. THE OTHER DIRECTION: a block naming a tree that is gone permits nothing while keeping
        //    its dated paragraph. One-way, this is exactly what would have stayed green.
        write(
            &scratch.join(PACKAGE),
            concat!(
                "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
                "[[annotations]]\npath = \"**\"\n",
                "[[annotations]]\npath = \"vendor/tree/**\"\n",
                "[[annotations]]\npath = \"vendor/removed/**\"\n",
            ),
        );
        let found = problems(&scratch);
        assert!(
            found
                .iter()
                .any(|p| p.contains("vendor/removed") && p.contains("does not exist")),
            "{found:?}"
        );
        // AND IT NAMES THE LINE, because a refusal a reader cannot open is one they search for.
        // Line 7 of the text above: the fourth `path` value. An exact string, so an
        // off-by-one in the reader fails here - it did, on this test's first run, and the
        // ASSERTION was what was wrong rather than the rule.
        assert!(found.iter().any(|p| p.contains("REUSE.toml:7")), "{found:?}");

        // 4. A copied tree nobody recorded - `AGENTS.md`'s own rule about vendoring.
        write(&scratch.join(PACKAGE), catch_all_and_one_block);
        write(&scratch.join(VENDOR_RECORD), "nothing about any tree\n");
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("VENDOR.md does not name it")), "{found:?}");

        // 5. FAIL CLOSED ON EITHER INPUT. A `REUSE.toml` this reader gets nothing out of is the
        //    scanner broken rather than the tree clean, and an unreadable record is a fault rather
        //    than a smaller scan - the distinction only three of this repo's gates draw.
        write(&scratch.join(VENDOR_RECORD), "| `vendor/tree/**` | upstream | MIT |\n");
        write(
            &scratch.join(PACKAGE),
            "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
        );
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("yielded no narrowing")), "{found:?}");

        write(&scratch.join(PACKAGE), catch_all_and_one_block);
        std::fs::remove_file(scratch.join(VENDOR_RECORD)).expect("the record");
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("VENDOR.md could not be read")), "{found:?}");

        // 6. A COPIED FILE, NOT A TREE. The rule's sentence is about a copy, and the catch-all
        //    attributes a loose file exactly as it attributes a directory - so a subject set of
        //    "the children that are directories" is smaller than the claim over it. Measured
        //    before the fix: a header-less `.rs` dropped into `vendor/` left `just hygiene` at
        //    `ok - 33 gate(s)`, exit 0, while byte-identical content one directory deeper
        //    refused. This is the half `is_dir()` could not see.
        write(&scratch.join(VENDOR_RECORD), "| `vendor/tree/**` | upstream | MIT |\n");
        write(&scratch.join("vendor/upstream.rs"), "fn upstream() {}\n");
        let found = problems(&scratch);
        assert!(
            found
                .iter()
                .any(|p| p.contains("vendor/upstream.rs") && p.contains("has no block COVERING it")),
            "{found:?}"
        );
        // AND THE REMEDY IT PRINTS HAS TO BE THE ONE THAT WORKS: a file is narrowed by its own
        // path, so a remedy naming `vendor/upstream.rs/**` would send a reader to a block that
        // matches nothing.
        assert!(found.iter().any(|p| p.contains("exactly `vendor/upstream.rs`")), "{found:?}");
        // Narrowed by its exact path, it is answered - the file case is held in both directions.
        write(
            &scratch.join(PACKAGE),
            concat!(
                "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
                "[[annotations]]\npath = \"**\"\n",
                "[[annotations]]\npath = \"vendor/tree/**\"\n",
                "[[annotations]]\npath = \"vendor/upstream.rs\"\n",
            ),
        );
        write(
            &scratch.join(VENDOR_RECORD),
            "| `vendor/tree/**` | upstream | MIT |\n| `vendor/upstream.rs` | upstream | MIT |\n",
        );
        let found = problems(&scratch);
        assert!(!found.iter().any(|p| p.contains("vendor/upstream.rs")), "{found:?}");
        std::fs::remove_file(scratch.join("vendor/upstream.rs")).expect("the copied file");
        write(&scratch.join(PACKAGE), catch_all_and_one_block);

        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn a_reuse_badge_naming_another_project_is_refused() {
        // BLOCKING REVIEW FINDING: this refusal was held by NOTHING. Deleting it outright left
        // 1149 xtask tests all passing and `check-workflows` at exit 0 - predicate tested, refusal
        // untested, one missing case rather than a systemic gap. Its four neighbours each redden a
        // test under the same treatment.
        let scratch = std::env::temp_dir().join(format!("sutura-reuse-slug-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::create_dir_all(scratch.join("vendor/tree")).expect("the vendored tree");
        let write = |path: &std::path::Path, text: &str| std::fs::write(path, text).expect("the scratch file");

        write(
            &scratch.join(PACKAGE),
            concat!(
                "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
                "[[annotations]]\npath = \"**\"\n",
                "[[annotations]]\npath = \"vendor/tree/**\"\n",
            ),
        );
        write(&scratch.join(VENDOR_RECORD), "| `vendor/tree/**` | upstream | MIT |\n");
        write(
            &scratch.join("flake.nix"),
            "        checks = {\n          reuse = licensing.check;\n        };\n",
        );
        write(
            &workflows.join("ci.yml"),
            "        run: nix build .#checks.x86_64-linux.reuse -L\n",
        );

        // The honest badge is silent - through `problems`, not through the helper.
        write(
            &scratch.join(README),
            "<img src=\"https://api.reuse.software/badge/github.com/telekom/sutura\">\n",
        );
        assert!(problems(&scratch).is_empty(), "{:?}", problems(&scratch));

        // Another project's badge under this name would report their compliance as ours.
        write(
            &scratch.join(README),
            "<img src=\"https://api.reuse.software/badge/github.com/someone/else\">\n",
        );
        let found = problems(&scratch);
        assert!(
            found.iter().any(|p| p.contains("REUSE badge") && p.contains("does not name")),
            "{found:?}"
        );
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn a_block_under_a_copied_tree_does_not_narrow_the_whole_of_it() {
        // BLOCKING REVIEW FINDING, and the one that shipped a false public claim: `starts_with`
        // let a block for ONE FILE inside a copied tree mark the whole tree narrowed, so its
        // siblings fell through to the catch-all and resolved to us - green at the gate, green at
        // `reuse lint`, green on the badge.
        let scratch = std::env::temp_dir().join(format!("sutura-cover-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::create_dir_all(scratch.join("vendor/newtree")).expect("the copied tree");
        let write = |path: &std::path::Path, text: &str| std::fs::write(path, text).expect("the scratch file");
        write(&scratch.join("vendor/newtree/a.c"), "int a;\n");
        write(&scratch.join("vendor/newtree/b.c"), "int b;\n");
        write(&scratch.join(VENDOR_RECORD), "| `vendor/newtree/**` | upstream | MIT |\n");
        write(
            &scratch.join(README),
            "<img src=\"https://api.reuse.software/badge/github.com/telekom/sutura\">\n",
        );
        write(
            &scratch.join("flake.nix"),
            "        checks = {\n          reuse = licensing.check;\n        };\n",
        );
        write(
            &workflows.join("ci.yml"),
            "        run: nix build .#checks.x86_64-linux.reuse -L\n",
        );

        // A block for ONE FILE inside the tree: refused, because it covers `a.c` and nothing else.
        write(
            &scratch.join(PACKAGE),
            concat!(
                "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
                "[[annotations]]\npath = \"**\"\n",
                "[[annotations]]\npath = \"vendor/newtree/a.c\"\n",
            ),
        );
        let found = problems(&scratch);
        assert!(
            found
                .iter()
                .any(|p| p.contains("vendor/newtree") && p.contains("has no block COVERING it")),
            "{found:?}"
        );

        // The child's own subtree block covers all of it. This is the shape the real tree uses.
        write(
            &scratch.join(PACKAGE),
            concat!(
                "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
                "[[annotations]]\npath = \"**\"\n",
                "[[annotations]]\npath = \"vendor/newtree/**\"\n",
            ),
        );
        assert!(problems(&scratch).is_empty(), "{:?}", problems(&scratch));
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn every_spelling_of_a_true_publish_value_publishes() {
        // BLOCKING REVIEW FINDING: the rule matched the literal `publish_results: true`, so a
        // quoting choice bypassed BOTH the badge rule and the record requirement while the action
        // published anyway - GitHub serialises a `with:` value to a string.
        for spelling in ["true", "'true'", "\"true\"", "True", "TRUE", "yes", "${{ vars.X }}"] {
            let text = format!("    with:\n      publish_results: {spelling}\n");
            assert!(publishes(&text), "{spelling} read as not publishing");
        }
        // Unambiguously false, in every quoting, is the one value that does not publish.
        for spelling in ["false", "'false'", "\"false\"", "False", "FALSE"] {
            let text = format!("    with:\n      publish_results: {spelling}\n");
            assert!(!publishes(&text), "{spelling} read as publishing");
        }
        // An ABSENT key is not publishing: the action's own default is false.
        assert!(!publishes("    with:\n      results_format: json\n"));
        // A different key that merely starts with the same word is not this input.
        assert!(!publishes("    with:\n      publish_results_dir: /tmp\n"));
    }

    #[test]
    fn a_quoted_publish_value_cannot_slip_past_the_badge_rule() {
        // THE SAME TRAP ONE LEVEL UP, and my own falsifier caught it: the test above calls
        // `publishes` directly, so reverting `problems` to the literal substring match left all
        // nine tests green. A parser held by a test whose CALLER is untested is a predicate with
        // no refusal behind it. This one goes through the production entry point, so the bypass -
        // a publishing workflow with no badge and no record - has to be refused.
        let scratch = std::env::temp_dir().join(format!("sutura-quoted-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        let write = |path: &std::path::Path, text: &str| std::fs::write(path, text).expect("the scratch file");
        write(
            &scratch.join(PACKAGE),
            "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
        );
        // NO badge and NO record: the state the bypass produced, where a score is live at
        // api.scorecard.dev with nothing in the README and nothing dated.
        write(&scratch.join(README), "no badges here\n");
        for spelling in ["'true'", "\"true\"", "True"] {
            write(
                &scratch.join(WORKFLOW),
                &format!("    with:\n      publish_results: {spelling}\n"),
            );
            let found = problems(&scratch);
            assert!(
                found.iter().any(|p| p.contains("visible only in YAML")),
                "{spelling} slipped past the badge rule: {found:?}"
            );
            assert!(
                found.iter().any(|p| p.contains("could not be read")),
                "{spelling} slipped past the record requirement: {found:?}"
            );
        }
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn explaining_the_input_in_a_comment_is_not_setting_it() {
        // The workflow's header argues at length about what `publish_results: true` sends, and a
        // rule that read its own explanation would make the header unwritable.
        let scratch = std::env::temp_dir().join(format!("sutura-scorecard-cmt-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        let write = |path: &std::path::Path, text: &str| std::fs::write(path, text).expect("the scratch file");
        write(
            &scratch.join(PACKAGE),
            "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
        );
        write(&scratch.join(README), "no badges here\n");
        write(
            &scratch.join(WORKFLOW),
            "# publish_results: true is what a badge would need\n  # publish_results: true\n",
        );
        // No badge and no publication: the two agree, so the pair of rules is silent.
        assert!(problems(&scratch).is_empty(), "{:?}", problems(&scratch));
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }
}
