//! What a line of that workflow IS, apart from what the job must hold.
//!
//! Every reader here answers one question about one line - is this a step key, does this print,
//! does this turn tracing on, is this path under `$RUNNER_TEMP` - and [`super::problems`] composes
//! them into the properties. The split is where the review value is: **six of these predicates
//! were green for the wrong reason**, each because it read a line as text where the thing it was
//! deciding is a shape, and each is now stated once with the escape it missed written beside it.

use std::collections::BTreeMap;

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

/// One job's own lines, from its header to the next thing at the same indentation.
///
/// Two spaces is where a job's name sits and four is where its keys do, so a comment block
/// introducing the NEXT job - which this file writes at two spaces - ends the block rather than
/// joining it.
pub(super) fn job<'a>(text: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let header = format!("  {name}:");
    let mut lines = text.lines().skip_while(|line| *line != header);
    lines.next()?;
    Some(
        lines
            .take_while(|line| line.trim().is_empty() || line.starts_with("    "))
            .collect(),
    )
}

/// One key of a step, whether it is written on the `-` line or below it.
///
/// **A step's first key may sit on the list marker**, and reading only the leading-key form was a
/// hole rather than a nicety: `- run: printenv <the key>` left that step's whole body out of
/// [`shell`], so the print check, the interpolation check and [`traces`] all went blind for it
/// while the gate still printed `ok`. `- continue-on-error: true` is the same shape one property
/// over. No workflow in this tree writes a step that way today, which is what makes it one line
/// from being green and wrong.
pub(super) fn step_key(line: &str) -> &str {
    let trimmed = line.trim_start();
    trimmed.strip_prefix('-').map_or(trimmed, str::trim_start)
}

/// The shell of every `run:` block in `block`.
///
/// A body ends at the first line indented no deeper than its own `run:` key, which is what keeps a
/// comment written between two steps out of it. A shell comment INSIDE a body stays in, because an
/// expression in one would still be an expression in the file.
pub(super) fn shell<'a>(block: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside: Option<usize> = None;
    for line in block {
        let indent = line.len().saturating_sub(line.trim_start().len());
        if let Some(depth) = inside {
            if !line.trim().is_empty() && indent <= depth {
                inside = None;
            } else {
                out.push(*line);
                continue;
            }
        }
        let key = step_key(line);
        if key.starts_with("run:") {
            inside = Some(indent);
            out.push(key);
        }
    }
    out
}

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

/// Does this line send its output to a FILE rather than to the log?
///
/// `>&1` and `>&2` ARE the log, so they are not an exemption - which is the distinction between
/// the line that stores the key and every line that would reveal it.
///
/// **`/dev/null` is not an exemption either, and that was a real hole**: the test is per LINE, so
/// `printenv "$KEY" 2>/dev/null` had a redirect whose target was a path and was therefore read as
/// storing the key, while stdout went to a public log. This job already writes `2>/dev/null` on
/// another line, so the shape is live rather than hypothetical.
///
/// **Split on the OPERATOR and not on the character**, which is what the empty-segment filter is:
/// `2>>/dev/null` is one append operator, and splitting the character made its second `>` a
/// redirect whose target is the empty string - matching neither exemption, so the `/dev/null` hole
/// re-opened in the append spelling of exactly the line that closed it.
pub(super) fn redirects_to_file(line: &str) -> bool {
    line.split('>').skip(1).filter(|rest| !rest.is_empty()).any(|rest| {
        let target = rest.trim_start();
        !target.starts_with('&') && !target.starts_with("/dev/null")
    })
}

/// Does this line put something in the log?
///
/// A print verb with nowhere else for the output to go. Its own predicate because it is a property
/// of the LINE: it was evaluated once per configured name, including for the names it can never
/// fire for.
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

/// Does this line turn shell tracing on? `set -x`, `set -eux` and `set -o xtrace` all do.
///
/// Refused for the whole job rather than only where the key is in scope, because the job's own
/// comment gives the reason: a traced command line is a value in a public log.
///
/// **Per COMMAND rather than per line**, and both halves of that were escapes from the draft's
/// `line.trim().strip_prefix("set ")`: a one-line body puts the whole command on the `run:` key,
/// so the line begins `run: set -eux`, and an operator puts it after something else, as in
/// `cd "$RUNNER_TEMP" && set -x`.
pub(super) fn traces(line: &str) -> bool {
    let body = step_key(line);
    let commands = body.strip_prefix("run:").unwrap_or(body);
    commands.split([';', '&', '|']).any(|command| {
        command
            .trim_start()
            .strip_prefix("set ")
            .is_some_and(|flags| flags.split_whitespace().any(trace_flag))
    })
}

/// Does this line turn tracing on for a shell body that has not been written yet?
///
/// Two keys, and **neither is ever a body line**, so [`traces`] over the shell could see neither:
/// `shell: bash -x {0}` replaces the interpreter for a step (or, under the job's `defaults:`, for
/// every step), and `SHELLOPTS: xtrace` in an `env:` block is honoured by bash on startup. The
/// module documented tracing as refused *for the whole job* while holding it for a body line
/// beginning `set `.
pub(super) fn configures_tracing(line: &str) -> bool {
    let key = step_key(line);
    if let Some(interpreter) = key.strip_prefix("shell:") {
        return interpreter.split_whitespace().any(trace_flag);
    }
    key.strip_prefix("SHELLOPTS:")
        .is_some_and(|options| options.contains("xtrace"))
}

/// Does this condition state the fork rule, and does nothing beside it answer for a fork?
///
/// **`!` before the rule and `!` anywhere are different questions**, and the first draft asked the
/// second: `!line.contains('!')`, which refuses the rule inverted - a job that runs on forks ONLY -
/// and equally refuses `github.event_name != 'schedule'` or `!cancelled()` beside a correct rule.
/// A gate that fails a correct strengthening is one somebody deletes, so what is read is the text
/// immediately before the rule, past the spaces and open parentheses a writer may put there.
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
            // The text immediately before the rule, past the spaces and open parentheses a
            // writer may put there. A `!` there states the rule backwards.
            Some(at) => {
                if disjunct
                    .get(..at)
                    .is_none_or(|before| before.trim_end_matches([' ', '(']).ends_with('!'))
                {
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
    let event = event.trim().trim_matches(['\'', '"']);
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

/// Does a print verb on this line take the credential FILE as its argument?
///
/// The file holds the same secret the environment does, so `cat "$RUNNER_TEMP/<file>"` is the whole
/// key in a public log - and the print check read a secret's NAME, which that line never spells.
///
/// **The verb's argument rather than the line**, because this job legitimately prints the file's
/// SIZE: `echo "… $(wc -c < "$RUNNER_TEMP/<file>") bytes"` names the path on a printing line and
/// reveals a byte count. A check keyed on the line fails correct configuration, which is the
/// direction that gets a gate deleted.
pub(super) fn emits_file(line: &str, file: &str) -> bool {
    PRINTS.iter().any(|verb| {
        line.split(verb)
            .skip(1)
            .any(|argument| argument.split_whitespace().next().is_some_and(|first| first.contains(file)))
    })
}

/// Does this line let a failure through? `continue-on-error:` with anything but `false`.
///
/// **The same downgrade as a `continue` in a guard, one altitude up.** On the job or on any step,
/// every guard below it still reaches its `exit 1`, the guard window is still satisfied, and an
/// unset `vars.` value SKIPS and reports a pass - which is the property telekom/sutura#81 states
/// most exactly. Only the in-body half was held.
pub(super) fn downgrades_failure(line: &str) -> bool {
    step_key(line)
        .strip_prefix("continue-on-error:")
        .is_some_and(|value| value.trim() != "false")
}

/// Does this line declare `job` among what this one waits for?
///
/// The list rather than a substring, because `needs: [ci]` and `needs: [cross]` differ by the
/// characters a `contains` would ignore.
pub(super) fn waits_for(line: &str, job: &str) -> bool {
    step_key(line).strip_prefix("needs:").is_some_and(|list| {
        list.trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .any(|name| name.trim().trim_matches(['\'', '"']) == job)
    })
}

/// Does this line exit non-zero? A guard that does not reach one is a skip.
pub(super) fn exits_non_zero(line: &str) -> bool {
    let Some(code) = line.trim().strip_prefix("exit ") else {
        return false;
    };
    code.trim().trim_end_matches(';').parse::<i32>().is_ok_and(|code| code != 0)
}
