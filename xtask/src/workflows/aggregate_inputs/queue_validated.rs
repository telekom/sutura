//! `ci.yml`'s `queue-validated` job (#1696): on a push to `main`, a leg the merge queue already ran
//! green on the same commit is skipped. This holds the job's text and every place its outputs are
//! read.
//!
//! **What it holds.** The job is pinned to [`BODY`] exactly: no `if:` and no `needs`, so it is
//! never skipped and never takes a dependent down with it; its step gated to a push to `main`;
//! `filter=latest`; and every check its query makes - the run's `event`, `head_sha`, `status` and
//! `conclusion`, and the job's `head_sha`, `status` and `conclusion`, `success` only. Each leg in
//! [`LEGS`] ends its job `if:` and its `ci-aggregate` `*_SELECTED` input with the conjunct
//! `needs.queue-validated.outputs.<its own output> != 'true'` and reads the job nowhere else in
//! either. Any other non-comment line naming the job is refused, so a leg that keeps its push run
//! (`e2e-datahub-adbc`, `cross`, `demo-container`, `kind-smoke`) cannot read it. No job may report
//! a leg's id as its `name:`, because the query matches a job by the name it reported.
//!
//! **What it does NOT hold.** GitHub's run record is trusted: that a run listed for
//! `event=merge_group` and this `head_sha` ran this commit's tree, and that its job reporting a
//! leg's name with `success` is that leg. That run executed the leg under `merge_group`, not `push`;
//! that no leg's steps behave differently between the two is review, not this rule. A `name:`
//! built from an expression is not evaluated. And the text before the conjunct is not held to be
//! the leg's selection - that stays review-only, as it was. A top-level `||` there only narrows the
//! skip: whenever the output is not `'true'` the expression is the one it replaced.

use super::{AGGREGATE, env_inputs, job_ids};
use crate::workflows::step;

/// The job key.
pub(super) const JOB: &str = "queue-validated";

/// A leg a push to `main` may skip: its job id, the output naming it, its `ci-aggregate` input.
type Leg = (&'static str, &'static str, &'static str);

/// Every [`Leg`].
const LEGS: &[Leg] = &[
    ("keycloak-served-test", "keycloak_served_test", "KC_SELECTED"),
    ("bigquery-driver-check", "bigquery_driver_check", "BQ_SELECTED"),
    ("multi-node", "multi_node", "MN_SELECTED"),
    ("k3s-autoscaling", "k3s_autoscaling", "K3S_SELECTED"),
    ("oracle-tier", "oracle_tier", "ORACLE_SELECTED"),
    ("ci-datahub-tier", "ci_datahub_tier", "DH_SELECTED"),
    ("ci-openmetadata-tier", "ci_openmetadata_tier", "OM_SELECTED"),
    ("ci-cube-tier", "ci_cube_tier", "CUBE_SELECTED"),
];

/// The job's body below its key, exact. An edit to the workflow is an edit here, in review.
const BODY: &str = r#"    runs-on: ubuntu-latest
    timeout-minutes: 5
    permissions:
      contents: read
      actions: read
    outputs:
      keycloak_served_test: ${{ steps.lookup.outputs.keycloak_served_test }}
      bigquery_driver_check: ${{ steps.lookup.outputs.bigquery_driver_check }}
      multi_node: ${{ steps.lookup.outputs.multi_node }}
      k3s_autoscaling: ${{ steps.lookup.outputs.k3s_autoscaling }}
      oracle_tier: ${{ steps.lookup.outputs.oracle_tier }}
      ci_datahub_tier: ${{ steps.lookup.outputs.ci_datahub_tier }}
      ci_openmetadata_tier: ${{ steps.lookup.outputs.ci_openmetadata_tier }}
      ci_cube_tier: ${{ steps.lookup.outputs.ci_cube_tier }}
    steps:
      - name: Legs the merge queue ran green on this commit
        id: lookup
        if: github.event_name == 'push' && github.ref == 'refs/heads/main'
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          set -u
          runs="$(gh api --paginate "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs?event=merge_group&head_sha=$GITHUB_SHA&per_page=100" \
            | jq -r --arg sha "$GITHUB_SHA" '.workflow_runs[] | select(.event == "merge_group" and .head_sha == $sha and .status == "completed" and .conclusion == "success") | .id')" || runs=""
          passed=""
          for run in $runs; do
            jobs="$(gh api --paginate "repos/$GITHUB_REPOSITORY/actions/runs/$run/jobs?filter=latest&per_page=100" \
              | jq -r --arg sha "$GITHUB_SHA" '.jobs[] | select(.head_sha == $sha and .status == "completed" and .conclusion == "success") | .name')" || jobs=""
            passed="$passed"$'\n'"$jobs"
          done
          for leg in keycloak-served-test bigquery-driver-check multi-node k3s-autoscaling oracle-tier ci-datahub-tier ci-openmetadata-tier ci-cube-tier; do
            if printf '%s\n' "$passed" | grep -qxF -- "$leg"; then hit=true; else hit=false; fi
            echo "$leg: $hit"
            echo "${leg//-/_}=$hit" >> "$GITHUB_OUTPUT"
          done"#;

/// Whether `key` is a `ci-aggregate` input allowed to read this job: one of the [`LEGS`].
pub(super) fn reads(key: &str) -> bool {
    LEGS.iter().any(|(_, _, selected)| *selected == key)
}

/// Every broken obligation of the job and its readers, empty when all hold. Fails closed on a
/// missing job, leg or aggregate input.
pub(super) fn check(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    match step::job(text, JOB) {
        None => out.push(format!("{JOB} rule: ci.yml declares no `{JOB}` job")),
        Some(lines) if lines.join("\n").trim_end() != BODY => out.push(format!(
            "{JOB} rule: ci.yml's `{JOB}` job differs from the text pinned in \
             xtask/src/workflows/aggregate_inputs/queue_validated.rs - every check it makes is what \
             lets a push skip a leg, so change both in one commit"
        )),
        Some(_) => {}
    }
    let inputs = env_inputs(text);
    for (leg, output, selected) in LEGS {
        if !job_if(text, leg).is_some_and(|gate| gated(&gate, output)) {
            out.push(format!(
                "{JOB} rule: `{leg}`'s `if:` must end in `{}` and read `{JOB}` nowhere else",
                conjunct(output)
            ));
        }
        if !inputs
            .iter()
            .any(|input| input.key == *selected && gated(&input.expression, output))
        {
            out.push(format!(
                "{JOB} rule: {AGGREGATE} env `{selected}` must end in `{}` and read `{JOB}` nowhere \
                 else, as `{leg}`'s `if:` does",
                conjunct(output)
            ));
        }
    }
    out.extend(stray(text));
    out.extend(impostors(text));
    out
}

/// The conjunct a leg's `if:` and its `*_SELECTED` end in.
fn conjunct(output: &str) -> String {
    format!(" && needs.{JOB}.outputs.{output} != 'true'")
}

/// `expression` ends in `output`'s conjunct and names the job nowhere before it.
fn gated(expression: &str, output: &str) -> bool {
    expression
        .strip_suffix(&conjunct(output))
        .is_some_and(|before| !before.is_empty() && !before.contains(JOB))
}

/// A job's own `if:`, `${{ }}` stripped and whitespace collapsed. `None` when it has none on one line.
fn job_if(text: &str, job: &str) -> Option<String> {
    step::job(text, job)?.into_iter().find_map(|line| {
        let value = line.strip_prefix("    if:")?.trim();
        let inner = value
            .strip_prefix("${{")
            .and_then(|rest| rest.strip_suffix("}}"))
            .unwrap_or(value);
        Some(inner.split_whitespace().collect::<Vec<_>>().join(" "))
    })
}

/// Every non-comment line naming the job outside the job itself, a leg's `needs:` or `if:`, and
/// the aggregate's `needs` or a leg's `*_SELECTED`.
fn stray(text: &str) -> Vec<String> {
    let jobs = job_ids(text);
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let at = index.saturating_add(1);
        if line.trim_start().starts_with('#') || !line.contains(JOB) {
            continue;
        }
        let owner = jobs.iter().rev().find(|(start, _)| *start <= at).map(|(_, id)| id.as_str());
        if !allowed(owner, line) {
            out.push(format!(
                "ci.yml:{at}  {JOB} rule: `{JOB}` is named outside the job, the `needs:` and `if:` of \
                 its legs and their `*_SELECTED` in {AGGREGATE} - a leg that keeps its push run may \
                 not read it"
            ));
        }
    }
    out
}

/// Whether `line`, inside job `owner`, is a place the job may be named.
fn allowed(owner: Option<&str>, line: &str) -> bool {
    let trimmed = line.trim_start();
    match owner {
        Some(JOB) => true,
        Some(AGGREGATE) => {
            line.starts_with("    needs:")
                || trimmed.trim_end_matches(',') == JOB
                || trimmed.split_once(':').is_some_and(|(key, _)| reads(key))
        }
        Some(owner) => {
            LEGS.iter().any(|(leg, ..)| *leg == owner) && (line.starts_with("    needs:") || line.starts_with("    if:"))
        }
        None => false,
    }
}

/// Every job-level `name:` that reports a leg's id: the query would take its result for the leg's.
fn impostors(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(name) = line.strip_prefix("    name:") else {
            continue;
        };
        let name = name.trim().trim_matches(['"', '\'']);
        if LEGS.iter().any(|(leg, ..)| *leg == name) {
            out.push(format!(
                "ci.yml:{}  {JOB} rule: a job reports the name `{name}`, and `{JOB}` would read its \
                 result as that leg's",
                index.saturating_add(1)
            ));
        }
    }
    out
}
