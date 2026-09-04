//! The acceptance venue's limit, which is a property of a workflow rather than of a page.
//!
//! `docs/where-identity-is-proven.md`'s third venue - a real dataset under a shared key - carries
//! one exclusion no page can enforce: **a fork's pull request gets no secret**, which is what
//! `docs/adr/0017` refused CI over and what its first amendment reversed on the strength of an
//! `environment:`. That, and every other property telekom/sutura#81 asks of the job, were
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
//!   complained. [`shape::PRINTS`] plus [`shape::redirects_to_file`] is the fix, and `printenv` has
//!   to be in the list *because* the same verb with a redirect is how the key is stored;
//! * `set -eux` contains no `-x` substring, so tracing was invisible to any naive test. See
//!   [`traces`];
//! * `exit 1` was searched for over the WHOLE job, so a guard downgraded from an exit to a
//!   `continue` was clean as long as some other guard still exited - and *unset configuration
//!   fails rather than skips* is the property telekom/sutura#81 states most exactly. Each guard now
//!   has to reach one within [`GUARD_WINDOW`] lines.
//!
//! # And three found by review of the draft above, which is the same list one round later
//!
//! * **the expiry signal was the proxy this whole record exists to retire.** `federated` read
//!   `id-token: write`, narrowed from *any workflow* to *this job* - which retires the one false
//!   positive and keeps the class, because that permission granted here for any other keyless
//!   exchange would report a half-finished GOOGLE migration that does not exist. [`FEDERATION`] is
//!   what `docs/adr/0017` actually names;
//! * **`!` before the rule and `!` anywhere are different questions.** `!line.contains('!')`
//!   refuses a correct condition strengthened with `!cancelled()` or a `!=`, and a gate that fails
//!   correct configuration is one somebody deletes. See [`states_fork_rule`];
//! * **`/dev/null` is a redirect to a path and is not storage.** `printenv "$KEY" 2>/dev/null`
//!   had a file redirect on the line and was therefore exempt from the print check while stdout
//!   went to a public log. This job already writes `2>/dev/null` elsewhere, so the shape is live.
//!
//! # And six more one review later, every one a line read where a SHAPE was meant
//!
//! Same method, same list, and the pattern they share is worth more than any of them: each was a
//! `contains` or a `starts_with` standing in for a thing with structure - a step, a boolean
//! expression, a redirect operator, a path, a command, a print's argument.
//!
//! * **a step's first key may sit on the list marker.** `- run:` is a step with no `name:`, and
//!   [`shape::shell`] tested `starts_with("run:")` on the trimmed line - so that step's whole body
//!   was outside the scan and `- run: printenv <the key>` was invisible to every check that reads
//!   the shell, while `bodies` stayed non-empty from the other steps so the fail-closed arm never
//!   fired. See [`shape::step_key`], which `continue-on-error` reads too;
//! * **a condition is a boolean expression, not a haystack.** `||` makes each branch sufficient, so
//!   `<the rule> || github.event_name == 'pull_request'` runs on every fork with the rule stated
//!   correctly beside it. Only [`shape::cannot_be_a_fork`] may stand there now;
//! * **`continue-on-error:` and a `continue` in a guard are one defect at two altitudes.** Every
//!   `exit 1` still fires, the guard window is still satisfied, and the job reports a pass. Only
//!   the in-body half was held - see [`shape::downgrades_failure`];
//! * **a substring and a basename were two answers under a message claiming one path.**
//!   `contains("runner.temp")` is satisfied by a DIRECTORY of that name inside the checkout, and
//!   the write/remove forms were built from the basename, so a nested path matched a write to the
//!   root of `$RUNNER_TEMP`. [`shape::under_runner_temp`] parses the expression;
//! * **tracing is configured by two keys no body holds.** `shell: bash -x {0}` and
//!   `SHELLOPTS: xtrace` never appear in a `run:` body, and a one-line body puts the command on
//!   the `run:` key itself. See [`shape::configures_tracing`] and [`traces`];
//! * **the print check read the secret's NAME, and the key has a second copy.** `cat` of the
//!   credential file spells no name, so the one line that puts the whole key in a public log was
//!   clean. [`shape::emits_file`] reads the verb's ARGUMENT, because this job legitimately prints
//!   that file's size.
//!
//! # What this does not reach
//!
//! The file the group points at for limits had none of its own. Every row is a property of the
//! scan rather than a property nobody wanted:
//!
//! | Not reached | Why |
//! | --- | --- |
//! | A multi-line `if: \|` condition | The condition is read off the `if:` key's own line, so a rule on a continuation line reads as absent - and this workflow writes that YAML style elsewhere |
//! | A disjunct beside the rule that is correct but unreadable | [`shape::cannot_be_a_fork`] is an allowlist of one shape, so `github.event_name != 'schedule'` beside the rule is REPORTED though it is right. Under-permissive by choice: the answer decides whether the key is in scope |
//! | A workflow-level `permissions:` | [`job`] returns the job's own lines, so a grant made once for the whole file is invisible here |
//! | `>> "$GITHUB_OUTPUT"`, `\| tee`, `base64`, `jq` over the key file | [`shape::redirects_to_file`] judges the target's SHAPE, not whether it is published, and [`shape::PRINTS`] is a vocabulary. A `cat` of the credential file IS now caught, by [`shape::emits_file`]; a `base64` of it is not |
//! | The key one hop from its name | `K="$SUTURA_BQ_KEY"` and then `echo "$K"` needs data flow, and nothing here has any. Both the name check and [`shape::emits_file`] read one line |
//! | A guard whose `exit` is in the NEXT step | The job's shell is one flat line list, so [`GUARD_WINDOW`] can cross a step boundary. Strictly stronger than the whole-job search it replaced, not airtight |
//! | Whether the name a guard mentions is the name it TESTS | `guarded` is a bag of environment-shaped words off any guard line, so a name merely appearing near one counts as tested |
//! | A lower-case environment name | [`env_name_shaped`] requires upper case, so `bq_key: ${{ secrets.x }}` is read by neither the emptiness check nor the print check |
//! | `::add-mask::` | The job's own comments call masking the mechanism that keeps a project, dataset and table out of a public log. Nothing here holds it |
//! | An `environment:` written as a mapping | The exclusivity count compares the key's inline value, so `environment:` with `name:` below it is read as declared and not counted |
//! | Whether `environment: bq-test` withholds anything | That is a repository setting no file in this tree states. The key is necessary and is not sufficient |

use std::collections::{BTreeMap, BTreeSet};

// What a LINE of that workflow is, as against what the job must hold. Every predicate there was
// found green for the wrong reason at least once, so each one is stated where its own escape can be
// written beside it.
mod shape;

use shape::{
    FORK_RULE, Source, configured, configures_tracing, downgrades_failure, emits_file, env_name_shaped, exits_non_zero, job,
    prints, shell, states_fork_rule, step_key, traces, under_runner_temp, waits_for,
};

/// The workflow that holds the acceptance venue's limit.
pub(super) const WORKFLOW: &str = ".github/workflows/ci.yml";

/// The job, at two spaces of indentation like every other job in that file.
pub(super) const JOB: &str = "bigquery-acceptance";

/// The job this one waits for, so a cloud request is not spent on a tree the lints refuse.
const NEEDS: &str = "ci";

/// What a Google token exchange looks like in a workflow.
///
/// **The signal `docs/adr/0017` names, rather than the one it used to name.** `id-token: write` is
/// not on this list and must not be: that permission has been in `release.yml` since keyless
/// signing landed, so its presence says nothing about Google. Any one of these is a workload-identity
/// exchange - the action that performs it, the pool it names, or the endpoint it calls.
const FEDERATION: &[&str] = &[
    "google-github-actions/auth",
    "workload_identity_provider",
    "sts.googleapis.com",
];

/// How far after a guard a non-zero `exit` may sit.
///
/// The shape in this job is three lines - the test, a message, the exit - and a `for` wrapping one
/// adds two. Deliberately a window rather than the whole job: `exit 1` ANYWHERE used to satisfy
/// this, so a guard downgraded to a `continue` beside an unrelated exit was invisible.
const GUARD_WINDOW: usize = 6;

/// Everything wrong with the acceptance job.
///
/// One composition of four questions, and they are separate because they read different things:
/// who may run this job at all, where the credential goes, whether unset configuration stops it,
/// and what reaches the log.
pub(super) fn problems(text: &str) -> Vec<String> {
    let Some(block) = job(text, JOB) else {
        return vec![format!(
            "{WORKFLOW} has no `{JOB}` job - it is the only venue in the map that runs against a \
             real data system, and the map claims it exists"
        )];
    };
    let bodies = shell(&block);
    if bodies.is_empty() {
        return vec![format!(
            "{WORKFLOW}: the `{JOB}` job has no shell - the scan is broken, not the job"
        )];
    }
    // Once, and read by three of the four below: two derivations of one fact can disagree, and
    // this one was computed separately for the emptiness check and for the credential check.
    let config = configured(&block);
    let credential = block
        .iter()
        .find_map(|line| line.trim().strip_prefix("GOOGLE_APPLICATION_CREDENTIALS: "))
        .map(str::trim);

    let mut problems = who_may_run(text, &block);
    problems.extend(credential_placement(&bodies, credential));
    problems.extend(unset_configuration_fails(&bodies, &config));
    problems.extend(what_reaches_the_log(
        &block,
        &bodies,
        &config,
        credential.and_then(under_runner_temp),
    ));
    problems.extend(one_credential_mechanism(&block, &config));
    problems
}

/// Who may run this job, and whether a failure in it counts.
///
/// The four properties that decide whether the leg happens at all under an identity that could see
/// the key - so a wrong answer here is not a weaker gate, it is a green run that proves nothing.
fn who_may_run(text: &str, block: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();

    // Property 1's other half. The file's own comment says this job waits for `ci` so a red local
    // gate spends no cloud request; nothing read it.
    if !block.iter().any(|line| waits_for(line, NEEDS)) {
        problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job waits for no `{NEEDS}` - it is the one job here that calls \
             a cloud provider, so running it beside the lints spends a request on a tree they were \
             about to refuse, and its own comment says it waits"
        ));
    }

    // `environment:` is the mechanism, and its EXCLUSIVITY is half of the mechanism: `job` reads
    // one block, so a second job naming the same environment holds the same secret while
    // answering none of the properties here and being invisible to all of them.
    match block
        .iter()
        .find_map(|line| step_key(line).strip_prefix("environment:"))
        .map(str::trim)
    {
        None => problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job declares no `environment:` - that is the mechanism that \
             withholds the key from a fork's pull request, and `docs/adr/0017` refused CI over \
             exactly the exposure it prevents"
        )),
        // The mapping form, `environment:` with a `name:` below it. Declared, and this comparison
        // cannot say by whom - see the limits table.
        Some("") => {}
        Some(name) => {
            let holders = text
                .lines()
                .filter(|line| {
                    step_key(line)
                        .strip_prefix("environment:")
                        .is_some_and(|value| value.trim() == name)
                })
                .count();
            if holders > 1 {
                problems.push(format!(
                    "{WORKFLOW}: {holders} jobs declare `environment: {name}` - every one of them \
                     can read that environment's secret, and the properties here are asserted of \
                     `{JOB}` alone. A second holder is a second answer to who may see the key"
                ));
            }
        }
    }

    // `continue-on-error` and a `continue` in a guard are one defect at two altitudes.
    if let Some(line) = block.iter().find(|line| downgrades_failure(line)) {
        problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job holds `{}` - a job or a step that continues on error \
             turns every guard below it into a warning, so unset configuration SKIPS and the run \
             reports a pass. Only `false` is a value this job may give that key",
            line.trim()
        ));
    }

    // The JOB's own condition, at four spaces. A STEP's `if:` sits deeper and skips one step, so
    // the job still runs on a fork and reports a pass for a leg that never happened - a `!` states
    // the rule backwards while satisfying any test that only looks for the text, and a `||` beside
    // it answers for a fork on its own.
    match block.iter().find_map(|line| {
        line.strip_prefix("    ")?
            .strip_prefix("if:")
            .map(|condition| (line, condition))
    }) {
        Some((_, condition)) if states_fork_rule(condition) => {}
        Some((line, _)) => problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job's own condition is `{}`, which has to test `{FORK_RULE}` \
             and may not negate it - skip where the runner had no choice, run where somebody in \
             this repository pushed. An event-name test answers both with one verdict, a `!` \
             answers both backwards, and a `||` beside the rule makes its own answer sufficient, \
             so only an `==` against an event no fork's pull request arrives as may stand there",
            line.trim()
        )),
        None => problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job has no condition of its own, so it runs on a fork's pull \
             request - the `environment:` still withholds the key, but the leg then fails for want \
             of a secret or skips one step and reports a pass. A step's `if:` is not this rule"
        )),
    }
    problems
}

/// Where the credential is written, and that the leg reads that same file.
fn credential_placement(bodies: &[&str], credential: Option<&str>) -> Vec<String> {
    match (credential, credential.and_then(under_runner_temp)) {
        (None, _) => vec![format!(
            "{WORKFLOW}: the `{JOB}` job points no `GOOGLE_APPLICATION_CREDENTIALS` at anything - \
             the leg would then read whatever credential the runner happens to have"
        )],
        (Some(path), None) => vec![format!(
            "{WORKFLOW}: the `{JOB}` job's credential path is `{path}`, which is not under \
             `${{{{ runner.temp }}}}/` - a key inside the checkout is one `git add .` from a public \
             leak, the secret sweep does not honour `.gitignore`, and a DIRECTORY named \
             `runner.temp` in the tree satisfies any test that reads this value as a haystack"
        )],
        (Some(_), Some(file)) => [
            ("written", format!("> \"$RUNNER_TEMP/{file}\"")),
            ("removed", format!("rm -f \"$RUNNER_TEMP/{file}\"")),
        ]
        .into_iter()
        // Per line, because neither form can span one - which is what the joined copy of every
        // body was for. The WHOLE path below `$RUNNER_TEMP` and not its basename: one answer, or
        // the message below is describing two.
        .filter(|(_, form)| !bodies.iter().any(|line| line.contains(form)))
        .map(|(what, form)| {
            format!(
                "{WORKFLOW}: the `{JOB}` job never has the credential {what} as `{form}` - the \
                 path the leg reads and the path the job writes and deletes are one path or they \
                 are two answers"
            )
        })
        .collect(),
    }
}

/// Every value the leg is pointed at, tested for emptiness by a guard that exits.
///
/// *Unset configuration FAILS rather than skips* is the property telekom/sutura#81 states most
/// exactly, and it is two questions: does a guard mention the name, and does that guard reach an
/// exit. `exit 1` ANYWHERE in the job satisfied the second for a while.
fn unset_configuration_fails(bodies: &[&str], config: &BTreeMap<&str, Source>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut guarded = BTreeSet::new();
    for (at, line) in bodies.iter().enumerate() {
        // A comment inside a body is prose. `shell` keeps it deliberately - an expression in one
        // would still be an expression in the file - so the guard shapes below read it too, and
        // `for ` plus ` in ` in an English sentence made a CORRECT job red for want of an `exit`
        // after a comment. That direction is how a gate gets deleted.
        if line.trim().starts_with('#') {
            continue;
        }
        // Two shapes, and both are guards: a `-z` emptiness test, and a `for name in A B` that
        // tests each of several in turn.
        if !(line.contains("-z ") || (line.contains("for ") && line.contains(" in "))) {
            continue;
        }
        // Only words shaped like an environment name, through the SAME predicate `configured`
        // reads - so what a guard is credited with testing and what the job declares cannot
        // disagree. A `word.len() > 2` filter stood here and was wrong in both directions: it
        // admitted `then`, `printenv` and `name`, and it dropped a one-character name, of which
        // this job already has one.
        guarded.extend(
            line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|word| env_name_shaped(word)),
        );
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
    for (name, source) in config {
        if !guarded.contains(name) {
            problems.push(format!(
                "{WORKFLOW}: the `{JOB}` job reads `{name}` from `{}` and never tests it for \
                 emptiness - unset configuration has to FAIL rather than skip, because the fixture \
                 once reported three passes against no project",
                source.named()
            ));
        }
    }
    problems
}

/// What of this job could reach a workflow log, which on this repository is public.
///
/// Three channels, and the third is the one that reaches the other two's blind spots: an
/// interpolation puts the value in the file, a print verb puts it on a line, and tracing puts
/// every argument of every command there without any of them being written down.
fn what_reaches_the_log(
    block: &[&str],
    bodies: &[&str],
    config: &BTreeMap<&str, Source>,
    credential_file: Option<&str>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for line in bodies.iter().filter(|line| line.contains("${{")) {
        problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job interpolates into a shell body - `{}`. A multi-line JSON \
             key is the shape that defeats naive log masking, so a value reaches this shell \
             through `env:` or not at all",
            line.trim()
        ));
    }
    // Per LINE and not per name, because whether a line prints is a property of the line: the
    // predicate was inside the loop above, asked once per configured value and asked at all for a
    // `vars.` entry, which this branch can never fire for.
    for line in bodies.iter().filter(|line| prints(line)) {
        for name in config
            .iter()
            .filter(|(_, source)| **source == Source::Secret)
            .map(|(name, _)| name)
        {
            if line.contains(*name) {
                problems.push(format!(
                    "{WORKFLOW}: the `{JOB}` job puts `{name}` on a line that prints - `{}`. A \
                     workflow log on a public repository is public, and the only reason a print \
                     verb may name the key at all is a redirect INTO a file",
                    line.trim()
                ));
            }
        }
        // The FILE is the second copy of the secret, and it spells no name the loop above reads.
        if let Some(file) = credential_file.filter(|file| emits_file(line, file)) {
            problems.push(format!(
                "{WORKFLOW}: the `{JOB}` job hands the credential file to a print verb - `{}`. \
                 `$RUNNER_TEMP/{file}` holds the same key `secrets` does, and the check above \
                 reads a secret's NAME, which a `cat` of that path never spells",
                line.trim()
            ));
        }
    }
    // The body first, so the message quotes the command where there is one; the two keys second,
    // because neither can appear in a body at all.
    if let Some(line) = bodies
        .iter()
        .find(|line| traces(line))
        .or_else(|| block.iter().find(|line| configures_tracing(line)))
    {
        problems.push(format!(
            "{WORKFLOW}: the `{JOB}` job turns shell tracing on - `{}`. Every command in that body \
             then reaches the log with its arguments, which is the channel this job's own comments \
             say `python3 -c` and `printenv` exist to avoid - and a `shell:` or a `SHELLOPTS:` does \
             it for a body nobody has written yet",
            line.trim()
        ));
    }
    problems
}

/// The key's expiry, read out of the job rather than remembered.
///
/// `docs/adr/0017` accepts a long-lived key as the *for now* and names what ends it: federation,
/// where the runner mints an OIDC token and **there is no key at all**. They are alternatives, so
/// this job holds exactly one - and holding both is the migration that stopped half way, where a
/// key nobody rotates outlives the paragraph that justified it.
///
/// **It reads a GOOGLE exchange and deliberately not `id-token: write`**, because the record's own
/// signal was that permission arriving and `release.yml` had granted it for keyless signing two
/// hours earlier. An earlier draft of this function narrowed that proxy to this job instead of
/// replacing it, which retired the one false positive and kept the class: `id-token: write` granted
/// here for any other keyless exchange would have reported a half-finished GOOGLE migration that
/// does not exist. What [`FEDERATION`] holds is what `docs/adr/0017` actually names as greenfield -
/// no Google auth action, no workload pool, no STS endpoint.
fn one_credential_mechanism(block: &[&str], config: &BTreeMap<&str, Source>) -> Option<String> {
    let federated = block.iter().any(|line| FEDERATION.iter().any(|marker| line.contains(marker)));
    let keyed = config.values().any(|source| *source == Source::Secret);
    match (federated, keyed) {
        (true, true) => Some(format!(
            "{WORKFLOW}: the `{JOB}` job exchanges for a Google credential AND places a secret - \
             those are the two alternatives `docs/adr/0017` prices against each other, so holding \
             both is a half-finished migration and the key is the half nobody will notice is still \
             there"
        )),
        (false, false) => Some(format!(
            "{WORKFLOW}: the `{JOB}` job authenticates with neither an environment secret nor a \
             Google token exchange, so it cannot be reaching the endpoint at all - a leg that \
             authenticates with nothing and passes is the fixture that once reported three passes \
             against no project"
        )),
        (true, false) => Some(format!(
            "{WORKFLOW}: the `{JOB}` job exchanges for a Google credential and holds no key, which \
             is the state `docs/adr/0017`'s expiry paragraph describes as the end of the \
             service-account key. That record still says a key is the cost of the evidence - amend \
             it, then delete this arm, because a gate whose failure is GOOD NEWS is one somebody \
             will silence"
        )),
        (false, true) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{FORK_RULE, JOB, WORKFLOW, problems};

    /// The one line that makes the fixture keyed, so every test that removes it removes the same
    /// thing.
    const KEY_ENV: &str = "          SUTURA_BQ_KEY: ${{ secrets.a_key }}\n";

    /// The job's own condition, built from the constant the gate reads - so a change to the rule
    /// cannot leave a fixture perturbing a string nothing looks for any more.
    fn condition() -> String {
        format!("if: github.event_name == 'push' || {FORK_RULE}")
    }

    /// A step written with no `name:`, which is the form the shell scan could not see.
    fn with_nameless_step(body: &str) -> String {
        CI.replace(
            "      - name: Remove the credential\n",
            &format!("      - run: {body}\n\n      - name: Remove the credential\n"),
        )
    }

    /// A Google token exchange in the job, which is the signal `docs/adr/0017` names.
    fn with_google_exchange(ci: &str) -> String {
        ci.replace(
            "      - name: Acceptance leg\n",
            "      - uses: google-github-actions/auth@v2\n      - name: Acceptance leg\n",
        )
    }

    /// `id-token: write` on the job - the permission the record proves is NOT the signal.
    ///
    /// `needs: [ci]` appears twice below, so this grants it to `cross` as well. Harmless, because
    /// [`super::job`] reads one block, and written here rather than rediscovered per test.
    fn with_id_token(ci: &str) -> String {
        ci.replace(
            "    needs: [ci]\n",
            "    needs: [ci]\n    permissions:\n      id-token: write\n",
        )
    }

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
        let collapsed = CI.replace(FORK_RULE, "github.event_name == 'pull_request'");
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
        let inverted = CI.replace(FORK_RULE, &format!("!({FORK_RULE})"));
        let found = problems(&inverted);
        assert!(found.iter().any(|p| p.contains("may not negate it")), "{found:?}");
    }

    #[test]
    fn a_condition_strengthened_beside_the_rule_is_not_a_negated_rule() {
        // The other direction, and the draft failed it: `!line.contains('!')` refused any `!` in
        // the condition, so this - a correct rule, guarded against a cancelled run - was reported
        // as the rule stated backwards. A gate that fails correct configuration is one somebody
        // deletes, so what is read is the text immediately before the rule.
        let stronger = CI.replace(&condition(), &format!("if: !cancelled() && ({FORK_RULE})"));
        assert_eq!(problems(&stronger), Vec::<String>::new());
    }

    #[test]
    fn a_fork_rule_demoted_to_one_step_leaves_the_job_itself_running_on_a_fork() {
        let demoted = CI.replace(&format!("    {}\n", condition()), "").replace(
            "      - name: Acceptance leg\n",
            &format!("      - name: Acceptance leg\n        if: {FORK_RULE}\n"),
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
    fn a_print_whose_only_redirect_is_dev_null_still_reaches_the_log() {
        // `2>/dev/null` redirects to a PATH, so a per-line test read the whole line as storing the
        // key while stdout went to a public log. The real job already writes `2>/dev/null` on
        // another line, so this is a live shape and not a hypothetical.
        let leaked = CI.replace(
            "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n",
            "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n          printenv SUTURA_BQ_KEY 2>/dev/null\n",
        );
        let found = problems(&leaked);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("on a line that prints"), "{found:?}");
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
    fn a_key_and_a_google_exchange_in_one_job_is_a_migration_that_stopped_half_way() {
        let found = problems(&with_google_exchange(CI));
        assert!(found.iter().any(|p| p.contains("half-finished migration")), "{found:?}");
    }

    #[test]
    fn an_id_token_grant_is_not_the_google_signal_this_record_corrected() {
        // THE correction, held as a test rather than as prose. `release.yml` has granted
        // `id-token: write` for keyless signing since telekom/sutura#97, so the permission says
        // nothing about Google - and a draft of this gate narrowed that proxy to this job instead
        // of replacing it, which retires the one false positive and keeps the class. Granting it
        // here for any other keyless exchange must not report a Google migration.
        assert_eq!(problems(&with_id_token(CI)), Vec::<String>::new());
    }

    #[test]
    fn a_job_authenticating_with_nothing_fails_rather_than_passing_against_no_project() {
        let neither = CI.replace(KEY_ENV, "");
        let found = problems(&neither);
        assert!(found.iter().any(|p| p.contains("authenticates with neither")), "{found:?}");
    }

    #[test]
    fn the_day_the_key_is_gone_this_asks_for_the_record_to_be_amended() {
        // The gate whose failure is good news, and it says so in its own message: the alternative
        // is a record that goes on pricing a key nobody holds any more.
        let found = problems(&with_google_exchange(CI).replace(KEY_ENV, ""));
        assert!(found.iter().any(|p| p.contains("expiry paragraph")), "{found:?}");
    }

    #[test]
    fn a_step_written_with_no_name_still_has_its_body_read() {
        // `- run:` is a step with no `name:`, and the scan asked `trimmed.starts_with("run:")` -
        // for `- run: |` the trimmed line begins `- `, so the block was never entered and every
        // check that reads the shell went blind for that step while the gate printed `ok`.
        let leaked = with_nameless_step("printenv SUTURA_BQ_KEY");
        let found = problems(&leaked);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("on a line that prints"), "{found:?}");

        let interpolated = with_nameless_step("echo '${{ secrets.a_key }}' > \"$RUNNER_TEMP/k\"");
        let found = problems(&interpolated);
        assert!(
            found.iter().any(|p| p.contains("interpolates into a shell body")),
            "{found:?}"
        );
    }

    #[test]
    fn a_disjunct_beside_the_fork_rule_answers_for_a_fork_on_its_own() {
        // `||` makes every branch sufficient, so a rule stated correctly beside one a fork
        // satisfies is a job that runs on every fork - and the draft read the text immediately
        // before the rule, which is `if: ` in both of these.
        for widened in [
            format!("if: {FORK_RULE} || github.event_name == 'pull_request'"),
            format!("if: true || {FORK_RULE}"),
        ] {
            let collapsed = CI.replace(&condition(), &widened);
            let found = problems(&collapsed);
            assert!(
                found.iter().any(|p| p.contains("makes its own answer sufficient")),
                "{widened}: {found:?}"
            );
        }
        // The real job's own disjunct, and the reason this is an allowlist rather than a whole
        // condition equality: a `push` is not a pull request from anywhere.
        assert_eq!(problems(CI), Vec::<String>::new());
    }

    #[test]
    fn continue_on_error_turns_every_guard_below_it_into_a_warning() {
        // The same downgrade as a `continue` in a guard, one altitude up: each `exit 1` still
        // fires, the guard window is still satisfied, and the job reports a pass.
        for downgraded in ["    continue-on-error: true\n", "      - continue-on-error: true\n"] {
            let carries_on = CI.replace(
                "    environment: bq-test\n",
                &format!("    environment: bq-test\n{downgraded}"),
            );
            let found = problems(&carries_on);
            assert!(
                found.iter().any(|p| p.contains("continues on error")),
                "{downgraded}: {found:?}"
            );
        }
        let declined = CI.replace(
            "    environment: bq-test\n",
            "    environment: bq-test\n    continue-on-error: false\n",
        );
        assert_eq!(problems(&declined), Vec::<String>::new());
    }

    #[test]
    fn the_credential_path_is_one_path_rather_than_a_substring_and_a_basename() {
        // `contains("runner.temp")` is satisfied by a DIRECTORY of that name inside the checkout,
        // which is the one place this check exists to refuse.
        let in_tree = CI.replace(
            "${{ runner.temp }}/bq-key.json",
            "${{ github.workspace }}/runner.temp/bq-key.json",
        );
        let found = problems(&in_tree);
        assert!(found.iter().any(|p| p.contains("not under")), "{found:?}");

        // And the write/remove match reduced the path to its BASENAME, so a nested path and a
        // write to the root of `$RUNNER_TEMP` were two answers while the message said one path.
        let nested = CI.replace("${{ runner.temp }}/bq-key.json", "${{ runner.temp }}/nested/bq-key.json");
        let found = problems(&nested);
        assert!(
            found.iter().any(|p| p.contains("are one path or they are two answers")),
            "{found:?}"
        );
    }

    #[test]
    fn tracing_reaches_two_keys_no_body_holds_and_a_command_beside_another() {
        // Four escapes from a `strip_prefix("set ")` over body lines. The first two are step keys
        // that never appear in the shell at all; the third puts the whole body on the `run:` key's
        // own line, so the line begins `run: `; the fourth puts it after an operator.
        for traced in [
            CI.replace(
                "      - name: Acceptance leg\n",
                "      - name: Acceptance leg\n        shell: bash -x {0}\n",
            ),
            CI.replace(
                "          SUTURA_BQ_DATASET: ${{ vars.SUTURA_BQ_DATASET }}\n",
                "          SUTURA_BQ_DATASET: ${{ vars.SUTURA_BQ_DATASET }}\n          SHELLOPTS: xtrace\n",
            ),
            with_nameless_step("set -eux; nix run .#bigquery-acceptance"),
            CI.replace(
                "          nix run .#bigquery-acceptance\n",
                "          cd \"$RUNNER_TEMP\" && set -x\n          nix run .#bigquery-acceptance\n",
            ),
        ] {
            let found = problems(&traced);
            assert!(
                found.iter().any(|p| p.contains("turns shell tracing on")),
                "{traced}: {found:?}"
            );
        }
    }

    #[test]
    fn the_credential_file_handed_to_a_print_verb_is_the_key_in_a_public_log() {
        // The print check read the secret's NAME, which a `cat` of the file it was written to
        // never spells - so the one line that puts the whole key in a public log was clean.
        let leaked = CI.replace(
            "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n",
            "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n          cat \"$RUNNER_TEMP/bq-key.json\"\n",
        );
        let found = problems(&leaked);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("hands the credential file to a print verb"), "{found:?}");

        // The twin, because the real job PRINTS THE SIZE: the verb's argument decides it, not the
        // presence of the path on the line, and a check that read the line fails this job.
        let sized = CI.replace(
            "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n",
            "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n          echo \"placed, $(wc -c < \"$RUNNER_TEMP/bq-key.json\") bytes\"\n",
        );
        assert_eq!(problems(&sized), Vec::<String>::new());
    }

    #[test]
    fn a_redirect_written_with_two_arrows_is_one_operator() {
        // `"x 2>>/dev/null".split('>')` yields an EMPTY middle segment, which starts with neither
        // `&` nor `/dev/null` and so read as a file - re-opening the hole the single-arrow form
        // closed.
        let leaked = CI.replace(
            "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n",
            "          printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-key.json\"\n          printenv SUTURA_BQ_KEY 2>>/dev/null\n",
        );
        let found = problems(&leaked);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("on a line that prints"), "{found:?}");

        // Still an exemption when it IS a file, so the append form of the write is not a leak.
        let appended = CI.replace("> \"$RUNNER_TEMP/bq-key.json\"", ">> \"$RUNNER_TEMP/bq-key.json\"");
        assert!(
            !problems(&appended).iter().any(|p| p.contains("on a line that prints")),
            "{:?}",
            problems(&appended)
        );
    }

    #[test]
    fn a_second_job_holding_this_environment_inherits_the_key_and_answers_for_nothing() {
        // `job` reads ONE block, so a second job naming the same environment is invisible to
        // every property above while holding the same secret.
        let shared = CI.replace(
            "  cross:\n    needs: [ci]\n",
            "  cross:\n    needs: [ci]\n    environment: bq-test\n",
        );
        let found = problems(&shared);
        assert!(
            found.iter().any(|p| p.contains("declare `environment: bq-test`")),
            "{found:?}"
        );
    }

    #[test]
    fn a_job_that_waits_for_nothing_spends_a_cloud_request_on_a_tree_the_lints_refuse() {
        let unordered = CI.replace("  bigquery-acceptance:\n    needs: [ci]\n", "  bigquery-acceptance:\n");
        let found = problems(&unordered);
        assert!(found.iter().any(|p| p.contains("waits for no `ci`")), "{found:?}");
    }

    #[test]
    fn a_comment_inside_a_body_is_prose_rather_than_a_guard() {
        // `shell` keeps a comment INSIDE a body deliberately - an expression in one would still be
        // an expression in the file - so the guard scan sees it, and `for ` plus ` in ` in a
        // sentence made a correct job red for want of an `exit` after a comment.
        let annotated = CI.replace(
            "          nix run .#bigquery-acceptance\n",
            "          # One request for each name in the list, and no guard here.\n          nix run .#bigquery-acceptance\n",
        );
        assert_eq!(problems(&annotated), Vec::<String>::new());
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
