//! What the job must hold, as against what a LINE of it is.
//!
//! That is the distinction the parent module already draws over `mod shape;`, and this file is its
//! other side: `shape` says what a step, a condition, a redirect or a print IS, and each function
//! here spends those predicates on one property of the job. The parent's `problems` is the
//! composition and nothing here composes anything - the four questions are separate because they
//! read different things, and a fifth reads the credential's expiry out of the job.
//!
//! **Every limit is recorded in the parent's own module documentation**, beside the list of ways an
//! earlier draft of each of these read as compliance. Nothing is restated here: two copies of that
//! record would disagree, and the one that rots first is the copy.
//!
//! Split out when `acceptance.rs` reached the unexemptable 1000-line gate, the same way that file
//! was split before: the machinery moves and every `#[test]` stays. `test-causality` never reverts
//! a file that adds a `#[test]`, so an assertion moved out of there would be orphaned by the revert
//! of its `mod` and read as green against base. Every property below is asserted through
//! `super::problems`, so the suite did not have to move a line to follow this.

use std::collections::{BTreeMap, BTreeSet};

use super::shape::{
    FORK_RULE, Source, configures_tracing, downgrades_failure, emits_file, env_name_shaped, exits_non_zero, keyed_block, prints,
    states_fork_rule, step_key, traces, waits_for,
};
use super::{JOB, WORKFLOW};

/// The job this one waits for, so a cloud request is not spent on a tree the lints refuse.
pub(super) const NEEDS: &str = "ci";

/// What a Google token exchange looks like in a workflow.
///
/// **The signal `docs/adr/0017` names, rather than the one it used to name.** `id-token: write` is
/// not on this list and must not be: that permission has been in `release.yml` since keyless
/// signing landed, so its presence says nothing about Google. Any one of these is a workload-identity
/// exchange - the action that performs it, the pool it names, or the endpoint it calls.
pub(super) const FEDERATION: &[&str] = &[
    "google-github-actions/auth",
    "workload_identity_provider",
    "sts.googleapis.com",
];

/// The workflow's own mappings that decide how this job's shells start.
///
/// `defaults:` -> `run:` -> `shell: bash -x {0}` and `env:` -> `SHELLOPTS: xtrace` are the two
/// tracing keys written at column zero, where they apply to every job including this one and sit
/// outside the lines [`job`](super::shape::job) returns. Read here because the alternative was a limits row: a
/// property claimed *for the whole job* that one scope up could contradict.
pub(super) const WORKFLOW_SCOPE: &[&str] = &["defaults", "env"];

/// How far after a guard a non-zero `exit` may sit.
///
/// The shape in this job is three lines - the test, a message, the exit - and a `for` wrapping one
/// adds two. Deliberately a window rather than the whole job: `exit 1` ANYWHERE used to satisfy
/// this, so a guard downgraded to a `continue` beside an unrelated exit was invisible.
pub(super) const GUARD_WINDOW: usize = 6;

/// Who may run this job, and whether a failure in it counts.
///
/// The four properties that decide whether the leg happens at all under an identity that could see
/// the key - so a wrong answer here is not a weaker gate, it is a green run that proves nothing.
pub(super) fn who_may_run(text: &str, block: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();

    // Property 1's other half. The file's own comment says this job waits for `ci` so a red local
    // gate spends no cloud request; nothing read it.
    if !waits_for(block, NEEDS) {
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
pub(super) fn credential_placement(commands: &[&str], credential: Option<&str>, file: Option<&str>) -> Vec<String> {
    match (credential, file) {
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
        .filter(|(_, form)| !commands.iter().any(|line| line.contains(form)))
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
pub(super) fn unset_configuration_fails(commands: &[&str], config: &BTreeMap<&str, Source>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut guarded = BTreeSet::new();
    for (at, line) in commands.iter().enumerate() {
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
        if !commands
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
///
/// The first channel reads every body line and the other two read only the commands, which is what
/// [`shape::is_comment`](super::shape::is_comment) is for: an expression in a comment is still in the file, a print in one is
/// not a print.
pub(super) fn what_reaches_the_log(
    text: &str,
    block: &[&str],
    bodies: &[&str],
    commands: &[&str],
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
    for line in commands.iter().filter(|line| prints(line)) {
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
    // The command first, so the message quotes it where there is one; the two keys second, because
    // neither can appear in a body at all - and over the WORKFLOW's own scope as well as the
    // job's, because a key written at column zero decides how this job's shells start while
    // sitting outside the lines `job` returns. Lazily, so a body that already traces spends
    // neither scan.
    if let Some(line) = commands.iter().copied().find(|line| traces(line)).or_else(|| {
        block
            .iter()
            .copied()
            .chain(WORKFLOW_SCOPE.iter().filter_map(|name| keyed_block(text, "", name)).flatten())
            .find(|line| configures_tracing(line))
    }) {
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
pub(super) fn one_credential_mechanism(block: &[&str], config: &BTreeMap<&str, Source>) -> Option<String> {
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
