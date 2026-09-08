//! What a line of that workflow IS, apart from what the job must hold.
//!
//! Every reader here answers one question about one line - is this a step key, does this print,
//! does this turn tracing on, is this path under `$RUNNER_TEMP` - and [`super::problems`] composes
//! them into the properties. The split is where the review value is: **ten of these predicates
//! were green for the wrong reason across two rounds of review**, each because it read a line as
//! text where the thing it was deciding is a shape, and each is now stated once with the escape it
//! missed written beside it. Four of the ten went the other way as well - a shape that made the
//! gate fail a CORRECT job - and that direction is the one that gets a gate deleted, so both are
//! written down where the predicate is.

use std::collections::BTreeMap;

use super::super::sources::command_spans;

/// The fork rule, on the head repository.
///
/// **Not `github.event_name`**, and the difference is the whole property: a pull request from a
/// BRANCH of this repository can see the environment's secret and must be held to the leg, while a
/// fork's cannot and must skip. An event-name test collapses those two into one answer.
pub(super) const FORK_RULE: &str = "github.event.pull_request.head.repo.full_name == github.repository";

/// The events a fork's pull request arrives as, and therefore the two an `==` beside the rule may
/// not name. `pull_request_target` is here because it is the one that would run a fork's code with
/// this repository's secrets, not because this workflow declares it.
const FORK_EVENTS: &[&str] = &["pull_request", "pull_request_target"];

/// Print verbs that put their argument in the log.
///
/// `printenv` is here BECAUSE it is how the key is written: the same command without a redirect
/// prints the key instead of storing it, and a check that knew only `echo` read that as clean.
pub(super) const PRINTS: &[&str] = &["echo", "printf", "printenv", "cat "];

/// The shared job and step readers. Acceptance and invocation detection must agree on which
/// lines are shell; keeping the raw reader in one place also preserves GitHub expressions.
pub(super) use crate::workflows::step::{job, keyed_block, shell, step_key};

/// Where a value the job reads comes from.
///
/// A closed pair rather than the two string literals this used to hold, because the difference
/// decides which checks apply - only a SECRET can be the key in a public log - and `kind ==
/// "secrets"` is a decision spelled as a comparison that cannot fail to compile when it is wrong.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    /// `${{ secrets.<name> }}` - withheld from a fork by the `environment:`.
    Secret,
    /// `${{ vars.<name> }}` - not a secret, and still something the leg has to be pointed at.
    Variable,
}

impl Source {
    /// How a failure message spells it, which is how the workflow spells it.
    pub(super) const fn named(self) -> &'static str {
        match self {
            Self::Secret => "secrets",
            Self::Variable => "vars",
        }
    }
}

/// The environment names this job takes from a secret or a variable, and which of the two.
pub(super) fn configured<'a>(block: &[&'a str]) -> BTreeMap<&'a str, Source> {
    let mut out = BTreeMap::new();
    for line in block {
        let Some((name, value)) = line.trim().split_once(": ") else {
            continue;
        };
        if !env_name_shaped(name) {
            continue;
        }
        if !value.contains("${{") {
            continue;
        }
        // `secrets.` first and it WINS a value naming both, which the two-pass form decided by
        // insertion order and therefore the other way. A secret interpolated beside a variable is
        // still a secret, and the checks a secret carries are the strict ones.
        if value.contains("secrets.") {
            out.insert(name, Source::Secret);
        } else if value.contains("vars.") {
            out.insert(name, Source::Variable);
        }
    }
    out
}

/// Is this word shaped like an environment variable name?
///
/// One predicate, read by [`configured`] and by the guard scan, so what the job DECLARES and what a
/// guard is credited with TESTING cannot disagree about what a name is.
pub(super) fn env_name_shaped(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// A path that IS a channel rather than a place: the log, a terminal, a discard.
///
/// Trees and not files, because one destination has many spellings - `/dev/stderr`, `/dev/fd/2`
/// and `/proc/self/fd/2` are the log three ways, and `>&2` is a fourth.
const CHANNELS: &[&str] = &["/dev/", "/proc/"];

/// Does this line send its output to a FILE rather than to the log?
///
/// **STORAGE is the recognised side, and the inversion IS the fix.** The exemption used to be a
/// negation - not `&`, not `/dev/null` - so every other device path read as storage:
/// `echo "$SUTURA_BQ_KEY" > /dev/stderr` is the destination `>&2` is pinned as, spelled as a path,
/// and it turned the whole print check off for that line. [`prints`] gates [`emits_file`], so one
/// `>` silenced both log channels and `cat "$RUNNER_TEMP/<file>" > /dev/stdout` passed too. A
/// target this cannot recognise now reads as the LOG, which is the only direction that cannot
/// report compliance over a leak.
///
/// **`/dev/null` is not an exemption either, and that was a real hole**: the test is per LINE, so
/// `printenv "$KEY" 2>/dev/null` had a redirect whose target was a path and was therefore read as
/// storing the key, while stdout went to a public log. This job already writes `2>/dev/null` on
/// another line, so the shape is live rather than hypothetical.
///
/// **Split on the OPERATOR and not on the character**, which is what the empty target is:
/// `2>>/dev/null` is one append operator, and splitting the character made its second `>` a
/// redirect to the empty string - which matched neither exemption, so the `/dev/null` hole
/// re-opened in the append spelling of exactly the line that closed it.
pub(super) fn redirects_to_file(line: &str) -> bool {
    line.split('>').skip(1).any(|rest| {
        let target = rest.trim_start().trim_start_matches(['"', '\'']);
        !target.is_empty() && !target.starts_with('&') && !CHANNELS.iter().any(|tree| target.starts_with(tree))
    })
}

/// Is this body line a shell comment, and therefore a CLAIM rather than a command?
///
/// One predicate for a distinction three readers had made separately or not at all, and it is read
/// ONCE - where [`super::problems`] separates the commands from every line of the shell - because
/// four readers each remembering to skip a comment is the shape this whole module is about.
/// [`shell`] keeps comments deliberately, since a `${{ }}` written in one is still an expression in
/// the file and the interpolation check is its reader; every other reader decides what the job
/// DOES, and there a comment answered both ways at once. `# written as > "$RUNNER_TEMP/<file>" by
/// the step above` satisfied *the credential is written* with no write in the job, and
/// `# never echo "$SUTURA_BQ_KEY"` was read as the key on a printing line and failed a CORRECT job
/// - which is the direction that gets a gate deleted.
pub(super) fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// Does this command put something in the log?
///
/// A print verb with nowhere else for the output to go. Its own predicate because it is a property
/// of the LINE: it was evaluated once per configured name, including for the names it can never
/// fire for. Asked of a command rather than of any body line - see [`is_comment`].
pub(super) fn prints(line: &str) -> bool {
    PRINTS.iter().any(|verb| line.contains(verb)) && !redirects_to_file(line)
}

/// Is this shell word a flag that turns tracing on?
///
/// The FLAG WORDS rather than a search for `-x`, because `set -eux` contains no such substring -
/// and the sign is part of the answer, since `set +x` turns tracing off. `--x` is a long option
/// rather than a bundle of short ones, which is why a second `-` disqualifies it.
fn trace_flag(word: &str) -> bool {
    word == "xtrace"
        || word
            .strip_prefix('-')
            .is_some_and(|set| !set.starts_with('-') && set.contains('x'))
}

/// What a compound command writes in front of the command it guards.
const KEYWORDS: &[&str] = &["then ", "do ", "else ", "elif "];

/// The command in one segment, past the punctuation and keywords a compound puts before it.
///
/// `if [ -n "$x" ]; then set -x; fi` and `(set -x)` are commands whose SEGMENT does not begin with
/// the command, so a `strip_prefix("set ")` over the segment read both as clean - the same
/// positional read as everything else in this file, one level down.
fn command(segment: &str) -> &str {
    let mut rest = segment.trim_start();
    loop {
        let trimmed = rest.trim_start_matches(['(', '{', '!', ' ']);
        let stripped = KEYWORDS
            .iter()
            .find_map(|keyword| trimmed.strip_prefix(keyword))
            .map_or(trimmed, str::trim_start);
        if stripped == rest {
            return rest;
        }
        rest = stripped;
    }
}

/// Does this line turn shell tracing on? `set -x`, `set -eux` and `set -o xtrace` all do.
///
/// Refused for the whole job rather than only where the key is in scope, because the job's own
/// comment gives the reason: a traced command line is a value in a public log.
///
/// **Per COMMAND rather than per line**, and both halves of that were escapes from the draft's
/// `line.trim().strip_prefix("set ")`: a one-line body puts the whole command on the `run:` key,
/// so the line begins `run: set -eux`, and an operator puts it after something else, as in
/// `cd "$RUNNER_TEMP" && set -x`.
///
/// **And a command is not the start of its segment**, which is [`command`]: a `then`, a `do` or a
/// parenthesis in front of it hid a trace behind one keyword. `export SHELLOPTS=xtrace` is the key
/// [`configures_tracing`] reads as YAML, spelled as a command, and was read by neither.
pub(super) fn traces(line: &str) -> bool {
    let body = step_key(line);
    let commands = body.strip_prefix("run:").unwrap_or(body);
    commands.split([';', '&', '|']).map(command).any(|invocation| {
        invocation
            .strip_prefix("set ")
            .is_some_and(|flags| flags.split_whitespace().any(trace_flag))
            || invocation
                .strip_prefix("export ")
                .unwrap_or(invocation)
                .strip_prefix("SHELLOPTS=")
                .is_some_and(shellopts_traces)
    })
}

/// Does this `SHELLOPTS` value turn tracing on? One value, two spellings - `env:` and `export`.
fn shellopts_traces(options: &str) -> bool {
    options.contains("xtrace")
}

/// Does this line turn tracing on for a shell body that has not been written yet?
///
/// Two keys, and **neither is ever a body line**, so [`traces`] over the shell could see neither:
/// `shell: bash -x {0}` replaces the interpreter for a step (or, under a `defaults:`, for every
/// step), and `SHELLOPTS: xtrace` in an `env:` block is honoured by bash on startup. The module
/// documented tracing as refused *for the whole job* while holding it for a body line beginning
/// `set `.
///
/// **Both keys are equally legal one scope up**, which is why the caller reads this over the
/// workflow's own `defaults:` and `env:` as well as over the job: written at column zero they turn
/// tracing on for this job and sit outside [`job`]'s lines entirely, so a job-scoped read claimed a
/// property the same file could contradict two lines higher.
pub(super) fn configures_tracing(line: &str) -> bool {
    let key = step_key(line);
    if let Some(interpreter) = key.strip_prefix("shell:") {
        return interpreter.split_whitespace().any(trace_flag);
    }
    key.strip_prefix("SHELLOPTS:").is_some_and(shellopts_traces)
}

/// Is the rule at `at` under a `!` - written against it, or against a group holding it?
///
/// **A `!` against a GROUP is the third spelling of this question**, and the text-before test could
/// not see it: `!(github.event_name == 'pull_request' && <the rule>)` leaves `&&` immediately
/// before the rule, so no `!` was found - and that condition is TRUE for every fork's pull request,
/// which is the one thing this property exists to refuse. So the parentheses still open where the
/// rule sits are tracked, and a `!` on any of them negates it.
fn negated(before: &str) -> bool {
    let mut groups = Vec::new();
    let mut previous = None;
    for character in before.chars() {
        match character {
            '(' => groups.push(previous == Some('!')),
            ')' => {
                groups.pop();
            }
            _ => {}
        }
        if !character.is_whitespace() {
            previous = Some(character);
        }
    }
    previous == Some('!') || groups.contains(&true)
}

/// Does this condition state the fork rule, and does nothing beside it answer for a fork?
///
/// **`!` before the rule and `!` anywhere are different questions**, and the first draft asked the
/// second: `!line.contains('!')`, which refuses the rule inverted - a job that runs on forks ONLY -
/// and equally refuses `github.event_name != 'schedule'` or `!cancelled()` beside a correct rule.
/// A gate that fails a correct strengthening is one somebody deletes, so what is read is the text
/// before the rule, through [`negated`].
///
/// **And a condition is a boolean expression, not a haystack.** Reading only the text before the
/// rule left `<the rule> || github.event_name == 'pull_request'` green - and `true || <the rule>`
/// with it - because `||` makes every branch sufficient on its own, so a correct rule beside a
/// branch a fork satisfies is a job that runs on every fork. A whole-condition equality is not
/// available (the real job legitimately needs `== 'push'`), so what is permitted beside the rule is
/// [`cannot_be_a_fork`].
pub(super) fn states_fork_rule(condition: &str) -> bool {
    let trimmed = condition.trim();
    let expression = trimmed
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(trimmed);
    let mut stated = false;
    for disjunct in expression.split("||") {
        match disjunct.find(FORK_RULE) {
            // A `!` anywhere in front of the rule that applies TO it states the rule backwards.
            Some(at) => {
                if disjunct.get(..at).is_none_or(negated) {
                    return false;
                }
                stated = true;
            }
            None if cannot_be_a_fork(disjunct) => {}
            None => return false,
        }
    }
    stated
}

/// Can a fork's pull request satisfy this branch of the condition on its own?
///
/// An equality against an event a fork's pull request cannot arrive as, and nothing else. That is
/// narrow deliberately: the answer decides whether the job runs with the environment's secret in
/// scope, so an expression this cannot read has to be refused rather than assumed harmless -
/// including a correct `github.event_name != 'schedule'`, which this reports and which is the safe
/// direction. `!= 'pull_request'` is not the same question, because a fork also reaches
/// `pull_request_target`.
pub(super) fn cannot_be_a_fork(disjunct: &str) -> bool {
    let Some((subject, event)) = disjunct.split_once("==") else {
        return false;
    };
    let event = unquoted(event);
    subject.trim() == "github.event_name" && !event.is_empty() && !FORK_EVENTS.contains(&event)
}

/// The credential's path below `$RUNNER_TEMP`, or nothing if it is not under it.
///
/// **A substring test and a basename test were two answers where the message claimed one path.**
/// `path.contains("runner.temp")` is satisfied by `${{ github.workspace }}/runner.temp/bq-key.json`,
/// a key inside the checkout, which is the single thing this check exists to refuse. And
/// reducing the path to its basename before building the write and remove forms made
/// `${{ runner.temp }}/nested/bq-key.json` agree with a write to the root of `$RUNNER_TEMP`. So the
/// expression is parsed, and what is returned is the whole remainder.
pub(super) fn under_runner_temp(path: &str) -> Option<&str> {
    let (expression, rest) = path.strip_prefix("${{")?.split_once("}}")?;
    if expression.trim() != "runner.temp" {
        return None;
    }
    rest.strip_prefix('/')
}

/// The path below a plain or braced `$RUNNER_TEMP` expansion, with the whole word or just the
/// directory optionally double-quoted. A single-quoted variable is literal, not this directory.
fn named_under_runner_temp(word: &str) -> Option<&str> {
    let quoted = word.starts_with('"');
    let word = word.strip_prefix('"').unwrap_or(word);
    let path = word
        .strip_prefix("$RUNNER_TEMP")
        .or_else(|| word.strip_prefix("${RUNNER_TEMP}"))?;
    let file = match (quoted, path.strip_prefix('"')) {
        (true, Some(after_directory)) => after_directory.strip_prefix('/')?,
        (true, None) => path.strip_prefix('/')?.strip_suffix('"')?,
        (false, _) => path.strip_prefix('/')?,
    };
    // Keep the complete filename, including a quoted space. Concatenated quoting and escapes
    // are outside this path grammar; accepting their prefix would name a different file.
    (!file.is_empty() && !file.contains(['"', '\'', '\\'])).then_some(file)
}

/// Raw argument words, keeping quoted whitespace and escaped characters inside their word.
/// This only finds boundaries: it does not expand variables or interpret the words' contents.
fn argument_words(text: &str) -> impl Iterator<Item = &str> {
    let mut quote = None;
    let mut escaped = false;
    text.split(move |character: char| {
        if escaped {
            escaped = false;
            return false;
        }
        match (quote, character) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (_, '\\') if quote != Some('\'') => escaped = true,
            _ => {}
        }
        quote.is_none() && character.is_whitespace()
    })
}

/// Every file below `$RUNNER_TEMP` this command redirects into.
///
/// The redirect TARGETS, through the same `split('>')` [`redirects_to_file`] reads, because the
/// question is the same one asked of a different half of the operator: that one asks whether the
/// output left the log, this asks which file it landed in. An append counts - `>>` yields an empty
/// first target and the path as the second, so both spellings resolve to one file.
pub(super) fn writes_under_runner_temp(line: &str) -> impl Iterator<Item = &str> {
    line.split('>')
        .skip(1)
        .filter_map(|rest| argument_words(rest.trim_start()).next())
        .filter_map(named_under_runner_temp)
}

/// Does this command delete `file` from `$RUNNER_TEMP`?
///
/// **An `rm`'s ARGUMENT LIST, and the whole-string form it replaces is what telekom/sutura#389 is
/// about.** *Placed and removed* was held by looking for the literal `rm -f "$RUNNER_TEMP/<file>"`,
/// which one `rm -f` over several paths does not contain for any of them but the first - so a job
/// that removes three key documents on one line reads as removing one, and a job that removes only
/// the first of three reads exactly the same. The verb is read through [`command`] for the reason
/// [`traces`] reads it that way, and `-f` is not required of it: `-f` decides what happens when the
/// file is absent, never whether the file is gone afterwards. Read past a `run:` key for [`traces`]'
/// reason: a one-line body puts the whole command on that key, which is how this job spells cleanup.
/// [`command_spans`] bounds the arguments: a trailing comment or a later `echo` removes nothing,
/// while an `rm` behind `then` or `&&` still names its own arguments. Execution is not evaluated.
pub(super) fn removes(line: &str, file: &str) -> bool {
    command_spans(line)
        .into_iter()
        .map(command)
        .filter_map(|invocation| invocation.strip_prefix("rm "))
        .flat_map(argument_words)
        .filter_map(named_under_runner_temp)
        .any(|named| named == file)
}

/// Does a print verb on this line take the credential FILE as an argument?
///
/// The file holds the same secret the environment does, so `cat "$RUNNER_TEMP/<file>"` is the whole
/// key in a public log - and the print check read a secret's NAME, which that line never spells.
///
/// **An ARGUMENT LIST, not the first token after the verb**, which is what the draft read and what
/// one flag defeated: `cat -v <file>`, `cat -- <file>` and `printf '%s' <file>` all answered no. The
/// case that has to keep answering no is not *argument one* either - this job legitimately prints
/// the file's SIZE, `$(wc -c < <file>)`, which names the path INSIDE a command substitution. So the
/// question is whether the name appears outside one.
pub(super) fn emits_file(line: &str, file: &str) -> bool {
    PRINTS.iter().any(|verb| {
        line.split(verb)
            .skip(1)
            .any(|arguments| named_outside_substitution(arguments, file))
    })
}

/// Does `file` appear in `text` outside every `$( … )`?
///
/// **Subtracted from the ARGUMENTS after a verb rather than from the line**, and that is what keeps
/// `echo "$(cat <file>)"` caught: the substitution hides the name from `echo`'s arguments, and the
/// `cat` inside it is a verb of its own whose arguments name the file outside any substitution of
/// theirs. An unterminated `$(` reads as text, so a line this cannot parse reports rather than
/// passes - the same direction [`redirects_to_file`] takes for a target it does not recognise.
fn named_outside_substitution(text: &str, file: &str) -> bool {
    // The head is everything before the first substitution opened; every part after it is read
    // from its own substitution's close, and an UNTERMINATED one is read whole rather than
    // skipped, so a line this cannot parse reports.
    let mut parts = text.split("$(");
    parts.next().is_some_and(|head| head.contains(file))
        || parts.any(|part| part.split_once(')').map_or(part, |(_, after)| after).contains(file))
}

/// Does this line let a failure through? `continue-on-error:` with anything but `false`.
///
/// **The same downgrade as a `continue` in a guard, one altitude up.** On the job or on any step,
/// every guard below it still reaches its `exit 1`, the guard window is still satisfied, and an
/// unset `vars.` value SKIPS and reports a pass - which is the property telekom/sutura#81 states
/// most exactly. Only the in-body half was held.
///
/// The value is compared with its quotes stripped, because `continue-on-error: 'false'` is valid
/// YAML saying exactly what the bare word says - and reporting it would fail a correct job.
pub(super) fn downgrades_failure(line: &str) -> bool {
    step_key(line)
        .strip_prefix("continue-on-error:")
        .is_some_and(|value| unquoted(value) != "false")
}

/// One YAML scalar, past the quotes and the spaces around it.
fn unquoted(value: &str) -> &str {
    value.trim().trim_matches(['\'', '"']).trim()
}

/// Does this block declare `job` among what it waits for?
///
/// The list rather than a substring, because `needs: [ci]` and `needs: [cross]` differ by the
/// characters a `contains` would ignore.
///
/// **Both YAML forms, which is why this reads the BLOCK and not one line.** A `needs:` with a block
/// sequence under it has an empty inline value, so a per-line read of the flow form reported a
/// correct job as waiting for nothing - the same direction as every other false positive here.
pub(super) fn waits_for(block: &[&str], job: &str) -> bool {
    let mut lines = block.iter().skip_while(|line| !step_key(line).starts_with("needs:"));
    let Some(key) = lines.next() else {
        return false;
    };
    let inline = step_key(key).strip_prefix("needs:").unwrap_or_default();
    let sequence = lines
        .take_while(|line| line.trim_start().starts_with("- "))
        .map(|line| line.trim_start().trim_start_matches("- "));
    std::iter::once(inline).chain(sequence).any(|value| {
        value
            .trim()
            .trim_matches(['[', ']'])
            .split(',')
            .any(|name| unquoted(name) == job)
    })
}

/// Does this line exit non-zero? A guard that does not reach one is a skip.
pub(super) fn exits_non_zero(line: &str) -> bool {
    let Some(code) = line.trim().strip_prefix("exit ") else {
        return false;
    };
    code.trim().trim_end_matches(';').parse::<i32>().is_ok_and(|code| code != 0)
}

#[cfg(test)]
mod tests {
    use super::{FORK_RULE, downgrades_failure, shell, states_fork_rule, waits_for};

    #[test]
    fn a_negation_reaches_the_rule_through_a_grouping() {
        // `!(A && <rule>)` is `!A || !<rule>`, so it is TRUE for every fork's pull request - while
        // leaving `&&` immediately before the rule, which is the text the draft read.
        for backwards in [
            format!("!({FORK_RULE})"),
            format!("!(github.event_name == 'pull_request' && {FORK_RULE})"),
            format!("!(github.event_name == 'push' && ({FORK_RULE}))"),
        ] {
            assert!(!states_fork_rule(&backwards), "{backwards}");
        }
        // A `!` whose group has CLOSED before the rule applies to something else, which is the
        // correct strengthening this has to keep accepting.
        for stated in [
            format!("${{{{ !cancelled() && ({FORK_RULE}) }}}}"),
            format!("github.event_name == 'push' || {FORK_RULE}"),
        ] {
            assert!(states_fork_rule(&stated), "{stated}");
        }
    }

    #[test]
    fn a_needs_written_as_a_block_sequence_is_the_same_declaration() {
        // Two YAML forms, one property. Read off the key's own line, the block form has an EMPTY
        // value - so a correct job was reported as waiting for nothing.
        assert!(waits_for(&["    needs: [ci]"], "ci"));
        assert!(waits_for(&["    needs:", "      - cross", "      - ci"], "ci"));
        assert!(!waits_for(&["    needs:", "      - cross"], "ci"));
        // The sequence ends where the next key begins, so a name below that is not in this list.
        assert!(!waits_for(
            &["    needs:", "      - cross", "    if: true", "      - ci"],
            "ci"
        ));
        assert!(!waits_for(&["    runs-on: ubuntu-latest"], "ci"));
    }

    #[test]
    fn a_quoted_false_is_the_one_value_this_job_may_give_that_key() {
        assert!(!downgrades_failure("    continue-on-error: false"));
        assert!(!downgrades_failure("    continue-on-error: 'false'"));
        assert!(downgrades_failure("    continue-on-error: true"));
        assert!(downgrades_failure("      - continue-on-error: 'true'"));
    }

    #[test]
    fn a_nameless_step_holds_its_body_open_to_its_own_column_and_no_further() {
        // The depth was the `-` column, two shallower than the `run:` key, so the step's SIBLING
        // keys were collected as shell - and an `env:` value's `${{ }}` was then reported as an
        // interpolation into a body that does not contain it.
        let step = [
            "      - run: |",
            "          set -eu",
            "        env:",
            "          K: ${{ secrets.a }}",
        ];
        assert_eq!(shell(&step), vec!["run: |", "          set -eu"]);
    }
}
