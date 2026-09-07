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
//!   not a statement that the declarations are correct.
//! * **Not that the workflow ever runs, or that anyone looks.** It reports no status context on a
//!   pull request and the only required context is `ci`. A falling score blocks nothing.
//! * **Not what the third party does with what it receives.** The record is a dated reading of the
//!   action's documentation, not a trace.

use std::path::Path;

use super::sources;

/// The workflow that runs and publishes the scan.
const WORKFLOW: &str = ".github/workflows/scorecard.yml";

/// The dated decision behind the publication.
const RECORD: &str = "devco/scorecard-publication";

/// The page the badges are on.
const README: &str = "README.md";

/// Where the repository's own identity is declared, so a badge slug has one authority.
const PACKAGE: &str = "REUSE.toml";

/// The key that declares it there.
const DOWNLOAD_LOCATION: &str = "SPDX-PackageDownloadLocation";

/// The action input that sends a score to `api.scorecard.dev`.
const PUBLISH: &str = "publish_results: true";

/// The badge image host and path, without the slug.
const SCORECARD_BADGE: &str = "https://api.scorecard.dev/projects/";

/// The REUSE badge image host and path, without the slug.
const REUSE_BADGE: &str = "https://api.reuse.software/badge/";

/// The flake check the REUSE badge stands on, as `flake.nix` declares it.
const CHECK: &str = "reuse";

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
    let publishes = uncommented(&workflow).any(|line| line.contains(PUBLISH));
    let badged = readme.contains(SCORECARD_BADGE);

    // BOTH DIRECTIONS, and only one of them is obvious. A badge with publication off is a dead
    // image; publication with no badge is a disclosure no reader of the README can see.
    if badged && !publishes {
        problems.push(format!(
            "{README} shows the OpenSSF Scorecard badge and {WORKFLOW} does not set `{PUBLISH}` - the badge is served from published results, so it would render nothing while asserting a score"
        ));
    }
    if publishes && !badged {
        problems.push(format!(
            "{WORKFLOW} sets `{PUBLISH}`, which sends this repository's score to the public api.scorecard.dev, and {README} shows no badge - the disclosure would be visible only in YAML"
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
        problems.extend(reuse_badge_problems(root, &readme, &slug));
    }

    problems
}

/// What the REUSE badge stands on: a declared check, and a CI file that actually builds it.
///
/// Two reads rather than one, because a declaration nothing invokes is the failure this rule
/// exists for - the same distinction `crate::venues` draws between a venue existing and CI
/// reaching it.
fn reuse_badge_problems(root: &Path, readme: &str, slug: &str) -> Vec<String> {
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

    problems
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

/// Every line that is not wholly a comment.
///
/// Whole lines only, deliberately. Stripping from the first `#` would also cut the `# v7.0.1`
/// that names the version behind every pinned action SHA, and the needle here is a key.
fn uncommented(text: &str) -> impl Iterator<Item = &str> {
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

        write(
            &scratch.join(PACKAGE),
            "SPDX-PackageDownloadLocation = \"https://github.com/telekom/sutura\"\n",
        );
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
