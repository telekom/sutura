//! What the TREE holds, as against what the page says about it.
//!
//! The third side of the split `page` and this module's parent already draw: `page` says what a
//! venues row or a verdict word IS, the parent says what the page must HOLD, and this says what is
//! true of the workspace the page describes - which tests exist, and which tasks CI actually runs.
//!
//! Split out when `venues.rs` crossed the unexemptable 1000-line gate a second time, and split at
//! this seam because these four items are the only ones there that read the TREE rather than the
//! page. Every `#[test]` stayed in the parent, which is what `.agents/skills/sutura/gates`
//! prescribes: a file that adds an assertion is never reverted, so moving the harness orphans
//! nothing.
//!
//! # The reading that was wrong, and it was wrong in the direction that passes
//!
//! [`invoked`] used to scan each non-comment line of a CI source for the substring `just ` or
//! `nix run .#`. A comment was the only prose it excluded - so
//! `echo to run this leg locally use nix run .#bigquery-two-principals please` resolved that name,
//! and a venue no job runs could claim a wiring at exit 0 with a summary byte-identical to the
//! honest one. [`starts_a_command`] reads a command instead.

use std::collections::BTreeSet;
use std::path::Path;

/// Every test function in the workspace, by name.
///
/// A test rather than any function, because the page's claim is that these names ARE the standing
/// tests: a helper renamed into one of them would satisfy an existence check and prove nothing.
pub(super) fn test_names(root: &Path, files: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let rust = files
        .iter()
        .filter(|f| Path::new(f).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rs")));
    for rel in rust {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if !(trimmed.starts_with("#[") && trimmed.contains("test")) {
                continue;
            }
            for ahead in lines.iter().skip(i.saturating_add(1)).take(4) {
                let candidate = ahead.trim().trim_start_matches("pub ").trim_start_matches("async ");
                if let Some(rest) = candidate.strip_prefix("fn ")
                    && let Some(name) = rest.split('(').next()
                {
                    out.insert(name.trim().to_owned());
                    break;
                }
            }
        }
    }
    out
}

/// The task or app name one invocation names, or `None` when the span is not an invocation.
///
/// `just bigquery-two-principals` and `nix run .#bigquery-two-principals` both resolve to
/// `bigquery-two-principals`, which is what makes a page citing the TASK comparable against a
/// workflow invoking the APP. That the two names coincide is this repository's convention and not a
/// derived fact - a task whose flake app is named differently would not be seen, and that is a
/// false NEGATIVE, which is the direction this check can afford.
pub(super) fn invocation(span: &str) -> Option<&str> {
    let rest = span
        .strip_prefix("nix run .#just -- ")
        .or_else(|| span.strip_prefix("nix run .#"))
        .or_else(|| span.strip_prefix("just "))?;
    let name: &str = rest.split_whitespace().next()?;
    let name = name.trim_end_matches(&[',', '.', ';', ':'][..]);
    let shaped = !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    shaped.then_some(name)
}

/// The tokens that end one shell command and begin another, so what follows one is a COMMAND.
///
/// A vocabulary rather than a parser, for `acceptance::shape::traces`' reason: what is needed is
/// *does a command start here*, and the shapes that start one in this tree are a line, a pipe, a
/// conjunction, a subshell, a `run:` key and the two verbs that prefix a command without being one.
const SEGMENTS: &[&str] = &["&&", "||", "|", ";", "$(", "(", ")", "{", "}", "`", "run:", "&"];

/// The words that may sit in front of a command without making it text.
const PREFIXES: &[&str] = &["- ", "exec ", "sudo ", "time ", "then ", "else ", "do ", "eval "];

/// Commands that RUN another command, with the real one after their argument terminator.
///
/// **A false negative this closes, measured rather than imagined.** The first hardening resolved 18
/// names where the substring scan resolved 19, and the one it lost was real:
/// `devenv shell -- just setup` in `nix/container-setup.sh`. That is an invocation, and losing it
/// would have been the gate quietly forgetting a task CI runs.
///
/// **Handled here rather than by splitting lines on `--`**, and the difference is the whole point
/// of this module's rewrite: splitting would make ` just x` its own segment inside
/// `echo "use -- just x"`, which is the prose hole all of this exists to close. A wrapper is
/// recognised as the segment's OWN command, so the text it wraps is still one segment.
const WRAPPERS: &[(&str, &str)] = &[
    ("devenv shell", " -- "),
    ("nix develop", " --command "),
    ("nix-shell", " --run "),
];

/// Is this segment's own command an invocation, rather than a word inside somebody's argument?
///
/// **The hole this closes, measured:** `invoked` used to scan each non-comment line for the
/// substring `nix run .#`, so
/// `echo to run this leg locally use nix run .#bigquery-two-principals please` resolved that name
/// and a venue no job runs could say `wired` at exit 0, with a summary byte-identical to the honest
/// one. A print's ARGUMENT is prose - the same distinction `acceptance::shape::emits_file` draws
/// one file over, where reading the verb rather than the line was also the fix.
///
/// **The direction it is wrong in, on purpose.** A command this cannot see is a false NEGATIVE, and
/// the three rules built on it each fail CLOSED on one: `wired` is refused, a built venue naming no
/// resolvable invocation is refused, and only `unrun` survives - which is the state that claims
/// nothing. **That direction still has to be MEASURED rather than trusted:** the first version of
/// this reader silently lost `devenv shell -- just setup`, which is why [`WRAPPERS`] exists and why
/// the count this gate prints is worth reading on a change to either.
pub(super) fn starts_a_command(segment: &str) -> Option<&str> {
    let mut command = segment.trim_start();
    loop {
        if let Some(rest) = PREFIXES.iter().find_map(|word| command.strip_prefix(word)) {
            command = rest.trim_start();
            continue;
        }
        // A wrapper's own argument terminator, and only when the wrapper is what this segment
        // starts with - so the terminator inside somebody's printed sentence is not one.
        let unwrapped = WRAPPERS.iter().find_map(|(wrapper, terminator)| {
            command
                .strip_prefix(wrapper)
                .and_then(|rest| rest.split_once(terminator))
                .map(|(_, after)| after)
        });
        match unwrapped {
            Some(rest) => command = rest.trim_start(),
            None => break,
        }
    }
    invocation(command)
}

/// Every `just` task and `nix run .#` app CI invokes, by bare name.
///
/// **Read out of the same three places `crate::workflows` scans, through the same walk**, because
/// the recorded way a reference leaves a gate's sight is a step moving out of a workflow - and a
/// second walk here would be a second answer to *where does CI invoke things from*.
///
/// A comment is not an invocation, matching `crate::workflows::collect`'s own rule: `ci.yml` and
/// `docs.yml` both discuss tasks in prose, and a gate that read those would refuse an `unrun` cell
/// because somebody explained the job in a comment. **Neither is a word inside a command's
/// arguments** - see [`starts_a_command`], which is the review finding that a comment was the only
/// prose this excluded while `echo <the same sentence>` was not.
///
/// `None` only when the scan itself is broken, which its caller turns into a failure rather than an
/// empty set: a set that found nothing would make every `unrun` cell pass, and *a scan that passes
/// by finding nothing* is the failure mode a text gate is most prone to here.
pub(super) fn invoked(root: &Path) -> Option<BTreeSet<String>> {
    let read = crate::workflows::sources::ci_sources(root)?;
    let mut out = BTreeSet::new();
    for source in &read {
        for line in source.text.lines() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            let mut segments: Vec<&str> = vec![line];
            for token in SEGMENTS {
                segments = segments.iter().flat_map(|part| part.split(token)).collect();
            }
            for segment in segments {
                if let Some(name) = starts_a_command(segment) {
                    out.insert(name.to_owned());
                }
            }
        }
    }
    Some(out)
}

/// The task and app names a `Reached by` cell cites, by bare name.
///
/// Backticked spans only, because that is how the page writes an invocation and because the cell's
/// prose ("a GitHub environment, on demand") would otherwise contribute words.
pub(super) fn cited_invocations(reached: &str) -> BTreeSet<String> {
    reached
        .split('`')
        .skip(1)
        .step_by(2)
        .filter_map(|span| invocation(span.trim()))
        .map(str::to_owned)
        .collect()
}
