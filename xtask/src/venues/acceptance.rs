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
//!   [`shape::traces`];
//! * `exit 1` was searched for over the WHOLE job, so a guard downgraded from an exit to a
//!   `continue` was clean as long as some other guard still exited - and *unset configuration
//!   fails rather than skips* is the property telekom/sutura#81 states most exactly. Each guard now
//!   has to reach one within [`properties::GUARD_WINDOW`] lines.
//!
//! # And three found by review of the draft above, which is the same list one round later
//!
//! * **the expiry signal was the proxy this whole record exists to retire.** `federated` read
//!   `id-token: write`, narrowed from *any workflow* to *this job* - which retires the one false
//!   positive and keeps the class, because that permission granted here for any other keyless
//!   exchange would report a half-finished GOOGLE migration that does not exist. [`properties::FEDERATION`] is
//!   what `docs/adr/0017` actually names;
//! * **`!` before the rule and `!` anywhere are different questions.** `!line.contains('!')`
//!   refuses a correct condition strengthened with `!cancelled()` or a `!=`, and a gate that fails
//!   correct configuration is one somebody deletes. See [`shape::states_fork_rule`];
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
//!   the `run:` key itself. See [`shape::configures_tracing`] and [`shape::traces`];
//! * **the print check read the secret's NAME, and the key has a second copy.** `cat` of the
//!   credential file spells no name, so the one line that puts the whole key in a public log was
//!   clean. [`shape::emits_file`] reads the verb's ARGUMENT, because this job legitimately prints
//!   that file's size.
//!
//! # And four more one round later, three of them that same read one layer in
//!
//! The review that found the six above found four it had not closed, and the pattern held: a
//! position or a `contains` standing in for a shape. The fourth is the same defect pointing the
//! other way - a gate that fails a correct job.
//!
//! * **a redirect to a device path IS the log, and the exemption was a negation.** Only `&` and
//!   `/dev/null` were exempt, so every other device path read as storage:
//!   `echo "$SUTURA_BQ_KEY" > /dev/stderr` is the destination `>&2` is pinned as, spelled as a
//!   path. And [`shape::prints`] gates [`shape::emits_file`], so one `>` silenced both log channels - a `cat` of
//!   the key file to `/dev/stdout` passed too. Storage is the recognised side now, and an
//!   unrecognised target reads as the log: [`shape::redirects_to_file`];
//! * **an ARGUMENT LIST, not the token after the verb.** `cat -v <the key file>`, `cat -- <it>` and
//!   `printf '%s' <it>` all answered no - one flag defeated the property. What must keep answering
//!   no is not *argument one* either but *inside `$( )`*, which is where this job names the file to
//!   print its size. See [`shape::emits_file`];
//! * **both tracing keys are legal one scope up.** A workflow-level `defaults:` with
//!   `shell: bash -x {0}`, or an `env:` with `SHELLOPTS: xtrace`, turns tracing on for this job
//!   from outside the lines [`job`] returns - so a property claimed *for the whole job* was
//!   defeated by writing the same key two lines higher. [`properties::WORKFLOW_SCOPE`] is read beside the
//!   block; and a command no longer has to BEGIN its segment, because `then set -x` and `(set -x)`
//!   are commands too. See [`shape::traces`];
//! * **a comment is a claim, and it answered in both directions.** [`shape::shell`] keeps comments
//!   deliberately, so the interpolation check can read one - and the three readers deciding what
//!   the job DOES read them too. `# written as > "$RUNNER_TEMP/<file>" by the step above` satisfied
//!   *the credential is written* with no write in the job, while `# never echo "$SUTURA_BQ_KEY"`
//!   was reported as the key on a printing line and failed a CORRECT job. [`shape::is_comment`] is
//!   read ONCE, where [`problems`] separates the commands from the shell's every line - four
//!   readers each remembering to skip a comment is the shape this list is about.
//!
//! # And one more, which is a QUANTIFIER rather than a shape
//!
//! * **the property was *this* credential placed and removed, and the sentence beside it said
//!   *the* credential.** Both forms were built from the one path `GOOGLE_APPLICATION_CREDENTIALS`
//!   names, so a job placing a second key document under `$RUNNER_TEMP` and never deleting it read
//!   as clean - measured on the real job with the two principals' writes added, exit 0 either way.
//!   `properties::credential_placement` reads the WRITES now, and `shape::removes` reads an `rm`'s
//!   argument list, because one `rm -f` over three paths contains the whole-string form of none of
//!   them but the first.
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
//! | A copy of a key whose write does not spell the secret | [`properties::credential_placement`] calls a redirect into `$RUNNER_TEMP` a second copy when the LINE names a secret, so a `cp` of the key file or a `base64 -d` of it places a copy this does not know about. Reading every write instead would fail a job that puts a log there, which is the direction that gets a gate deleted |
//! | A removal in a step whose `if:` never fires | The job's shell is one flat line list, so an `rm` is read wherever it is written. `docs/adr/0017` records this half as review's, and it still is |
//! | A guard whose `exit` is in the NEXT step | The job's shell is one flat line list, so [`properties::GUARD_WINDOW`] can cross a step boundary. Strictly stronger than the whole-job search it replaced, not airtight |
//! | Whether the name a guard mentions is the name it TESTS | `guarded` is a bag of environment-shaped words off any guard line, so a name merely appearing near one counts as tested |
//! | A lower-case environment name | [`shape::env_name_shaped`] requires upper case, so `bq_key: ${{ secrets.x }}` is read by neither the emptiness check nor the print check |
//! | A file that lives under a device tree | [`shape::CHANNELS`] is a prefix, so `> /dev/shm/key` reads as the LOG and is reported. Over-permissive is the direction that reports compliance over a leak, so this one is deliberate |
//! | A backquoted substitution | [`shape::emits_file`] subtracts `$( )` and not `` ` ` ``, so a byte count taken the old way is reported. Same direction, same reason |
//! | The credential's PATH on a printing line | `echo "placed at $RUNNER_TEMP/<file>"` names the file outside a substitution and is reported, though only the path reaches the log. The vocabulary cannot tell a verb that reads the file from one that prints its name |
//! | `::add-mask::` | The job's own comments call masking the mechanism that keeps a project, dataset and table out of a public log. Nothing here holds it - and the cheap form would be a `contains` over the job, which is the exact shape every entry in the lists above was found to be. It needs the guard-to-value reasoning [`properties::unset_configuration_fails`] has, per masked name |
//! | An `environment:` written as a mapping | The exclusivity count compares the key's inline value, so `environment:` with `name:` below it is read as declared and not counted |
//! | Whether `environment: bq-test` withholds anything | That is a repository setting no file in this tree states. The key is necessary and is not sufficient |

// What a LINE of that workflow is, as against what the job must hold. Every predicate there was
// found green for the wrong reason at least once, so each one is stated where its own escape can be
// written beside it.
mod shape;

// What the JOB must hold - the other side of that same distinction, and one function per property.
// Moved out when this file reached the unexemptable 1000-line gate; nothing there composes, and
// every assertion stayed here because the causality gate never reverts a file that adds a `#[test]`.
mod properties;

/// The workflow the suite below perturbs, and the perturbations more than one test makes.
///
/// The HARNESS in its own file and every assertion in this one, which is what the 1000-line cap
/// prescribes: the causality gate never reverts a file that adds a `#[test]`, so an assertion moved
/// out of here would be orphaned by the revert of its `mod` and read as green against base.
#[cfg(test)]
mod tests_support;

use properties::{credential_placement, one_credential_mechanism, unset_configuration_fails, what_reaches_the_log, who_may_run};
use shape::{configured, is_comment, job, shell, under_runner_temp};

/// The workflow that holds the acceptance venue's limit.
pub(super) const WORKFLOW: &str = ".github/workflows/ci.yml";

/// The job, at two spaces of indentation like every other job in that file.
pub(super) const JOB: &str = "bigquery-acceptance";

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
    // What the job DOES, as against every line of its shell. A comment is a CLAIM: [`shell`] keeps
    // one deliberately, because a `${{ }}` written in a comment is still an expression in the
    // file, and the interpolation check below is its only reader. Subtracted ONCE here rather than
    // skipped inside each reader that decides what the job does, so a reader added later cannot
    // forget - `# written as > "$RUNNER_TEMP/<file>"` was satisfying the write with no write.
    let commands: Vec<&str> = bodies.iter().copied().filter(|line| !is_comment(line)).collect();
    // Once, and read by three of the four below: two derivations of one fact can disagree, and
    // this one was computed separately for the emptiness check and for the credential check.
    let config = configured(&block);
    let credential = block
        .iter()
        .find_map(|line| line.trim().strip_prefix("GOOGLE_APPLICATION_CREDENTIALS: "))
        .map(str::trim);
    // Same rule, for the path: one parse, or the placement check's message and the file the log
    // check looks for are two answers to where the credential is.
    let credential_file = credential.and_then(under_runner_temp);

    let mut problems = who_may_run(text, &block);
    problems.extend(credential_placement(&commands, &config, credential, credential_file));
    problems.extend(unset_configuration_fails(&commands, &config));
    problems.extend(what_reaches_the_log(
        text,
        &block,
        &bodies,
        &commands,
        &config,
        credential_file,
    ));
    problems.extend(one_credential_mechanism(&block, &config));
    problems
}

#[cfg(test)]
mod tests {
    use super::shape::FORK_RULE;
    use super::tests_support::{
        CI, KEY_ENV, beside_the_write, condition, instead_of_the_write, with_google_exchange, with_id_token, with_nameless_step,
    };
    use super::{JOB, WORKFLOW, problems};

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
    fn a_second_key_document_placed_under_runner_temp_has_to_be_removed_by_name() {
        // telekom/sutura#389: *placed and removed* was built from the one path the leg is pointed
        // at, so every other copy of a secret was held by nothing. Measured on the real `ci.yml`
        // before this: the two principals' writes added and left out of the cleanup, `check-venues`
        // exit 0, byte-identical to the run where all three are removed.
        let second = beside_the_write("printenv SUTURA_BQ_KEY > \"$RUNNER_TEMP/bq-principal-a.json\"");
        let found = problems(&second);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("bq-principal-a.json"), "{found:?}");
        assert!(found[0].contains("second copy of a secret"), "{found:?}");

        // And the other direction, which is what a whole-string `rm -f "<path>"` could not do: one
        // `rm` over several paths removes every one of them, and a job that spells its cleanup that
        // way is correct. Reversed order deliberately - the old form matched only a leading path.
        let removed = second.replace(
            "rm -f \"$RUNNER_TEMP/bq-key.json\"",
            "rm -f \"$RUNNER_TEMP/bq-principal-a.json\" \"$RUNNER_TEMP/bq-key.json\"",
        );
        assert_eq!(problems(&removed), Vec::<String>::new());
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
            let found = problems(&beside_the_write(added));
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
        let found = problems(&beside_the_write("printenv SUTURA_BQ_KEY 2>/dev/null"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("on a line that prints"), "{found:?}");
    }

    #[test]
    fn a_device_path_is_the_log_written_as_a_target_rather_than_storage() {
        // `> /dev/stderr` is the destination `>&2` is pinned as, spelled as a path. The exemption
        // was a NEGATION - not `&`, not `/dev/null` - so every other device path read as storage
        // and one `>` turned the whole print check off for the line.
        for added in [
            "echo \"$SUTURA_BQ_KEY\" > /dev/stderr",
            "echo \"$SUTURA_BQ_KEY\" > /dev/stdout",
            "echo \"$SUTURA_BQ_KEY\" >> /dev/fd/2",
        ] {
            let found = problems(&beside_the_write(added));
            assert_eq!(found.len(), 1, "{added}: {found:?}");
            assert!(found[0].contains("on a line that prints"), "{added}: {found:?}");
        }
        // The second channel the same `>` closed: `prints` gates the credential-file check, so a
        // device path exempted the line from a `cat` of the key as well as from its name.
        let found = problems(&beside_the_write("cat \"$RUNNER_TEMP/bq-key.json\" > /dev/stdout"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("hands the credential file"), "{found:?}");
    }

    #[test]
    fn the_credential_file_reaches_a_print_verb_past_a_flag_and_out_of_a_substitution() {
        // The draft read the FIRST token after the verb, so one flag defeated the property. The
        // `$( )` case is the last of these: the substitution hides the name from `echo`, and the
        // `cat` inside it is a verb of its own whose arguments name the file outside any of theirs.
        for added in [
            "cat -v \"$RUNNER_TEMP/bq-key.json\"",
            "cat -- \"$RUNNER_TEMP/bq-key.json\"",
            "printf '%s' \"$RUNNER_TEMP/bq-key.json\"",
            "echo \"$(cat \"$RUNNER_TEMP/bq-key.json\")\"",
        ] {
            let found = problems(&beside_the_write(added));
            assert_eq!(found.len(), 1, "{added}: {found:?}");
            assert!(found[0].contains("hands the credential file"), "{added}: {found:?}");
        }
        // The case the predicate exists to let through is the size this job prints, and it is
        // asserted by `the_credential_file_handed_to_a_print_verb_is_the_key_in_a_public_log`.
    }

    #[test]
    fn a_comment_neither_satisfies_a_property_nor_fails_a_correct_job() {
        // `shell` keeps a comment so the interpolation check can read it, and the three readers
        // deciding what the job DOES then read a CLAIM as a command - in both directions.
        let claimed = instead_of_the_write("# written as > \"$RUNNER_TEMP/bq-key.json\" by the step above").replace(
            "        run: rm -f \"$RUNNER_TEMP/bq-key.json\"\n",
            "        run: |\n          # and removed with rm -f \"$RUNNER_TEMP/bq-key.json\"\n",
        );
        let found = problems(&claimed);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found.iter().any(|p| p.contains("are one path or they are two answers")),
            "{found:?}"
        );
        assert!(found.iter().any(|p| p.contains("no `rm` in it names that path")), "{found:?}");
        // The direction that gets a gate deleted: a comment WARNING against the print was read as
        // the print, so the gate failed a job doing exactly what the comment says.
        let annotated = beside_the_write("# never echo \"$SUTURA_BQ_KEY\" - a workflow log here is public");
        assert_eq!(problems(&annotated), Vec::<String>::new());
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
    fn tracing_one_scope_up_or_behind_a_keyword_is_still_tracing_for_this_job() {
        // Both keys are legal at the WORKFLOW level, where they decide how this job's shells start
        // and sit outside the lines `job` returns - so the property claimed for the whole job was
        // defeated by writing the same key two lines higher. The last three are one altitude down:
        // a command is not the start of its segment, and `SHELLOPTS` has a command spelling too.
        for traced in [
            format!("defaults:\n  run:\n    shell: bash -x {{0}}\n{CI}"),
            format!("env:\n  SHELLOPTS: xtrace\n{CI}"),
            with_nameless_step("if [ -n \"$RUNNER_TEMP\" ]; then set -x; fi"),
            with_nameless_step("(set -x)"),
            with_nameless_step("export SHELLOPTS=xtrace"),
        ] {
            let found = problems(&traced);
            assert!(
                found.iter().any(|p| p.contains("turns shell tracing on")),
                "{traced}: {found:?}"
            );
        }
        // The workflow-level `env:` this repository's own file writes names neither key, which is
        // what stops the widened scan from failing a correct workflow.
        assert_eq!(
            problems(&format!("env:\n  BINARIES: sutura sutura-serve\n{CI}")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_credential_file_handed_to_a_print_verb_is_the_key_in_a_public_log() {
        // The print check read the secret's NAME, which a `cat` of the file it was written to
        // never spells - so the one line that puts the whole key in a public log was clean.
        let found = problems(&beside_the_write("cat \"$RUNNER_TEMP/bq-key.json\""));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("hands the credential file to a print verb"), "{found:?}");

        // The twin, because the real job PRINTS THE SIZE: the verb's argument decides it, not the
        // presence of the path on the line, and a check that read the line fails this job.
        let sized = beside_the_write("echo \"placed, $(wc -c < \"$RUNNER_TEMP/bq-key.json\") bytes\"");
        assert_eq!(problems(&sized), Vec::<String>::new());
    }

    #[test]
    fn a_redirect_written_with_two_arrows_is_one_operator() {
        // `"x 2>>/dev/null".split('>')` yields an EMPTY middle segment, which starts with neither
        // `&` nor `/dev/null` and so read as a file - re-opening the hole the single-arrow form
        // closed.
        let found = problems(&beside_the_write("printenv SUTURA_BQ_KEY 2>>/dev/null"));
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
