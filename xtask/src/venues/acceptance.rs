//! The acceptance venue's limit, which is a property of a workflow rather than of a page.
//!
//! `docs/where-identity-is-proven.md`'s third venue - a real dataset under a shared key - carries
//! one exclusion no page can enforce: **a fork's pull request gets no secret**, which is what
//! `docs/adr/0017` refused CI over and what its first amendment reversed on the strength of an
//! `environment:`. That, and the four other properties telekom/sutura#81 asks of the job, were
//! configured and held by nothing.
//!
//! What is read: its own job, an `environment:`, the fork rule on the HEAD REPOSITORY rather than
//! on the event name, the credential written outside the checkout and removed, no GitHub
//! expression in any shell body, the secret never on a line that prints, and every value the leg
//! is pointed at tested for emptiness with a non-zero exit - because unset configuration has to
//! FAIL rather than skip.
//!
//! **Fails closed**: no such job, or a job with no shell at all, is the scan breaking rather than
//! the job being clean.

use std::collections::{BTreeMap, BTreeSet};

/// The workflow that holds the acceptance venue's limit.
pub(super) const WORKFLOW: &str = ".github/workflows/ci.yml";

/// The job, at two spaces of indentation like every other job in that file.
pub(super) const JOB: &str = "bigquery-acceptance";

/// The fork rule, on the head repository.
///
/// **Not `github.event_name`**, and the difference is the whole property: a pull request from a
/// BRANCH of this repository can see the environment's secret and must be held to the leg, while a
/// fork's cannot and must skip. An event-name test collapses those two into one answer.
const FORK_RULE: &str = "github.event.pull_request.head.repo.full_name == github.repository";

/// One job's own lines, from its header to the next thing at the same indentation.
///
/// Two spaces is where a job's name sits and four is where its keys do, so a comment block
/// introducing the NEXT job - which this file writes at two spaces - ends the block rather than
/// joining it.
fn job(text: &str, name: &str) -> Option<Vec<String>> {
    let header = format!("  {name}:");
    let mut lines = text.lines().skip_while(|line| *line != header);
    lines.next()?;
    Some(
        lines
            .take_while(|line| line.trim().is_empty() || line.starts_with("    "))
            .map(str::to_owned)
            .collect(),
    )
}

/// The shell of every `run:` block in `block`.
///
/// A body ends at the first line indented no deeper than its own `run:` key, which is what keeps a
/// comment written between two steps out of it. A shell comment INSIDE a body stays in, because an
/// expression in one would still be an expression in the file.
fn shell(block: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut inside: Option<usize> = None;
    for line in block {
        let indent = line.len().saturating_sub(line.trim_start().len());
        if let Some(depth) = inside {
            if !line.trim().is_empty() && indent <= depth {
                inside = None;
            } else {
                out.push(line.as_str());
                continue;
            }
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("run:") {
            inside = Some(indent);
            out.push(trimmed);
        }
    }
    out
}

/// The environment names this job takes from a secret or a variable, and which of the two.
fn configured(block: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in block {
        let trimmed = line.trim();
        let Some((name, value)) = trimmed.split_once(": ") else {
            continue;
        };
        if !name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
            continue;
        }
        for kind in ["secrets.", "vars."] {
            if value.contains("${{") && value.contains(kind) {
                out.insert(name.to_owned(), kind.trim_end_matches('.').to_owned());
            }
        }
    }
    out
}

/// Everything wrong with the acceptance job.
pub(super) fn problems(text: &str) -> Vec<String> {
    let Some(block) = job(text, JOB) else {
        return vec![format!(
            "{WORKFLOW} has no `{JOB}` job - it is the only venue in the map that runs against a \
             real data system, and the map claims it exists"
        )];
    };
    let mut problems = Vec::new();
    let bodies = shell(&block);
    if bodies.is_empty() {
        return vec![format!(
            "{WORKFLOW}: the `{JOB}` job has no shell - the scan is broken, not the job"
        )];
    }
    let joined = bodies.join("\n");

    if !block.iter().any(|line| line.trim().starts_with("environment:")) {
        problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job declares no `environment:` - that is the mechanism that \
             withholds the key from a fork's pull request, and `docs/adr/0017` refused CI over \
             exactly the exposure it prevents"
        ));
    }

    if !block.iter().any(|line| line.contains(FORK_RULE)) {
        problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job's condition does not test `{FORK_RULE}` - skip where the \
             runner had no choice, run where somebody in this repository pushed. An event-name \
             test answers both with one verdict"
        ));
    }

    for line in &bodies {
        if line.contains("${{") {
            problems.push(format!(
                "{WORKFLOW}: the `{JOB}` job interpolates into a shell body - `{}`. A multi-line \
                 JSON key is the shape that defeats naive log masking, so a value reaches this \
                 shell through `env:` or not at all",
                line.trim()
            ));
        }
    }

    let credential = block
        .iter()
        .find_map(|line| line.trim().strip_prefix("GOOGLE_APPLICATION_CREDENTIALS: "))
        .map(str::trim);
    match credential {
        None => problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job points no `GOOGLE_APPLICATION_CREDENTIALS` at anything - \
             the leg would then read whatever credential the runner happens to have"
        )),
        Some(path) => {
            if !path.contains("runner.temp") {
                problems.push(format!(
                    "{WORKFLOW}: the `{JOB}` job's credential path is `{path}`, which is not under \
                     `runner.temp` - a key inside the checkout is one `git add .` from a public \
                     leak, and the secret sweep does not honour `.gitignore`"
                ));
            }
            let file = path.rsplit('/').next().unwrap_or(path);
            for (what, form) in [
                ("written", format!("> \"$RUNNER_TEMP/{file}\"")),
                ("removed", format!("rm -f \"$RUNNER_TEMP/{file}\"")),
            ] {
                if !joined.contains(&form) {
                    problems.push(format!(
                        "{WORKFLOW}: the `{JOB}` job never has the credential {what} as `{form}` - \
                         the path the leg reads and the path the job writes and deletes are one \
                         path or they are two answers"
                    ));
                }
            }
        }
    }

    let mut guarded = BTreeSet::new();
    for line in &bodies {
        let is_guard = line.contains("-z ");
        let is_list = line.contains("for ") && line.contains(" in ");
        for word in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if (is_guard || is_list) && word.len() > 2 {
                guarded.insert(word.to_owned());
            }
        }
    }
    for (name, kind) in configured(&block) {
        if !guarded.contains(&name) {
            problems.push(format!(
                "{WORKFLOW}: the `{JOB}` job reads `{name}` from `{kind}` and never tests it for \
                 emptiness - unset configuration has to FAIL rather than skip, because the fixture \
                 once reported three passes against no project"
            ));
        }
        for line in &bodies {
            if kind == "secrets" && line.contains(&name) && (line.contains("echo") || line.contains("cat ")) {
                problems.push(format!(
                    "{WORKFLOW}: the `{JOB}` job puts `{name}` on a line that prints - `{}`. A \
                     workflow log on a public repository is public",
                    line.trim()
                ));
            }
        }
    }
    if !joined.contains("exit 1") {
        problems.push(format!(
            "{WORKFLOW}: nothing in the `{JOB}` job exits non-zero, so an unset value can only \
             skip - and a skip on an in-repo run reads as a pass"
        ));
    }
    problems.extend(one_credential_mechanism(&block, &configured(&block)));

    problems
}

/// The key's expiry, read out of the job rather than remembered.
///
/// `docs/adr/0017` accepts a long-lived key as the *for now* and names what ends it: federation,
/// where the runner mints an OIDC token and **there is no key at all**. They are alternatives, so
/// this job holds exactly one - and holding both is the migration that stopped half way, where a
/// key nobody rotates outlives the paragraph that justified it.
///
/// **It reads THIS job because the record's own signal was wrong**: it named the arrival of
/// `id-token` anywhere, and `release.yml` had granted it for keyless signing two hours earlier.
fn one_credential_mechanism(block: &[String], config: &BTreeMap<String, String>) -> Vec<String> {
    let federated = block.iter().any(|line| line.contains("id-token: write"));
    let keyed = config.values().any(|kind| kind == "secrets");
    match (federated, keyed) {
        (true, true) => vec![format!(
            "{WORKFLOW}: the `{JOB}` job mints an OIDC token AND places a secret - those are the \
             two alternatives `docs/adr/0017` prices against each other, so holding both is a \
             half-finished migration and the key is the half nobody will notice is still there"
        )],
        (false, false) => vec![format!(
            "{WORKFLOW}: the `{JOB}` job authenticates with neither a secret nor `id-token: \
             write`, so it cannot be reaching the endpoint at all - a leg that authenticates with \
             nothing and passes is the fixture that once reported three passes against no project"
        )],
        (true, false) => vec![format!(
            "{WORKFLOW}: the `{JOB}` job is federated and holds no key, which is the state \
             `docs/adr/0017`'s expiry paragraph describes as the end of the service-account key. \
             That record still says a key is the cost of the evidence - amend it, then delete this \
             arm, because a gate whose failure is GOOD NEWS is one somebody will silence"
        )],
        (false, true) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{JOB, WORKFLOW, problems};

    /// The real acceptance job, with each property removed in turn.
    const CI: &str = "\
jobs:
  bigquery-acceptance:
    needs: [ci]
    if: github.event_name == 'push' || github.event.pull_request.head.repo.full_name == github.repository
    environment: bq-test
    steps:
      - name: Place the credential outside the checkout
        env:
          SUTURA_BQ_KEY: ${{ secrets.a_key }}
        run: |
          set -eu
          if [ -z \"${SUTURA_BQ_KEY:-}\" ]; then
            echo \"the environment holds no key\" >&2
            exit 1
          fi
          umask 077
          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"

      # A comment between two steps, mentioning ${{ }} the way this file does.
      - name: Acceptance leg
        env:
          GOOGLE_APPLICATION_CREDENTIALS: ${{ runner.temp }}/bq-key.json
          SUTURA_BQ_DATASET: ${{ vars.SUTURA_BQ_DATASET }}
        run: |
          set -eu
          for name in SUTURA_BQ_DATASET; do
            if [ -z \"$(printenv \"$name\" || true)\" ]; then
              echo \"the environment defines no $name\" >&2
              exit 1
            fi
          done
          nix run .#bigquery-acceptance

      - name: Remove the credential
        if: always()
        run: rm -f \"$RUNNER_TEMP/bq-key.json\"

  cross:
    needs: [ci]
";

    #[test]
    fn the_acceptance_job_as_it_stands_passes() {
        assert_eq!(problems(CI), Vec::<String>::new());
    }

    #[test]
    fn an_event_name_test_in_place_of_the_head_repository_fails() {
        let collapsed = CI.replace(
            "if: github.event_name == 'push' || github.event.pull_request.head.repo.full_name == github.repository",
            "if: github.event_name == 'push' || github.event_name == 'pull_request'",
        );
        let found = problems(&collapsed);
        assert!(
            found.iter().any(|p| p.contains("skip where the runner had no choice")),
            "{found:?}"
        );
    }

    #[test]
    fn a_job_with_no_environment_fails_because_that_is_the_fork_mechanism() {
        let exposed = CI.replace("    environment: bq-test\n", "");
        let found = problems(&exposed);
        assert!(found.iter().any(|p| p.contains("declares no `environment:`")), "{found:?}");
    }

    #[test]
    fn a_credential_inside_the_checkout_fails_and_so_does_one_the_leg_never_reads() {
        let in_tree = CI.replace(
            "GOOGLE_APPLICATION_CREDENTIALS: ${{ runner.temp }}/bq-key.json",
            "GOOGLE_APPLICATION_CREDENTIALS: ./bq-key.json",
        );
        let found = problems(&in_tree);
        assert!(found.iter().any(|p| p.contains("not under")), "{found:?}");

        let elsewhere = CI.replace(
            "printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"",
            "printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/other.json\"",
        );
        let found = problems(&elsewhere);
        assert!(
            found.iter().any(|p| p.contains("are one path or they are two answers")),
            "{found:?}"
        );
    }

    #[test]
    fn a_secret_interpolated_into_a_shell_body_fails() {
        let interpolated = CI.replace("printenv SUTURA_BQ_KEY >", "echo '${{ secrets.a_key }}' >");
        let found = problems(&interpolated);
        assert!(
            found.iter().any(|p| p.contains("interpolates into a shell body")),
            "{found:?}"
        );
    }

    #[test]
    fn a_configured_value_with_no_emptiness_test_fails_rather_than_skipping() {
        let unguarded = CI.replace(
            "          for name in SUTURA_BQ_DATASET; do\n",
            "          for name in NOTHING_AT_ALL; do\n",
        );
        let found = problems(&unguarded);
        assert!(found.iter().any(|p| p.contains("never tests it for emptiness")), "{found:?}");
    }

    #[test]
    fn a_job_that_is_not_there_at_all_fails_closed() {
        let found = problems("jobs:\n  ci:\n    runs-on: ubuntu-latest\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("has no `bigquery-acceptance` job"), "{found:?}");
    }

    #[test]
    fn a_key_and_a_federated_token_in_one_job_is_a_migration_that_stopped_half_way() {
        let both = CI.replace(
            "    needs: [ci]\n",
            "    needs: [ci]\n    permissions:\n      id-token: write\n",
        );
        let found = problems(&both);
        assert!(found.iter().any(|p| p.contains("half-finished migration")), "{found:?}");
    }

    #[test]
    fn a_job_authenticating_with_nothing_fails_rather_than_passing_against_no_project() {
        let neither = CI.replace("          SUTURA_BQ_KEY: ${{ secrets.a_key }}\n", "");
        let found = problems(&neither);
        assert!(found.iter().any(|p| p.contains("authenticates with neither")), "{found:?}");
    }

    #[test]
    fn the_day_the_key_is_gone_this_asks_for_the_record_to_be_amended() {
        // The gate whose failure is good news, and it says so in its own message: the alternative
        // is a record that goes on pricing a key nobody holds any more.
        let federated = CI
            .replace(
                "    needs: [ci]\n",
                "    needs: [ci]\n    permissions:\n      id-token: write\n",
            )
            .replace("          SUTURA_BQ_KEY: ${{ secrets.a_key }}\n", "");
        let found = problems(&federated);
        assert!(found.iter().any(|p| p.contains("expiry paragraph")), "{found:?}");
    }

    #[test]
    fn the_real_acceptance_job_is_what_this_half_is_for() {
        // The gate over the tree rather than over a fixture, which is what goes red when somebody
        // edits the job. `JOB` is named here too, so a rename cannot leave this reading a file
        // that no longer holds the job the page's venue means.
        let root = crate::repo::root().expect("the repo root");
        let workflow = std::fs::read_to_string(root.join(WORKFLOW)).expect(WORKFLOW);
        assert!(workflow.contains(JOB), "{WORKFLOW} no longer declares `{JOB}`");
        assert_eq!(problems(&workflow), Vec::<String>::new());
    }
}
