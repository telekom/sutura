//! Which jobs gate a merge, and which only look as though they do.
//!
//! **The finding this exists for:** the four `cross / link (<triple>)` legs were widely assumed to
//! gate a merge and were required by nothing. Measured against the API on 2026-09-05, the `main`
//! branch ruleset requires exactly one context, `ci`, and classic branch protection is absent - so
//! a red link leg has never blocked a merge, in the queue or out of it, with no override and no
//! bypass. Renaming those legs broke no required check, which is the good news; that none of them
//! was ever required is the finding.
//!
//! Nothing in the repository could say so, because the required set lived only in GitHub's API.
//! `devco/required-contexts` is that set written down, with the date, the exact response and the
//! scope that could not be read - and this is what keeps it honest against the tree.
//!
//! # Three rules
//!
//! * **every required context resolves to a job.** A required context that no longer REPORTS is a
//!   permanently pending merge, which is worse than an ungated leg - it is the failure mode that
//!   makes *just add the four legs to the required list* a bad remedy. A rename fails here instead.
//! * **every job that can report on a pull request is declared, in one section or the other.** A
//!   new job is then a decision rather than an omission, which is the whole content of the
//!   advisory section: it makes the status quo visible.
//! * **every advisory entry names a job that exists**, so an entry cannot outlive its job.
//!
//! # What it does NOT reach, and this one is the important limit
//!
//! **It cannot check the file against the ruleset.** The API is the authority and it is unreachable
//! from `checks.hygiene` - no network, no token - which is the same limit `crate::venues` states
//! about its own invocation rule and `crate::workflows` states about `nix eval`. So what is held is
//! that the declaration and the tree agree, never that the declaration is true. The date and the
//! verbatim response in the file are what a reader checks it by.
//!
//! **Nor whether a job reports at all.** A job with `if: false` is still declared and still
//! classified; the classification is about what WOULD gate. And a job's `if:` is not read, so
//! `docs.yml:publish` - which never runs on a pull request - is declared advisory rather than
//! excluded, because excluding it would need this gate to evaluate a GitHub expression.

use std::collections::BTreeSet;
use std::path::Path;

use crate::repo;

/// The declaration. Beside `max-lines-ignore` because it is the same kind of file: gate policy a
/// reviewer edits, in a reviewable one-line diff next to the reason for it.
const DECLARATION: &str = "devco/required-contexts";

/// The events on which a job reports a context that could gate a merge.
///
/// `merge_group` as well as `pull_request`: the queue's own runs report the same contexts, and the
/// merge-queue rule takes its signal from the same required set.
const GATING_EVENTS: [&str; 2] = ["pull_request", "merge_group"];

/// One job, as the declaration names it and as a context is reported.
struct Job {
    /// `<workflow file>:<job id>` - the identity a `name:` change does not move.
    declared_as: String,
    /// The context string GitHub reports: the job's `name:` where it has one, else its id.
    context: String,
}

/// Every problem. Empty means the declaration and the workflows agree.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(DECLARATION)) else {
        return vec![format!(
            "{DECLARATION} is not readable - it IS the record of what gates a merge"
        )];
    };
    let required = section(&text, "[required]");
    let advisory = section(&text, "[advisory]");
    if required.is_empty() {
        // FAIL CLOSED. An empty required set would make the resolution rule below pass by checking
        // nothing, and would also assert that this repository gates a merge on no check at all.
        return vec![format!(
            "{DECLARATION} declares no required context - the reader stopped matching it, or the record is empty"
        )];
    }

    let jobs = gating_jobs(root);
    if jobs.is_empty() {
        return vec![String::from(
            "no job in any pull-request-triggered workflow was read - the scan is broken, not the declaration",
        )];
    }

    let mut problems = Vec::new();
    for context in &required {
        if !jobs.iter().any(|job| job.context == *context) {
            problems.push(format!(
                "{DECLARATION} requires the context `{context}` and no job reports it - a required context that never reports is a PERMANENTLY PENDING merge"
            ));
        }
    }
    for entry in &advisory {
        if !jobs.iter().any(|job| job.declared_as == *entry) {
            problems.push(format!("{DECLARATION} lists `{entry}` as advisory and no such job exists"));
        }
    }
    for job in &jobs {
        let classified = required.contains(&job.context) || advisory.contains(&job.declared_as);
        if !classified {
            problems.push(format!(
                "`{}` can report on a pull request and {DECLARATION} classifies it as neither required nor advisory - say which, because a job nobody requires gates nothing",
                job.declared_as
            ));
        }
    }
    problems
}

/// The entries under one `[section]` header, up to the next header.
///
/// Comment and blank lines are skipped, which is load-bearing: this file's argument is longer than
/// its data, and one paragraph quotes a context name.
fn section(text: &str, header: &str) -> BTreeSet<String> {
    let mut entries = BTreeSet::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == header;
            continue;
        }
        if inside && !trimmed.is_empty() && !trimmed.starts_with('#') {
            entries.insert(String::from(trimmed));
        }
    }
    entries
}

/// Every job in every workflow that can report a context on a pull request or a queue entry.
fn gating_jobs(root: &Path) -> Vec<Job> {
    let mut paths = Vec::new();
    repo::collect_files(root, &root.join(".github/workflows"), &["yml", "yaml"], &mut paths);
    paths.sort();
    let mut jobs = Vec::new();
    for rel in &paths {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        if !gates_on_an_event(&text) {
            continue;
        }
        let file = rel.rsplit('/').next().unwrap_or(rel);
        jobs.extend(jobs_in(file, &text));
    }
    jobs
}

/// Does this workflow run on an event whose run reports a context that could gate a merge?
///
/// The `on:` block's own keys, which is narrower than a `contains` and has to be: `ci.yml` names
/// `pull_request` in eleven comments and in a dozen `github.event_name` tests, and a workflow
/// triggered only by a tag would be read as gating on every one of them.
fn gates_on_an_event(text: &str) -> bool {
    block_keys(text, "on:")
        .iter()
        .any(|key| GATING_EVENTS.contains(&key.as_str()))
}

/// Every job in one workflow, with the context each reports.
fn jobs_in(file: &str, text: &str) -> Vec<Job> {
    let lines: Vec<&str> = text.lines().collect();
    block_keys(text, "jobs:")
        .into_iter()
        .map(|id| Job {
            declared_as: format!("{file}:{id}"),
            // A `name:` at the job's own key depth is the context GitHub reports; without one the
            // id is. Reading the wrong one would make a required context resolve to nothing.
            context: job_name(&lines, &id).unwrap_or_else(|| id.clone()),
        })
        .collect()
}

/// The `name:` of one job, if it declares one.
///
/// Read at the job's key depth only - a `name:` two levels in is a STEP's, and every step has one,
/// so the first `name:` below the job key is usually not the job's.
fn job_name(lines: &[&str], id: &str) -> Option<String> {
    let opener = format!("{id}:");
    let mut inside = false;
    for raw in lines {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let indent = raw.len().saturating_sub(trimmed.len());
        if !inside {
            inside = indent == JOB_INDENT && trimmed == opener;
            continue;
        }
        if indent <= JOB_INDENT {
            // The next job, or the end of the block: this one declared no `name:`.
            return None;
        }
        if indent == JOB_INDENT.saturating_add(2)
            && let Some(name) = trimmed.strip_prefix("name:")
        {
            return Some(String::from(name.trim().trim_matches('"').trim_matches('\'')));
        }
    }
    None
}

/// A job key's column. Two spaces under `jobs:`, which is the only shape GitHub accepts.
const JOB_INDENT: usize = 2;

/// The mapping keys directly inside one top-level block.
///
/// Comments are skipped, and a line deeper than the block's own keys is not one - so a `run: |`
/// body cannot contribute a key, which is the failure `crate::workflows::declared_block` records
/// about counting braces over raw text.
fn block_keys(text: &str, opener: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut inside = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        let indent = raw.len().saturating_sub(trimmed.len());
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if indent == 0 {
            inside = trimmed == opener;
            continue;
        }
        if !inside || indent != JOB_INDENT {
            continue;
        }
        let Some(key) = trimmed.strip_suffix(':') else {
            continue;
        };
        if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            keys.push(String::from(key));
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// A workflow whose `on:` block gates, whose comments name the events it is NOT triggered by,
    /// and one of whose jobs renames its context.
    const WORKFLOW: &str = concat!(
        "# A comment about pull_request, which is not a trigger.\n",
        "on:\n",
        "  push:\n",
        "    branches: [main]\n",
        "  pull_request:\n",
        "concurrency:\n",
        "  group: x\n",
        "jobs:\n",
        "  ci:\n",
        "    runs-on: ubuntu-latest\n",
        "    steps:\n",
        "      - name: A step, whose name is not the job's\n",
        "        run: |\n",
        "          jobs:\n",
        "          echo hi\n",
        "  cross:\n",
        "    name: link matrix\n",
        "    uses: ./.github/workflows/cross-link.yml\n",
    );

    #[test]
    fn a_jobs_block_yields_the_job_ids_and_nothing_else() {
        // `concurrency:` has a key at the same depth, a step has a `name:`, and a `run:` body has a
        // line that reads exactly like the block opener. All three would corrupt the answer, and
        // the second would make a required context resolve to a step's name.
        assert_eq!(
            super::block_keys(WORKFLOW, "jobs:"),
            vec![String::from("ci"), String::from("cross")]
        );
        assert_eq!(
            super::block_keys(WORKFLOW, "on:"),
            vec![String::from("push"), String::from("pull_request")]
        );
    }

    #[test]
    fn the_context_is_the_jobs_own_name_where_it_has_one() {
        let jobs = super::jobs_in("ci.yml", WORKFLOW);
        let ci = jobs.first().expect("the ci job");
        assert_eq!(ci.declared_as, "ci.yml:ci");
        // NOT the step's name, which is the one a naive first-match would return.
        assert_eq!(ci.context, "ci");
        let cross = jobs.get(1).expect("the cross job");
        assert_eq!(cross.declared_as, "ci.yml:cross");
        assert_eq!(cross.context, "link matrix");
    }

    #[test]
    fn a_workflow_that_only_mentions_the_event_in_prose_does_not_gate() {
        // `ci.yml` names `pull_request` in a dozen comments and expressions, so a `contains` here
        // would classify a tag-triggered workflow's jobs as gating and demand a declaration for
        // every one of them.
        let tagged = concat!(
            "# Runs at tag time, never on a pull_request.\n",
            "on:\n",
            "  push:\n",
            "    tags: ['v*']\n",
            "jobs:\n",
            "  release:\n",
            "    if: github.event_name != 'pull_request'\n",
        );
        assert!(!super::gates_on_an_event(tagged));
        assert!(super::gates_on_an_event(WORKFLOW));
    }

    #[test]
    fn a_section_is_read_without_its_argument() {
        // The declaration's prose is longer than its data and quotes a context name, so a reader
        // that kept comments would declare a context nobody requires.
        let text = concat!(
            "# The header quotes `ci` and names ci.yml:cross.\n",
            "[required]\n",
            "\n",
            "# The one context the ruleset requires.\n",
            "ci\n",
            "\n",
            "[advisory]\n",
            "ci.yml:cross\n",
        );
        assert_eq!(super::section(text, "[required]"), BTreeSet::from([String::from("ci")]));
        assert_eq!(
            super::section(text, "[advisory]"),
            BTreeSet::from([String::from("ci.yml:cross")])
        );
    }

    #[test]
    fn the_committed_declaration_classifies_every_gating_job() {
        // Over the REAL files. This is the assertion that reddens when a job is added to a
        // pull-request-triggered workflow without anybody deciding whether it gates anything.
        let root = crate::repo::root().expect("the repo root");
        let problems = super::problems(&root);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
