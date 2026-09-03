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
//!
//! # Four ways an earlier draft of this read as compliance
//!
//! Measured by editing the real job one property at a time and re-running the gate, which is the
//! only way to find out whether an assertion would catch a regression:
//!
//! * the fork rule was searched for as TEXT, so `!(...)` around it - the rule inverted - passed,
//!   and so did the rule demoted to one step's `if:` while the job itself ran on every fork. It is
//!   now read off the job's own condition, negation refused;
//! * the print check knew `echo` and `cat`, so a bare `printenv SUTURA_BQ_KEY` added beside the
//!   write passed: the key, in a public log, with the write still in place so nothing else
//!   complained. [`PRINTS`] plus [`redirects_to_file`] is the fix, and `printenv` has to be in the
//!   list *because* the same verb with a redirect is how the key is stored;
//! * `set -eux` contains no `-x` substring, so tracing was invisible to any naive test. See
//!   [`traces`];
//! * `exit 1` was searched for over the WHOLE job, so a guard downgraded from an exit to a
//!   `continue` was clean as long as some other guard still exited - and *unset configuration
//!   fails rather than skips* is the property telekom/sutura#81 states most exactly. Each guard now
//!   has to reach one within [`GUARD_WINDOW`] lines.

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

/// Print verbs that put their argument in the log.
///
/// `printenv` is here BECAUSE it is how the key is written: the same command without a redirect
/// prints the key instead of storing it, and a check that knew only `echo` read that as clean.
const PRINTS: &[&str] = &["echo", "printf", "printenv", "cat "];

/// How far after a guard a non-zero `exit` may sit.
///
/// The shape in this job is three lines - the test, a message, the exit - and a `for` wrapping one
/// adds two. Deliberately a window rather than the whole job: `exit 1` ANYWHERE used to satisfy
/// this, so a guard downgraded to a `continue` beside an unrelated exit was invisible.
const GUARD_WINDOW: usize = 6;

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

/// Does this line send its output to a FILE rather than to the log?
///
/// `>&1` and `>&2` ARE the log, so they are not an exemption - which is the distinction between
/// the line that stores the key and every line that would reveal it.
fn redirects_to_file(line: &str) -> bool {
    line.split('>').skip(1).any(|rest| !rest.trim_start().starts_with('&'))
}

/// Does this line turn shell tracing on? `set -x`, `set -eux` and `set -o xtrace` all do.
///
/// Refused for the whole job rather than only where the key is in scope, because the job's own
/// comment gives the reason: a traced command line is a value in a public log.
fn traces(line: &str) -> bool {
    let Some(flags) = line.trim().strip_prefix("set ") else {
        return false;
    };
    flags.split_whitespace().any(|word| {
        word == "xtrace"
            || word
                .strip_prefix('-')
                .is_some_and(|set| !set.starts_with('-') && set.contains('x'))
    })
}

/// Does this line exit non-zero? A guard that does not reach one is a skip.
fn exits_non_zero(line: &str) -> bool {
    let Some(code) = line.trim().strip_prefix("exit ") else {
        return false;
    };
    code.trim().trim_end_matches(';').parse::<i32>().is_ok_and(|code| code != 0)
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

    // The JOB's own condition, at four spaces. A STEP's `if:` sits deeper and skips one step, so
    // the job still runs on a fork and reports a pass for a leg that never happened - and a `!`
    // states the rule backwards while satisfying any test that only looks for the text.
    match block
        .iter()
        .find(|line| line.strip_prefix("    ").is_some_and(|key| key.starts_with("if:")))
    {
        Some(line) if line.contains(FORK_RULE) && !line.contains('!') => {}
        Some(line) => problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job's own condition is `{}`, which has to test `{FORK_RULE}` \
             and may not negate it - skip where the runner had no choice, run where somebody in \
             this repository pushed. An event-name test answers both with one verdict and a `!` \
             answers both backwards",
            line.trim()
        )),
        None => problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job has no condition of its own, so it runs on a fork's pull \
             request - the `environment:` still withholds the key, but the leg then fails for want \
             of a secret or skips one step and reports a pass. A step's `if:` is not this rule"
        )),
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
    for (at, line) in bodies.iter().enumerate() {
        let is_guard = line.contains("-z ");
        let is_list = line.contains("for ") && line.contains(" in ");
        if !(is_guard || is_list) {
            continue;
        }
        for word in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if word.len() > 2 {
                guarded.insert(word.to_owned());
            }
        }
        if !bodies
            .iter()
            .skip(at.saturating_add(1))
            .take(GUARD_WINDOW)
            .copied()
            .any(exits_non_zero)
        {
            problems.push(format!(
                "{WORKFLOW}: the `{JOB}` job's guard `{}` reaches no non-zero `exit` within \
                 {GUARD_WINDOW} lines - unset configuration has to FAIL rather than skip, and a \
                 guard that warns and carries on is a skip that reads as a pass on an in-repo run",
                line.trim()
            ));
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
            let prints = PRINTS.iter().any(|verb| line.contains(verb)) && !redirects_to_file(line);
            if kind == "secrets" && line.contains(&name) && prints {
                problems.push(format!(
                    "{WORKFLOW}: the `{JOB}` job puts `{name}` on a line that prints - `{}`. A \
                     workflow log on a public repository is public, and the only reason a print \
                     verb may name the key at all is a redirect INTO a file",
                    line.trim()
                ));
            }
        }
    }
    if let Some(line) = bodies.iter().find(|line| traces(line)) {
        problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job turns shell tracing on - `{}`. Every command in that body \
             then reaches the log with its arguments, which is the channel this job's own comments \
             say `python3 -c` and `printenv` exist to avoid",
            line.trim()
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
    fn a_negated_fork_rule_states_the_rule_backwards_while_still_containing_it() {
        // The literal is present, so a text search finds it and the job runs on a fork ONLY.
        let inverted = CI.replace(
            "if: github.event_name == 'push' || github.event.pull_request.head.repo.full_name == github.repository",
            "if: github.event_name == 'push' || !(github.event.pull_request.head.repo.full_name == github.repository)",
        );
        let found = problems(&inverted);
        assert!(found.iter().any(|p| p.contains("may not negate it")), "{found:?}");
    }

    #[test]
    fn a_fork_rule_demoted_to_one_step_leaves_the_job_itself_running_on_a_fork() {
        let demoted = CI
            .replace(
                "    if: github.event_name == 'push' || github.event.pull_request.head.repo.full_name == github.repository\n",
                "",
            )
            .replace(
                "      - name: Acceptance leg\n",
                "      - name: Acceptance leg\n        if: github.event.pull_request.head.repo.full_name == github.repository\n",
            );
        let found = problems(&demoted);
        assert!(found.iter().any(|p| p.contains("has no condition of its own")), "{found:?}");
    }

    #[test]
    fn a_print_verb_naming_the_key_without_a_redirect_is_the_key_in_a_public_log() {
        // The write STAYS, so nothing else in the job is disturbed and only the print check can
        // produce the failure. `printenv` with a redirect is how the key is stored, so the verb
        // alone cannot decide it - and `>&2` is the log rather than a file.
        for added in ["printenv SUTURA_BQ_KEY", "echo \"$SUTURA_BQ_KEY\" >&2"] {
            let leaked = CI.replace(
                "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n",
                &format!("          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n          {added}\n"),
            );
            let found = problems(&leaked);
            assert_eq!(found.len(), 1, "{added}: {found:?}");
            assert!(found[0].contains("on a line that prints"), "{added}: {found:?}");
            assert!(found[0].contains(added), "{added}: {found:?}");
        }
    }

    #[test]
    fn shell_tracing_puts_every_argument_in_the_log_including_the_key() {
        // `set -eux` contains no `-x` as a substring, which is how a naive test misses it.
        let traced = CI.replace("          set -eu\n", "          set -eux\n");
        let found = problems(&traced);
        assert!(found.iter().any(|p| p.contains("turns shell tracing on")), "{found:?}");
        assert_eq!(
            problems(&CI.replace("set -eu", "set -o errexit -o nounset")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_guard_that_warns_and_carries_on_is_a_skip_beside_an_unrelated_exit() {
        // The key's own guard still exits, so `exit 1` is present in the job - which is exactly
        // what a whole-job search reads as compliance.
        let carries_on = CI.replace(
            "              echo \"the environment defines no $name\" >&2\n              exit 1",
            "              echo \"the environment defines no $name - skipping\" >&2\n              continue",
        );
        let found = problems(&carries_on);
        assert!(found.iter().any(|p| p.contains("reaches no non-zero `exit`")), "{found:?}");
        assert!(
            carries_on.contains("exit 1"),
            "the job still holds an exit for the key's guard"
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
