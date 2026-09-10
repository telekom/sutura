//! Is the RECORDED ABSENCE of a third-party binary cache still the truth?
//!
//! `docs/adr/0026` retired a binary cache that was wired into `ci.yml` and `cross-link.yml` and gated on `secrets.NIX_CACHE_SUBSTITUTER`, `secrets.NIX_CACHE_PUBLIC_KEY`, `vars.NIX_CACHE_NAME`
//! and `secrets.NIX_CACHE_AUTH_TOKEN` - **none of which has ever existed in this repository.** So
//! its populate step was `completed/skipped` on every `main` push it ran on (run 34247863640,
//! step 6), silently and green, and a deletion that nothing witnesses is one editor away from
//! coming back the same way.
//!
//! **Its own file for `crate::workflows::sast`'s reason, and the seam is the question.** The parent
//! asks *may a `pull_request` path WRITE the Actions cache*; this asks *is a recorded absence still
//! absent* - Scorecard's SAST zero, held one file over. Nothing here counts, gates or anchors a
//! cache write.
//!
//! # The refusals, and why each is the shape it is
//!
//! * **No hosted publisher while the record stands.** [`HOSTED`] over the whole `.github` tree, not
//!   just the pull-request closure: re-adding it to `release.yml` is the same return. Provisioning
//!   a cache later is a GOOD change that names the record to edit, not forbidding the tool.
//! * **No step decided by state the workflow cannot see.** [`UNOBSERVABLE`] refuses a gate reading
//!   `secrets.` or `vars.`. `secrets` is not available to an `if:` at all - `cross-link.yml` said
//!   so in prose while depending on it - and a `vars.X != ''` test in one is exactly the shape that
//!   skipped invisibly here. A rule about SHAPE, so a *different* secret-gated step is caught
//!   without anybody remembering to list it.
//! * **No line NAMES a store outside this repository**, in either spelling nix accepts.
//!   [`SUBSTITUTER`] is the one the other two cannot reach: the retired wiring's trust half was two
//!   `${{ }}` interpolations inside `install-nix-action`'s `extra_nix_config` VALUE, neither a gate
//!   nor a step. A substituter plus a trusted key means CI fetches paths signed elsewhere, so it is
//!   refused as a LINE.
//! * **The stores trusted on every path, and the additive PR-only pair** ([`ALLOWED`] and
//!   [`ALLOWED_PRS`]). Every store this repository WRITES it also reads, so the two lists are the
//!   write list: the three-store list anywhere, the four-store list (those three plus the PR cache)
//!   ONLY behind a pull_request-only `if:` ([`narrower_than_pr`]); main never lists the PR key.
//! * **And the record itself.** Missing or empty is refused, for `sast`'s reason.
//!
//! # What this does NOT hold, and one sentence of it used to claim otherwise
//!
//! The [`SUBSTITUTER`] heading read *no store trusted* when it shipped while the rule read only
//! `<name> =`, so `nix build --extra-substituters https://…` was GREEN on merged `main` - an
//! overstated refusal on a supply-chain path is worse than none. Both spellings are refused now;
//! the route that survives is named on [`trusted_stores`]: a value assembled from pieces passes
//! unseen.
//!
//! [`HOSTED`] carries [`super::WRITERS`]'s limit - a name list, so a publisher nobody named passes
//! unseen; `zizmor` and review cover the rest.

use std::collections::BTreeSet;
use std::path::Path;

use super::{key_of, steps};

/// Publishers to a store OUTSIDE this repository.
///
/// Not in [`super::WRITERS`]: gating one correctly is not enough on its own, and
/// `cachix/cachix-action` was the entry whose correctly-gated presence once hid the store anchor's
/// hole - see [`super::STORE_CACHE`]. One of these is now permitted in exactly one file ([`PUBLISH`]) and
/// the other is refused everywhere.
const HOSTED: [&str; 2] = ["cachix/cachix-action", "DeterminateSystems/flakehub-cache-action"];

/// The one publisher this repository runs, and the ONE file it may appear in.
///
/// `docs/adr/0027` supersedes `0026`: a binary cache is provisioned now, so this publisher is no
/// longer refused outright. **Naming the FILE and not merely the action is what keeps the permission
/// narrow.** The publish workflow triggers only on a push to the default branch and takes its
/// credential from an environment whose policy admits only that branch; the same step added to
/// `ci.yml` or `cross-link.yml` runs on the pull-request path, where a same-repository pull request
/// can read secrets - so it stays refused there, which is the whole point of the pairing.
///
/// The other entry in [`HOSTED`] has no permitted file and is refused wherever it appears.
const PUBLISH: (&str, &str) = ("cachix/cachix-action", ".github/workflows/cachix-push.yml");

/// The environment a ci.yml pull-request job must declare to host a `cachix/cachix-action` step.
///
/// Half (a) adds a second, additive cache on the pull-request path, so cachix-action must also run
/// in `ci.yml` - but ONLY in a job whose `environment:` is `cachix-push-pr` AND whose gate is
/// pull_request-only ([`in_pr_publish_job`]). The credential behind that environment is a PR-token
/// (Write on the PR cache, no Write on `sutura`); the environment is the security boundary the gate
/// cannot verify, and this pairing is the text side that keeps it as narrow as the forge allows.
const PUBLISH_PRS: &str = "cachix-push-pr";

/// Whether a cachix-action step's JOB in `ci.yml` is the PR publish write-half: it declares
/// `environment: cachix-push-pr` and its job-level `if:` is PR-only. The job block owns the step;
/// a step outside such a job (main `ci`, `crap-comment`, `bigquery-acceptance`, any other workflow)
/// stays refused by the pairing.
pub(super) fn in_pr_publish_job(text: &str, step_line: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    // Find the job block owning `step_line`: scan two-space job keys; its end is the next key no
    // deeper than two spaces. Comment blocks at two spaces precede a job key and do not own steps.
    let mut spans = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if line.len().saturating_sub(trimmed.len()) == 2 && trimmed.ends_with(':') {
            let end = lines
                .iter()
                .enumerate()
                .skip(i + 1)
                .find(|(_, next)| {
                    let nt = next.trim_start();
                    !nt.is_empty() && next.len().saturating_sub(nt.len()) <= 2 && nt.ends_with(':')
                })
                .map_or(lines.len(), |(j, _)| j);
            spans.push((i, end.min(lines.len())));
        }
    }
    let Some((start, end)) = spans.iter().copied().find(|(s, e)| *s < step_line && step_line < *e) else {
        return false;
    };
    let end = end.min(lines.len());

    // The environment must be declared at the job's own key depth.
    let has_env = (start..end).any(|at| {
        lines.get(at).is_some_and(|line| {
            key_of(line).is_some_and(|k| {
                k.trim_start()
                    .strip_prefix("environment:")
                    .is_some_and(|v| v.trim() == PUBLISH_PRS)
            })
        })
    });
    if !has_env {
        return false;
    }
    // And the job's own `if:` must be PR-only, read ONLY at the job's own key depth (a sibling of
    // its `environment:`/`steps:` keys) - a step-level `if:` under a step marker must not stand in
    // for it, or deleting the job gate would re-open main with the gate still GREEN.
    let scope = lines
        .get(start)
        .map_or(2, |l| l.len().saturating_sub(l.trim_start().len()) + 2);
    (start..end).any(|at| {
        lines.get(at).is_some_and(|line| {
            line.len().saturating_sub(line.trim_start().len()) == scope
                && key_of(line).is_some_and(|k| k.trim_start().strip_prefix("if:").is_some_and(|v| narrower_than_pr(v.trim())))
        })
    })
}

/// Contexts a workflow cannot observe, and therefore may not decide a step on.
///
/// `secrets` is not exposed to an `if:` at ALL, and `vars.X != ''` is `false` when unprovisioned -
/// either way the step reports `completed/skipped`: green, invisible. Matched as a bare prefix so
/// `vars.ANYTHING` is covered - the failure is the SHAPE, not the names.
const UNOBSERVABLE: [&str; 2] = ["secrets.", "vars."];

/// The nix settings that make CI TRUST a store, refused in `.github` outright.
///
/// The publisher half of the retired wiring was a step, which [`HOSTED`] can see. The trust half
/// was two `${{ }}` interpolations inside `install-nix-action`'s `extra_nix_config` VALUE, which no
/// step-shaped rule reaches - and it is the half that matters for supply chain, because a
/// substituter plus a trusted key means CI fetches store paths signed by a key held outside this
/// repository. `ci.yml` deliberately withholds the flag that would let `flake.nix` add its own, and
/// `docs/adr/0026` records that no third-party one is configured here either. So this is a LINE
/// rule rather than a step rule: one of these names anywhere in a workflow or a composite action,
/// comments excluded.
///
/// **TWO SPELLINGS, AND THE FIRST DRAFT HELD ONLY ONE.** As shipped this read `<name> =` only, so
/// `nix build --extra-substituters https://…` was GREEN on merged `main`. Nix accepts `name = value`
/// from `nix.conf`/`NIX_CONFIG`/`extra_nix_config` AND `--name value` / `--option name value` from a
/// command line; both are refused now (see [`Names`]).
///
/// Matched as a SUBSTRING, so `extra-substituters` and `extra-trusted-public-keys` fall out of two
/// entries rather than four.
///
/// Comments excluded is load-bearing rather than tidy: six files in `.github` explain in prose why
/// the flag is absent, and a rule that read those would refuse the documentation of itself.
const SUBSTITUTER: [&str; 2] = ["substituters", "trusted-public-keys"];

/// The nix flag that names a setting in its next argument, so `--option substituters …` is caught.
///
/// Its own constant because the token is not the setting: the name arrives UNDASHED and would read
/// as a bare mention without this, which is the third form the shipped rule missed.
const OPTION: &str = "option";

/// The record that retires the hosted cache, and therefore the file every refusal here points at.
const RECORD: &str = "docs/adr/0026-no-third-party-binary-cache.md";

/// Where the files these two rules read live, relative to the repository root.
///
/// Read directly rather than through [`crate::workflows::reach::Closure`], and that is the point of the pair: the closure is
/// what ordinary CI reaches, so a hosted publisher re-added to `release.yml` or to a workflow
/// nobody has wired yet would pass it. `docs/adr/0026` is a statement about the repository.
const GITHUB: [&str; 2] = [".github/workflows", ".github/actions"];

/// Every refusal in this module, over one repository root.
///
/// One entry point rather than three, so the parent's [`super::problems`] states *is the recorded
/// absence still absent* once instead of naming this module's internals.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let mut out = record(root);
    out.extend(retired(&read_github(root)));
    out
}

/// Whether the record the refusals here enforce is still there and still says something.
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
pub(super) fn retired(files: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (label, text) in files {
        for step in steps(text) {
            if let Some(publisher) = step.uses().and_then(|uses| {
                HOSTED
                    .iter()
                    .find(|name| uses.starts_with(**name))
                    .map(|name| String::from(*name))
            }) {
                // cachix-action is permitted in its one write workflow as before, OR in a ci.yml
                // pull-request job that declares environment `cachix-push-pr` - the PR write half.
                // Both are bounded by the credential's environment policy, which the gate cannot
                // verify but the forge holds. The other HOSTED entries stay refused everywhere.
                let here = publisher == PUBLISH.0 && (label.ends_with(PUBLISH.1) || in_pr_publish_job(text, step.line));
                if !here {
                    out.push(format!(
                        "{label}:{}  {publisher} publishes to a store outside this repository. Only `{}` may, and only in `{}` (or a pull_request ci.yml job declaring environment `{PUBLISH_PRS}`), where the trigger and the environment both admit one branch - see docs/adr/0027",
                        step.line, PUBLISH.0, PUBLISH.1
                    ));
                }
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
        out.extend(super::realise::problems(label, text));
    }
    out
}

/// How one line names a nix substituter setting, or [`None`] when it merely mentions one.
///
/// Two variants and not one boolean, because the message a reader acts on differs: a configuration
/// assignment changes every nix invocation in the job, a command-line option changes exactly one.
/// Both make CI trust a store this repository does not control, which is why both are refused.
#[derive(Debug, PartialEq, Eq)]
enum Names {
    /// `substituters = https://…` - the `nix.conf`, `NIX_CONFIG` and `extra_nix_config` spelling.
    Assigned,
    /// `--extra-substituters https://…`, `--substituters=…`, or `--option substituters https://…`.
    Flag,
}

/// How `code` names `setting`, reading the line as whitespace-separated tokens.
///
/// **Tokens rather than a substring scan, and the reason is the dash.** The shipped version asked
/// only whether an `=` followed the name, which no command line has; asking instead what the token
/// AROUND the name looks like answers both spellings from one walk. Leading dashes are trimmed with
/// [`str::trim_start_matches`] rather than `strip_prefix("--")` because [`key_of`] removes one `-`
/// from a line that begins with the list marker, so a backslash-continued
/// `--extra-substituters …` arrives here with a single dash.
fn names(code: &str, setting: &str) -> Option<Names> {
    let tokens: Vec<&str> = code.split_whitespace().collect();
    for (at, token) in tokens.iter().enumerate() {
        let (dashed, bare) = undashed(token);
        // `--substituters=https://…` names the setting in its head; the tail is the value.
        if !bare.split('=').next().unwrap_or_default().contains(setting) {
            continue;
        }
        if dashed {
            return Some(Names::Flag);
        }
        // `--option substituters https://…`: the name is a BARE token and the flag is its
        // predecessor, so this is the one form the token itself cannot reveal.
        let after_option = at
            .checked_sub(1)
            .and_then(|before| tokens.get(before))
            .is_some_and(|previous| {
                let (dashed, bare) = undashed(previous);
                dashed && bare == OPTION
            });
        if after_option {
            return Some(Names::Flag);
        }
        let assigns = bare
            .split_once(setting)
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('='))
            || tokens.get(at.saturating_add(1)).is_some_and(|next| next.starts_with('='));
        if assigns {
            return Some(Names::Assigned);
        }
    }
    None
}

/// One token split into *did it carry leading dashes* and *what is left*.
fn undashed(token: &str) -> (bool, &str) {
    let bare = token.trim_start_matches('-');
    (bare.len() < token.len(), bare)
}

/// The stores outside this repository that CI trusts on EVERY path, and the keys that sign them.
///
/// **Three, because `cachix-push.yml` WRITES three** - the shared closure, the cross build, the
/// connectors closure. A store this repository fills and never reads is spend with no return, so the
/// read list is the write list; `sutura-fuzzing` is absent because nothing pushes to it.
///
/// **Committed rather than held in a secret or a variable, deliberately.** A committed value is one a
/// reviewer sees in the diff and one this gate can compare against; a variable can be swapped with no
/// diff at all and nothing would notice. Nix's signature check is what bounds the trust. What bounds
/// the POISON risk is narrower and belongs beside the claim: every write credential is an
/// *environment* secret only a push to the default branch can reach, so these stores hold what this
/// repository's own CI built under scoped tokens - content-addressing alone would not give that.
const ALLOWED: [(&str, &str); 2] = [("substituters", STORES), ("trusted-public-keys", KEYS)];
const STORES: &str = "https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org";
const KEYS: &str = "sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0= sutura-cross-build.cachix.org-1:JIqwyJtgFFxOkA3JoUKOlbevOlflZ4J0Q3U04BJiq/w= sutura-connectors.cachix.org-1:1uJE7nE3cANwRW5R4Gf2i2YZldiAAC6p/jlUs0lMens=";

/// The additive PR-path pair: [`ALLOWED`]'s three plus the repository's PR cache. Permitted ONLY in a
/// step whose `if:` forces the `pull_request` event ([`narrower_than_pr`]). FIXED store names, so the
/// gate pins the exact four-store value list and a fifth store appended still refuses.
///
/// The security bound is the KEY, not this text: main never lists `sutura-prs`'s key here, so even
/// a compromised PR run that wrote attacker paths to `sutura-prs` is read only by PR-gated runs,
/// never by a merged-main resolver. A PR run still needs the main three to restore what they hold,
/// which is why the pair carries all four.
const ALLOWED_PRS: [(&str, &str); 2] = [("substituters", PR_STORES), ("trusted-public-keys", PR_KEYS)];
const PR_STORES: &str = "https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org https://sutura-prs.cachix.org";
const PR_KEYS: &str = "sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0= sutura-cross-build.cachix.org-1:JIqwyJtgFFxOkA3JoUKOlbevOlflZ4J0Q3U04BJiq/w= sutura-connectors.cachix.org-1:1uJE7nE3cANwRW5R4Gf2i2YZldiAAC6p/jlUs0lMens= sutura-prs.cachix.org-1:UTZrp8XfnC5a3XIvdtADv3/s+vQ686Ac7doOEu2Gizw=";

/// The pull-request gate, token for token - the ONLY place [`ALLOWED_PRS`] is permitted.
const PR_EVENT: &str = "github.event_name == 'pull_request'";

/// Is `gate` PR-only, mirroring [`super::narrower_than_main_push`] for the `pull_request` event?
///
/// Exact `github.event_name == 'pull_request'`, or plus `&&` conjuncts that cannot widen back to
/// main: no `||`, and none of `github.event_name == 'push'`, `github.ref == 'refs/heads/main'`, or a
/// `vars.`/`secrets.` read (unobservable, separately refused).
fn narrower_than_pr(gate: &str) -> bool {
    let Some(rest) = gate.strip_prefix(PR_EVENT) else {
        return false;
    };
    rest.strip_prefix(" && ").map_or(rest.is_empty(), |extra| {
        !extra.is_empty()
            && !extra.contains("||")
            && !extra.contains("github.event_name == 'push'")
            && !extra.contains("github.ref == 'refs/heads/main'")
            && !extra.contains("vars.")
            && !extra.contains("secrets.")
    })
}

/// Whether one line assigns `setting` to exactly a permitted value and to nothing else.
///
/// Only the additive `extra-substituters` form, never plain `substituters` (the DESTRUCTIVE
/// spelling that drops nix's defaults) and never a flag; against the WHOLE value list, so
/// appending one more store still refuses. [`ALLOWED`] (the three written on every path) is permitted
/// everywhere; [`ALLOWED_PRS`] (those three plus the PR cache) only when the value line sits inside a
/// `cachix/install-nix-action` step's `extra_nix_config` whose `if:` is PR-only
/// ([`narrower_than_pr`]) - removing that `if:` reopens main = red. The value AND its placement are
/// the gate.
fn permitted(code: &str, setting: &str, is_pr_gated_config_line: bool) -> bool {
    let Some((name, value)) = code.split_once('=') else {
        return false;
    };
    // ONLY the `extra-` form, and this is the half that was learned the expensive way: plain
    // `substituters = …` REPLACES nix's default list rather than adding to it, so it drops
    // `cache.nixos.org` and the next build fetches nothing - it compiles the bootstrap chain
    // (`stage0-posix`, `mescc-tools`, `bash`) from source. Permitting the additive spelling and
    // refusing the destructive one means the gate now refuses that outage even for the allowed store.
    if name.trim() != format!("extra-{setting}") {
        return false;
    }
    // Token-wise against the WHOLE list, so a store dropped, added or reordered all refuse. ONE
    // closure for both tables: the copy that used to serve the main path compared the value against
    // a single token, so it silently refused any list longer than one store.
    let listed = |table: &[(&str, &str); 2]| {
        table
            .iter()
            .find(|(named, _)| *named == setting)
            .is_some_and(|(_, allowed)| value.split_whitespace().eq(allowed.split_whitespace()))
    };
    listed(&ALLOWED) || (is_pr_gated_config_line && listed(&ALLOWED_PRS))
}

/// Line indices (0-based) inside a PR-gated install-nix-action step's `extra_nix_config` block -
/// the only container where the four-store [`ALLOWED_PRS`] list is law (installer step AND PR-only
/// `if:`; a bare/moved/ungated line is not, so the list there is refused).
fn pr_gated_config_lines(text: &str) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for step in steps(text) {
        let is_installer = step.uses().as_deref() == Some("cachix/install-nix-action");
        let pr_only = step.gate().is_some_and(|g| narrower_than_pr(&g));
        if !is_installer || !pr_only {
            continue;
        }
        let base = step.line.saturating_sub(1); // 0-based start of the step
        let mut opened: Option<usize> = None; // the `extra_nix_config:` opener's indentation
        for (j, line) in step.lines.iter().enumerate() {
            let indent = line.len().saturating_sub(line.trim_start().len());
            if opened.is_none() {
                if key_of(line).is_some_and(|k| k.trim_start().strip_prefix("extra_nix_config:").is_some()) {
                    opened = Some(indent);
                }
                continue;
            }
            // A line no deeper than the opener ends the block.
            if !line.trim().is_empty() && indent <= opened.unwrap_or(0) {
                opened = None;
                continue;
            }
            out.insert(base + j);
        }
    }
    out
}

/// Every line of one file that would make CI trust a store outside this repository.
///
/// A LINE rule and not a step rule, and the residual limit belongs where a reader meets it: this
/// sees a setting NAMED on one line, in either of the two spellings nix accepts. **A value
/// assembled from pieces** - a variable holding the URL, a `printf` building the config, a name
/// split across a continuation - **and a substituter reaching the runner through some other
/// action's input still pass unseen**, and no text rule can close that. `zizmor` and review are
/// what cover it.
fn trusted_stores(label: &str, text: &str) -> Vec<String> {
    // The PR-gated install line indices, computed once so substituter lines there are admitted to
    // the two-store comparison; everywhere else is refused.
    let pr_config = pr_gated_config_lines(text);
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(code) = key_of(line) else { continue };
        for setting in SUBSTITUTER {
            let Some(named) = names(code, setting) else { continue };
            if named == Names::Assigned && permitted(code, setting, pr_config.contains(&index)) {
                continue;
            }
            let how = match named {
                Names::Assigned => format!("assigns `{setting}`, so every nix invocation in the job trusts it"),
                Names::Flag => format!("passes `{setting}` as a command-line option, so that nix invocation trusts it"),
            };
            out.push(format!(
                "{label}:{}  {how} - a store outside this repository, and {RECORD} records which stores the main resolver trusts - the ones this repository itself publishes, an additive PR-only cache only behind a pull_request gate - so update that record rather than this gate",
                index.saturating_add(1)
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    // The parent's pinned main-push condition and its step-fixture builder: the shapes these
    // refusals are provoked WITH belong to the write half, so they are borrowed rather than
    // duplicated - a second copy of `MAIN_PUSH` is a second thing to keep true.
    use super::super::tests::step_using;
    use super::super::{MAIN_PUSH, STORE_CACHE};

    /// One labelled file, for the absence rules - they read `(String, String)` off a directory walk
    /// rather than the closure's borrowed pairs.
    fn owned(label: &str, text: &str) -> Vec<(String, String)> {
        vec![(String::from(label), String::from(text))]
    }

    /// The publisher permission is a PAIRING, and the file half is the half that matters: the same
    /// step on the pull-request path would have a same-repository pull request's secrets in reach.
    #[test]
    fn the_publisher_is_permitted_in_one_file_and_refused_in_every_other() {
        // A permitted publisher still realises after cachix-action (#560), as the live tree does.
        let step = concat!(
            "      - uses: cachix/cachix-action@38b082610b782e7e93e209c35fd730d399dee866 # v17\n",
            "      - name: Realise and publish the shared closure\n",
            "        run: nix build .#checks.x86_64-linux.hygiene\n",
        );
        assert!(
            super::retired(&owned(".github/workflows/cachix-push.yml", step)).is_empty(),
            "the publish workflow is the one file it may appear in"
        );
        for elsewhere in [
            ".github/workflows/ci.yml",
            ".github/workflows/cross-link.yml",
            ".github/workflows/release.yml",
        ] {
            let problems = super::retired(&owned(elsewhere, step));
            assert!(!problems.is_empty(), "still refused in {elsewhere}");
        }
        // The other publisher has no permitted file, so even the publish workflow refuses it.
        let other = "      - uses: DeterminateSystems/flakehub-cache-action@v1\n";
        assert!(
            !super::retired(&owned(".github/workflows/cachix-push.yml", other)).is_empty(),
            "a publisher with no permitted file is refused wherever it appears"
        );
    }

    /// Half (a) widens the write pairing: cachix-action may ALSO run in a ci.yml PULL-REQUEST job
    /// declaring `environment: cachix-push-pr`, and stays red in the ungated main `ci` job.
    #[test]
    fn the_publisher_is_permitted_in_the_pr_write_job_and_refused_in_the_main_job() {
        // The PR write job: cachix-action under `environment: cachix-push-pr`, gated to pull_request.
        let pr_job = concat!(
            "  ci:\n",
            "    steps:\n",
            "      - uses: cachix/install-nix-action@13d8dd58 # v31.11.1\n",
            "        if: github.event_name == 'pull_request'\n",
            "  pr-cache:\n",
            "    needs: [ci]\n",
            "    if: github.event_name == 'pull_request'\n",
            "    environment: cachix-push-pr\n",
            "    steps:\n",
            "      - uses: cachix/cachix-action@38b082610b782e7e93e209c35fd730d399dee866 # v17\n",
            "        with:\n",
            "          name: sutura-prs\n",
            "          authToken: ${{ secrets.CACHIX_AUTH_TOKEN }}\n",
            "      - name: Realise this PR's shared closure\n", // #560: a permitted writer still realises
            "        run: nix build .#checks.x86_64-linux.nextest\n",
        );
        assert!(
            super::retired(&owned(".github/workflows/ci.yml", pr_job)).is_empty(),
            "the PR write job is the one ci.yml context cachix-action may run in"
        );

        // The SAME cachix-action step in the ungated main `ci` job stays refused.
        let main_job = concat!(
            "  ci:\n",
            "    steps:\n",
            "      - uses: cachix/cachix-action@38b082610b782e7e93e209c35fd730d399dee866 # v17\n",
            "        with:\n",
            "          name: sutura-prs\n",
            "          authToken: ${{ secrets.CACHIX_AUTH_TOKEN }}\n",
        );
        let found = super::retired(&owned(".github/workflows/ci.yml", main_job));
        assert!(!found.is_empty(), "{found:#?}");

        // And the environment name alone is not enough - no PR gate is refused.
        let ungated_env = pr_job.replace(
            "    if: github.event_name == 'pull_request'\n    environment: cachix-push-pr\n",
            "    environment: cachix-push-pr\n",
        );
        assert!(
            !super::retired(&owned(".github/workflows/ci.yml", &ungated_env)).is_empty(),
            "cachix-push-pr with no PR gate is not the PR write half and is refused"
        );
    }

    /// The allowance is the whole risk of enabling a binary cache: it has to admit exactly the stores
    /// this repository publishes and refuse every neighbouring shape, or it reads as coverage while
    /// trusting anything.
    #[test]
    fn exactly_the_published_stores_are_permitted_and_every_neighbour_is_still_refused() {
        let allowed = "      extra_nix_config: |\n        extra-substituters = https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org\n        extra-trusted-public-keys = sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0= sutura-cross-build.cachix.org-1:JIqwyJtgFFxOkA3JoUKOlbevOlflZ4J0Q3U04BJiq/w= sutura-connectors.cachix.org-1:1uJE7nE3cANwRW5R4Gf2i2YZldiAAC6p/jlUs0lMens=\n";
        assert!(
            super::trusted_stores("ci.yml", allowed).is_empty(),
            "the committed stores and their keys are the one permitted pair"
        );

        for (why, text) in [
            (
                "a FOURTH store appended to the permitted line",
                "        extra-substituters = https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org https://example.invalid\n",
            ),
            ("a different store", "        extra-substituters = https://example.invalid\n"),
            (
                "the DESTRUCTIVE spelling, which replaces nix's default list and cost a bootstrap build",
                "        substituters = https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org\n",
            ),
            (
                "the permitted key on another host",
                "        extra-trusted-public-keys = example.invalid-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0=\n",
            ),
            (
                "a different key for the permitted store",
                "        extra-trusted-public-keys = sutura.cachix.org-1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n",
            ),
            (
                "the permitted store passed as a FLAG rather than assigned",
                "        run: nix build --extra-substituters https://sutura.cachix.org .#xtask\n",
            ),
            (
                "the permitted store via --option",
                "        run: nix build --option substituters https://sutura.cachix.org .#xtask\n",
            ),
            (
                "ONE permitted store alone - the whole value list has to match, not a subset",
                "        extra-substituters = https://sutura-cross-build.cachix.org\n",
            ),
            (
                "the PR store list UNGATED on the main path (-a2) - a merged main run must not",
                "        extra-substituters = https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org https://sutura-prs.cachix.org\n",
            ),
        ] {
            assert!(!super::trusted_stores("ci.yml", text).is_empty(), "still refused: {why}");
        }
    }

    /// The trust rule is PATH-aware: the PR store list is law ONLY inside an install step whose
    /// `if:` is PR-only (green there, red on one more store).
    #[test]
    fn the_pr_path_allows_its_own_store_list_and_refuses_one_more() {
        // (-b): the exact PR store list inside a PR-gated install step is GREEN.
        let pr_step = concat!(
            "      - uses: cachix/install-nix-action@13d8dd58 # v31.11.1\n",
            "        if: github.event_name == 'pull_request'\n",
            "        with:\n",
            "          extra_nix_config: |\n",
            "            extra-substituters = https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org https://sutura-prs.cachix.org\n",
            "            extra-trusted-public-keys = sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0= sutura-cross-build.cachix.org-1:JIqwyJtgFFxOkA3JoUKOlbevOlflZ4J0Q3U04BJiq/w= sutura-connectors.cachix.org-1:1uJE7nE3cANwRW5R4Gf2i2YZldiAAC6p/jlUs0lMens= sutura-prs.cachix.org-1:UTZrp8XfnC5a3XIvdtADv3/s+vQ686Ac7doOEu2Gizw=\n",
        );
        assert!(
            super::trusted_stores("ci.yml", pr_step).is_empty(),
            "the PR path accepts its own additive cache behind a pull_request gate"
        );
        // (-b2): one MORE store appended to the PR list is still refused.
        let more = pr_step.replace(super::PR_STORES, &format!("{} https://evil.cachix.org", super::PR_STORES));
        assert!(
            !super::trusted_stores("ci.yml", &more).is_empty(),
            "a further store appended to the PR list is still refused"
        );
    }

    /// THE LOAD-BEARING ROW: dropping (or weakening) the PR gate reopens main = red - the identical
    /// PR-list lines with the `if:` gone are a main-run installer and must be refused.
    #[test]
    fn removing_the_pr_gate_reopens_main_and_is_refused() {
        let ungated = concat!(
            "      - uses: cachix/install-nix-action@13d8dd58 # v31.11.1\n",
            "        with:\n",
            "          extra_nix_config: |\n",
            "            extra-substituters = https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org https://sutura-prs.cachix.org\n",
            "            extra-trusted-public-keys = sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0= sutura-cross-build.cachix.org-1:JIqwyJtgFFxOkA3JoUKOlbevOlflZ4J0Q3U04BJiq/w= sutura-connectors.cachix.org-1:1uJE7nE3cANwRW5R4Gf2i2YZldiAAC6p/jlUs0lMens= sutura-prs.cachix.org-1:UTZrp8XfnC5a3XIvdtADv3/s+vQ686Ac7doOEu2Gizw=\n",
        );
        assert!(
            !super::trusted_stores("ci.yml", ungated).is_empty(),
            "the PR store list UNGATED is a merged-main installer and must be refused"
        );

        // Weakening the gate to not-PR-only is the same re-opening and the same red.
        for weakened in [
            "github.event_name == 'push'",
            "github.event_name == 'pull_request' || github.event_name == 'push'",
            "github.ref == 'refs/heads/main'",
        ] {
            let fuzzed = ungated.replace(
                "        with:\n          extra_nix_config:",
                &format!("        if: {weakened}\n        with:\n          extra_nix_config:"),
            );
            assert!(
                !super::trusted_stores("ci.yml", &fuzzed).is_empty(),
                "a fuzzed PR gate ({weakened}) reopens main and is refused"
            );
        }
    }

    #[test]
    fn the_deleted_hosted_publisher_is_refused_by_name_and_points_at_the_record() {
        // THE DELETION, PROVOKED. `docs/adr/0026` removed exactly this step from `ci.yml` and
        // `cross-link.yml`; putting it back is what this must make red. Written verbatim as it
        // stood, pin and all, so the fixture is the thing that was deleted rather than a paraphrase.
        //
        // Still red after `docs/adr/0027` provisioned a cache, and that is the point of the pairing:
        // the publisher is permitted in ONE file, and `ci.yml` is not it. The record this message
        // names is therefore 0027 - which supersedes 0026 - because 0027 is where the permission and
        // its one file are decided. The other rules in this module still name 0026, which is what
        // deleted the wiring they refuse.
        let back = concat!(
            "      - name: Populate the binary cache\n",
            "        uses: cachix/cachix-action@38b082610b782e7e93e209c35fd730d399dee866 # v17\n",
            "        with:\n",
            "          name: sutura\n",
        );
        let found = super::retired(&owned(".github/workflows/ci.yml", back));
        // Also refused by #560's realise rule there, so pin the HOSTED refusal + its record pointer.
        assert!(found.iter().any(|p| p.contains("docs/adr/0027")), "{found:#?}");
        let problem = found
            .iter()
            .find(|p| p.starts_with(".github/workflows/ci.yml:1  cachix/cachix-action"))
            .map_or("", String::as_str);
        assert!(
            problem.contains("docs/adr/0027"),
            "the deletion refusal names the record it points at: {problem}"
        );

        // The other publisher, and a fork of it: matched on the prefix, so a subpath cannot evade.
        for name in ["DeterminateSystems/flakehub-cache-action", "cachix/cachix-action/sub"] {
            let step = format!("      - uses: {name}@aaaa # v1\n");
            let found = super::retired(&owned("release.yml", &step));
            assert!(
                found
                    .iter()
                    .any(|p| p.contains("publishes to a store outside this repository")),
                "{name}: {found:#?}"
            );
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
        let save = step_using(STORE_CACHE, "save:", "${{ vars.WRITE_THE_CACHE == 'yes' }}");
        assert_eq!(super::retired(&owned("ci.yml", &save)).len(), 1);

        // AND THE OTHER DIRECTION, so this cannot pass by refusing everything: the live shape,
        // whose gate names only contexts a workflow can evaluate for itself.
        let live = step_using(STORE_CACHE, "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        assert!(
            super::retired(&owned("ci.yml", &live)).is_empty(),
            "a live step that gates on its own context is not retired"
        );
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
    fn every_spelling_that_names_a_substituter_is_refused() {
        // ONE FIXTURE PER FORM, and the reason there is a form list at all: as shipped this rule
        // read `<name> =` and nothing else, so a nix COMMAND LINE was green on merged `main` while
        // the module doc claimed no store outside this repository is trusted. Each row below was
        // GREEN before this change; the two `Assigned` rows are the ones that already worked.
        let assigned = [
            // The `extra_nix_config` / `nix.conf` / `NIX_CONFIG` spelling.
            "            substituters = https://example.invalid",
            "            extra-substituters = https://example.invalid",
            "            trusted-public-keys = example.invalid-1:AAAA",
            "            extra-trusted-public-keys = example.invalid-1:AAAA",
            // No space around the `=`, and the interpolated form the retired wiring used.
            "            substituters=https://example.invalid",
            "            ${{ secrets.S != '' && format('substituters = {0}', secrets.S) || '' }}",
            // Appended to nix.conf from a shell body, which is not a workflow key at all.
            "          echo 'substituters = https://example.invalid' | sudo tee -a /etc/nix/nix.conf",
        ];
        let flagged = [
            // THE FORMS THAT ESCAPED. Every one of these reads green under a `<name> =` scan.
            "          nix build --substituters https://example.invalid .#xtask",
            "          nix build --extra-substituters https://example.invalid .#xtask",
            "          nix build --trusted-public-keys 'example.invalid-1:AAAA' .#xtask",
            "          nix build --extra-trusted-public-keys 'example.invalid-1:AAAA' .#xtask",
            "          nix build --substituters=https://example.invalid .#xtask",
            // `--option <name> <value>`: the name arrives UNDASHED, so only its predecessor says
            // what it is - the third form, and the one a dash test alone still misses.
            "          nix build --option substituters https://example.invalid .#xtask",
            "          nix build --option extra-trusted-public-keys 'example.invalid-1:AAAA' .#xtask",
            // A backslash continuation, where `key_of` has already eaten one dash off the front.
            "            --extra-substituters https://example.invalid \\",
        ];
        for line in assigned {
            let found = super::trusted_stores("ci.yml", line);
            assert!(!found.is_empty(), "not refused: {line}");
            assert!(
                found
                    .iter()
                    .all(|problem| problem.contains("assigns `") && problem.contains(super::RECORD)),
                "{found:#?}"
            );
        }
        for line in flagged {
            let found = super::trusted_stores("ci.yml", line);
            assert!(!found.is_empty(), "not refused: {line}");
            assert!(
                found
                    .iter()
                    .all(|problem| problem.contains("as a command-line option") && problem.contains(super::RECORD)),
                "{line}: {found:#?}"
            );
        }
    }

    #[test]
    fn a_line_that_only_mentions_a_substituter_is_not_one() {
        // THE OTHER DIRECTION, and it is not decoration: six files under `.github` explain in prose
        // why the flag is absent, `install-nix-action` carries a live `fallback = true`, and this
        // module's own header names all four settings. A rule that fired on those would refuse the
        // documentation of itself, and widening it to command lines is exactly the change that
        // could have done so.
        for line in [
            "          # with it, a pull request could add a substituter and a trusted key",
            "          # substituters = https://example.invalid was weighed and refused",
            "          # nix build --extra-substituters https://example.invalid was refused",
            "            fallback = true",
            "          printf 'no substituters are configured here\\n'",
            "        SUBSTITUTER_NOTE: substituters are documented in docs/adr/0026",
            // `option` WITHOUT dashes is an English word, and this row is what holds the dash
            // half of the `--option` predecessor test: dropping it left every mutation green and
            // turned prose about the option into a refusal.
            "          # nix takes the option substituters from its configuration or a command line",
            "          printf 'the option substituters is recorded in docs/adr/0026\\n'",
            // The retired secret's NAME is not the setting: singular, and upper case.
            "          NIX_CACHE_SUBSTITUTER: ${{ secrets.NIX_CACHE_SUBSTITUTER }}",
        ] {
            let found = super::trusted_stores("ci.yml", line);
            assert!(found.is_empty(), "over-fired on: {line}\n{found:#?}");
        }
    }

    #[test]
    fn each_spelling_is_classified_as_the_form_a_reader_has_to_act_on() {
        // The two variants carry different remedies - a config assignment changes every nix
        // invocation in the job, an option changes one - so the classification is asserted rather
        // than only the refusal count. This is also the unit that a `Flag`-for-`Assigned` swap
        // reddens without touching a workflow.
        use super::Names;
        assert_eq!(
            super::names("substituters = https://x", "substituters"),
            Some(Names::Assigned)
        );
        assert_eq!(
            super::names("nix build --option substituters https://x", "substituters"),
            Some(Names::Flag)
        );
        assert_eq!(
            super::names("nix build --extra-substituters https://x", "substituters"),
            Some(Names::Flag)
        );
        // `--option` with something else after it says nothing about substituters.
        assert_eq!(super::names("nix build --option cores 4", "substituters"), None);
        // A bare mention with neither a dash nor an `=` is prose.
        assert_eq!(super::names("substituters are refused here", "substituters"), None);
    }

    #[test]
    fn the_committed_tree_names_no_substituter_in_either_spelling() {
        // THE LIVE ASSERTION for the widened rule. It is what would have caught
        // `--extra-substituters` on merged `main`, and it is the row that goes red if a shell body
        // in `.github` ever hands nix a store this repository does not control.
        let Some(root) = crate::repo::root() else { return };
        let found: Vec<String> = super::read_github(&root)
            .iter()
            .flat_map(|(label, text)| super::trusted_stores(label, text))
            .collect();
        assert!(found.is_empty(), "{found:#?}");
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
        assert!(super::record(&root).is_empty(), "a written record leaves nothing to report");
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
}
