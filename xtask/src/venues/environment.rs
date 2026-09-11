//! The acceptance venue's ENVIRONMENT contract: three lists of names, reconciled.
//!
//! The `bigquery-acceptance` job is pointed at a GitHub environment, and what that environment
//! carries was described in three places with nothing reconciling any two of them:
//!
//! 1. what `test-infra/pulumi/google/sync-bq-test-env.sh` pushes;
//! 2. what `test-infra/README.md`'s table says the environment carries;
//! 3. what a workflow reads as `vars.SUTURA_BQ_*`, `secrets.SVC_*` or `secrets.SUTURA_BQ_*`.
//!
//! `crate::venues::acceptance` reads (3) for the properties that matter at the seam - the fork
//! rule, the credential write and removal, an emptiness guard per value, and what may reach a
//! public log. What nothing read is whether a value the job is pointed at is one the environment
//! can be MADE to carry.
//!
//! **The instance, measured on 2026-09-05:** the sync script pushes ten `vars` and the README's
//! table listed five, three commits after the other five were added. So a reader of the table
//! could not tell that an `infra-set` predating them leaves the environment incomplete - and the
//! failure that follows is a RED `bigquery-acceptance` on every push rather than a skip, because
//! the job fails closed on an unset value deliberately. The near-miss is worse than the drift: a
//! name misspelled in (1) or renamed in (3) is a job that fails closed forever, with two pages
//! agreeing and the environment silently missing one key.
//!
//! # What it does NOT reach
//!
//! **Whether the environment is actually provisioned.** That authority is the GitHub API, which is
//! unreachable from the sandbox this gate runs in - the same limit `crate::venues`' invocation rule
//! states about itself. So what is held is that the three descriptions agree, never that somebody
//! ran `just infra-up` and `just infra-set`.
//!
//! **Whether a pushed name is READ by the job.** One-sided on purpose: a value the environment
//! carries and no job consumes costs nothing, and the direction that breaks a run is the other one.
//! It is also the direction a consumer lands in a later change than the value it consumes, so
//! gating it would fail the tree between the two.
//!
//! **A name assembled from parts.** `vars.SUTURA_BQ_${{ ... }}` is not a name and is not read; a
//! literal is. Nothing in these workflows writes that shape today.

use std::collections::BTreeSet;
use std::path::Path;

use crate::repo;

/// The script that pushes the environment. The authority for WHICH names exist.
const SYNC: &str = "test-infra/pulumi/google/sync-bq-test-env.sh";

/// The page whose table has to be that same list.
const PAGE: &str = "test-infra/README.md";

/// The two kinds, each with the python dict in [`SYNC`] that declares it and the row in [`PAGE`]
/// that has to match.
///
/// A table rather than two copies of one function: the rule is identical and the only difference
/// is where each side is written.
const KINDS: [Kind; 2] = [
    Kind {
        name: "secrets",
        dict: "SECRETS = {",
        row: "| `secrets` |",
        // TWO, because this environment's secrets are not only keys: the identity values the
        // two-principal cell is pointed at carry the `SUTURA_BQ_` prefix and moved here from
        // `vars`. With `SVC_` alone a `secrets.SUTURA_BQ_<typo>` read was compared against
        // nothing, which is the job that fails closed forever with both pages agreeing.
        prefixes: &["SVC_", "SUTURA_BQ_"],
    },
    Kind {
        name: "vars",
        dict: "VARS = {",
        row: "| `vars` |",
        prefixes: &["SUTURA_BQ_"],
    },
];

/// One kind of environment entry.
struct Kind {
    /// What GitHub calls it, and what the report says.
    name: &'static str,
    /// The line in [`SYNC`] that opens the dict of `"<pushed name>": "<stack output>"` pairs.
    dict: &'static str,
    /// The row in [`PAGE`]'s table that must hold exactly those names.
    row: &'static str,
    /// The prefixes that make a workflow reference one of THIS stack's values rather than one of
    /// GitHub's own. Without them `secrets.GITHUB_TOKEN` would be reported as unprovisioned.
    prefixes: &'static [&'static str],
}

/// What reconciling the three lists found.
///
/// A named struct rather than a tuple for the reason `crate::shipped::Reconciliation` gives: two
/// values side by side say nothing about which is the failure and which is the evidence that
/// anything was compared. `names` is what keeps the verdict from reading as coverage over an empty
/// set, and it is derived on every run so there is no figure to keep true.
pub(super) struct Contract {
    /// Disagreements between the three lists. Empty means they agree.
    pub(super) problems: Vec<String>,
    /// How many names were reconciled, across both kinds.
    pub(super) names: usize,
}

impl Contract {
    /// A verdict that compared nothing, with the sentence saying so.
    fn broken(why: String) -> Self {
        Self {
            problems: vec![why],
            names: 0,
        }
    }
}

/// Every problem across the three lists, and how many names were reconciled.
pub(super) fn problems(root: &Path) -> Contract {
    let Ok(script) = std::fs::read_to_string(root.join(SYNC)) else {
        return Contract::broken(format!(
            "{SYNC} is not readable - it IS the list of names the environment can carry"
        ));
    };
    let Ok(page) = std::fs::read_to_string(root.join(PAGE)) else {
        return Contract::broken(format!("{PAGE} is not readable"));
    };
    let (workflows, unreadable) = workflow_text(root);
    if workflows.is_empty() {
        return Contract::broken(String::from(
            "no workflow under .github/workflows could be read - the scan is broken, not the contract",
        ));
    }

    let mut problems = Vec::new();
    let mut names = 0_usize;
    // ONE unreadable file, not all of them. The floor above fails closed only when EVERY workflow
    // is unreadable, and a single dropped file is a `vars.<PREFIX>_...` read this contract never
    // compared - reported in review. Same shape as the unreadable page `check-docs` dropped in
    // silence, and `crate::hook_coverage` had it right: an unreadable input is a recorded failure.
    problems.extend(unreadable);
    for kind in &KINDS {
        let pushed = pushed_names(&script, kind.dict);
        if pushed.is_empty() {
            // FAIL CLOSED. An empty set would make both rules below pass by comparing nothing,
            // which is the shape a text scan fails in: the reader stopped matching the script.
            problems.push(format!(
                "no `{}` name parsed out of {SYNC}'s `{}` - the reader stopped matching it",
                kind.name, kind.dict
            ));
            continue;
        }
        names = names.saturating_add(pushed.len());
        let documented = row_names(&page, kind.row);
        if documented.is_empty() {
            problems.push(format!(
                "{PAGE} has no `{}` row, so nothing states what the environment carries",
                kind.name
            ));
        }
        for name in pushed.difference(&documented) {
            problems.push(format!(
                "{SYNC} pushes `{name}` and {PAGE}'s `{}` row does not list it",
                kind.name
            ));
        }
        for name in documented.difference(&pushed) {
            problems.push(format!(
                "{PAGE}'s `{}` row lists `{name}` and {SYNC} pushes no such name",
                kind.name
            ));
        }
        for name in referenced(&workflows, kind.name, kind.prefixes).difference(&pushed) {
            problems.push(format!(
                "a workflow reads `{}.{name}` and {SYNC} pushes no such name",
                kind.name
            ));
        }
    }
    Contract { problems, names }
}

/// The keys of one python dict in the sync script, which are the names it pushes.
///
/// The dict body is the lines between the opening and the closing brace, and a key is the first
/// quoted token on a line. Comment lines are skipped, which is load-bearing: the script explains
/// the second group of values in a paragraph inside the dict, and that paragraph quotes names.
fn pushed_names(script: &str, opener: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut inside = false;
    for line in script.lines() {
        let trimmed = line.trim();
        if !inside {
            inside = trimmed.starts_with(opener);
            continue;
        }
        if trimmed.starts_with('}') {
            break;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(name) = quoted(trimmed) {
            names.insert(name);
        }
    }
    names
}

/// The first double-quoted token on a line, if it is there and non-empty.
fn quoted(line: &str) -> Option<String> {
    let after = line.split_once('"')?.1;
    let (name, _) = after.split_once('"')?;
    (!name.is_empty()).then(|| String::from(name))
}

/// The backticked names in one row of the page's table.
///
/// The row IS the list, so both directions are compared against it. A name is read out of a
/// backtick span rather than out of the prose beside it: the row explains what the CI service
/// account key is for, and that sentence is not a name.
fn row_names(page: &str, row: &str) -> BTreeSet<String> {
    page.lines()
        .filter(|line| line.trim_start().starts_with(row))
        .flat_map(|line| line.split('`').skip(1).step_by(2))
        .filter(|span| span.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        .map(String::from)
        .collect()
}

/// Every `<kind>.<PREFIX>...` name a workflow reads, for any of that kind's prefixes.
fn referenced(text: &str, kind: &str, prefixes: &[&str]) -> BTreeSet<String> {
    let needle = format!("{kind}.");
    let mut names = BTreeSet::new();
    for chunk in text.split(needle.as_str()).skip(1) {
        let name: String = chunk.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        // GitHub matches a secret or variable name case-insensitively, and this repository writes
        // one of them lower-case in the workflow and upper-case in the script - so a case-sensitive
        // comparison would report a correct reference as unprovisioned.
        let name = name.to_uppercase();
        if prefixes.iter().any(|prefix| name.starts_with(prefix)) {
            names.insert(name);
        }
    }
    names
}

/// Every workflow, concatenated, and one sentence per file that could not be read.
///
/// What is asked of the text is which NAMES appear, so which file each is in adds nothing a reader
/// needs - and `crate::venues::acceptance` is what reads the job's shape. **What a reader does need
/// is which file was not read at all**, because the answer this scan gives is over the tree and a
/// dropped file makes it over a subset.
fn workflow_text(root: &Path) -> (String, Vec<String>) {
    let mut unreadable = Vec::new();
    // The walk's own finding lands in the channel this function already publishes, rather than
    // being dropped: an unreachable workflow directory makes the answer below over a subset.
    let mut paths = match repo::collect_files(root, &root.join(".github/workflows"), &["yml", "yaml"])
        .into_listing(repo::Unmigrated::Venues)
    {
        Ok((_root, found)) => found,
        Err(why) => {
            unreadable.push(why.describe());
            Vec::new()
        }
    };
    paths.sort();
    let mut text = Vec::new();
    for rel in &paths {
        match std::fs::read_to_string(root.join(rel)) {
            Ok(read) => text.push(read),
            Err(error) => unreadable.push(format!(
                "{rel} could not be read, so any name it reads was not compared: {error}"
            )),
        }
    }
    (text.join("\n"), unreadable)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// The sync script's shape, with the comment that quotes names inside the dict - which is what
    /// forced the comment skip.
    const SCRIPT: &str = concat!(
        "SECRETS = {\n",
        "    \"SVC_KEY_CI\": \"ci_key\",\n",
        "}\n",
        "VARS = {\n",
        "    \"SUTURA_BQ_DATASET\": \"ci_dataset\",\n",
        "    # The policied dataset is NOT \"SUTURA_BQ_DATASET\" - it is the one below.\n",
        "    \"SUTURA_BQ_RLS_DATASET\": \"dataset\",\n",
        "}\n",
    );

    #[test]
    fn the_names_pushed_are_the_dict_keys_and_not_the_prose_inside_it() {
        // A comment inside the dict quotes a name, and reading it as a key would make the page's
        // row and the script agree about a name the script does not push.
        let secrets = super::pushed_names(SCRIPT, "SECRETS = {");
        assert_eq!(secrets, BTreeSet::from([String::from("SVC_KEY_CI")]));
        let vars = super::pushed_names(SCRIPT, "VARS = {");
        assert_eq!(
            vars,
            BTreeSet::from([String::from("SUTURA_BQ_DATASET"), String::from("SUTURA_BQ_RLS_DATASET")])
        );
        // And a dict the reader cannot find yields nothing, which the caller turns into a failure
        // rather than into an agreement over an empty set.
        assert!(super::pushed_names(SCRIPT, "MISSING = {").is_empty());
    }

    #[test]
    fn a_row_that_lists_fewer_names_than_the_script_pushes_is_reported() {
        // The measured instance: ten pushed, five listed, and nothing said so for three commits.
        let page = "| `vars` | `SUTURA_BQ_DATASET` |\n";
        let listed = super::row_names(page, "| `vars` |");
        let pushed = super::pushed_names(SCRIPT, "VARS = {");
        let missing: Vec<&String> = pushed.difference(&listed).collect();
        assert_eq!(missing, vec![&String::from("SUTURA_BQ_RLS_DATASET")]);
        // The other direction too: the row IS the list, so a name it keeps after the script drops
        // it is the same drift pointing the other way.
        let stale = super::row_names("| `vars` | `SUTURA_BQ_GONE`, `SUTURA_BQ_DATASET` |\n", "| `vars` |");
        assert_eq!(
            stale.difference(&pushed).collect::<Vec<&String>>(),
            vec![&String::from("SUTURA_BQ_GONE")]
        );
    }

    #[test]
    fn a_reference_is_matched_case_insensitively_and_only_for_this_stacks_names() {
        // This repository writes one secret lower-case in the workflow and upper-case in the
        // script, so a case-sensitive comparison would report a correct reference as
        // unprovisioned - and `secrets.GITHUB_TOKEN` must not be reported at all.
        let workflow = concat!(
            "          SUTURA_BQ_KEY: ${{ secrets.svc_key_ci }}\n",
            "          GH: ${{ secrets.GITHUB_TOKEN }}\n",
            "      group: x-${{ vars.SUTURA_BQ_DATASET }}\n",
        );
        assert_eq!(
            super::referenced(workflow, "secrets", &["SVC_"]),
            BTreeSet::from([String::from("SVC_KEY_CI")])
        );
        assert_eq!(
            super::referenced(workflow, "vars", &["SUTURA_BQ_"]),
            BTreeSet::from([String::from("SUTURA_BQ_DATASET")])
        );
    }

    #[test]
    fn a_secret_pushed_under_this_stacks_bq_prefix_is_reconciled_and_githubs_own_is_not() {
        // The three identity values the two-principal cell is pointed at are `secrets` and carry
        // the `SUTURA_BQ_` prefix, so `SVC_` alone no longer describes this environment's secrets:
        // a `secrets.SUTURA_BQ_<misspelled>` read would be compared against nothing, and that is
        // the job that fails closed forever with both pages agreeing. Read off KINDS rather than a
        // hand-written slice, so dropping the prefix there is what this reddens for.
        let kind = super::KINDS
            .iter()
            .find(|kind| kind.name == "secrets")
            .expect("the secrets kind");
        assert_eq!(
            super::referenced(
                "          AUD: ${{ secrets.SUTURA_BQ_WORKLOAD_AUDIENCE }}\n",
                kind.name,
                kind.prefixes
            ),
            BTreeSet::from([String::from("SUTURA_BQ_WORKLOAD_AUDIENCE")])
        );
        // AND THE ARM THAT MUST NOT FIRE: GitHub's own names are why a prefix is read at all.
        assert!(
            super::referenced("${{ secrets.GITHUB_TOKEN }}\n", kind.name, kind.prefixes).is_empty(),
            "GitHub's own secret is not one this stack provisions"
        );
    }

    #[test]
    fn the_committed_contract_agrees_in_all_three_places() {
        // Over the REAL files, because the fixtures above prove the reader. This is the assertion
        // that reddens when a value is added to the script and not to the page, which is the state
        // the tree was in when this was written.
        let root = crate::repo::root().expect("the repo root");
        let contract = super::problems(&root);
        assert!(contract.problems.is_empty(), "{:?}", contract.problems);
        // And it compared something: a reader that stopped matching either file would agree over
        // an empty set, which is the way a text scan goes quiet.
        assert!(contract.names > 5, "reconciled {} name(s)", contract.names);
    }

    #[test]
    fn one_unreadable_workflow_is_a_recorded_failure_and_not_a_smaller_scan() {
        // Reported in review: the floor fails closed only when EVERY workflow is unreadable, so a
        // single dropped file was a `vars.<PREFIX>_...` read this contract never compared.
        let scratch = std::env::temp_dir().join(format!("sutura-environment-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::write(workflows.join("readable.yml"), "vars.SUTURA_BQ_PROJECT\n").expect("the readable workflow");
        // NON-UTF-8 bytes, which is what `read_to_string` refuses. A directory named
        // `unreadable.yml` would not do: `repo::collect_files` recurses into one rather than
        // collecting it, so the scan would never reach it.
        std::fs::write(workflows.join("unreadable.yml"), [0xff_u8, 0xfe, 0xfd]).expect("the unreadable workflow");
        let (text, unreadable) = super::workflow_text(&scratch);
        assert!(text.contains("SUTURA_BQ_PROJECT"), "the readable file is still read: {text}");
        assert_eq!(unreadable.len(), 1, "{unreadable:?}");
        assert!(
            unreadable.first().is_some_and(|line| line.contains("was not compared")),
            "{unreadable:?}"
        );
        // AND THE ARM THAT STILL FIRES: with every file readable there is no such sentence.
        std::fs::remove_file(workflows.join("unreadable.yml")).expect("the unreadable workflow");
        assert!(
            super::workflow_text(&scratch).1.is_empty(),
            "the unreadable workflow leaves no parsed copies"
        );
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }
}
