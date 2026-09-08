//! Can the publication `README.md`'s badge is served from actually LAND?
//!
//! `scorecard::problems` holds *badge ⟺ publication declared*, in both directions, and that half
//! works. **It says nothing about whether the declared publication is accepted**, and the gap was
//! not hypothetical: three runs of `.github/workflows/scorecard.yml` reported `completed success`
//! while `api.scorecard.dev` held no record of this project at all.
//!
//! # What was measured, because this rule is a reading of somebody else's code
//!
//! Run `34174327175`'s log, 2026-09-08, verbatim from the step that publishes:
//!
//! ```text
//! error sending scorecard results to webapp: http response 400, status: 400 Bad Request,
//! error: {"code":400,"message":"workflow verification failed: workflow verification failed:
//! scorecard job has invalid runner label: 'rust-mcp',
//! see https://github.com/ossf/scorecard-action#workflow-restrictions for details."}
//! ...
//! ::warning::Unable to POST scorecard results to webapp: ...
//! ```
//!
//! Three things follow, and all three are load-bearing:
//!
//! * **The action does not decline for a private repository - it attempts the publish and the API
//!   refuses it.** `options.go` computes `PublishResults = input && !private` and only PRINTS it
//!   (`Publication enabled: false` appeared in that same log); `main.go` branches on the raw
//!   `INPUT_PUBLISH_RESULTS` environment variable instead. So the score was signed into the public
//!   sigstore transparency log and `POST`ed while the repository was private.
//! * **The refusal is FAIL-OPEN inside the action.** `signing.ProcessSignature` retries on a
//!   backoff schedule, then logs `::warning::` and `return nil` - so `main.go`'s `log.Fatalf` never
//!   fires and the step exits 0. Read at commit `2d1146689b8cda280b9bc96326124645441f03bc`, which
//!   is the SHA `scorecard.yml` pins.
//! * **The refusal has nothing to do with visibility, and that is measured rather than argued.**
//!   Run `34198064772`, with the repository **public**, `Private repository: false` in its own log
//!   and `publish_results: true`: the same `http response 400 ... invalid runner label:
//!   'rust-mcp'`, retried three times, then the same `::warning::`, and the step exited 0 again.
//!   So `runs-on` is THE blocker - the badge could never have resolved, private or public, and
//!   nothing here would have said so. That is why this is a gate rather than a note, and why
//!   `ubuntu-latest` on the scoring job is a requirement of the tool rather than a preference.
//!
//!   **And `Publication enabled:` is not a control**, which the same three logs settle between
//!   them: it printed `false` in all three - twice with `Private repository: true` and once with
//!   `false` - while the publish was attempted every time (a sigstore `tlog entry created with
//!   index:` line precedes each 400). `options.go`'s field carries no `env:` tag, so that line
//!   reads `false` unconditionally.
//!
//! # What it holds
//!
//! Whenever `scorecard.yml` publishes, its shape must satisfy every rule
//! `ossf/scorecard-webapp`'s `verifyScorecardWorkflow` applies before accepting a score - read at
//! commit `9c2f66d5f6ff56ca4a4ac2fba6ec8dcc5379d31c`, the revision the action's own README cites:
//! one Ubuntu-hosted runner label from its allowlist on the job that runs the action, only approved
//! actions as that job's steps and every step a `uses:`, no container or services and no job-level
//! `env:` or `defaults:` on it, no OTHER job holding `id-token: write`, no workflow-level `env:` or
//! `defaults:`, and no workflow-level permission set to write.
//!
//! **The step list is the rule most likely to bite the next change**, and it bit this one: a
//! `run:` step added to that job to check anything would be refused outright
//! (`errEmptyStepUses`), which is why this repository's own witness is a SEPARATE job.
//!
//! **And the WITNESS is held too, because the API's rules end at the scoring job.** Nothing in
//! `verifyScorecardWorkflow` reads the `published` job's shell - that file's half stops where
//! `errEmptyStepUses` does - so a witness that could never fire would pass every API rule while
//! a refused publication went green. This gate therefore reads the `published` job's `run:` body
//! itself and requires it to be a FAIL-CLOSED read of the scoring step's OWN output: it fetches
//! this run's scoring job log and exits non-zero when that log carries the action's `Unable to
//! POST` line. A `&& false` on the refusal arm - a control that cannot fail - is refused here, by
//! shape and not by a message that merely appears in the file.
//!
//! # What it does NOT hold
//!
//! * **Not that the publication landed.** This is a precondition read from a file. The `published`
//!   job in `scorecard.yml` is what reads the scoring step's own log for a refusal, and only a run
//!   can answer that. What the gate holds is that the witness is a real, fail-closed read of the
//!   step's own output - not that the step ran, which only a run can answer.
//! * **Not that the allowlist is current.** It is a dated copy of a third party's source. A runner
//!   label `OpenSSF` adds later reads as a failure here until this file is updated - which is the
//!   safe direction, and the reason the constant names its revision.
//! * **Not the container.** The SHA pins the action's manifest; what runs comes from a mutable
//!   `ghcr.io` tag, as `docs/adr/0024` records. A future image could change the publish path
//!   without any file here moving.

use std::path::Path;

use super::{scorecard, step};

/// The runner labels `api.scorecard.dev` accepts on the job that runs the action.
///
/// A dated copy of `ubuntuRunners` in `ossf/scorecard-webapp`'s `app/server/verify_workflow.go` at
/// `9c2f66d5f6ff56ca4a4ac2fba6ec8dcc5379d31c`. **Note what is absent:** `ubuntu-24.04` and
/// anything newer, so "a current Ubuntu runner" is not the property - membership is.
const UBUNTU: [&str; 4] = ["ubuntu-latest", "ubuntu-22.04", "ubuntu-20.04", "ubuntu-18.04"];

/// The only actions that may be steps of that job, from the same source.
const APPROVED: [&str; 5] = [
    "actions/checkout",
    "ossf/scorecard-action",
    "actions/upload-artifact",
    "github/codeql-action/upload-sarif",
    "step-security/harden-runner",
];

/// The action whose presence names the job the API verifies.
const ACTION: &str = "ossf/scorecard-action";

/// The witness's step name in `scorecard.yml`, held so the rule reads the RIGHT job's run body.
///
/// A dated copy of OUR OWN string, chosen so a rename reddens the gate rather than silently
/// reading a different step: the gate belongs beside the shell it holds, and the two must not
/// drift apart.
const WITNESS_STEP: &str = "The publication was not refused";

/// The action's own `::warning::` line, verbatim from run `34174327175`'s log - the ONE sentence
/// that says the publication was refused. `signing.ProcessSignature` prints it after its retries
/// and returns nil, so it is the only record of the refusal in the run, and it lives in the
/// SCORING STEP's own output. That last part is the property the witness must read FROM, because
/// the shape it replaced - "does `api.scorecard.dev` hold a record yet" - reddens on the API's own
/// indexing lag and cannot tell *refused* from *not indexed*.
///
/// A dated copy of a THIRD PARTY's string, like [`UBUNTU`]: the action could reword it and this
/// gate would read the absence as a pass. That is the safe direction, and why the witness's
/// fail-closed shape is held below rather than the message being trusted on its own.
const REFUSAL: &str = "Unable to POST scorecard results to webapp";

/// The witnessed fail-closed refusal guard, built from [`REFUSAL`], and the name by which the
/// `&& false` mutation is held.
///
/// `if grep -qF '<refusal>' score.log; then` - the needle DIRECTLY gating the `then`, with no
/// `&& false`, no `|| true` between them and no `!` inverting it. Any of those would turn the
/// witness into a control that can never fire while the needle still appears in the file, which is
/// exactly the dead-check shape this repository's reviews refuse. The body that guards it must
/// `exit 1` rather than warn, because a warning is what the action itself downgraded the refusal
/// to.
fn fail_closed_guard() -> String {
    format!("if grep -qF '{REFUSAL}' score.log; then")
}

/// One job of the workflow: its name, and its own lines.
type Job<'a> = (String, Vec<&'a str>);

/// One step of a job: its own text, and the action it calls if it calls one.
type Step = (String, Option<String>);

/// Every way the workflow's SHAPE would make `api.scorecard.dev` refuse what it sends.
///
/// A `Vec` rather than a `Verdict`, for [`scorecard::problems`]' reason: one gate prints one
/// verdict.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let workflow = std::fs::read_to_string(root.join(scorecard::WORKFLOW)).unwrap_or_default();

    // NOTHING TO HOLD WHERE NOTHING IS SENT, which is the same conditional `scorecard::problems`
    // draws around the dated record: these rules are the API's, and a workflow that does not
    // reach the API is not subject to them.
    if !scorecard::publishes(&workflow) {
        return Vec::new();
    }

    let mut problems = Vec::new();
    let file = scorecard::WORKFLOW;

    if key_at_root(&workflow, "env") {
        problems.push(format!(
            "{file} publishes and declares a workflow-level `env:` - api.scorecard.dev refuses the results of such a workflow outright, because a top-level variable reaches the job that signs them"
        ));
    }
    if key_at_root(&workflow, "defaults") {
        problems.push(format!(
            "{file} publishes and declares a workflow-level `defaults:` - api.scorecard.dev refuses that for the same reason as `env:`"
        ));
    }
    // A BLOCK OR A SCALAR, because `permissions: write-all` and a `permissions:` mapping with one
    // write scope are the same refusal reached by two spellings.
    let scoped = step::keyed_block(&workflow, "", "permissions").is_some_and(|lines| lines.iter().any(|line| writes(line)));
    let inline = scorecard::uncommented(&workflow).any(|line| line.starts_with("permissions:") && writes(line));
    if scoped || inline {
        problems.push(format!(
            "{file} publishes and grants a workflow-level write permission - api.scorecard.dev refuses any workflow that does, whatever the scope"
        ));
    }

    let jobs = jobs(&workflow);
    let Some(scoring) = jobs.iter().find(|(_, lines)| uses(lines).any(|action| action == ACTION)) else {
        problems.push(format!(
            "{file} publishes and no job in it runs `{ACTION}` - api.scorecard.dev has no job to verify, so the results are refused"
        ));
        return problems;
    };

    for (name, lines) in &jobs {
        if name != &scoring.0 && grants_id_token(lines) {
            problems.push(format!(
                "{file} publishes and its `{name}` job holds `id-token: write` - api.scorecard.dev refuses a workflow where any job but the scoring one can mint that token"
            ));
        }
    }

    let scoring_lines = &scoring.1;
    match labels(scoring_lines) {
        Some(labels) if labels.len() == 1 => {
            let only = labels.first().map_or("", String::as_str);
            if !UBUNTU.contains(&only) {
                problems.push(format!(
                    "the `{}` job of {file} runs on `{only}`, which is not one of the runner labels api.scorecard.dev accepts ({}) - the publication is REFUSED with HTTP 400 and the action downgrades that to a warning, so the step still exits 0 and the badge never resolves",
                    scoring.0,
                    UBUNTU.join(", ")
                ));
            }
        }
        Some(labels) => problems.push(format!(
            "the `{}` job of {file} declares {} runner labels - api.scorecard.dev requires exactly one, from {}",
            scoring.0,
            labels.len(),
            UBUNTU.join(", ")
        )),
        None => problems.push(format!(
            "the `{}` job of {file} declares no `runs-on:` this reader can resolve to a label list, so whether api.scorecard.dev would accept it is unread rather than answered wrongly",
            scoring.0
        )),
    }

    // A KEY OF THE JOB'S OWN MAPPING, by indentation. `keyed_block` is no use here: it matches the
    // header line exactly, so `container: alpine` - the spelling with the value inline - would
    // read as absent.
    for key in ["container", "services", "env", "defaults"] {
        let declared = scoring_lines
            .iter()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter_map(|line| line.strip_prefix("    "))
            .any(|rest| !rest.starts_with(' ') && rest.starts_with(&format!("{key}:")));
        if declared {
            problems.push(format!(
                "the `{}` job of {file} declares `{key}:` - api.scorecard.dev refuses the scoring job's results when it does",
                scoring.0
            ));
        }
    }

    for (step_lines, action) in steps(scoring_lines) {
        match action {
            Some(action) if APPROVED.contains(&action.as_str()) => {}
            Some(action) => problems.push(format!(
                "a step of the `{}` job of {file} runs `{action}`, which is not on api.scorecard.dev's approved list ({}) - the whole publication is refused, not that step",
                scoring.0,
                APPROVED.join(", ")
            )),
            None => problems.push(format!(
                "a step of the `{}` job of {file} has no `uses:` - api.scorecard.dev refuses the results of a scoring job containing ANY step that is not a call to an approved action, so a `run:` witness belongs in a separate job: {}",
                scoring.0,
                step_lines.trim()
            )),
        }
    }

    // THE WITNESS, AND IT MUST BE A REAL FAIL-CLOSED READ OF THE STEP'S OWN OUTPUT - not the
    // "does `api.scorecard.dev` hold a record yet" question this PR replaced, and not a message
    // that merely appears somewhere in the file. `verifyScorecardWorkflow` ends at the SCORING
    // job's steps (`errEmptyStepUses`), so nothing in the API's own rule-set can hold the shell
    // that does the witnessing - that is this file's half, and the reason a `run:` witness is a
    // SEPARATE job at all. The property, held by shape: a `published` job reads THIS run's
    // scoring job log and FAILS CLOSED when that log carries the action's own refusal line.
    let witness: Option<Vec<&str>> = jobs
        .iter()
        .map(|(_, lines)| lines)
        .find(|lines| step_run_body(lines, WITNESS_STEP).is_some_and(|b| b.iter().any(|l| l.contains(REFUSAL))))
        .cloned();
    match witness {
        None => problems.push(format!(
            "{file} publishes and no `{WITNESS_STEP}` step reads the scoring step's own output for `{REFUSAL}` - the witness is MISSING, so a publication the API refuses still exits 0 and this gate goes green on nothing"
        )),
        Some(lines) => {
            let body = step_run_body(&lines, WITNESS_STEP).unwrap_or_default();
            let trimmed: Vec<&str> = body.iter().map(|line| line.trim()).collect();
            if !trimmed.iter().any(|line| line.contains("/logs")) {
                problems.push(format!(
                    "the `{WITNESS_STEP}` witness reads no scoring job LOG - it does not bind to the STEP'S OWN OUTPUT, and the shape that does not is the API-record poll this PR replaced because it reddens on freshness"
                ));
            }
            if !trimmed.iter().any(|line| *line == fail_closed_guard().as_str()) {
                problems.push(format!(
                    "the `{WITNESS_STEP}` witness is not FAIL-CLOSED on the refusal - it needs `{}` (the needle DIRECTLY gating the `then`, so a `&& false` / `|| true` / `!` on the arm would never fire while the needle still appears), and `.github/workflows/scorecard.yml` is the file that proves the shape",
                    fail_closed_guard()
                ));
            }
            if !trimmed.contains(&"exit 1") {
                problems.push(format!(
                    "the `{WITNESS_STEP}` witness never EXITS NON-ZERO on the refusal - it can only warn, which is exactly what the action already did when it downgraded the 400 to a `::warning::`"
                ));
            }
        }
    }

    problems
}

/// The `run:` body of the step named `step_name` in a job, as its trimmed lines.
///
/// `None` where the job has no such step, or the step carries no block-scalar `run:` body - each
/// of which is a caller's failure rather than its pass, for the reason [`step::keyed_block`'s]
/// header gives about a parse that has desynchronised.
fn step_run_body<'a>(lines: &[&'a str], step_name: &str) -> Option<Vec<&'a str>> {
    let at = lines.iter().position(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("- name:")
            .is_some_and(|rest| rest.trim().trim_matches(['\'', '"']) == step_name)
    })?;
    let rest = lines.get(at + 1..)?;
    let run = rest.iter().position(|line| line.trim_start().starts_with("run: |"))?;
    let body = rest.get(run + 1..)?;
    let body: Vec<&str> = body
        .iter()
        .take_while(|line| line.starts_with(' ') || line.trim().is_empty())
        .map(|line| line.trim())
        .collect();
    (!body.is_empty()).then_some(body)
}

/// Is `name` a key of the workflow's own top-level mapping?
fn key_at_root(workflow: &str, name: &str) -> bool {
    scorecard::uncommented(workflow).any(|line| line.starts_with(&format!("{name}:")))
}

/// Does this line grant `write`?
fn writes(line: &str) -> bool {
    let value = line.split(':').nth(1).unwrap_or_default();
    let value = value.split('#').next().unwrap_or_default().trim();
    value == "write" || value == "write-all"
}

/// Does this job's own block grant `id-token: write`?
fn grants_id_token(lines: &[&str]) -> bool {
    lines
        .iter()
        .filter(|line| !line.trim_start().starts_with('#'))
        .any(|line| line.trim_start().starts_with("id-token:") && writes(line))
}

/// Every job of the workflow, by name, with its own lines.
fn jobs(workflow: &str) -> Vec<Job<'_>> {
    let mut jobs = Vec::new();
    let mut inside = false;
    for line in scorecard::uncommented(workflow) {
        if line.starts_with("jobs:") {
            inside = true;
            continue;
        }
        if inside && !line.trim().is_empty() && !line.starts_with(' ') {
            break;
        }
        let Some(rest) = line.strip_prefix("  ") else { continue };
        if !inside || rest.starts_with(' ') {
            continue;
        }
        let Some(name) = rest.strip_suffix(':') else { continue };
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            continue;
        }
        if let Some(lines) = step::job(workflow, name) {
            jobs.push((name.to_owned(), lines));
        }
    }
    jobs
}

/// Every action a job's steps call, by the name before the `@`.
fn uses<'a>(lines: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
    lines
        .iter()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| action(line))
}

/// The action a `uses:` line calls, or `None` where the line is not one.
fn action(line: &str) -> Option<String> {
    let rest = line.trim_start().trim_start_matches("- ").strip_prefix("uses:")?;
    let reference = rest.split('#').next()?.trim().trim_matches(['"', '\'']);
    let name = reference.split('@').next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The `runs-on:` label list of a job, in every spelling GitHub accepts for it.
fn labels(lines: &[&str]) -> Option<Vec<String>> {
    let line = lines
        .iter()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find(|line| line.trim_start().starts_with("runs-on:"))?;
    let value = line.split_once(':')?.1.split('#').next()?.trim();
    if let Some(flow) = value.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return Some(
            flow.split(',')
                .map(|label| label.trim().trim_matches(['"', '\'']).to_owned())
                .filter(|label| !label.is_empty())
                .collect(),
        );
    }
    if !value.is_empty() {
        return Some(vec![value.trim_matches(['"', '\'']).to_owned()]);
    }
    // A block sequence under the key, which is the one remaining legal spelling.
    let at = lines.iter().position(|candidate| candidate == line)?;
    let block: Vec<String> = lines
        .iter()
        .skip(at.saturating_add(1))
        .take_while(|line| line.trim_start().starts_with("- "))
        .map(|line| line.trim().trim_start_matches("- ").trim_matches(['"', '\'']).to_owned())
        .collect();
    (!block.is_empty()).then_some(block)
}

/// Each step of a job, as its own text, with the action it calls.
fn steps(lines: &[&str]) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    let mut depth = 0usize;
    for line in lines.iter().filter(|line| !line.trim_start().starts_with('#')) {
        let indent = line.len().saturating_sub(line.trim_start().len());
        if line.trim_start().starts_with("- ") && current.as_ref().is_none_or(|_| indent <= depth) {
            if let Some(block) = current.take() {
                steps.push(finish(&block));
            }
            depth = indent;
            current = Some(vec![line]);
        } else if let Some(block) = current.as_mut() {
            block.push(line);
        }
    }
    if let Some(block) = current.take() {
        steps.push(finish(&block));
    }
    steps
}

/// One collected step, with the action it calls.
fn finish(block: &[&str]) -> Step {
    let called = block.iter().find_map(|line| action(line));
    (block.join("\n"), called)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A workflow shaped exactly like the real one and accepted by every rule.
    const CLEAN: &str = concat!(
        "name: scorecard\n",
        "on:\n",
        "  push:\n",
        "    branches: [main]\n",
        "permissions: {}\n",
        "jobs:\n",
        "  score:\n",
        "    runs-on: ubuntu-latest\n",
        "    permissions:\n",
        "      id-token: write\n",
        "      contents: read\n",
        "    steps:\n",
        "      - uses: actions/checkout@aaaa # v7.0.1\n",
        "        with:\n",
        "          persist-credentials: false\n",
        "      - name: Scorecard\n",
        "        uses: ossf/scorecard-action@bbbb # v2.4.4\n",
        "        with:\n",
        "          publish_results: true\n",
        "\n",
        "  published:\n",
        "    runs-on: rust-mcp\n",
        "    permissions:\n",
        "      contents: read\n",
        "    steps:\n",
        "      - name: The publication was not refused\n",
        "        run: |\n",
        "          set -eu\n",
        "          gh api \"repos/${SLUG}/actions/jobs/${job}/logs\" > score.log\n",
        "          if grep -qF 'Unable to POST scorecard results to webapp' score.log; then\n",
        "            exit 1\n",
        "          fi\n",
        "          curl -sS https://api.scorecard.dev/\n",
    );

    /// Write `workflow` into a scratch tree and read it back through the entry point.
    fn found(workflow: &str) -> Vec<String> {
        let scratch = std::env::temp_dir().join(format!(
            "sutura-publication-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = scratch.join(scorecard::WORKFLOW);
        std::fs::create_dir_all(path.parent().expect("the workflow directory")).expect("the scratch tree");
        std::fs::write(&path, workflow).expect("the scratch workflow");
        let problems = problems(&scratch);
        std::fs::remove_dir_all(&scratch).expect("the scratch tree");
        problems
    }

    #[test]
    fn the_committed_workflow_would_be_accepted() {
        // THROUGH `problems` against the real tree: the one assertion that goes red if
        // `scorecard.yml` drifts back to a shape api.scorecard.dev refuses.
        use crate::repo;
        let root = repo::root().expect("repo root");
        let refusals = problems(&root);
        assert!(refusals.is_empty(), "{refusals:?}");
    }

    #[test]
    fn a_workflow_that_sends_nothing_is_subject_to_none_of_this() {
        // The API's rules are the API's. A shape it would refuse is not a defect in a workflow
        // that never reaches it - and this is the conditional that makes the rule about publishing
        // rather than about YAML taste.
        let quiet = CLEAN
            .replace("publish_results: true", "publish_results: false")
            .replace("runs-on: ubuntu-latest", "runs-on: rust-mcp");
        assert!(found(&quiet).is_empty(), "{:?}", found(&quiet));
    }

    #[test]
    fn every_refusal_reason_is_reached_through_the_entry_point() {
        assert!(found(CLEAN).is_empty(), "{:?}", found(CLEAN));

        // 1. THE ONE THAT SHIPPED. A self-hosted label, refused with HTTP 400 and downgraded to a
        //    warning by the action, so three runs read `success` while nothing was published.
        let refusals = found(&CLEAN.replace("runs-on: ubuntu-latest", "runs-on: rust-mcp"));
        assert!(
            refusals.iter().any(|r| r.contains("not one of the runner labels")),
            "{refusals:?}"
        );

        // 2. A label the allowlist does not carry, however current it is.
        let refusals = found(&CLEAN.replace("runs-on: ubuntu-latest", "runs-on: ubuntu-24.04"));
        assert!(refusals.iter().any(|r| r.contains("ubuntu-24.04")), "{refusals:?}");

        // 3. More than one label, and the flow spelling, so a list is read rather than a string.
        let refusals = found(&CLEAN.replace("runs-on: ubuntu-latest", "runs-on: [ubuntu-latest, self-hosted]"));
        assert!(
            refusals.iter().any(|r| r.contains("declares 2 runner labels")),
            "{refusals:?}"
        );

        // 4. A `run:` step in the scoring job - the trap this repository's own witness had to
        //    avoid, and the reason the witness is a separate job.
        let refusals = found(&CLEAN.replace(
            "      - name: Scorecard\n        uses: ossf/scorecard-action@bbbb # v2.4.4\n",
            "      - name: Scorecard\n        uses: ossf/scorecard-action@bbbb # v2.4.4\n      - name: Check\n        run: curl -sS https://api.scorecard.dev/\n",
        ));
        assert!(refusals.iter().any(|r| r.contains("has no `uses:`")), "{refusals:?}");

        // 5. An action nobody approved, even a pinned first-party one.
        let refusals = found(&CLEAN.replace("actions/checkout@aaaa", "actions/cache@aaaa"));
        assert!(refusals.iter().any(|r| r.contains("actions/cache")), "{refusals:?}");

        // 6. The token minted somewhere else in the same workflow.
        let refusals = found(&CLEAN.replace(
            "  published:\n    runs-on: rust-mcp\n    permissions:\n      contents: read\n",
            "  published:\n    runs-on: rust-mcp\n    permissions:\n      id-token: write\n",
        ));
        assert!(refusals.iter().any(|r| r.contains("`published` job")), "{refusals:?}");

        // 7. A workflow-level variable, default, or write permission.
        let refusals = found(&CLEAN.replace("permissions: {}\n", "permissions: {}\nenv:\n  A: b\n"));
        assert!(refusals.iter().any(|r| r.contains("workflow-level `env:`")), "{refusals:?}");
        let refusals = found(&CLEAN.replace("permissions: {}\n", "permissions: {}\ndefaults:\n  run:\n    shell: bash\n"));
        assert!(
            refusals.iter().any(|r| r.contains("workflow-level `defaults:`")),
            "{refusals:?}"
        );
        let refusals = found(&CLEAN.replace("permissions: {}\n", "permissions:\n  contents: write\n"));
        assert!(
            refusals.iter().any(|r| r.contains("workflow-level write permission")),
            "{refusals:?}"
        );
        // `write-all` as a SCALAR reaches the same rule as a mapping with one write scope: the
        // API refuses either, and one message for two spellings is one rule rather than two.
        let refusals = found(&CLEAN.replace("permissions: {}\n", "permissions: write-all\n"));
        assert!(
            refusals.iter().any(|r| r.contains("workflow-level write permission")),
            "{refusals:?}"
        );

        // 8. A container or a job-level variable on the scoring job.
        let refusals = found(&CLEAN.replace("  score:\n    runs-on", "  score:\n    container: alpine\n    runs-on"));
        assert!(refusals.iter().any(|r| r.contains("`container:`")), "{refusals:?}");

        // 9. Publishing with no job that runs the action - the results have no subject to verify.
        let refusals = found(&CLEAN.replace("uses: ossf/scorecard-action@bbbb", "uses: ossf/other-action@bbbb"));
        assert!(refusals.iter().any(|r| r.contains("no job in it runs")), "{refusals:?}");
    }

    #[test]
    fn a_witness_that_cannot_fail_is_refused_rather_than_trusted() {
        // The claim this PR makes is that the witness FAILS CLOSED on the scoring STEP'S OWN
        // OUTPUT - the `Unable to POST` line in this run's log, never a missing API record. Each
        // mutation below is the failure a review of this PR would reach for, and each reddens THIS
        // test by name rather than a message that merely still appears in the file.
        let guard = fail_closed_guard();
        assert!(found(CLEAN).is_empty(), "{:?}", found(CLEAN));

        // 1. THE MUTATION THE WITNESS EXISTS TO REDDEN ON: a `&& false` on the refusal arm. The
        //    needle still appears in the file, but the guard can never fire - a control that
        //    cannot fail is the exact defect this closes.
        let refusals = found(&CLEAN.replace(&guard, &guard.replace("; then", " && false; then")));
        assert!(refusals.iter().any(|r| r.contains("FAIL-CLOSED")), "{refusals:?}");

        // 2. The witness reads a missing API record instead of the scoring step's own log - the
        //    shape this PR replaced, because the API indexes with a lag of its own, so *no record
        //    yet* cannot be told from *refused*.
        let refusals = found(&CLEAN.replace(
            "gh api \"repos/${SLUG}/actions/jobs/${job}/logs\" > score.log\n",
            "curl -sS https://api.scorecard.dev/projects/some/repo\n",
        ));
        assert!(refusals.iter().any(|r| r.contains("STEP'S OWN OUTPUT")), "{refusals:?}");

        // 3. The witness can only warn, never refuse - the same fail-open the action itself had
        //    when it downgraded the 400 to a `::warning::`.
        let refusals = found(&CLEAN.replace("            exit 1\n", "            echo would-refuse\n"));
        assert!(refusals.iter().any(|r| r.contains("EXITS NON-ZERO")), "{refusals:?}");

        // 4. The witness is gone, by rename - the same dead-control shape as it being deleted.
        let refusals = found(&CLEAN.replace("The publication was not refused", "The publication landed"));
        assert!(refusals.iter().any(|r| r.contains("witness is MISSING")), "{refusals:?}");
    }
}
