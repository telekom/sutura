//! What an `OpenSSF` Scorecard run may send, and to whom.
//!
//! Scorecard measures a repository's security posture, and measuring it is not free of
//! consequence: three of its checks answer their question by asking a service outside GitHub, and
//! `ossf/scorecard-action` can PUBLISH the resulting score to a public `OpenSSF` endpoint. This
//! repository is private as read on 2026-09-07 (`gh api repos/telekom/sutura` -> `private: true`),
//! so either of those is a disclosure about a tree nobody outside has.
//!
//! **A RULE OF `check-workflows` rather than a gate of its own**, and not only for tidiness:
//! `xtask/src/main.rs` stands at 999 lines against the 1000-line cap `max-lines` enforces and
//! `crates/` and `xtask/` are the two prefixes that cap cannot be exempted for - so a new entry in
//! the task table is a gate nobody can add until that file is split. It belongs here anyway. The
//! question *what may a workflow do* is `check-workflows`', it already walks the three places CI
//! invokes something from, and a second gate over the same walk would be a second answer.
//!
//! **The point of these rules is that neither can start happening through a YAML edit.** Not because
//! publishing is wrong - it may well be what this project wants once it is public - but because it
//! is a decision, and a decision that lives in a comment is a decision nobody made.
//! `docs/adr/0024` is where it is written down.
//!
//! # What it holds
//!
//! * **One list.** `.github/workflows/scorecard.yml` derives its `--checks=` argument from
//!   `devco/scorecard-checks` rather than spelling a second copy. A literal there is refused - a
//!   second list of check names is exactly the drift this repository already has gates about.
//! * **The record permits nothing on its own.** An unreadable or empty record FAILS, rather than
//!   being read as "no restriction": `scorecard` with an empty `--checks` runs its whole suite,
//!   including the three below, so a fail-open here would be the disclosure it exists to prevent.
//! * **No check that phones home**, by name, with the endpoint beside it in [`PHONES_HOME`].
//!   Adding one is then an edit to this file as well as to the record.
//! * **Nothing publishes.** No workflow, composite action or CI shell may use
//!   `ossf/scorecard-action` or set `publish_results` - the two ways a score reaches
//!   `api.scorecard.dev`. Read over the same three places [`super::sources`] walks, so
//!   a step that moves out of a workflow does not move out of this gate's sight.
//!
//! # What it does NOT hold, and each is real
//!
//! * **Not that a check's behaviour matches [`PHONES_HOME`].** That mapping is the tool's, at the
//!   version `flake.nix` pins, and this is a dated reading of its documentation rather than a
//!   trace of a run. A check that starts querying a third party keeps passing here.
//! * **Not that the record's names are checks the tool has.** Spelling is the tool's authority and
//!   a misspelling is an error from `scorecard` at run time; asking the binary would need it in
//!   the hygiene sandbox, which has no network and no Go toolchain.
//! * **Not that the workflow RUNS.** It reports no status context on a pull request - it
//!   deliberately does not run on one - and the only required context is `ci`. So this gate holds
//!   what the workflow may do, and nothing holds that a falling score is ever looked at.
//! * **Not the outbound traffic itself.** A check could reach a network endpoint this reader
//!   cannot see. The mechanism is a name list, not a sandbox.

use std::collections::BTreeSet;
use std::path::Path;

use super::sources;

/// The record: one check per line, comments and blanks ignored.
const RECORD: &str = "devco/scorecard-checks";

/// The workflow that runs it.
const WORKFLOW: &str = ".github/workflows/scorecard.yml";

/// The flake app the workflow must reach the tool through, so nix stays its only pin.
const INVOCATION: &str = "nix run .#scorecard";

/// Every check that answers its question by asking somebody other than GitHub, and who that is.
///
/// A pair rather than a bare name, because a refusal that does not say WHERE the data goes is a
/// refusal the reader has to research before they can agree with it.
const PHONES_HOME: &[(&str, &str)] = &[
    (
        "Vulnerabilities",
        "OSV, at osv.dev - and it sends the dependency set, not only the repository name",
    ),
    (
        "CII-Best-Practices",
        "the OpenSSF Best Practices badge API, at bestpractices.dev",
    ),
    ("Fuzzing", "the OSS-Fuzz project list"),
];

/// The action whose `publish_results` input sends a score to `api.scorecard.dev`.
const ACTION: &str = "ossf/scorecard-action";

/// That input, refused separately: the action is one way to set it and not the only one.
const PUBLISH: &str = "publish_results";

/// Every way this tree could disclose more than it has decided to.
///
/// A `Vec` rather than a `Verdict`, like [`super::contexts::problems`]: one gate prints one
/// verdict, and a rule that owns its own exit code is a rule whose venue nobody can see.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();

    // THE RECORD, and an unreadable one is a failure rather than a smaller scan. An empty
    // `--checks` runs the whole suite, so "could not read the list" and "the list is empty" both
    // mean the same thing here: this gate does not know what a run will check.
    let record = match std::fs::read_to_string(root.join(RECORD)) {
        Ok(text) => text,
        Err(error) => {
            problems.push(format!(
                "could not read {RECORD}: {error} - it is the only list of what a run may check, and without it an invocation falls back to the whole suite, which reaches osv.dev, bestpractices.dev and the OSS-Fuzz project list"
            ));
            return problems;
        }
    };
    let checks = recorded(&record);
    if checks.is_empty() {
        problems.push(format!(
            "{RECORD} names no check - an empty list is not a permissive one, it is a run that checks everything"
        ));
    }
    problems.extend(phoning_home(&checks));

    // THE WORKFLOW. Read directly rather than through the walk below, because its absence is a
    // finding: the record would then describe a run nothing performs.
    match std::fs::read_to_string(root.join(WORKFLOW)) {
        Ok(text) => problems.extend(workflow_problems(&text)),
        Err(error) => problems.push(format!("could not read {WORKFLOW}: {error}")),
    }

    // PUBLISHING, over every file CI invokes something from. The SAME walk the reference scan and
    // the context classification use, so a step that moves into a composite action or into
    // `nix/*.sh` stays in view - that exact move has already taken five references out of one
    // gate's sight.
    match sources::ci_sources(root) {
        Some(sources) => {
            for source in &sources {
                problems.extend(publishing(&source.label, &source.text));
            }
        }
        None => problems.push(String::from(
            "the CI sources could not be walked, so nothing was read for `publish_results`",
        )),
    }

    problems
}

/// The checks the record names: every line that is neither blank nor a comment.
fn recorded(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// One problem per recorded check that reaches a service outside GitHub.
fn phoning_home(checks: &[&str]) -> Vec<String> {
    let named: BTreeSet<&str> = checks.iter().copied().collect();
    PHONES_HOME
        .iter()
        .filter(|(check, _)| named.contains(check))
        .map(|(check, endpoint)| {
            format!("{RECORD} names `{check}`, which queries {endpoint} - so a run discloses this repository to a third party")
        })
        .collect()
}

/// What the workflow must and must not say.
///
/// Comment lines are skipped for the `--checks` rule, so the file may EXPLAIN the literal it
/// refuses without tripping over its own explanation - the same distinction
/// `workflows::collect`'s `a_comment_is_not_a_reference` draws.
fn workflow_problems(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let code: Vec<&str> = uncommented(text).collect();

    if !code.iter().any(|line| line.contains(INVOCATION)) {
        problems.push(format!(
            "{WORKFLOW} does not run `{INVOCATION}` - nix is the only pin for a tool whose version decides what it reports"
        ));
    }
    if !code.iter().any(|line| line.contains(RECORD)) {
        problems.push(format!(
            "{WORKFLOW} never reads {RECORD} - the check set has to come from the record, not from this file"
        ));
    }
    // `split` rather than index arithmetic: `clippy::string_slice` refuses the latter, and it is
    // right to - a byte offset into text this gate does not control can land mid-character.
    for line in &code {
        for rest in line.split("--checks=").skip(1) {
            if !derives_from_the_record(rest) {
                problems.push(format!(
                    "{WORKFLOW} spells a literal check set (`--checks={}`) - it must expand the value read from {RECORD}, so there is one list rather than two",
                    rest.split_whitespace().next().unwrap_or(rest)
                ));
            }
        }
    }
    problems
}

/// Is this `--checks=` value the shell expansion of what the record was read into?
///
/// Both quoted and bare, because the workflow may legitimately write either and the rule is about
/// where the value CAME FROM rather than about quoting.
fn derives_from_the_record(rest: &str) -> bool {
    rest.starts_with("\"$checks\"") || rest.starts_with("$checks") || rest.starts_with("\"${checks}\"")
}

/// One problem per way this file could send a score to `api.scorecard.dev`.
fn publishing(label: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for line in uncommented(text) {
        if line.contains(ACTION) {
            problems.push(format!(
                "{label} uses `{ACTION}` - it is a `using: docker` action whose image is a mutable registry tag, so a SHA in `uses:` pins the manifest and not the code, and its `{PUBLISH}` input sends the score to api.scorecard.dev"
            ));
        }
        if line.contains(PUBLISH) {
            problems.push(format!(
                "{label} sets `{PUBLISH}` - publishing sends this repository's score, every check's reason and the commit to api.scorecard.dev, a public OpenSSF endpoint, and nothing here has decided to"
            ));
        }
    }
    problems
}

/// Every line that is not wholly a comment.
///
/// Whole lines only, deliberately. Stripping from the first `#` would also cut the `# v7.0.1`
/// that names the version behind every pinned action SHA, and this gate has no rule that a
/// trailing comment could hide from: both needles it looks for are keys and `uses:` values.
fn uncommented(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.trim_start().starts_with('#'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_or_a_blank_line_records_no_check() {
        // The record is mostly argument, so this is the rule that keeps its prose out of the
        // `--checks` value the workflow builds from it.
        let checks = recorded("# why\n\n  # indented\nLicense\n  SAST  \n");
        assert_eq!(checks, vec!["License", "SAST"]);
    }

    #[test]
    fn a_check_that_queries_a_third_party_is_refused_by_name() {
        let problems = phoning_home(&["License", "Vulnerabilities"]);
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].contains("osv.dev"),
            "the refusal must name the endpoint: {}",
            problems[0]
        );
    }

    #[test]
    fn every_phoning_check_is_refused_and_an_ordinary_one_is_not() {
        // Each entry, so adding one to the constant without meaning it is visible here.
        for (check, _) in PHONES_HOME {
            assert_eq!(phoning_home(&[check]).len(), 1, "{check} was not refused");
        }
        assert!(phoning_home(&["Token-Permissions", "SBOM"]).is_empty());
    }

    #[test]
    fn a_literal_check_set_in_the_workflow_is_refused() {
        let text = concat!(
            "jobs:\n",
            "  score:\n",
            "    steps:\n",
            "      - run: |\n",
            "          checks=$(cat devco/scorecard-checks)\n",
            "          nix run .#scorecard -- --checks=License,SAST\n",
        );
        let problems = workflow_problems(text);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("literal check set"), "{}", problems[0]);
    }

    #[test]
    fn the_expansion_of_the_record_is_accepted_quoted_or_bare() {
        for form in ["\"$checks\"", "$checks", "\"${checks}\""] {
            let text = format!(
                "      - run: |\n          checks=$(cat devco/scorecard-checks)\n          nix run .#scorecard -- --checks={form} --format=default\n"
            );
            assert!(workflow_problems(&text).is_empty(), "{form} was refused");
        }
    }

    #[test]
    fn a_workflow_that_never_reads_the_record_is_refused() {
        // The failure this rule exists for is a second list, which passes every other rule here.
        let text = "      - run: nix run .#scorecard -- --checks=$checks\n";
        let problems = workflow_problems(text);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("never reads"), "{}", problems[0]);
    }

    #[test]
    fn a_workflow_that_reaches_the_tool_some_other_way_is_refused() {
        let text = "      - run: |\n          cat devco/scorecard-checks\n          scorecard --checks=$checks\n";
        let problems = workflow_problems(text);
        assert!(problems.iter().any(|p| p.contains("nix run .#scorecard")), "{problems:?}");
    }

    #[test]
    fn the_action_and_the_publish_input_are_both_refused() {
        let action = publishing("scorecard.yml", "      - uses: ossf/scorecard-action@2d1146\n");
        assert_eq!(action.len(), 1, "{action:?}");
        assert!(action[0].contains("mutable registry tag"), "{}", action[0]);

        let publish = publishing("scorecard.yml", "          publish_results: true\n");
        assert_eq!(publish.len(), 1, "{publish:?}");
        assert!(publish[0].contains("api.scorecard.dev"), "{}", publish[0]);
    }

    #[test]
    fn explaining_the_action_in_a_comment_is_not_using_it() {
        // The workflow's own header argues at length about why `ossf/scorecard-action` was not
        // taken, and a gate that failed on its own explanation would be deleted rather than
        // obeyed. The same distinction `workflows::collect` draws for `nix run .#` references.
        let text = concat!(
            "# WHY NOT ossf/scorecard-action: its publish_results input sends the score away.\n",
            "  # publish_results: true would be the line\n",
            "jobs:\n",
        );
        assert!(publishing("scorecard.yml", text).is_empty());
    }

    #[test]
    fn the_committed_tree_satisfies_every_rule_through_the_entry_point() {
        // THROUGH `problems`, not through its helpers. The first version of this test called
        // `recorded`, `phoning_home`, `workflow_problems` and `publishing` in turn - which
        // re-implements the call site, so a `problems` that stopped making one of those calls
        // would have passed it. A falsifier has to call the production entry point.
        use crate::repo;
        let root = repo::root().expect("repo root");
        let found = problems(&root);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn every_rule_is_reached_through_the_entry_point() {
        // One scratch tree, mutated four ways, each read back through `problems`. This is what
        // makes a dropped `problems.extend(...)` line a failing test rather than an invisible
        // hole: neutralising any one leg leaves the corresponding assertion below unmet.
        let scratch = std::env::temp_dir().join(format!("sutura-scorecard-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::create_dir_all(scratch.join("devco")).expect("the devco directory");

        let compliant = concat!(
            "      - run: |\n",
            "          checks=$(grep -v '^#' devco/scorecard-checks | paste -sd, -)\n",
            "          nix run .#scorecard -- --checks=\"$checks\" --format=default\n",
        );
        let record = scratch.join(RECORD);
        let workflow = scratch.join(WORKFLOW);
        let write = |path: &std::path::Path, text: &str| std::fs::write(path, text).expect("the scratch file");

        // A tree that satisfies every rule, so each mutation below is the only difference.
        write(&record, "# prose\nLicense\nSAST\n");
        write(&workflow, compliant);
        assert!(problems(&scratch).is_empty(), "{:?}", problems(&scratch));

        // 1. A check that queries a third party.
        write(&record, "License\nFuzzing\n");
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("OSS-Fuzz")), "{found:?}");

        // 2. A record of pure prose, which would otherwise run the whole suite.
        write(&record, "# every line a comment\n\n");
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("names no check")), "{found:?}");

        // 3. A second list of check names, spelled in the workflow.
        write(&record, "License\n");
        write(
            &workflow,
            "      - run: |\n          cat devco/scorecard-checks\n          nix run .#scorecard -- --checks=License,SAST\n",
        );
        let found = problems(&scratch);
        assert!(found.iter().any(|p| p.contains("literal check set")), "{found:?}");

        // 4. `publish_results` in a DIFFERENT CI source, which is what makes the walk the rule
        //    rather than the one file: a step that moves out of `scorecard.yml` stays in view.
        write(&workflow, compliant);
        write(
            &workflows.join("elsewhere.yml"),
            "jobs:\n  x:\n    steps:\n      - with:\n          publish_results: true\n",
        );
        let found = problems(&scratch);
        assert!(
            found
                .iter()
                .any(|p| p.contains("publish_results") && p.contains("public OpenSSF endpoint")),
            "{found:?}"
        );

        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }
}
