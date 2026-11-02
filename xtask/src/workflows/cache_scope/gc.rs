//! Does the store cache collect garbage down to the flake's own outputs, and only there (#1696)?
//!
//! **The pairing this holds.** `nix-community/cache-nix-action` collects garbage in its post step,
//! before it saves, only when `gc-max-store-size` is set - and a collection with no roots takes the
//! dependency closure the entry exists to carry, because a `nix build` overwrites `./result` and
//! nothing a run built is a root. So the store-cache action sets the knob only beside a step that
//! roots every derivation the flake's `checks` and `apps` declare, and this refuses either half
//! without the other:
//!
//! * a store step whose `gc-max-store-size` is not exactly `KNOB` - the value that collects on the
//!   `ci` scope and on no other, since nothing roots another scope's store;
//! * no step of the action writing under `/nix/var/nix/gcroots/`, or one gated on anything but the
//!   `ci` scope plus `MAIN_PUSH` - the one run whose post step collects;
//! * a `gc-max-store-size` key anywhere else under `.github`, the `-linux`/`-macos` overrides
//!   included: an override wins over the knob, and a store step elsewhere collects a store nothing
//!   roots;
//! * an installer that can run on `push` without `keep-outputs = true`, in a job that writes the
//!   `ci` store on `push` - without it a rooted derivation does not keep what it built, and the
//!   collection takes the dependency closure after all.
//!
//! **What it cannot see.** It reads text. That the script roots the RIGHT set is the step's own
//! job. `keep-outputs` set any other way - `NIX_CONFIG`, a `nix.conf` written elsewhere - or undone
//! by a later `keep-outputs = false` line passes unseen; the root step's own refusal without it
//! stays the backstop, since a failed step skips the action's post step - the collection and the
//! save together (`post-if: success()`).

use std::path::Path;

use super::{MAIN_PUSH, STORE_ACTION, STORE_CACHE, key_of, steps};

/// The store-cache action, the one file that may set the knob.
const ACTION: &str = ".github/actions/nix-store-cache/action.yml";

/// The knob, token for token: collect on the `ci` scope, and `''` - no collection - on every other.
const KNOB: &str = "${{ inputs.scope == 'ci' && '0K' || '' }}";

/// What marks the root step, on a non-comment line.
const GCROOTS: &str = "/nix/var/nix/gcroots/";

/// Every `.github` file this reads: the action itself, and everywhere a knob must not be.
const GITHUB: [&str; 2] = [".github/workflows", ".github/actions"];

/// The installer whose `extra_nix_config` must carry [`KEEP_OUTPUTS`] in a `ci` store writer.
const INSTALLER: &str = "cachix/install-nix-action";

/// The line that makes a rooted derivation keep what it built.
const KEEP_OUTPUTS: &str = "keep-outputs = true";

/// The pairing, over every `.github` workflow and action file.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for dir in GITHUB {
        let census = crate::repo::collect_files(root, &root.join(dir), &["yml", "yaml"]);
        let read = census.inspect(
            &[],
            |_rel: &str| true,
            |rel, bytes| {
                if let Ok(text) = std::str::from_utf8(bytes) {
                    files.push((String::from(rel), String::from(text)));
                }
            },
        );
        if let Err(why) = read {
            return vec![format!(
                "could not read every file under {dir} - a `gc-max-store-size` there would pass unseen: {why:?}"
            )];
        }
    }
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(label, text)| (label.as_str(), text.as_str())).collect();
    judge(&borrowed)
}

/// The four refusals over labelled text.
fn judge(files: &[(&str, &str)]) -> Vec<String> {
    let Some((_, action)) = files.iter().find(|(label, _)| *label == ACTION) else {
        return vec![format!(
            "{ACTION} is missing, so the store cache's collection has nothing to read"
        )];
    };
    let mut out = Vec::new();
    let all = steps(action);

    let mut knobs = Vec::new();
    let store: Vec<_> = all
        .iter()
        .filter(|step| step.uses().is_some_and(|uses| uses.starts_with(STORE_CACHE)))
        .collect();
    if store.is_empty() {
        out.push(format!(
            "{ACTION}: no `{STORE_CACHE}` step, so this scan stopped matching the action"
        ));
    }
    for step in store {
        if let Some(at) = step
            .lines
            .iter()
            .position(|line| key_of(line).is_some_and(|key| key.starts_with("gc-max-store-size:")))
        {
            knobs.push(step.line.saturating_add(at));
        }
        if step.input("gc-max-store-size:") != Some(KNOB) {
            out.push(format!(
                "{ACTION}:{}  `gc-max-store-size` must be `{KNOB}`, the one value a root step covers - another collects a store nothing roots, or nothing at all",
                step.line
            ));
        }
    }

    let rooted = format!("inputs.scope == 'ci' && {MAIN_PUSH}");
    let roots: Vec<_> = all.iter().filter(|step| step.contains_verbatim(GCROOTS)).collect();
    if roots.is_empty() {
        out.push(format!(
            "{ACTION}: no step writes under `{GCROOTS}`, so `gc-max-store-size` collects a store with no roots and the dependency closure goes first"
        ));
    }
    for step in roots {
        let gate = step.gate().unwrap_or_default();
        if gate != rooted {
            out.push(format!(
                "{ACTION}:{}  the gc-root step is gated on `{gate}`, not `{rooted}` - it must run on exactly the run whose post step collects",
                step.line
            ));
        }
    }

    for (label, text) in files {
        for (index, line) in text.lines().enumerate() {
            let at = index.saturating_add(1);
            let Some(key) = key_of(line).filter(|key| key.starts_with("gc-max-store-size")) else {
                continue;
            };
            if *label == ACTION && knobs.contains(&at) {
                continue;
            }
            let name = key.split(':').next().unwrap_or_default();
            out.push(format!(
                "{label}:{at}  `{name}` is set outside the store step's pinned `gc-max-store-size` - an override wins over it, and a store elsewhere has no roots"
            ));
        }
    }
    out.extend(files.iter().flat_map(|(label, text)| unkept(label, text)));
    out
}

/// Every installer that can run on `push` without [`KEEP_OUTPUTS`], in a job that writes the `ci`
/// store on `push`. A workflow whose `on:` names neither `push` nor `workflow_call` is skipped; one
/// whose triggers this cannot read is not.
///
/// Jobs are read below `jobs:` only: `cachix-push.yml`'s writer is the job `push`, and its header
/// read from the top of the file is the `on:` block's `push:` trigger.
fn unkept(label: &str, text: &str) -> Vec<String> {
    let triggers = crate::workflows::contexts::block_keys(text, "on:");
    let Some((before, jobs)) = text.split_once("\njobs:\n") else {
        return Vec::new();
    };
    if !label.starts_with(".github/workflows/")
        || (!triggers.is_empty() && !triggers.iter().any(|on| on == "push" || on == "workflow_call"))
    {
        return Vec::new();
    }
    // `split`, not `lines`: a blank line just above `jobs:` is still a line.
    let first = before.split('\n').count().saturating_add(2);
    let mut out = Vec::new();
    for id in crate::workflows::contexts::block_keys(text, "jobs:") {
        let header = format!("  {id}:");
        let (Some(at), Some(body)) = (
            jobs.lines().position(|line| line == header),
            crate::workflows::step::job(jobs, &id),
        ) else {
            continue;
        };
        let gate = body.iter().find(|line| line.starts_with("    if:")).and_then(|line| {
            super::Step {
                line: 0,
                lines: vec![*line],
            }
            .gate()
        });
        let joined = body.join("\n");
        let all = steps(&joined);
        let writes = all.iter().any(|step| {
            step.uses()
                .is_some_and(|uses| uses == STORE_ACTION || uses == "$/.github/actions/nix-store-cache")
                && step.input("scope:") == Some("ci")
        });
        if !writes || !on_push(gate.as_deref()) {
            continue;
        }
        for step in all.iter().filter(|step| step.uses().as_deref() == Some(INSTALLER)) {
            if on_push(step.gate().as_deref()) && !step.contains_verbatim(KEEP_OUTPUTS) {
                out.push(format!(
                    "{label}:{}  job `{id}` writes the `ci` store on `push`, and this {INSTALLER} step can run there without `{KEEP_OUTPUTS}` - the collection before the save would take what the rooted derivations built",
                    first.saturating_add(at).saturating_add(step.line)
                ));
            }
        }
    }
    out
}

/// Can `gate` be true on `push`? No gate, a `||`, or a head conjunct this does not recognise all
/// say yes - the direction that keeps an installer checked.
fn on_push(gate: Option<&str>) -> bool {
    let Some(head) = gate
        .filter(|gate| !gate.contains("||"))
        .map(|gate| gate.split(" && ").next().unwrap_or(gate))
    else {
        return true;
    };
    let event = |op: &str| {
        head.strip_prefix(&format!("github.event_name {op} '"))
            .and_then(|rest| rest.strip_suffix('\''))
            .map(|name| name.eq_ignore_ascii_case("push"))
    };
    event("==").or_else(|| event("!=").map(|push| !push)).unwrap_or(true)
}
