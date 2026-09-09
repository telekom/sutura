//! Can a `pull_request` path write the Actions cache? The gate that answers no.
//!
//! **The rule this holds.** A workflow run restores caches from its own ref or the default branch
//! and from nowhere else, so an entry a pull request writes is readable by exactly one pull request
//! and is then deleted by `cache-prune.yml` when it closes. Measured on 2026-09-08: 6,393 active
//! entries, roughly 97% of them under `refs/pull/<n>/merge`. The shape ordinary CI is allowed is
//! therefore restore on every event, save only from a push to `main`.
//!
//! **Why it is a gate and not a sentence in a header.** Before this, the claim lived in prose and
//! was held by nothing: `check-workflows` read flake references, `zizmor` reads security shapes,
//! and neither can tell a cache action that saves from one that does not. A rule with no mechanism
//! is a wish, and the same file's own history is the argument - the previous action wrote on every
//! event for as long as it was there and every gate stayed green.
//!
//! **It reads text, offline, in `checks.hygiene`'s sandbox** - no nix, no network, no API. So it
//! cannot know what an action DOES; it knows which action a step names and how that step is gated.
//! Three consequences, each of them a limit rather than a caveat:
//!
//! * [`WRITERS`] is a NAME LIST. A cache-writing action nobody has named here passes unseen. That
//!   is the failure mode to widen when a new one arrives, and it is why [`NO_RESTORE_ONLY`] refuses
//!   by name rather than by capability.
//! * A gate is accepted from the step's own `if:` or from a `save:` input, and **not** from the
//!   enclosing job's `if:` or from a `needs:` chain. Narrower than the truth on purpose: the step
//!   is where a reader looks, and a job-level condition is one refactor away from covering a step
//!   it never meant to.
//! * "Restore everywhere" is held too, on the wire the write-half cannot see. [`STORE_CACHE`]'s
//!   `save:` gates the write and the anchor counts it; `if:` is how a pull request's restore is
//!   stopped while that `save:` stays intact - either on the step itself or on the caller that
//!   invokes [`STORE_ACTION`]. So [`judge`] also refuses an `if:` on the restore step, and on any
//!   step that `uses` the store-cache action. A restore gated to `main` is convention closed.
//! * Whether the entry FITS is not a question about text at all. GitHub refuses a single cache
//!   entry over 10 GB and nothing here can weigh a store, so the run reports its own size instead -
//!   see the summary step in `.github/actions/nix-store-cache`.
//!
//! # And the carrier that was DELETED, held as an absence
//!
//! `docs/adr/0026` retired a third-party binary cache that was wired into `ci.yml` and
//! `cross-link.yml` and gated on `secrets.NIX_CACHE_SUBSTITUTER`,
//! `secrets.NIX_CACHE_PUBLIC_KEY`, `vars.NIX_CACHE_NAME` and `secrets.NIX_CACHE_AUTH_TOKEN` -
//! **none of which has ever existed in this repository.** So its populate step was
//! `completed/skipped` on every `main` push it ran on (run 34247863640, step 6), silently and
//! green, and a deletion that nothing witnesses is one editor away from coming back the same way.
//! Three refusals plus the record hold it, shaped after [`super::sast`] because that module answers
//! the same question one row over - *is the recorded absence still the truth*:
//!
//! * **No hosted publisher while the record stands.** [`HOSTED`] over the whole `.github` tree,
//!   not just the pull-request closure: re-adding it to `release.yml` is the same return. The
//!   direction is deliberate - provisioning a binary cache later is a GOOD change, and it makes
//!   `docs/adr/0026` wrong the moment it lands, so the refusal names the record to edit rather
//!   than forbidding the tool.
//! * **No step decided by state the workflow cannot see.** [`UNOBSERVABLE`] refuses a gate reading
//!   `secrets.` or `vars.`. `secrets` is not available to an `if:` at all - `cross-link.yml` said
//!   so in prose while depending on it - and a `vars.X != ''` test in one is exactly the shape
//!   that skipped invisibly here. It is the general rule and not a name list, so a *different*
//!   secret-gated step gets caught by shape rather than by somebody remembering to list it.
//! * **No store outside this repository trusted.** [`SUBSTITUTER`] is the one the other two cannot
//!   reach: the retired wiring's trust half was two `${{ }}` interpolations inside
//!   `install-nix-action`'s `extra_nix_config` VALUE, not a gate and not a step, so a step-shaped
//!   rule passes it. That half is the supply-chain surface - a substituter plus a trusted key means
//!   CI fetches paths signed by a key held elsewhere - so it is refused as a LINE.
//! * **And the record itself.** Missing or empty is refused, for [`super::sast`]'s reason: a
//!   refusal enforcing a decision nobody wrote down is a rule with no reason.
//!
//! What the three do NOT hold: [`SUBSTITUTER`] reads `<name> =` on ONE line, so a value assembled
//! from pieces, or one reaching the runner through some other action's input, passes unseen;
//! [`HOSTED`] is a name list with [`WRITERS`]'s limit; the `hosted:` INPUT the deletion also
//! removed carried no decision and is held by nothing; and nothing here can tell whether a store a
//! workflow names is trustworthy, only that it is named. `zizmor` and review cover the rest.

use std::collections::BTreeSet;
use std::path::Path;

use super::reach::Closure;

/// The main-push condition, token for token.
///
/// Pinned rather than matched loosely, for `crate::workflows::step`'s reason: a `contains` check
/// passes on `github.event_name != 'push'` and on an inverted ternary. Extra `&&` conjuncts are
/// accepted, because `A && B && C` is strictly narrower than `A && B` for any `C` (`!cancelled()`
/// is the plausible one), and a `||` anywhere in the remainder is refused because it can widen.
/// The conjunct that USED to be here - `vars.NIX_CACHE_NAME != ''` on the retired populate step -
/// is now refused a layer up by [`UNOBSERVABLE`]: narrower is not the same as observable.
const MAIN_PUSH: &str = "github.event_name == 'push' && github.ref == 'refs/heads/main'";

/// Actions that write a cache, and are therefore only allowed behind [`MAIN_PUSH`].
///
/// Matched on the `owner/repo` prefix so a version pin, a subpath (`actions/cache/save`) or a
/// trailing comment does not evade it.
const WRITERS: [&str; 2] = ["nix-community/cache-nix-action", "actions/cache"];

/// Publishers to a store OUTSIDE this repository, refused outright while [`RECORD`] stands.
///
/// Not in [`WRITERS`]: gating one correctly is no longer enough, because `docs/adr/0026` records
/// that none runs here at all. Both were in that list until this record existed, and
/// `cachix/cachix-action` was the entry whose correctly-gated presence hid the store anchor's hole
/// - see [`STORE_CACHE`].
const HOSTED: [&str; 2] = ["cachix/cachix-action", "DeterminateSystems/flakehub-cache-action"];

/// Contexts a workflow cannot observe, and therefore may not decide a step on.
///
/// `secrets` is not exposed to an `if:` at ALL, so a step gated on one never runs - and the
/// retired populate step's own comment asserted that while the step next to it depended on
/// `vars.NIX_CACHE_NAME != ''`, which GitHub evaluates to `false` on an unprovisioned repository.
/// Either way the step reports `completed/skipped`: green, invisible, and indistinguishable from a
/// step that did its job. Matched as a bare prefix so `vars.ANYTHING` is covered, because the
/// failure mode is the SHAPE and not the four names that happened to be used.
const UNOBSERVABLE: [&str; 2] = ["secrets.", "vars."];

/// The nix settings that make CI TRUST a store, refused in `.github` outright.
///
/// The publisher half of the retired wiring was a step, which [`HOSTED`] can see. The trust half
/// was two `${{ }}` interpolations inside `install-nix-action`'s `extra_nix_config` VALUE, which no
/// step-shaped rule reaches - and it is the half that matters for supply chain, because a
/// substituter plus a trusted key means CI fetches store paths signed by a key held outside this
/// repository. `ci.yml` deliberately withholds the flag that would let `flake.nix` add its own, and
/// `docs/adr/0026` records that no third-party one is configured here either. So this is a LINE
/// rule rather than a step rule: an assignment of one of these names anywhere in a workflow or a
/// composite action, comments excluded.
///
/// Comments excluded is load-bearing rather than tidy: six files in `.github` explain in prose why
/// the flag is absent, and a rule that read those would refuse the documentation of itself.
const SUBSTITUTER: [&str; 2] = ["substituters", "trusted-public-keys"];

/// The record that retires the hosted cache, and therefore the file every refusal here points at.
const RECORD: &str = "docs/adr/0026-no-third-party-binary-cache.md";

/// Where the files these two rules read live, relative to the repository root.
///
/// Read directly rather than through [`Closure`], and that is the point of the pair: the closure is
/// what ordinary CI reaches, so a hosted publisher re-added to `release.yml` or to a workflow
/// nobody has wired yet would pass it. `docs/adr/0026` is a statement about the repository.
const GITHUB: [&str; 2] = [".github/workflows", ".github/actions"];

/// The one that carries `/nix/store`, and therefore the one the anchor below counts.
///
/// **Separate from [`WRITERS`] because a floor over the whole list does not hold anything.** The
/// anchor first counted every gated writer, and `ci.yml`'s two `cachix/cachix-action` steps - gated
/// correctly, and inert while unprovisioned - satisfied it on their own. Deleting the store cache
/// outright then left the gate GREEN, which is the shape this tree calls a floor counted off the
/// same derivation as its own loop. Measured by provocation, not reasoned. Those two steps are
/// deleted now and [`HOSTED`] refuses their return, so the tree can no longer show it either -
/// which is why `a_gated_writer_that_is_not_the_store_cache_does_not_satisfy_the_anchor` holds it
/// on `actions/cache` in a fixture instead.
const STORE_CACHE: &str = "nix-community/cache-nix-action";

/// The local composite action that carries `/nix/store`, matched as `uses:` on its callers.
///
/// The write-half is held inside the action (`save:` gated on [`MAIN_PUSH`]), so a caller is the
/// one place that can stop the RESTORE without tripping it: adding `if: ${{ MAIN_PUSH }}` to the
/// caller - or `if: false` to kill it outright - leaves the store step's own `save:` gate intact
/// and `check-workflows` green. Holding "restore everywhere" therefore pins the restore step AND
/// its callers ungated; `save:` is the only gate any of them may carry.
const STORE_ACTION: &str = "./.github/actions/nix-store-cache";

/// Writers with no restore-only mode, refused in ordinary CI outright.
///
/// `magic-nix-cache-action`'s `use-gha-cache` takes `enabled`, `disabled` or `no-preference` and
/// turns the GitHub Actions cache off for reads and writes TOGETHER - read from its `action.yml` at
/// the commit this repository used to pin. So no condition can make it restore-only, and gating it
/// would mean a pull request either writes entries nobody can read or gets no cache at all. That is
/// what forced the swap to a fork of `actions/cache`; this list is what stops it coming back.
///
/// A NAME REFUSAL AND NOTHING MORE. It cannot see that a future version grew such an input; if one
/// does, this list is the thing to change, with the input named beside it.
const NO_RESTORE_ONLY: [&str; 1] = ["DeterminateSystems/magic-nix-cache-action"];

/// Every cache rule this module holds: the restore-only shape over ordinary CI, plus the retired
/// hosted cache held as an absence over the whole `.github` tree.
///
/// The closure half is over the whole closure rather than one file name, for `super::reach`'s
/// reason: a hard line cap moves steps between files, and a refusal that names a file stops
/// covering the step that left it. The absence half is deliberately WIDER than the closure - see
/// [`GITHUB`].
pub(super) fn problems(root: &Path, closure: &Closure) -> Vec<String> {
    let files: Vec<(&str, &str)> = closure.inspected().iter().map(|file| (file.label(), file.text())).collect();
    let mut out = judge(&files);
    out.extend(record(root));
    out.extend(retired(&read_github(root)));
    out
}

/// Whether the record the two absence rules enforce is still there and still says something.
///
/// `super::sast` learned the empty half the hard way one file over: a blank file satisfies a
/// `read_to_string` and turns an argued decision into a filename.
fn record(root: &Path) -> Vec<String> {
    match std::fs::read_to_string(root.join(RECORD)) {
        Ok(text) if text.trim().is_empty() => vec![format!(
            "{RECORD} is empty - the retired binary cache needs a dated reason, not a blank file"
        )],
        Err(error) => vec![format!(
            "{RECORD} could not be read: {error} - that record is where deleting the third-party binary cache is argued, and the refusals below enforce its claims"
        )],
        Ok(_) => Vec::new(),
    }
}

/// Every workflow and composite action under [`GITHUB`], as `(label, text)`.
///
/// Sorted, so a refusal list is stable between runs and a test can assert on the first entry. An
/// unreadable directory returns nothing rather than a problem: `super::run` has already refused an
/// unreadable CI closure by the time this is called, and this rule is not the place to report it.
fn read_github(root: &Path) -> Vec<(String, String)> {
    let mut out = BTreeSet::new();
    for dir in GITHUB {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // A workflow is a file in `workflows/`; a composite action is `action.yml` inside its
            // own directory under `actions/`. Both shapes, one walk.
            let candidates = if path.is_dir() {
                vec![path.join("action.yml"), path.join("action.yaml")]
            } else {
                vec![path]
            };
            for file in candidates {
                let extension = file.extension().and_then(|e| e.to_str()).unwrap_or_default();
                if !extension.eq_ignore_ascii_case("yml") && !extension.eq_ignore_ascii_case("yaml") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&file) else {
                    continue;
                };
                let label = file
                    .strip_prefix(root)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                out.insert((label, text));
            }
        }
    }
    out.into_iter().collect()
}

/// The retired hosted cache, held as an absence: no publisher, and no unobservable gate.
///
/// Over labelled text so a fixture can exercise both arms - the live tree can only ever show them
/// passing, which is the whole difficulty with gating something that is not there.
fn retired(files: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (label, text) in files {
        for step in steps(text) {
            if let Some(publisher) = step.uses().and_then(|uses| {
                HOSTED
                    .iter()
                    .find(|name| uses.starts_with(**name))
                    .map(|name| String::from(*name))
            }) {
                out.push(format!(
                    "{label}:{}  {publisher} publishes to a store outside this repository, and {RECORD} records that none runs here - provisioning one is a good change, so update that record rather than this gate",
                    step.line
                ));
            }
            let Some(gate) = step.gate() else { continue };
            if let Some(context) = UNOBSERVABLE.iter().find(|name| gate.contains(**name)) {
                out.push(format!(
                    "{label}:{}  is gated on `{gate}`, which reads `{context}` - a workflow cannot observe whether a repository secret or variable exists, so an unprovisioned one makes this step `completed/skipped`: green, and witnessed by nothing. That is what {RECORD} deleted",
                    step.line
                ));
            }
        }
        out.extend(trusted_stores(label, text));
    }
    out
}

/// Every line of one file that would make CI trust a store outside this repository.
///
/// A LINE rule and not a step rule, and the limit is worth stating where a reader meets it: the
/// setting has to appear as `<name> =` on one line, which is how `nix.conf` and
/// `install-nix-action`'s `extra_nix_config` block both spell it. A value assembled from pieces, or
/// one reaching the runner through a different action's input, passes unseen.
fn trusted_stores(label: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(code) = key_of(line) else { continue };
        for setting in SUBSTITUTER {
            let assigns = code
                .split_once(setting)
                .is_some_and(|(_, rest)| rest.trim_start().starts_with('='));
            if assigns {
                out.push(format!(
                    "{label}:{}  assigns `{setting}`, which makes CI trust a store outside this repository - {RECORD} records the Actions cache as the only carrier, so update that record rather than this gate",
                    index.saturating_add(1)
                ));
            }
        }
    }
    out
}

/// The same rules over labelled text, so the ANCHOR can be tested.
///
/// Split from [`problems`] because the anchor is the one rule a provocation on the real tree cannot
/// exercise: with the store cache deleted, `ci.yml`'s two correctly gated `cachix/cachix-action`
/// steps satisfied a floor written over every writer, and the gate stayed GREEN. Fixtures are the
/// only way to hold *a gated writer that is not the store cache does not count* as a test rather
/// than as a paragraph - see `a_gated_hosted_cache_does_not_satisfy_the_store_anchor`.
fn judge(files: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut store = 0_usize;

    for (label, text) in files {
        for step in steps(text) {
            let Some(uses) = step.uses() else { continue };
            // THE RESTORE HALF, ON THE WIRE THE WRITE-HALF CANNOT SEE. A caller names no writer, so
            // only the action's own `save:` gate protects the write - which means a caller can add
            // `if: ${{ MAIN_PUSH }}` (or `if: false`) to stop a pull request's RESTORE while
            // leaving `save:` intact and `check-workflows` green. `if:` on a caller is refused.
            if uses == STORE_ACTION {
                if step.has("if:") {
                    out.push(format!(
                        "{label}:{}  {uses} must carry no `if:` - it restores on every event, and gating it (e.g. `if: ${{{{ {MAIN_PUSH} }}}}`) stops a pull request's restore while the `save:` inside the action passes the write anchor",
                        step.line
                    ));
                }
                continue;
            }
            if let Some(refused) = NO_RESTORE_ONLY.iter().find(|name| uses.starts_with(**name)) {
                out.push(format!(
                    "{label}:{}  {refused} has no restore-only mode, so it may not run in ordinary CI",
                    step.line
                ));
                continue;
            }
            if !WRITERS.iter().any(|name| uses.starts_with(*name)) {
                continue;
            }
            match step.gate() {
                Some(gate) if narrower_than_main_push(&gate) => {
                    if uses.starts_with(STORE_CACHE) {
                        store = store.saturating_add(1);
                        // `save:` gates the write; an `if:` on the same step would stop the restore
                        // while the anchor below still passes. So the store step may carry no `if:`.
                        if step.has("if:") {
                            out.push(format!(
                                "{label}:{}  {uses} must carry no `if:` - `save:` gates the write, and an `if:` (e.g. `{MAIN_PUSH}`) would stop a pull request's restore while the write anchor passes",
                                step.line
                            ));
                        }
                    }
                }
                Some(gate) => out.push(format!(
                    "{label}:{}  {uses} is gated on `{gate}`, which is not `{MAIN_PUSH}` plus `&&` conjuncts",
                    step.line
                )),
                None => out.push(format!(
                    "{label}:{}  {uses} can write the Actions cache on any event - it needs `save:` or `if:` gated on `{MAIN_PUSH}`",
                    step.line
                )),
            }
        }
    }

    // AN EMPTY PASS IS THIS GATE'S OWN FAILURE MODE, and the reason it is checked here rather than
    // trusted: every rule above is a refusal, so a scan that finds no store-cache step at all
    // reports clean over nothing - which is exactly what a renamed action or a reindented step
    // would do. Counted on [`STORE_CACHE`] alone; see that constant for the provocation that says
    // why a count over every writer held nothing.
    if store == 0 {
        out.push(format!(
            "no gated `{STORE_CACHE}` step in ordinary CI - either nothing carries /nix/store between runs any more, or this scan stopped matching the files"
        ));
    }
    out
}

/// Is `gate` at least as narrow as [`MAIN_PUSH`]?
///
/// `A && B` exactly, or `A && B && …` where the remainder holds no `||`. Anything else is refused,
/// including an expression that merely CONTAINS the pin: `!(A && B)` and `A && B || C` both do.
fn narrower_than_main_push(gate: &str) -> bool {
    let Some(rest) = gate.strip_prefix(MAIN_PUSH) else {
        return false;
    };
    rest.strip_prefix(" && ")
        .map_or(rest.is_empty(), |extra| !extra.is_empty() && !extra.contains("||"))
}

/// One step of a workflow or composite action, and the line a failure should name.
struct Step<'a> {
    /// One-based, so a reader can open `ci.yml:258`.
    line: usize,
    lines: Vec<&'a str>,
}

impl Step<'_> {
    /// The `uses:` value with its version pin and any trailing comment removed.
    ///
    /// `None` for a `run:` step, and for a step whose only `uses:` is inside a `#` comment - the
    /// dead-check shape `super::step`'s header records, and the one that would otherwise refuse
    /// this repository's own prose about the action it stopped using.
    fn uses(&self) -> Option<String> {
        self.lines.iter().find_map(|line| {
            let key = key_of(line)?;
            let value = key.strip_prefix("uses:")?.trim();
            let value = value.split('#').next().unwrap_or_default().trim();
            let name = value.split('@').next().unwrap_or_default().trim();
            (!name.is_empty()).then(|| String::from(name))
        })
    }

    /// What gates this step, from its own `if:` or from a `save:` input, with `${{ }}` unwrapped
    /// and whitespace collapsed.
    ///
    /// Both, and the first one found, because the two spellings are not interchangeable:
    /// `actions/cache` has no `save` input and must be gated by `if:`, while its nix-store fork
    /// must restore on every event and so can only be gated by `save:`. A `save: false` is a
    /// constant rather than a condition and is refused as such - restore-only everywhere would mean
    /// nothing ever writes, which is what the anchor above exists to catch.
    fn gate(&self) -> Option<String> {
        self.lines.iter().find_map(|line| {
            let key = key_of(line)?;
            let value = key.strip_prefix("if:").or_else(|| key.strip_prefix("save:"))?.trim();
            let inner = value
                .strip_prefix("${{")
                .and_then(|rest| rest.strip_suffix("}}"))
                .unwrap_or(value);
            Some(inner.split_whitespace().collect::<Vec<_>>().join(" "))
        })
    }

    /// Whether the step carries `key` as its own key, comment lines excluded.
    ///
    /// Distinct from [`Step::gate`], which collapses `if:` and `save:` into one condition: the
    /// restore-half has to know which spelling is present, because `save:` is where the write is
    /// held and `if:` is where a restore is stopped.
    fn has(&self, key: &str) -> bool {
        self.lines
            .iter()
            .any(|line| key_of(line).is_some_and(|k| k.strip_prefix(key).is_some()))
    }
}

/// One line's key, whether it sits on the `-` marker or below it, and `None` for a comment.
///
/// The comment arm is what keeps a header that DISCUSSES an action from reading as a step that
/// uses one - this file's own refusal is quoted in three headers in `.github/`.
fn key_of(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let key = trimmed.strip_prefix('-').map_or(trimmed, str::trim_start);
    (!key.starts_with('#')).then_some(key)
}

/// Every step in one file, by indentation.
///
/// An indentation reader and not a YAML parser, for `super::step::shell`'s stated reason: this has
/// to run where there is no nix and no dependency. A step is a `- ` list item; it ends at the next
/// nonempty line no deeper than its marker. Workflows and composite actions write steps at
/// different columns, so the depth comes from the marker rather than from a constant - a composite
/// action's `runs.steps` sits two levels shallower than a job's.
fn steps(text: &str) -> Vec<Step<'_>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- ") {
            continue;
        }
        let depth = line.len().saturating_sub(trimmed.len());
        let end = lines
            .iter()
            .enumerate()
            .skip(index.saturating_add(1))
            .find(|(_, next)| !next.trim().is_empty() && next.len().saturating_sub(next.trim_start().len()) <= depth)
            .map_or(lines.len(), |(at, _)| at);
        out.push(Step {
            line: index.saturating_add(1),
            lines: lines.get(index..end).unwrap_or_default().to_vec(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{MAIN_PUSH, narrower_than_main_push, steps};

    /// A step naming `action`, gated by `gate` written as `key`.
    fn workflow(action: &str, key: &str, gate: &str) -> String {
        format!(
            concat!(
                "jobs:\n",
                "  ci:\n",
                "    steps:\n",
                "      - uses: actions/checkout@aaaa # v7\n",
                "\n",
                "      - uses: {}@bbbb # v1\n",
                "        with:\n",
                "          {} {}\n",
                "\n",
                "      - run: nix build .#checks.x86_64-linux.hygiene -L\n",
            ),
            action, key, gate
        )
    }

    #[test]
    fn a_step_gated_on_the_main_push_is_the_only_accepted_shape() {
        for accepted in [MAIN_PUSH.to_owned(), format!("{MAIN_PUSH} && !cancelled()")] {
            assert!(narrower_than_main_push(&accepted), "{accepted}");
        }
        // Each of these is a `pull_request` path that can save, and each was reachable by editing
        // one token of the accepted form.
        for refused in [
            "github.event_name == 'pull_request' && github.ref == 'refs/heads/main'",
            "github.event_name != 'push' && github.ref == 'refs/heads/main'",
            "github.ref == 'refs/heads/main'",
            "github.event_name == 'push'",
            &format!("{MAIN_PUSH} || github.event_name == 'pull_request'"),
            &format!("!({MAIN_PUSH})"),
            &format!("{MAIN_PUSH} && "),
            "true",
        ] {
            assert!(!narrower_than_main_push(refused), "{refused}");
        }
    }

    #[test]
    fn a_writer_with_no_gate_is_refused_and_a_gated_one_is_not() {
        let root = crate::repo::root().expect("the repository root");
        let ungated = workflow("nix-community/cache-nix-action", "save:", "true");
        let step = steps(&ungated)
            .into_iter()
            .find(|step| step.uses().as_deref() == Some("nix-community/cache-nix-action"))
            .expect("the cache step");
        assert_eq!(step.gate().as_deref(), Some("true"));
        assert!(!narrower_than_main_push(&step.gate().unwrap_or_default()));

        let gated = workflow("nix-community/cache-nix-action", "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        let step = steps(&gated)
            .into_iter()
            .find(|step| step.uses().as_deref() == Some("nix-community/cache-nix-action"))
            .expect("the cache step");
        assert!(narrower_than_main_push(&step.gate().unwrap_or_default()), "{:?}", step.gate());
        // The repository root is read by the live assertion below; touching it here keeps this
        // test honest about needing a checkout rather than silently passing without one.
        assert!(root.join("flake.nix").exists());
    }

    #[test]
    fn a_commented_out_writer_is_not_a_step_and_a_discussed_one_is_not_either() {
        // Both shapes are live in this tree: `.github/actions/nix-store-cache` DISCUSSES
        // `magic-nix-cache-action` in its header, and a parked step is the dead-check shape
        // `super::step` records. A `contains` scan over the file refuses both.
        let prose = concat!(
            "jobs:\n",
            "  ci:\n",
            "    steps:\n",
            "      # Why not DeterminateSystems/magic-nix-cache-action@aaaa: no restore-only mode.\n",
            "      - uses: actions/checkout@aaaa # v7\n",
            "        with:\n",
            "          # uses: actions/cache@bbbb\n",
            "          persist-credentials: false\n",
        );
        for step in steps(prose) {
            let uses = step.uses().unwrap_or_default();
            assert!(
                !uses.contains("magic-nix-cache") && !uses.contains("actions/cache"),
                "a comment is not a step: {uses}"
            );
        }
    }

    /// One step naming `action` with `key: gate` under `with:`, at a job's indentation.
    fn step_using(action: &str, key: &str, gate: &str) -> String {
        format!("      - uses: {action}@bbbb # v1\n        with:\n          {key} {gate}\n")
    }

    #[test]
    fn a_gated_writer_that_is_not_the_store_cache_does_not_satisfy_the_anchor() {
        // THE HOLE THIS GATE WAS FIRST WRITTEN WITH, FOUND BY PROVOCATION AND NOW HELD BY A TEST.
        // `ci.yml` gated two `cachix/cachix-action` steps correctly, and a floor written over every
        // writer counted them - so deleting the store cache outright left the gate green. That is a
        // floor counted off the same derivation as its own loop. It was unprovable on the tree then
        // because the tree had both; it is unprovable now because `docs/adr/0026` deleted the
        // hosted steps and `HOSTED` refuses their return. So the fixture uses `actions/cache`, which
        // is still an accepted writer and is still not the store cache.
        let other = step_using("actions/cache", "if:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        let found = super::judge(&[("ci.yml", &other)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found.first().is_some_and(|problem| problem.contains(super::STORE_CACHE)),
            "a correctly gated writer is not a store cache: {found:#?}"
        );

        // The same file plus a gated store cache is the shape this repository has, and it passes.
        let store = step_using(super::STORE_CACHE, "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        let both = format!("{other}\n{store}");
        let clean = super::judge(&[("ci.yml", &both)]);
        assert!(clean.is_empty(), "{clean:#?}");
    }

    /// One labelled file, for the absence rules - they read `(String, String)` off a directory walk
    /// rather than the closure's borrowed pairs.
    fn owned(label: &str, text: &str) -> Vec<(String, String)> {
        vec![(String::from(label), String::from(text))]
    }

    #[test]
    fn the_deleted_hosted_publisher_is_refused_by_name_and_points_at_the_record() {
        // THE DELETION, PROVOKED. `docs/adr/0026` removed exactly this step from `ci.yml` and
        // `cross-link.yml`; putting it back is what this must make red. Written verbatim as it
        // stood, pin and all, so the fixture is the thing that was deleted rather than a paraphrase.
        let back = concat!(
            "      - name: Populate the binary cache\n",
            "        uses: cachix/cachix-action@38b082610b782e7e93e209c35fd730d399dee866 # v17\n",
            "        with:\n",
            "          name: sutura\n",
        );
        let found = super::retired(&owned(".github/workflows/ci.yml", back));
        assert_eq!(found.len(), 1, "{found:#?}");
        let problem = found.first().map_or("", String::as_str);
        assert!(
            problem.starts_with(".github/workflows/ci.yml:1  cachix/cachix-action"),
            "{problem}"
        );
        assert!(problem.contains(super::RECORD), "{problem}");

        // The other publisher, and a fork of it: matched on the prefix, so a subpath cannot evade.
        for name in ["DeterminateSystems/flakehub-cache-action", "cachix/cachix-action/sub"] {
            let step = format!("      - uses: {name}@aaaa # v1\n");
            assert_eq!(super::retired(&owned("release.yml", &step)).len(), 1, "{name}");
        }

        // AND THE DIRECTION THAT KEEPS THE RECORD HONEST: a publisher is refused wherever it is,
        // including a workflow ordinary CI never reaches. `read_github` is what makes that true and
        // `GITHUB` says why.
        assert!(!super::GITHUB.is_empty(), "the walk has to cover somewhere");
    }

    #[test]
    fn a_step_gated_on_a_secret_or_a_variable_is_refused() {
        // THE SHAPE, NOT THE NAMES. Each of these was live in this tree and each was green while
        // doing nothing: the populate step's own `if:`, and the same test written as a `save:`.
        for gate in [
            format!("{MAIN_PUSH} && vars.NIX_CACHE_NAME != ''"),
            String::from("secrets.NIX_CACHE_AUTH_TOKEN != ''"),
            format!("{MAIN_PUSH} && vars.SOMETHING_ELSE == 'yes'"),
        ] {
            let step = step_using("actions/checkout", "if:", &format!("${{{{ {gate} }}}}"));
            let found = super::retired(&owned("ci.yml", &step));
            assert_eq!(found.len(), 1, "{gate}: {found:#?}");
            assert!(found.first().is_some_and(|p| p.contains("completed/skipped")), "{found:#?}");
        }

        // A `save:` reading a variable is the same defect in the other spelling, and `gate()` is
        // what makes one rule cover both.
        let save = step_using(super::STORE_CACHE, "save:", "${{ vars.WRITE_THE_CACHE == 'yes' }}");
        assert_eq!(super::retired(&owned("ci.yml", &save)).len(), 1);

        // AND THE OTHER DIRECTION, so this cannot pass by refusing everything: the live shape,
        // whose gate names only contexts a workflow can evaluate for itself.
        let live = step_using(super::STORE_CACHE, "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        assert!(super::retired(&owned("ci.yml", &live)).is_empty());
        // A secret in a `with:` VALUE is legitimate and is NOT this rule's business - the bigquery
        // credential is exactly that. Stated as a test because it is the limit, not an oversight.
        let credential = step_using("actions/checkout", "token:", "${{ secrets.GITHUB_TOKEN }}");
        assert!(
            super::retired(&owned("ci.yml", &credential)).is_empty(),
            "a `with:` value is not a gate"
        );
    }

    #[test]
    fn a_workflow_that_trusts_a_store_outside_this_repository_is_refused() {
        // THE TRUST HALF, PROVOKED, and it is the half no step-shaped rule sees. Written verbatim
        // as `ci.yml` and `cross-link.yml` carried it, secret interpolation and all.
        let wired = concat!(
            "      - uses: cachix/install-nix-action@13d8dd58 # v31.11.1\n",
            "        with:\n",
            "          extra_nix_config: |\n",
            "            fallback = true\n",
            "            ${{ secrets.NIX_CACHE_SUBSTITUTER != '' && format('substituters = {0}', secrets.NIX_CACHE_SUBSTITUTER) || '' }}\n",
            "            ${{ secrets.NIX_CACHE_PUBLIC_KEY != '' && format('trusted-public-keys = {0}', secrets.NIX_CACHE_PUBLIC_KEY) || '' }}\n",
        );
        let found = super::retired(&owned(".github/workflows/ci.yml", wired));
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found.iter().any(|p| p.contains("assigns `substituters`")), "{found:#?}");
        assert!(
            found.iter().any(|p| p.contains("assigns `trusted-public-keys`")),
            "{found:#?}"
        );
        assert!(found.iter().all(|p| p.contains(super::RECORD)), "{found:#?}");

        // A bare `substituters = https://…`, the shape a local edit reaches for, and the line
        // number it must name.
        let bare = "      - with:\n          extra_nix_config: |\n            substituters = https://example.invalid\n";
        let plain = super::retired(&owned("ci.yml", bare));
        assert_eq!(plain.len(), 1, "{plain:#?}");
        assert!(plain.first().is_some_and(|p| p.starts_with("ci.yml:3")), "{plain:#?}");

        // AND THE PROSE, which is why comments are excluded: six files in `.github` explain why
        // the flag is absent, and `fallback = true` is the live line that must not trip it.
        let prose = concat!(
            "          # with it, a pull request could add a substituter and a trusted key\n",
            "          # substituters = https://example.invalid was weighed and refused\n",
            "          extra_nix_config: |\n",
            "            fallback = true\n",
        );
        assert!(
            super::retired(&owned("ci.yml", prose)).is_empty(),
            "a comment is not an assignment"
        );
    }

    #[test]
    fn the_record_itself_has_to_exist_and_say_something() {
        // The two refusals above enforce a decision, and a decision nobody wrote down is a rule
        // with no reason. `super::sast` learned the EMPTY half one file over.
        let root = std::env::temp_dir().join(format!("sutura-cache-scope-record-{}", std::process::id()));
        let record = root.join(super::RECORD);
        let parent = record.parent().expect("the record sits in a directory");
        std::fs::create_dir_all(parent).expect("temp adr dir");

        let missing = super::record(&root);
        assert_eq!(missing.len(), 1, "{missing:#?}");
        assert!(
            missing.first().is_some_and(|p| p.contains("could not be read")),
            "{missing:#?}"
        );

        std::fs::write(&record, "   \n\n").expect("blank the record");
        let blank = super::record(&root);
        assert_eq!(blank.len(), 1, "{blank:#?}");
        assert!(blank.first().is_some_and(|p| p.contains("is empty")), "{blank:#?}");

        std::fs::write(&record, "the decision, argued\n").expect("write the record");
        assert!(super::record(&root).is_empty());
        std::fs::remove_dir_all(&root).expect("the temp tree this test created is removable");
    }

    #[test]
    fn the_committed_tree_carries_no_hosted_publisher_and_no_unobservable_gate() {
        // THE LIVE ASSERTION for the absence half, and the one that goes red if the deletion is
        // reverted in ANY `.github` file rather than only in the pull-request closure.
        let Some(root) = crate::repo::root() else { return };
        let files = super::read_github(&root);
        assert!(
            files.len() >= 10,
            "the walk found {} file(s), which is too few to have read .github at all",
            files.len()
        );
        assert!(
            files.iter().any(|(label, _)| label == ".github/workflows/ci.yml"),
            "{files:#?}"
        );
        assert!(
            files
                .iter()
                .any(|(label, _)| label == ".github/actions/nix-store-cache/action.yml"),
            "a composite action is a directory, and the walk has to open action.yml inside it"
        );
        let found = super::retired(&files);
        assert!(found.is_empty(), "{found:#?}");
        assert!(super::record(&root).is_empty(), "{:#?}", super::record(&root));
    }

    #[test]
    fn an_ungated_writer_is_named_with_its_file_and_line() {
        // The message a reader acts on, and the arm a `None => {}` mutation would silence. Asserted
        // on the text because the label and line moved when `problems` was split from `judge`, and
        // a refusal that names nothing openable is half a gate.
        let store = step_using(super::STORE_CACHE, "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        let ungated = format!("{store}\n{}", step_using("actions/cache", "path:", "/nix/store"));
        let found = super::judge(&[("ci.yml", &ungated)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        let problem = found.first().map_or("", String::as_str);
        assert!(problem.starts_with("ci.yml:5  actions/cache"), "{problem}");
        assert!(problem.contains("can write the Actions cache on any event"), "{problem}");
    }

    #[test]
    fn a_writer_with_no_restore_only_mode_is_refused_by_name() {
        // The arm that keeps `magic-nix-cache-action` out, in a fixture rather than only as a
        // provocation on `ci.yml` - so neutralising the name list is red without an edit to a
        // workflow.
        let store = step_using(super::STORE_CACHE, "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        let back = format!("{store}\n      - uses: DeterminateSystems/magic-nix-cache-action@908b263f # v14\n");
        let found = super::judge(&[("ci.yml", &back)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found.first().is_some_and(|p| p.contains("has no restore-only mode")),
            "{found:#?}"
        );
    }

    #[test]
    fn a_caller_that_gates_the_store_restore_is_refused() {
        // THE RESTORE HALF, PROVOKED. A caller names no writer (`uses: ./.github/actions/...`),
        // so `if:` on it stops a pull request's RESTORE while the action's own `save:` keeps the
        // write-half green - the review exercised `if: ${{ MAIN_PUSH }}` and `if: false` and both
        // stayed GREEN before this. The ungated store step is included so the anchor is satisfied
        // and the caller gate is the sole finding.
        let store = step_using(super::STORE_CACHE, "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        for gate in [MAIN_PUSH.to_owned(), "false".to_owned()] {
            let caller = step_using(super::STORE_ACTION, "if:", &format!("${{{{ {gate} }}}}"));
            let found = super::judge(&[("ci.yml", &format!("{caller}{store}"))]);
            assert_eq!(found.len(), 1, "{found:#?}");
            assert!(found.first().is_some_and(|p| p.contains("must carry no `if:`")), "{found:#?}");
        }
    }

    #[test]
    fn a_store_step_gated_with_if_is_refused_even_while_save_is_the_main_push() {
        // The pinned expression as an `if:` on the restore step itself. `save:` is where the write
        // is held, so `if: ${{ MAIN_PUSH }}` still satisfies the anchor while never restoring on a
        // pull request. Only the restore-half sees that, because `gate()` collapses the two into
        // one condition that IS `MAIN_PUSH`; `has("if:")` tells them apart.
        let store = format!(
            "      - uses: {action}@bbbb # v1\n        if: ${{{{ {MAIN_PUSH} }}}}\n        with:\n          save: ${{{{ {MAIN_PUSH} }}}}\n",
            action = super::STORE_CACHE,
        );
        let found = super::judge(&[("ci.yml", &store)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found.first().is_some_and(|p| p.contains("must carry no `if:`")), "{found:#?}");
    }

    #[test]
    fn an_ungated_store_step_and_its_ungated_caller_pass_the_restore_rule() {
        // The shape this repository ships: a caller with no `if:` invoking the action, whose
        // restore step carries `save:` only. Guarantees the new rules do not over-fire on the form
        // the live tree uses.
        let caller = format!("      - uses: {}@bbbb # v1\n", super::STORE_ACTION);
        let store = step_using(super::STORE_CACHE, "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        let clean = super::judge(&[("ci.yml", &format!("{caller}{store}"))]);
        assert!(clean.is_empty(), "{clean:#?}");
    }

    #[test]
    fn the_committed_tree_writes_the_actions_cache_only_from_a_push_to_main() {
        // THE LIVE ASSERTION. The unit fixtures above are shapes; this is the repository, and it is
        // what fails when somebody adds a cache write to a pull-request path.
        let Some(root) = crate::repo::root() else { return };
        let ordinary = crate::workflows::contexts::OrdinaryCi::read(&root);
        let found = super::problems(&root, ordinary.closure());
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn the_composite_action_is_the_one_place_that_saves() {
        // The anchor, from the other side: the rule above passes when nothing saves at all, and
        // this is what says the save exists. Named on the file rather than counted, because a
        // count stays green while the save moves to a job no pull request reaches.
        let Some(root) = crate::repo::root() else { return };
        let Ok(text) = std::fs::read_to_string(root.join(".github/actions/nix-store-cache/action.yml")) else {
            panic!("the store-cache action must exist: it is the one place ordinary CI saves");
        };
        let saving: Vec<_> = steps(&text)
            .into_iter()
            .filter(|step| {
                step.uses()
                    .is_some_and(|uses| uses.starts_with("nix-community/cache-nix-action"))
            })
            .collect();
        assert_eq!(saving.len(), 1, "one restore/save step, not {}", saving.len());
        let gate = saving.first().and_then(super::Step::gate).unwrap_or_default();
        assert_eq!(gate, MAIN_PUSH, "the save condition must be the pinned main-push expression");
    }
}
