//! Keep PR verdict collection on the queue event and pin which callers consume it.

use std::path::Path;

use super::job_needs;
use crate::workflows::step;

pub(super) fn check(root: &Path, ci: &str) -> Vec<String> {
    let release = std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap_or_default();
    let mut out = Vec::new();
    let lookup = step::named_step(ci, "identity-classify", "Find reusable PR verdicts");
    let lookup_valid = lookup.is_some_and(|lines| {
        lines.contains(&"        id: pr-verdict")
            && lines.contains(&"        if: github.event_name == 'merge_group'")
            && lines.contains(&"        continue-on-error: true")
            && lines.contains(&"        run: bash nix/ci-pr-verdict.sh")
    });
    if !lookup_valid {
        out.push(String::from(
            "queue-validated rule: the lookup must use the checked queue event and fall back on failure",
        ));
    }
    let ci_ready = ["identity-classify", "qa-plan"];
    let release_ready = ["qa-plan"];
    let qa_valid = job_needs(ci, "qa-plan") == ["identity-classify"]
        && step::job(ci, "qa-plan").is_some_and(|lines| {
            lines.contains(&"    uses: $/.github/workflows/qa.yml")
                && lines.contains(&"      fast-checked: true")
                && lines.contains(&"      plan-only: true")
                && lines.contains(&"      tested-commit: ${{ needs.identity-classify.outputs.tested_commit }}")
                && lines.contains(&"      green-jobs: ${{ needs.identity-classify.outputs.green_jobs }}")
        })
        && qa_uses_plan(ci, &ci_ready)
        && live_jobs_use_plan(ci, &ci_ready);
    if !qa_valid {
        out.push(String::from(
            "queue-validated rule: ordinary QA must wait for fast checks, use their evidence, and share the plan with the direct live jobs",
        ));
    }
    let release_valid = job_needs(&release, "qa-plan") == ["gates"]
        && step::job(&release, "qa-plan").is_some_and(|lines| {
            lines.contains(&"    uses: $/.github/workflows/qa.yml")
                && lines.contains(&"      plan-only: true")
                && lines.contains(&"      run-all-checks: true")
                && !lines
                    .iter()
                    .any(|line| line.contains("green-jobs:") || line.contains("tested-commit:"))
        })
        && qa_uses_plan(&release, &release_ready)
        && step::job(&release, "qa").is_some_and(|lines| lines.contains(&"      run-all-checks: true"))
        && live_jobs_use_plan(&release, &release_ready)
        && ["qa-plan", "qa", "bigquery-conformance", "e2e-datahub-adbc"]
            .iter()
            .all(|required| job_needs(&release, "publish").iter().any(|need| need == *required));
    if !release_valid {
        out.push(String::from(
            "queue-validated rule: release publication must require full QA and both direct live jobs without PR verdicts",
        ));
    }
    if !step::job(ci, "qa-receipt").is_some_and(|lines| {
        lines.contains(&"    name: QA checkout (${{ github.sha }})")
            && lines.contains(&"    if: github.event_name == 'pull_request'")
    }) || !job_needs(ci, "qa-receipt").iter().any(|need| need == "ci-aggregate")
    {
        out.push(String::from(
            "queue-validated rule: checkout receipts must attest the PR checkout after the required checks",
        ));
    }
    if !job_needs(ci, "causality-receipt").iter().any(|need| need == "causality")
        || !step::job(ci, "causality-receipt").is_some_and(|lines| {
            lines.contains(&"    if: github.event_name == 'pull_request' && needs.causality.outputs.reusable == 'true'")
        })
    {
        out.push(String::from(
            "queue-validated rule: a PR causality receipt requires the causality job's reusable verdict",
        ));
    }
    out
}

fn qa_uses_plan(workflow: &str, ready: &[&str]) -> bool {
    job_needs(workflow, "qa").iter().map(String::as_str).eq(ready.iter().copied())
        && step::job(workflow, "qa").is_some_and(|lines| {
            lines.contains(&"    uses: $/.github/workflows/qa.yml")
                && lines.contains(&"      suite-plan: ${{ needs.qa-plan.outputs.suite-plan }}")
                && !lines
                    .iter()
                    .any(|line| line.contains("green-jobs:") || line.contains("tested-commit:"))
        })
}

fn live_jobs_use_plan(workflow: &str, ready: &[&str]) -> bool {
    ["bigquery-conformance", "e2e-datahub-adbc"].iter().all(|job| {
        job_needs(workflow, job).iter().map(String::as_str).eq(ready.iter().copied())
            && step::job(workflow, job).is_some_and(|lines| lines.contains(&"    environment: bq-test"))
            && step::named_step(workflow, job, "Run live suite").is_some_and(|lines| {
                lines.contains(&"          SUTURA_BQ_KEY: ${{ secrets.svc_suturua_bq_ci }}")
                    && lines
                        .iter()
                        .any(|line| *line == format!("        run: bash nix/ci-live-qa.sh {job}"))
            })
    })
}
