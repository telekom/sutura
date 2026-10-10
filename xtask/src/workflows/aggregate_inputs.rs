//! Which job each `ci-aggregate` env input reads, pinned against a committed table.
//!
//! **The finding this exists for.** `ci-aggregate`'s `BQ_SELECTED` env reads
//! `needs.ci.outputs.data_source_bigquery`, and nothing held that the source job was `ci`.
//! `identity-classify` publishes the same output, so rewiring `BQ_SELECTED` to
//! `needs.identity-classify.outputs.data_source_bigquery` resolves to the same value and every
//! gate stayed green - the aggregate still saw `'true'` or `''` and ruled correctly on a leg
//! whose selection was now being read from a DIFFERENT job's classification. The two classifications
//! can disagree (#983), so the source job is not a cosmetic detail.
//!
//! The rule: a committed table names, for each env var the `ci-aggregate` step declares, the job
//! whose output or result it MUST read. A different job name, or an expression that is not a
//! `needs.<job>.` reference at all, is refused. The table is the mechanism - a comment saying "read
//! from `ci`" is one edit from gone, and this is the same argument `obligations` made for the
//! step-level `if:` conditions it pins.
//!
//! **What it does NOT reach.** It reads the `env:` block of the single step in `ci-aggregate`,
//! not every `${{ }}` expression in the file. A `needs.<job>` reference inside the `run:` shell is
//! invisible - the shell runs on a runner, not in this gate, and a reference inside it is a
//! variable the shell reads rather than one GitHub expands. And it cannot tell whether the job it
//! names actually PUBLISHES the output - that is `affected`'s
//! `every_emitted_category_is_republished_as_a_ci_job_output`, which holds the two lists together
//! and is the property that makes a wrong source job dangerous: the replacement publishes the same
//! name, so the expression resolves and only this table notices the change.
//!
//! **The second rule: every `ci.yml` job is in the aggregate's `needs` or on [`UNAGGREGATED`].**
//! `cross` ran red on a pull request while `ci-aggregate` stayed green, because it was in no
//! `needs` and nothing compared the job list with that list. A job outside `needs` can fail
//! without failing a required context, so being outside it is a decision with a reason written
//! down, not an omission. What it does NOT reach: a job in a called workflow (it reads `ci.yml`'s
//! own keys, so `cross` counts and `cross-link.yml`'s `link` does not - the caller's result carries
//! it), and whether the `run:` shell reads a needed job's result at all - a `needs:` entry with no
//! `*_RESULT` env is only half a gate, and the env table above is what holds the other half.
//!
//! Its own module rather than a row in `obligations`, which holds STEP `if:` conditions by step
//! name: this holds ENV INPUT source jobs by env var name, over a different block of the same file.
//!
//! Two env inputs are pinned to their exact normalised condition, not only their source job: an
//! equivalent rewrite is refused until the table is edited, which fails closed. That pin holds
//! the TEXT, not that the condition is the right one - and it does not hold that
//! `CAUS_REQUIRED`/`PGD_REQUIRED` agree with the moved job's own `if:` (those read
//! `identity-classify`, these read `ci`; a disagreement fails closed in one direction and runs an
//! unrequested gate in the other). `E2E_REQUIRED`, `BQC_REQUIRED` and every `*_SELECTED` input
//! predate this and stay review-only.

use std::path::Path;

/// The committed table: for each `ci-aggregate` env var, the job whose output/result it must read.
///
/// `E2E_REQUIRED` is a compound expression, but every `needs.<job>.` reference in it names `ci`
/// (it reads `needs.ci.outputs.data_source_bigquery`, `needs.ci.outputs.catalog_datahub` and
/// `needs.ci.outputs.identity`). The table entry is the single job all of its `needs.` references
/// must name; a reference to any other job is refused.
const REQUIRED: &[(&str, &str)] = &[
    ("CI_RESULT", "ci"),
    ("KC_RESULT", "keycloak-served-test"),
    ("KC_SELECTED", "ci"),
    ("BQ_RESULT", "bigquery-driver-check"),
    ("BQ_SELECTED", "ci"),
    ("E2E_RESULT", "e2e-datahub-adbc"),
    ("E2E_REQUIRED", "ci"),
    ("ORACLE_RESULT", "oracle-tier"),
    ("ORACLE_SELECTED", "ci"),
    ("DH_RESULT", "ci-datahub-tier"),
    ("DH_SELECTED", "ci"),
    ("OM_RESULT", "ci-openmetadata-tier"),
    ("OM_SELECTED", "ci"),
    ("CUBE_RESULT", "ci-cube-tier"),
    ("CUBE_SELECTED", "ci"),
    ("BQC_RESULT", "bigquery-conformance"),
    ("BQC_REQUIRED", "ci"),
    ("CAUS_RESULT", "causality"),
    ("CAUS_REQUIRED", "ci"),
    ("PGD_RESULT", "postgres-linked-driver"),
    ("PGD_REQUIRED", "ci"),
    ("CROSS_RESULT", "cross"),
];

/// Env inputs whose whole condition is pinned, not only the job it reads: the selection the two
/// moved jobs (#1280) are required under. Exact text, so a neutralised copy (`false && ..`) is refused.
const EXPRESSIONS: &[(&str, &str)] = &[
    (
        "CAUS_REQUIRED",
        "needs.ci.outputs.rust == 'true' && github.event_name != 'push'",
    ),
    (
        "PGD_REQUIRED",
        "(needs.ci.outputs.run_all == 'true' || needs.ci.outputs.nix == 'true' || needs.ci.outputs.data_source_postgres == 'true') && github.event_name != 'push'",
    ),
];

/// The `ci.yml` jobs `ci-aggregate` deliberately does NOT `need`, each with why. A job in neither
/// this list nor `needs` is refused: its result reaches no required context, so a red run of it
/// blocks nothing - the state `cross` was in, and nothing noticed.
const UNAGGREGATED: &[(&str, &str)] = &[
    (
        "pr-cache",
        "warms the next push; a fork cannot see its environment, so a required result would never arrive",
    ),
    (
        "crap-comment",
        "posts the CRAP delta as a comment; `check-crap` inside `ci` gates the policy",
    ),
    (
        "identity-classify",
        "a classifier: its consumers are in `needs`, and the aggregate reads their selection from `ci`",
    ),
    (
        "demo-container",
        "a red demo must never block unrelated work; it is required at the release boundary instead",
    ),
    (
        "kind-smoke",
        "needs a container runtime; advisory on `demo-container`'s reasoning until its flake rate is known",
    ),
];

/// The job key of the aggregate.
const AGGREGATE: &str = "ci-aggregate";

/// How many env inputs this rule holds, for the success line.
pub(super) const fn held() -> usize {
    REQUIRED.len()
}

/// How many of those env inputs are pinned to their exact condition, for the success line.
pub(super) const fn pinned() -> usize {
    EXPRESSIONS.len()
}

/// One env-var line of the aggregate step, parsed into its key and the job names its expression
/// references.
struct EnvInput {
    line: usize,
    key: String,
    /// The value with the surrounding `${{ }}` removed, trimmed - the exact condition text the
    /// expression table pins.
    expression: String,
    /// Every `<job>` found in `needs.<job>.` within the value. Empty when the value is not a
    /// `needs.` reference at all - which is itself a violation, since the table is non-empty.
    jobs: Vec<String>,
}

/// Every broken table entry, empty when all env inputs read their committed source job.
///
/// Fails closed on an unreadable `ci.yml`, a missing `ci-aggregate` job, and a missing `env:`
/// block: each is a reader bug rather than a clean tree, and a pass over nothing is the failure
/// mode a text scan is most prone to.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(".github/workflows/ci.yml")) else {
        return vec![String::from(
            "ci-aggregate env-input rule: could not read .github/workflows/ci.yml",
        )];
    };
    let mut out = check(&text);
    out.extend(unaggregated(&text));
    out
}

/// Every `ci.yml` job is in `ci-aggregate`'s `needs` or on [`UNAGGREGATED`].
///
/// Fails closed when it finds no jobs or no `needs`: a scan that read nothing is broken, not clean.
fn unaggregated(text: &str) -> Vec<String> {
    let jobs = job_ids(text);
    let needs = aggregate_needs(text);
    if jobs.is_empty() || needs.is_empty() {
        return vec![format!(
            "ci-aggregate needs rule: found {} job(s) and {} `needs` entr(ies) - the scan is broken, \
             not the workflows",
            jobs.len(),
            needs.len()
        )];
    }
    let mut out = Vec::new();
    for (line, id) in &jobs {
        if id == AGGREGATE || needs.contains(id) || UNAGGREGATED.iter().any(|(name, _)| name == id) {
            continue;
        }
        out.push(format!(
            "ci.yml:{line}  job `{id}` is not in {AGGREGATE}'s `needs` and is not on UNAGGREGATED, so a \
             red `{id}` blocks no merge - add it to `needs` and read its result, or record in \
             xtask/src/workflows/aggregate_inputs.rs why it is exempt"
        ));
    }
    for (name, _) in UNAGGREGATED {
        if needs.iter().any(|id| id == name) {
            out.push(format!(
                "{AGGREGATE} needs rule: `{name}` is on UNAGGREGATED and in `needs` - drop the exemption"
            ));
        } else if !jobs.iter().any(|(_, id)| id == name) {
            out.push(format!(
                "{AGGREGATE} needs rule: UNAGGREGATED names `{name}` and no such job exists - the entry is stale"
            ));
        }
    }
    out
}

/// Every job key under `jobs:`, with its 1-based line.
fn job_ids(text: &str) -> Vec<(usize, String)> {
    let mut in_jobs = false;
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len().saturating_sub(trimmed.len());
        if indent == 0 {
            in_jobs = trimmed == "jobs:";
        } else if in_jobs
            && indent == 2
            && let Some(id) = trimmed.strip_suffix(':')
        {
            out.push((index.saturating_add(1), String::from(id)));
        }
    }
    out
}

/// The names in the aggregate's `needs:`, flow list (possibly over several lines) or block list.
fn aggregate_needs(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(job_at) = lines.iter().position(|line| *line == format!("  {AGGREGATE}:")) else {
        return Vec::new();
    };
    let indent = |line: &str| line.len().saturating_sub(line.trim_start().len());
    let body = lines
        .iter()
        .skip(job_at.saturating_add(1))
        .take_while(|line| line.trim().is_empty() || line.trim_start().starts_with('#') || indent(line) > 2);
    let mut raw = String::new();
    let mut open = false;
    for line in body {
        let code = line.split('#').next().unwrap_or_default().trim();
        if let Some(rest) = code.strip_prefix("needs:") {
            open = true;
            raw.push_str(rest);
        } else if open && (indent(line) > 4 || (raw.contains('[') && !raw.contains(']'))) {
            raw.push(' ');
            raw.push_str(code.strip_prefix("- ").unwrap_or(code));
        } else if open {
            break;
        }
    }
    raw.split(|c: char| matches!(c, ',' | '[' | ']') || c.is_whitespace())
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect()
}

/// The env-input rule itself, over the text, so a test can put a tree in front of it.
fn check(text: &str) -> Vec<String> {
    let inputs = env_inputs(text);
    if inputs.is_empty() {
        return vec![format!(
            "ci-aggregate env-input rule: no `env:` block found under {AGGREGATE}'s step - \
             the scan is broken, not the workflows"
        )];
    }
    let mut out = Vec::new();
    for input in &inputs {
        let Some(required) = REQUIRED.iter().find(|(key, _)| *key == input.key) else {
            // An env var the table does not name. The aggregate gained an input nobody pinned,
            // which is the same gap this rule exists for - add it to the table or remove it.
            out.push(format!(
                "ci.yml:{}  {AGGREGATE} env `{}` is not in the committed source-job table",
                input.line, input.key
            ));
            continue;
        };
        let expected = required.1;
        if input.jobs.is_empty() {
            out.push(format!(
                "ci.yml:{}  {AGGREGATE} env `{}` reads no `needs.<job>` reference - \
                 expected `needs.{expected}`",
                input.line, input.key
            ));
            continue;
        }
        for job in &input.jobs {
            if job != expected {
                out.push(format!(
                    "ci.yml:{}  {AGGREGATE} env `{}` reads `needs.{job}` - \
                     the committed table requires `needs.{expected}`",
                    input.line, input.key
                ));
            }
        }
        if let Some((_, want)) = EXPRESSIONS.iter().find(|(key, _)| *key == input.key)
            && input.expression != *want
        {
            out.push(format!(
                "ci.yml:{}  {AGGREGATE} env `{}` is `{}` - the committed table pins `{want}`, \
                 the condition a moved job is required under",
                input.line, input.key, input.expression
            ));
        }
    }
    // A table entry the aggregate no longer declares is a pin that has stopped holding: the env
    // var was removed and the table row is now dead weight, or the var was renamed and the old
    // row masks the new one's absence. Either is a reader bug rather than a clean tree.
    for (key, _) in REQUIRED {
        if !inputs.iter().any(|input| &input.key == key) {
            out.push(format!(
                "{AGGREGATE} env-input rule: the table pins `{key}` but the aggregate declares \
                 no such env var - the table is stale"
            ));
        }
    }
    out
}

/// Every `KEY: ${{ needs.<job>... }}` line under the `ci-aggregate` step's `env:` block.
///
/// Reads the text by indentation rather than parsing YAML, for the same reason every other reader
/// in this gate does: this runs in a nix sandbox with no YAML dependency. The `ci-aggregate` job
/// has exactly one step, and that step's `env:` block is the deeper-indented lines between the
/// `env:` key and the next key at the step's own depth.
fn env_inputs(text: &str) -> Vec<EnvInput> {
    let lines: Vec<&str> = text.lines().collect();
    // Find the `ci-aggregate:` job key at depth 2 (under `jobs:`).
    let Some(job_at) = lines.iter().position(|line| {
        let trimmed = line.trim_start();
        let indent = line.len().saturating_sub(trimmed.len());
        indent == 2 && trimmed == format!("{AGGREGATE}:")
    }) else {
        return Vec::new();
    };
    // Scan forward for the `env:` key within this job. The job ends at the next key at depth <= 2.
    // The `env:` sits inside the job's single step, so it is deeper than the job's own keys (depth
    // 4) and the step marker. A `run:` at the step's key depth means the step has no `env:`.
    let mut env_line = None;
    let mut env_depth = 0;
    let mut step_depth = 0;
    for (index, line) in lines.iter().enumerate().skip(job_at + 1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let indent = line.len().saturating_sub(trimmed.len());
        // A key at depth <= 2 is the next job - the aggregate declared no step with an env:.
        if indent <= 2 {
            return Vec::new();
        }
        // Record the step marker's depth when we first see a `- ` item.
        if step_depth == 0 && trimmed.starts_with("- ") {
            step_depth = indent;
            continue;
        }
        // `env:` is a step-level key, deeper than the step marker. Record it and stop scanning.
        // A step key that is `run:` (without a preceding `env:`) means no env: block.
        if step_depth > 0 && indent > step_depth && trimmed == "env:" {
            env_line = Some(index);
            env_depth = indent;
            break;
        }
        if step_depth > 0 && indent > step_depth && trimmed.starts_with("run:") {
            return Vec::new();
        }
    }
    let Some(env_at) = env_line else {
        return Vec::new();
    };
    // Collect the `KEY: value` lines deeper than `env:`, up to the next key at or above `env:`'s
    // depth.
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(env_at + 1) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len().saturating_sub(trimmed.len());
        if indent <= env_depth {
            break;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        // Skip non-env-var keys (should not happen at this depth, but fail safe).
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let expression = expression_of(value);
        let jobs = jobs_in(value);
        out.push(EnvInput {
            line: index.saturating_add(1),
            key: String::from(key),
            expression,
            jobs,
        });
    }
    out
}

/// The condition inside the `${{ }}`, trimmed; a value without the wrapper is returned trimmed.
fn expression_of(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(value)
        .trim()
        .to_owned()
}

/// Every `<job>` in a `needs.<job>.` reference within `value`.
///
/// The value is a `${{ ... }}` expression that may contain one or more `needs.<job>.<path>`
/// references. This extracts the job name after each `needs.` and before the next `.`. A value
/// with no `needs.` reference returns an empty vec, which the caller refuses.
fn jobs_in(value: &str) -> Vec<String> {
    let mut jobs = Vec::new();
    let mut rest = value;
    while let Some((_, after)) = rest.split_once("needs.") {
        // The job name is the identifier after `needs.` and before the next `.`.
        let job: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if !job.is_empty() {
            jobs.push(job);
        }
        rest = after;
    }
    jobs
}

#[cfg(test)]
mod tests {
    use core::fmt::Write as _;

    use super::*;

    /// The real `ci-aggregate` env block at the indentation `ci.yml` uses.
    fn aggregate_workflow(env: &str) -> String {
        let mut out = String::new();
        out.push_str("jobs:\n");
        out.push_str("  ci:\n");
        out.push_str("    runs-on: ubuntu-latest\n");
        out.push_str("    steps:\n");
        out.push_str("      - run: echo hi\n");
        out.push('\n');
        writeln!(out, "  {AGGREGATE}:").expect("writing into a String cannot fail");
        out.push_str("    runs-on: ubuntu-latest\n");
        out.push_str("    needs: [ci, keycloak-served-test, bigquery-driver-check, e2e-datahub-adbc, oracle-tier]\n");
        out.push_str("    if: ${{ !cancelled() }}\n");
        out.push_str("    steps:\n");
        out.push_str("      - name: Aggregate the category-gated legs\n");
        out.push_str("        env:\n");
        out.push_str(env);
        out.push('\n');
        out.push_str("        run: |\n");
        out.push_str("          set -eu\n");
        out.push_str("          echo done\n");
        out
    }

    /// The real env block, indented to depth 10 (two under `env:` at depth 8).
    fn real_env() -> String {
        [
            "          CI_RESULT: ${{ needs.ci.result }}",
            "          KC_RESULT: ${{ needs.keycloak-served-test.result }}",
            "          KC_SELECTED: ${{ needs.ci.outputs.identity }}",
            "          BQ_RESULT: ${{ needs.bigquery-driver-check.result }}",
            "          BQ_SELECTED: ${{ needs.ci.outputs.data_source_bigquery }}",
            "          E2E_RESULT: ${{ needs.e2e-datahub-adbc.result }}",
            "          E2E_REQUIRED: ${{ (github.event_name == 'push' || (github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository && github.event.pull_request.user.login != 'dependabot[bot]')) && (needs.ci.outputs.data_source_bigquery == 'true' || needs.ci.outputs.catalog_datahub == 'true' || needs.ci.outputs.identity == 'true') }}",
            "          ORACLE_RESULT: ${{ needs.oracle-tier.result }}",
            "          ORACLE_SELECTED: ${{ needs.ci.outputs.data_source_oracle }}",
            "          DH_RESULT: ${{ needs.ci-datahub-tier.result }}",
            "          DH_SELECTED: ${{ needs.ci.outputs.catalog_datahub }}",
            "          OM_RESULT: ${{ needs.ci-openmetadata-tier.result }}",
            "          OM_SELECTED: ${{ needs.ci.outputs.catalog_openmetadata }}",
            "          BQC_RESULT: ${{ needs.bigquery-conformance.result }}",
            "          BQC_REQUIRED: ${{ github.event_name == 'merge_group' && needs.ci.outputs.data_source_bigquery == 'true' }}",
            "          CAUS_RESULT: ${{ needs.causality.result }}",
            "          CAUS_REQUIRED: ${{ needs.ci.outputs.rust == 'true' && github.event_name != 'push' }}",
            "          PGD_RESULT: ${{ needs.postgres-linked-driver.result }}",
            "          PGD_REQUIRED: ${{ (needs.ci.outputs.run_all == 'true' || needs.ci.outputs.nix == 'true' || needs.ci.outputs.data_source_postgres == 'true') && github.event_name != 'push' }}",
            "          CROSS_RESULT: ${{ needs.cross.result }}",
        ]
        .join("\n")
    }

    #[test]
    fn the_real_ci_aggregate_passes() {
        let workflow = aggregate_workflow(&real_env());
        assert!(check(&workflow).is_empty(), "{}", check(&workflow).join("\n"));
    }

    #[test]
    fn bq_selected_rewired_to_identity_classify_fails() {
        let env = real_env().replace(
            "BQ_SELECTED: ${{ needs.ci.outputs.data_source_bigquery }}",
            "BQ_SELECTED: ${{ needs.identity-classify.outputs.data_source_bigquery }}",
        );
        let workflow = aggregate_workflow(&env);
        let problems = check(&workflow);
        let bq = problems
            .iter()
            .find(|p| p.contains("BQ_SELECTED"))
            .expect("BQ_SELECTED violation");
        assert!(bq.contains("reads `needs.identity-classify`"), "{bq}");
        assert!(bq.contains("requires `needs.ci`"), "{bq}");
    }

    #[test]
    fn kc_selected_rewired_to_identity_classify_fails() {
        let env = real_env().replace(
            "KC_SELECTED: ${{ needs.ci.outputs.identity }}",
            "KC_SELECTED: ${{ needs.identity-classify.outputs.identity }}",
        );
        let workflow = aggregate_workflow(&env);
        let problems = check(&workflow);
        let kc = problems
            .iter()
            .find(|p| p.contains("KC_SELECTED"))
            .expect("KC_SELECTED violation");
        assert!(kc.contains("reads `needs.identity-classify`"), "{kc}");
        assert!(kc.contains("requires `needs.ci`"), "{kc}");
    }

    #[test]
    fn an_env_var_not_in_the_table_fails() {
        let mut env = real_env();
        env.push('\n');
        env.push_str("          NEW_VAR: ${{ needs.ci.result }}");
        let workflow = aggregate_workflow(&env);
        let problems = check(&workflow);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("NEW_VAR") && p.contains("not in the committed")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_table_entry_the_aggregate_dropped_fails() {
        let env = real_env()
            .lines()
            .filter(|line| !line.contains("E2E_RESULT"))
            .collect::<Vec<_>>()
            .join("\n");
        let workflow = aggregate_workflow(&env);
        let problems = check(&workflow);
        assert!(
            problems.iter().any(|p| p.contains("E2E_RESULT") && p.contains("stale")),
            "{problems:?}"
        );
    }

    #[test]
    fn an_env_reading_no_needs_reference_fails() {
        let env = real_env().replace("CI_RESULT: ${{ needs.ci.result }}", "CI_RESULT: ${{ github.event_name }}");
        let workflow = aggregate_workflow(&env);
        let problems = check(&workflow);
        assert!(
            problems.iter().any(|p| p.contains("CI_RESULT") && p.contains("no `needs")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_missing_aggregate_job_fails() {
        let workflow = "jobs:\n  ci:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo\n";
        assert!(
            check(workflow).iter().any(|p| p.contains("no `env:` block")),
            "a missing aggregate job should fail with 'no env: block'"
        );
    }

    #[test]
    fn a_missing_env_block_fails() {
        let workflow = format!("jobs:\n  {AGGREGATE}:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo\n");
        let problems = check(&workflow);
        assert!(problems.iter().any(|p| p.contains("no `env:` block")), "{problems:?}");
    }

    #[test]
    fn jobs_in_extracts_multiple_references() {
        let value = "${{ needs.ci.outputs.a == 'true' || needs.ci.outputs.b == 'true' }}";
        assert_eq!(
            jobs_in(value),
            vec!["ci".to_owned(), "ci".to_owned()],
            "two references to the same job"
        );
    }

    #[test]
    fn jobs_in_returns_empty_for_no_needs() {
        assert_eq!(jobs_in("${{ github.event_name }}"), Vec::<String>::new());
    }

    #[test]
    fn a_neutralised_moved_job_condition_fails() {
        for key in ["CAUS_REQUIRED", "PGD_REQUIRED"] {
            let env = real_env().replace(&format!("{key}: ${{{{"), &format!("{key}: ${{{{ false &&"));
            let workflow = aggregate_workflow(&env);
            let problems = check(&workflow);
            assert!(
                problems
                    .iter()
                    .any(|p| p.contains(key) && p.contains("the committed table pins")),
                "{key}: {problems:?}"
            );
        }
    }

    #[test]
    fn a_condition_with_an_arm_dropped_fails() {
        let env = real_env()
            .lines()
            .map(|line| {
                if line.starts_with("          CAUS_REQUIRED:") {
                    line.replace(" && github.event_name != 'push' }}", " }}")
                } else {
                    String::from(line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let workflow = aggregate_workflow(&env);
        let problems = check(&workflow);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("CAUS_REQUIRED") && p.contains("the committed table pins")),
            "{problems:?}"
        );
    }

    /// A `ci.yml` of the exempt jobs, `ci`, `extra` and the aggregate, whose `needs:` is `needs`.
    fn jobs_fixture(extra: &[&str], needs: &str) -> String {
        let mut out = String::from("on:\n  push:\n    branches: [main]\njobs:\n");
        for job in UNAGGREGATED
            .iter()
            .map(|(name, _)| *name)
            .chain(["ci"])
            .chain(extra.iter().copied())
        {
            writeln!(out, "  {job}:\n    runs-on: ubuntu-latest").expect("writing into a String cannot fail");
        }
        writeln!(out, "  {AGGREGATE}:\n    needs: {needs}\n    if: ${{{{ !cancelled() }}}}")
            .expect("writing into a String cannot fail");
        out
    }

    #[test]
    fn a_job_the_aggregate_does_not_need_and_no_exemption_names_is_refused() {
        // THE DEFECT: `cross` ran red, `ci-aggregate` stayed green, and nothing read the gap.
        let problems = unaggregated(&jobs_fixture(&["cross"], "[ci]"));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("job `cross`") && problems[0].contains("blocks no merge"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_needed_job_passes_in_every_list_shape() {
        for needs in [
            "[ci, cross]",
            "[\n      ci,\n      # a note\n      cross, # another\n    ]",
            "\n      - ci\n      - cross",
        ] {
            let problems = unaggregated(&jobs_fixture(&["cross"], needs));
            assert!(problems.is_empty(), "`{needs}`: {problems:?}");
        }
    }

    #[test]
    fn an_exemption_that_contradicts_or_outlives_its_job_is_refused() {
        let contradicted = unaggregated(&jobs_fixture(&[], "[ci, kind-smoke]"));
        assert!(
            contradicted
                .iter()
                .any(|p| p.contains("`kind-smoke`") && p.contains("drop the exemption")),
            "{contradicted:?}"
        );
        let gone = jobs_fixture(&[], "[ci]").replace("  kind-smoke:\n", "  smoke-renamed:\n");
        let stale = unaggregated(&gone);
        assert!(
            stale.iter().any(|p| p.contains("`kind-smoke`") && p.contains("stale")),
            "{stale:?}"
        );
    }

    #[test]
    fn a_scan_that_reads_nothing_is_not_clean() {
        assert!(unaggregated("jobs:\n").iter().any(|p| p.contains("scan is broken")));
    }

    #[test]
    fn the_committed_ci_aggregate_pins_hold() {
        // Over the REAL ci.yml, which the fixture cells above do not read.
        let root = crate::repo::root().expect("the repo root");
        let problems = problems(&root);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
