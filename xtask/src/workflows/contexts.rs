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
//! * **every required context resolves to a job that reports that context.** A required context
//!   that no longer REPORTS is a permanently pending merge, which is worse than an ungated leg - it
//!   is the failure mode that makes *just add the four legs to the required list* a bad remedy. A
//!   rename fails here instead. **A CALLED workflow's job cannot resolve one**: its context is
//!   prefixed by the caller's job, so its bare id is a string nothing reports, and letting it
//!   resolve would answer *this reports* about a context that does not exist.
//! * **every job that can report on a pull request is declared, in one section or the other.** A
//!   new job is then a decision rather than an omission, which is the whole content of the
//!   advisory section: it makes the status quo visible. **A workflow a gating one CALLS counts** -
//!   its jobs report on the same pull request while its own `on:` is `workflow_call`, so the event
//!   test alone read none of them. See [`gating_jobs`] and [`super::reach`].
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
//!
//! **AND IT CANNOT SPELL A MATRIX LEG OR A REUSABLE-WORKFLOW CALL.** A context here is a job's
//! `name:` or its id, and neither shape is that: `cross-link.yml`'s own header states the one this
//! repository has, `cross / link (<triple>)` - a caller's name, the called job's name and the
//! matrix value. So requiring one used to print *no job reports it - a required context that never
//! reports is a PERMANENTLY PENDING merge*, which is FALSE, on the one remedy
//! `devco/required-contexts` actually discusses. [`derivable`] refuses such an entry with the true
//! sentence instead, and names the remedy this reader CAN check: an aggregating job, whose plain
//! context resolves like any other. Synthesising the legs from `strategy.matrix` was weighed and
//! is the wrong change - it would make this gate a partial evaluator of GitHub's own expansion,
//! measured against documentation rather than against the thing.

use std::collections::BTreeSet;
use std::path::Path;

use super::reach;
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
    /// Is [`Self::context`] the whole string GitHub reports, or only part of one?
    ///
    /// **A property of the JOB, not of its file**, and it was per-file first, which left the
    /// sibling of the very hole it closed. Three ways the bare string is not the whole context, and
    /// each makes letting it resolve a `[required]` entry an answer of *this context reports* about
    /// a string GitHub never sends - a permanently pending merge, declared green:
    ///
    /// * a job in a CALLED workflow reports `<caller job> / <job>`;
    /// * a job with a `strategy:` reports `<job> (<matrix value>)` per leg and the bare name never;
    /// * a job whose `name:` carries `${{ }}` reports the EXPANDED string, and this reader holds
    ///   the literal text.
    ///
    /// Such a job is still classified - that is the point of reading it at all - it just cannot
    /// resolve a requirement.
    reports_its_context: bool,
}

/// Every problem. Empty means the declaration and the workflows agree.
pub(super) fn problems(root: &Path, ci: &OrdinaryCi) -> Vec<String> {
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

    let Gating { jobs, refusals } = gating_jobs(ci);
    if jobs.is_empty() {
        return vec![String::from(
            "no job in any pull-request-triggered workflow was read - the scan is broken, not the declaration",
        )];
    }

    let mut problems = Vec::new();
    // ONE unreachable file, not all of them. The floor above fails closed only when EVERY workflow
    // is unreadable, so a single dropped file was a gating job this record never classified -
    // reported in review. `crate::hook_coverage` had this right: an unreadable input is a recorded
    // failure, never a smaller scan.
    problems.extend(refusals);
    for context in &required {
        if !derivable(context) {
            problems.push(format!(
                "{DECLARATION} requires the context `{context}`, and this reader derives a PLAIN job's context only - its `name:` or its id. A matrix leg reports `<job> (<value>)` and a reusable-workflow call reports `<caller> / <job> (<value>)`, and no `jobs:` key spells either, so whether that context reports is UNCHECKED here rather than answered wrongly. Require an aggregating job instead: its context resolves like any other, and one name beats four to maintain by hand"
            ));
            continue;
        }
        // `reports_its_context` and not merely a name match: a called workflow's job is read here
        // so it can be CLASSIFIED, and its bare id is not a context anything reports.
        if !jobs.iter().any(|job| job.reports_its_context && job.context == *context) {
            problems.push(format!(
                "{DECLARATION} requires the context `{context}` and no job reports it - a required context that never reports is a PERMANENTLY PENDING merge"
            ));
        }
    }
    for entry in &advisory {
        if !jobs.iter().any(|job| job.declared_as == *entry) {
            problems.push(format!(
                "{DECLARATION} lists `{entry}` as advisory, but no gating job matches it - either the job does not exist or its workflow gates on neither `pull_request` nor `merge_group`"
            ));
        }
    }
    for job in &jobs {
        let required_by_context = job.reports_its_context && required.contains(&job.context);
        let classified = required_by_context || advisory.contains(&job.declared_as);
        if !classified {
            problems.push(format!(
                "`{}` can report on a pull request and {DECLARATION} classifies it as neither required nor advisory - say which, because a job nobody requires gates nothing",
                job.declared_as
            ));
        }
    }
    problems
}

/// Can this reader derive whether the context a `[required]` entry names reports?
///
/// Only for a plain job. A trailing `)` is a matrix leg's value and a ` / ` is a reusable-workflow
/// call's caller prefix, and a `jobs:` key spells neither - see the header for why synthesising
/// them is the wrong change.
fn derivable(context: &str) -> bool {
    !context.contains(" / ") && !context.ends_with(')')
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

/// Every job that can report a context, and every reason that answer is over a subset.
///
/// A named struct rather than a tuple, because `clippy::type_complexity` refuses the tuple - and it
/// is right to for `crate::shipped::Reconciliation`'s reason: two `Vec`s side by side say nothing
/// about which is the answer and which is the reason the answer is over a subset.
struct Gating {
    /// Every job in a workflow that can report on a pull request or a queue entry.
    jobs: Vec<Job>,
    /// One sentence per workflow that could not be read, and per call the reach walk could not
    /// follow. Both are the same failure: a file whose jobs are classified by nothing.
    refusals: Vec<String>,
}

/// Every job in every workflow that can report a context on a pull request or a queue entry, and
/// one sentence per file that could not be reached.
///
/// **Two ways a workflow gets here, and the second is what was missing.** A workflow whose own
/// `on:` block names a gating event is a root. A workflow a root CALLS is one too - its jobs report
/// `<caller> / <job>` on the same pull request - and its own `on:` is `workflow_call`, so the
/// event test alone classified none of them: `cross-link.yml`'s legs were a job nobody had
/// classified while this gate printed *every gating job classified*. The whole content of the
/// advisory section is that a new job is a DECISION rather than an omission, and a called workflow
/// was the omission.
fn gating_jobs(ci: &OrdinaryCi) -> Gating {
    let mut refusals = ci.unreadable.clone();
    refusals.extend(ci.closure.drift());
    let mut jobs = Vec::new();
    for file in ci.closure.inspected().iter().filter(|file| file.is_workflow()) {
        // A ROOT runs on its own trigger, so its job's `name:` or id IS the context. A workflow the
        // walk only reached through a call reports `<caller job> / <job> (<value>)`, which no
        // `jobs:` key spells - so its jobs are classified and cannot resolve a required context.
        jobs.extend(jobs_in(file.label(), file.text(), file.is_root()));
    }
    Gating { jobs, refusals }
}

/// What ordinary CI is: every workflow that runs on a gating event, plus everything those call.
///
/// **One derivation, read by both of this gate's rules**, which is the point. The release-output
/// refusal used to root its own walk at the literal name `ci.yml` while this half derived the set -
/// so `docs.yml` and `security-audit.yml`, both `pull_request`-triggered, ran on every ordinary
/// pull request outside that refusal's sight. A walk that begins at a name is the very defect the
/// walk was added to fix, one file over.
pub(super) struct OrdinaryCi {
    closure: reach::Closure,
    /// One sentence per workflow file that could not be read at all. A root nobody could read is
    /// this reader's failure to report, not the walk's.
    unreadable: Vec<String>,
}

impl OrdinaryCi {
    /// Derive the roots and walk them.
    pub(super) fn read(root: &Path) -> Self {
        let mut unreadable = Vec::new();
        // The walk's own finding lands in the channel this reader already publishes: a workflow
        // directory it cannot enumerate leaves every job below classified by nothing.
        let mut paths = match repo::collect_files(root, &root.join(".github/workflows"), &["yml", "yaml"])
            .into_listing(repo::Unmigrated::Workflows)
        {
            Ok((_root, found)) => found,
            Err(why) => {
                unreadable.push(why.describe());
                Vec::new()
            }
        };
        paths.sort();
        let mut roots = Vec::new();
        for rel in &paths {
            let text = match std::fs::read_to_string(root.join(rel)) {
                Ok(text) => text,
                Err(error) => {
                    unreadable.push(format!(
                        "{rel} could not be read, so any job it declares is classified by nothing: {error}"
                    ));
                    continue;
                }
            };
            if !gates_on_an_event(&text) {
                continue;
            }
            let file = rel.rsplit('/').next().unwrap_or(rel);
            roots.push(reach::Reached::workflow(file, text));
        }
        Self {
            closure: reach::Closure::from_roots(root, roots),
            unreadable,
        }
    }

    pub(super) const fn closure(&self) -> &reach::Closure {
        &self.closure
    }

    /// One sentence per file this reader could not open, root or called alike.
    pub(super) fn unreachable(&self) -> Vec<String> {
        let mut out = self.unreadable.clone();
        out.extend(self.closure.drift());
        out
    }
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
///
/// `in_a_root` is the caller's answer - whether the workflow runs on its own trigger or only
/// because something calls it is not readable from one job. Everything else about whether the
/// context is the WHOLE string is read here, per job, because that is where the property lives.
fn jobs_in(file: &str, text: &str, in_a_root: bool) -> Vec<Job> {
    let lines: Vec<&str> = text.lines().collect();
    block_keys(text, "jobs:")
        .into_iter()
        .map(|id| {
            // A `name:` at the job's own key depth is the context GitHub reports; without one the
            // id is. Reading the wrong one would make a required context resolve to nothing.
            let name = job_property(&lines, &id, "name");
            // A `strategy:` makes every leg report `<job> (<value>)`, and an expression in a
            // `name:` is reported EXPANDED. Either way the string held here is a fragment.
            let matrix = job_property(&lines, &id, "strategy").is_some();
            let expanded = name.as_ref().is_some_and(|n| n.contains("${{"));
            Job {
                declared_as: format!("{file}:{id}"),
                context: name.unwrap_or_else(|| id.clone()),
                reports_its_context: in_a_root && !matrix && !expanded,
            }
        })
        .collect()
}

/// The value of one key at a job's own depth, if the job declares it.
///
/// Read at the job's key depth only - a `name:` two levels in is a STEP's, and every step has one,
/// so the first `name:` below the job key is usually not the job's. A key with no inline value,
/// `strategy:` being the one this gate asks about, answers `Some("")`: the question there is
/// whether it is DECLARED.
fn job_property(lines: &[&str], id: &str, key: &str) -> Option<String> {
    let opener = format!("{id}:");
    let wanted = format!("{key}:");
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
            // The next job, or the end of the block: this one declared no such key.
            return None;
        }
        if indent == JOB_INDENT.saturating_add(2)
            && let Some(value) = trimmed.strip_prefix(wanted.as_str())
        {
            return Some(String::from(value.trim().trim_matches('"').trim_matches('\'')));
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
        let jobs = super::jobs_in("ci.yml", WORKFLOW, true);
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
    fn a_required_context_this_reader_cannot_spell_is_refused_with_the_true_sentence() {
        // Reported in review: requiring `cross / link (aarch64-unknown-linux-musl)` printed
        // *no job reports it - a required context that never reports is a PERMANENTLY PENDING
        // merge*, which is false. The job DOES report it; this reader cannot derive the string.
        assert!(!super::derivable("cross / link (aarch64-unknown-linux-musl)"));
        assert!(!super::derivable("link (x86_64-unknown-linux-musl)"));
        // AND THE ARM THAT STILL FIRES: a plain job's context is derivable and still resolved, so
        // the rename this rule exists for is caught exactly as before.
        assert!(super::derivable("ci"));
        assert!(super::derivable("structural gates"));
    }

    #[test]
    fn one_unreadable_workflow_is_a_recorded_failure_and_not_a_smaller_scan() {
        // Reported in review: both floors fail closed only when EVERY workflow is unreadable, so a
        // single dropped file was a gating job classified by nothing. Read over a directory
        // holding a file this process may not open.
        let scratch = std::env::temp_dir().join(format!("sutura-contexts-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::write(workflows.join("readable.yml"), WORKFLOW).expect("the readable workflow");
        // NON-UTF-8 bytes, which is what `read_to_string` refuses - the shape
        // `crate::shipped` already records for a page under `docs/`. A directory named
        // `unreadable.yml` would not do: `repo::collect_files` recurses into one rather than
        // collecting it, so the scan would never reach it and the test would prove nothing.
        std::fs::write(workflows.join("unreadable.yml"), [0xff_u8, 0xfe, 0xfd]).expect("the unreadable workflow");
        let read = super::gating_jobs(&super::OrdinaryCi::read(&scratch));
        assert_eq!(read.jobs.len(), 2, "the readable file is still read");
        assert_eq!(read.refusals.len(), 2, "{:?}", read.refusals);
        assert!(
            read.refusals.iter().any(|line| line.contains("classified by nothing")),
            "{:?}",
            read.refusals
        );
        // AND the call the readable file makes: `cross-link.yml` is not in this scratch tree, so
        // the walk cannot open it and says so rather than classifying a smaller set of jobs.
        assert!(
            read.refusals.iter().any(|line| line.starts_with("UNFOLLOWED CALL:")),
            "{:?}",
            read.refusals
        );
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn a_job_in_a_called_workflow_is_classified_by_this_record_too() {
        // THE HOLE THIS CLOSES. A called workflow's own `on:` is `workflow_call`, so the event
        // test read none of its jobs while its legs reported on every pull request. Over a scratch
        // tree whose declaration classifies the CALLER and not the called job.
        let scratch = std::env::temp_dir().join(format!("sutura-called-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::create_dir_all(scratch.join("devco")).expect("the devco directory");
        std::fs::write(
            workflows.join("ci.yml"),
            concat!(
                "on:\n  pull_request:\njobs:\n",
                "  ci:\n    runs-on: ubuntu-latest\n",
                "  cross:\n    uses: ./.github/workflows/cross-link.yml\n"
            ),
        )
        .expect("the caller");
        std::fs::write(
            workflows.join("cross-link.yml"),
            "on:\n  workflow_call:\njobs:\n  link:\n    runs-on: ubuntu-latest\n",
        )
        .expect("the called workflow");
        std::fs::write(
            scratch.join(super::DECLARATION),
            "[required]\nci\n\n[advisory]\nci.yml:cross\n",
        )
        .expect("the declaration");

        let problems = super::problems(&scratch, &super::OrdinaryCi::read(&scratch));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems
                .first()
                .is_some_and(|line| line.starts_with("`cross-link.yml:link` can report on a pull request")),
            "{problems:?}"
        );

        // AND THE FALSE POSITIVE READING A CALLED WORKFLOW OPENS, which is the reason a job carries
        // whether its context is the whole string: `link` is the called job's id and the context it
        // reports is `cross / link (<value>)`, so requiring `link` must NOT resolve. Before this
        // was carried, the same read that closed the classification hole answered *this context
        // reports* about a string GitHub never sends - a permanently pending merge, declared green.
        std::fs::write(
            scratch.join(super::DECLARATION),
            "[required]\nci\nlink\n\n[advisory]\nci.yml:cross\ncross-link.yml:link\n",
        )
        .expect("the declaration");
        let problems = super::problems(&scratch, &super::OrdinaryCi::read(&scratch));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems
                .first()
                .is_some_and(|line| line.contains("requires the context `link` and no job reports it")),
            "{problems:?}"
        );
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn a_root_job_whose_context_is_only_part_of_one_cannot_resolve_a_requirement() {
        // THE SIBLING OF THE CASE ABOVE, and the granularity it was got wrong at: whether the bare
        // id is the WHOLE context is a property of the JOB. A `strategy:` on a ROOT job makes every
        // leg report `<job> (<value>)` and the bare name never - and a per-FILE answer said yes,
        // because the file is a root. Requiring that name was `ok`, exit 0: a permanently pending
        // merge declared green. Same for a `name:` carrying an expression, reported EXPANDED.
        let scratch = std::env::temp_dir().join(format!("sutura-perjob-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::create_dir_all(scratch.join("devco")).expect("the devco directory");
        std::fs::write(
            workflows.join("ci.yml"),
            concat!(
                "on:\n  pull_request:\njobs:\n",
                "  ci:\n    runs-on: ubuntu-latest\n",
                "  sharded:\n    strategy:\n      matrix:\n        shard: [1, 2]\n",
                "  titled:\n    name: ${{ matrix.os }} build\n",
            ),
        )
        .expect("the caller");
        std::fs::write(
            scratch.join(super::DECLARATION),
            "[required]\nci\nsharded\n\n[advisory]\nci.yml:sharded\nci.yml:titled\n",
        )
        .expect("the declaration");

        let problems = super::problems(&scratch, &super::OrdinaryCi::read(&scratch));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems
                .first()
                .is_some_and(|line| line.contains("requires the context `sharded` and no job reports it")),
            "{problems:?}"
        );

        // AND THE ARM THAT MUST STILL FIRE: a plain root job resolves exactly as before, so this
        // is a refusal of the fragment rather than of everything.
        let jobs = super::jobs_in(
            "ci.yml",
            &std::fs::read_to_string(workflows.join("ci.yml")).expect("ci.yml"),
            true,
        );
        let reports: Vec<(&str, bool)> = jobs
            .iter()
            .map(|job| (job.declared_as.as_str(), job.reports_its_context))
            .collect();
        assert_eq!(
            reports,
            vec![("ci.yml:ci", true), ("ci.yml:sharded", false), ("ci.yml:titled", false)]
        );
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn an_unseen_advisory_names_both_possible_causes() {
        let scratch = std::env::temp_dir().join(format!("sutura-advisory-{}", std::process::id()));
        let workflows = scratch.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("the scratch tree");
        std::fs::create_dir_all(scratch.join("devco")).expect("the devco directory");
        std::fs::write(
            workflows.join("ci.yml"),
            "on:\n  pull_request:\njobs:\n  ci:\n    runs-on: ubuntu-latest\n",
        )
        .expect("the gating workflow");
        std::fs::write(
            workflows.join("manual.yml"),
            "on:\n  workflow_dispatch:\njobs:\n  probe:\n    runs-on: ubuntu-latest\n",
        )
        .expect("the non-gating workflow");
        std::fs::write(
            scratch.join(super::DECLARATION),
            "[required]\nci\n\n[advisory]\nmanual.yml:probe\n",
        )
        .expect("the declaration");

        let problems = super::problems(&scratch, &super::OrdinaryCi::read(&scratch));
        assert_eq!(
            problems,
            [
                "devco/required-contexts lists `manual.yml:probe` as advisory, but no gating job matches it - either the job does not exist or its workflow gates on neither `pull_request` nor `merge_group`"
            ]
        );
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
    }

    #[test]
    fn the_committed_declaration_classifies_every_gating_job() {
        // Over the REAL files. This is the assertion that reddens when a job is added to a
        // pull-request-triggered workflow without anybody deciding whether it gates anything.
        let root = crate::repo::root().expect("the repo root");
        let problems = super::problems(&root, &super::OrdinaryCi::read(&root));
        assert!(problems.is_empty(), "{problems:?}");
    }
}
