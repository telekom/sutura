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
                && lines.contains(&"      fast-checked: true")
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
            && lines.contains(
                &"    if: ${{ !cancelled() && github.event_name == 'pull_request' && needs.ci-aggregate.result == 'success' }}",
            )
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

#[cfg(test)]
mod tests {
    use super::check;

    /// A minimal, release-shaped `.github/workflows/release.yml`, with the `qa-plan` job's
    /// `with:` block swapped in by the caller - this is the one axis the test below varies.
    fn release_fixture(qa_plan_with: &str) -> String {
        format!(
            "jobs:\n\
             \x20 gates:\n\
             \x20   runs-on: ubuntu-latest\n\
             \x20   steps:\n\
             \x20     - run: echo gates\n\
             \n\
             \x20 qa-plan:\n\
             \x20   needs: [gates]\n\
             \x20   uses: $/.github/workflows/qa.yml\n\
             \x20   with:\n\
             {qa_plan_with}\
             \n\
             \x20 qa:\n\
             \x20   needs: [qa-plan]\n\
             \x20   uses: $/.github/workflows/qa.yml\n\
             \x20   with:\n\
             \x20     run-all-checks: true\n\
             \x20     suite-plan: ${{{{ needs.qa-plan.outputs.suite-plan }}}}\n\
             \n\
             \x20 e2e-datahub-adbc:\n\
             \x20   needs: [qa-plan]\n\
             \x20   environment: bq-test\n\
             \x20   steps:\n\
             \x20     - name: Run live suite\n\
             \x20       env:\n\
             \x20         SUTURA_BQ_KEY: ${{{{ secrets.svc_suturua_bq_ci }}}}\n\
             \x20       run: bash nix/ci-live-qa.sh e2e-datahub-adbc\n\
             \n\
             \x20 bigquery-conformance:\n\
             \x20   needs: [qa-plan]\n\
             \x20   environment: bq-test\n\
             \x20   steps:\n\
             \x20     - name: Run live suite\n\
             \x20       env:\n\
             \x20         SUTURA_BQ_KEY: ${{{{ secrets.svc_suturua_bq_ci }}}}\n\
             \x20       run: bash nix/ci-live-qa.sh bigquery-conformance\n\
             \n\
             \x20 publish:\n\
             \x20   needs: [qa-plan, qa, bigquery-conformance, e2e-datahub-adbc]\n\
             \x20   steps:\n\
             \x20     - run: echo publish\n"
        )
    }

    /// A scratch directory removed on drop, so a panic between creation and cleanup does not
    /// leak it - unlike a bare `remove_dir_all` at the end of the function, which a failed
    /// assertion skips entirely.
    struct ScratchDir(std::path::PathBuf);

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            if let Err(err) = std::fs::remove_dir_all(&self.0) {
                eprintln!("scratch dir cleanup failed for {}: {err}", self.0.display());
            }
        }
    }

    /// Writes `release` to a scratch `.github/workflows/release.yml` and returns `check`'s
    /// verdict over it - `check` reads `release.yml` off disk, so this is the cheapest fixture
    /// that reaches it without a second, filesystem-free entry point.
    fn release_problems(release: &str) -> Vec<String> {
        let dir = ScratchDir(std::env::temp_dir().join(format!(
            "queue-validated-fixture-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        )));
        let workflows = dir.0.join(".github/workflows");
        std::fs::create_dir_all(&workflows).expect("scratch workflows dir");
        std::fs::write(workflows.join("release.yml"), release).expect("write release.yml fixture");
        check(&dir.0, "jobs:\n")
    }

    const RELEASE_RULE: &str =
        "queue-validated rule: release publication must require full QA and both direct live jobs without PR verdicts";

    #[test]
    fn release_qa_plan_without_fast_checked_is_red() {
        let release = release_fixture("\x20     plan-only: true\n\x20     run-all-checks: true\n");
        let problems = release_problems(&release);
        assert!(problems.iter().any(|p| p == RELEASE_RULE), "{problems:?}");
    }

    #[test]
    fn release_qa_plan_with_fast_checked_is_green() {
        let release = release_fixture("\x20     plan-only: true\n\x20     fast-checked: true\n\x20     run-all-checks: true\n");
        let problems = release_problems(&release);
        assert!(!problems.iter().any(|p| p == RELEASE_RULE), "{problems:?}");
    }
}
