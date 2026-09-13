//! Which stores CI trusts, and whether a job that installs nix trusts them AT ALL.
//!
//! Two halves of one question, and they fail in opposite directions. [`trusted_stores`] refuses a
//! substituter this repository does not publish - trusting too much. [`blind`] refuses an installer
//! step that names none of the ones it does - trusting too little, which costs money rather than
//! safety and is why it went unnoticed for so long.
//!
//! **THE COVERAGE HALF IS THE ONE THAT WAS MISSING, and the measurement is why it exists.** The
//! allow-list rule only ever fired on a store OUTSIDE the list, so a job declaring NOTHING was
//! green: `docs.yml`, `release.yml`, `release-performance.yml` and `fuzz.yml` all installed nix and
//! read `cache.nixos.org` alone. Measured on `release.yml`'s `gates` job: 992 store paths built,
//! 722 copied, 339 of them from `cache.nixos.org` and zero from a store this repository publishes.
//! Declaring the pair on `docs.yml` took its structural-gates step from 225 s to 63 s.
//!
//! **PER STEP, NOT PER JOB OR PER FILE, and that is the shape a regression comes back in.** With
//! the rule written per job, deleting the pair from ONE of `docs.yml`'s two installer steps left
//! the gate green while half the workflow went back to upstream - the wiring that still produces
//! the headline saving and still pays the full cost on the other half. So the unit is the step.
//!
//! # What this does NOT hold
//!
//! * A step is *covered* when it names the permitted pair. Whether nix then REACHES those stores is
//!   a network fact no text rule sees, and a wrong public key fails by not fetching rather than by
//!   erroring - so a green gate here means declared, never served.
//! * [`BLIND`] is a name list. A job added to it is argued in the row beside it; a job that simply
//!   does not appear is covered by the refusal, which is the direction that fails safe.
//! * [`trusted_stores`]'s own limit is on that function: a value assembled from pieces passes
//!   unseen.

use std::collections::BTreeSet;

use super::super::{Step, key_of, steps};
use super::RECORD;

/// The action that puts nix on a runner, and therefore the step that decides what nix will trust.
const INSTALLER: &str = "cachix/install-nix-action";

/// The nix settings that make CI TRUST a store, refused in `.github` outright.
///
/// The publisher half of the retired wiring was a step, which [`super::HOSTED`] can see. The trust half
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
/// diff at all and nothing would notice. Nix's signature check is what bounds the trust. The main-store
/// write credential is an *environment* secret only a push to the default branch can reach. PR and
/// mixed credentials must be treated as able to write any trusted store until their scope is
/// independently verified, so this gate cannot claim that every store contains only this repository's
/// own CI output.
const ALLOWED: [(&str, &str); 2] = [("substituters", STORES), ("trusted-public-keys", KEYS)];
pub(super) const STORES: &str =
    "https://sutura.cachix.org https://sutura-cross-build.cachix.org https://sutura-connectors.cachix.org";
pub(super) const KEYS: &str = "sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0= sutura-cross-build.cachix.org-1:JIqwyJtgFFxOkA3JoUKOlbevOlflZ4J0Q3U04BJiq/w= sutura-connectors.cachix.org-1:1uJE7nE3cANwRW5R4Gf2i2YZldiAAC6p/jlUs0lMens=";

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

/// Is `gate` PR-only, mirroring [`super::super::narrower_than_main_push`] for the `pull_request` event?
///
/// Exact `github.event_name == 'pull_request'`, or plus `&&` conjuncts that cannot widen back to
/// main: no `||`, and none of `github.event_name == 'push'`, `github.ref == 'refs/heads/main'`, or a
/// `vars.`/`secrets.` read (unobservable, separately refused).
pub(super) fn narrower_than_pr(gate: &str) -> bool {
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
pub(super) fn trusted_stores(label: &str, text: &str) -> Vec<String> {
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

/// Installer steps that may name no store, with the argument for each one beside it.
///
/// **An exemption list without its reason rots**, so the reason is a field rather than a doc
/// paragraph and `the_exemptions_all_still_name_a_live_installer_step` refuses a row whose job is
/// gone. Keyed on (file, job): the same workflow's OTHER jobs stay covered, which is what keeps a
/// blanket per-file escape out of reach.
type Blind = (&'static str, &'static str, &'static str);
const BLIND: [Blind; 6] = [
    (
        ".github/workflows/runner-probe.yml",
        "reference",
        "it exists to measure a COLD host - a substituter is the variable it is holding still",
    ),
    (
        ".github/workflows/runner-probe.yml",
        "one-job",
        "same probe, one job: a fetched path would be a build this measurement did not do",
    ),
    (
        ".github/workflows/runner-probe.yml",
        "bounded",
        "same probe under a bounded allocation; substituting would make the bound unmeasurable",
    ),
    (
        ".github/workflows/security-audit.yml",
        "audit",
        "`nix run .#deny` and nothing else - the closure is nixpkgs plus an advisory database, so no path this repository publishes is in it",
    ),
    (
        ".github/workflows/version-bump.yml",
        "bump",
        "`nix run .#git-cliff` and `.#cargo`, both nixpkgs; it builds no first-party derivation",
    ),
    (
        ".github/workflows/format.yml",
        "format",
        "`nix run .#dprint` and `.#ruff`, both nixpkgs and one trivial wrapper over them; no path this repository publishes is in the closure",
    ),
];

/// Both refusals over one file: a store outside the list, and an installer that names none in it.
pub(super) fn problems(label: &str, text: &str) -> Vec<String> {
    let mut out = trusted_stores(label, text);
    out.extend(blind(label, text));
    out
}

/// Every [`INSTALLER`] step that declares neither half of the permitted pair.
///
/// BOTH settings, because either alone is inert: a substituter with no trusted key serves nothing
/// nix will accept, and a key with no substituter names a host nix never asks. So the step is
/// covered only when it carries both, and a step carrying one is reported as the half it is missing
/// rather than as a pass.
///
/// **It asks whether the setting is NAMED, not whether the value is permitted, and that division is
/// deliberate.** [`trusted_stores`] already refuses a wrong value on that same line, so judging it
/// twice would report one defect as two and make the remedy ambiguous. The two rules compose to
/// *names both settings, with a permitted value* - neither half alone.
fn blind(label: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for step in steps(text) {
        if !step.uses().is_some_and(|uses| uses.starts_with(INSTALLER)) {
            continue;
        }
        let missing: Vec<&str> = ALLOWED
            .iter()
            .map(|(setting, _)| *setting)
            .filter(|setting| !declares(&step, setting))
            .collect();
        if missing.is_empty() || exempt(label, text, step.line) {
            continue;
        }
        out.push(format!(
            "{label}:{}  this {INSTALLER} step names no `extra-{}` - so nix resolves from upstream alone and the stores `cachix-push.yml` fills are never read. Copy the committed pair from `.github/workflows/ci.yml`, or add the job to `BLIND` with the reason it must build cold",
            step.line,
            missing.join("` and no `extra-")
        ));
    }
    out
}

/// Whether `step` assigns `setting` on a line of its own.
fn declares(step: &Step<'_>, setting: &str) -> bool {
    step.lines
        .iter()
        .any(|line| key_of(line).is_some_and(|code| names(code, setting) == Some(Names::Assigned)))
}

/// Whether the job owning `step_line` is named in [`BLIND`] for this file.
fn exempt(label: &str, text: &str, step_line: usize) -> bool {
    let job = super::job_of(text, step_line);
    BLIND
        .iter()
        .any(|(file, name, _)| label.ends_with(file) && job.as_deref() == Some(*name))
}

#[cfg(test)]
mod tests {
    // The absence half's entry point and its labelled-file helper: the store rules are provoked
    // through the same door the parent uses, so a fixture that passes here passes there.
    use super::super::retired;
    use super::super::tests::owned;

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
        let found = retired(&owned(".github/workflows/ci.yml", wired));
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
        let plain = retired(&owned("ci.yml", bare));
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
        assert!(retired(&owned("ci.yml", prose)).is_empty(), "a comment is not an assignment");
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

    /// One installer step with the pair and one without, in the same workflow. **This is the shape
    /// the missing rule let through**: an auditor deleted the two lines from ONE of `docs.yml`'s two
    /// installer steps and `check-workflows` exited 0, which is the wiring that still produces the
    /// headline saving while half the run goes back to upstream. So the assertion is that exactly
    /// the blind step is named, not that the file is refused.
    #[test]
    fn a_half_wired_workflow_is_refused_on_the_step_that_is_blind() {
        let wired = super::super::tests::wired_installer("");
        let blind = concat!(
            "      - uses: cachix/install-nix-action@13d8dd58 # v31.11.1\n",
            "        with:\n",
            "          extra_nix_config: |\n",
            "            fallback = true\n",
        );
        let half = format!("jobs:\n  verify:\n    steps:\n{wired}\n  publish:\n    steps:\n{blind}");
        let found = super::blind(".github/workflows/docs.yml", &half);
        assert_eq!(found.len(), 1, "exactly the blind step is refused: {found:#?}");
        let at = wired.lines().count() + 7;
        assert!(
            found[0].starts_with(&format!(".github/workflows/docs.yml:{at}")),
            "the refusal names the blind step's own line, not the file: {found:#?}"
        );

        // Both wired is green, so the refusal is not firing on the installer's mere presence.
        let both = format!("jobs:\n  verify:\n    steps:\n{wired}\n  publish:\n    steps:\n{wired}");
        assert_eq!(super::blind(".github/workflows/docs.yml", &both), Vec::<String>::new());

        // ONE HALF IS NOT COVERAGE: a substituter with no trusted key serves nothing nix accepts.
        let keyless = wired
            .lines()
            .filter(|l| !l.contains("trusted-public-keys"))
            .collect::<Vec<_>>()
            .join("\n");
        let found = super::blind(
            ".github/workflows/docs.yml",
            &format!("jobs:\n  verify:\n    steps:\n{keyless}\n"),
        );
        assert!(
            found.first().is_some_and(
                |problem| problem.contains("no `extra-trusted-public-keys`") && !problem.contains("no `extra-substituters`")
            ),
            "the refusal names the half that is missing: {found:#?}"
        );
    }

    /// The exemption is keyed on (file, job), so the same workflow's other jobs stay covered - a
    /// per-file escape would have let the blind shape back in under a name already on the list.
    #[test]
    fn an_exempt_job_may_be_blind_and_its_neighbour_may_not() {
        let blind = concat!(
            "      - uses: cachix/install-nix-action@13d8dd58 # v31.11.1\n",
            "        with:\n",
            "          extra_nix_config: |\n",
            "            fallback = true\n",
        );
        let (file, job, _) = super::BLIND[0];
        let exempt = format!("jobs:\n  {job}:\n    steps:\n{blind}");
        assert!(
            super::blind(file, &exempt).is_empty(),
            "an installer inside an exempted job names no store by design"
        );
        let neighbour = format!("jobs:\n  something-else:\n    steps:\n{blind}");
        assert!(
            !super::blind(file, &neighbour).is_empty(),
            "the exemption is the JOB, not the file"
        );
        let elsewhere = format!("jobs:\n  {job}:\n    steps:\n{blind}");
        assert!(
            !super::blind(".github/workflows/ci.yml", &elsewhere).is_empty(),
            "a job of the same name in another file is not exempt"
        );
    }

    /// An exemption whose job is gone is an argument for nothing, and it reads as coverage.
    #[test]
    fn the_exemptions_all_still_name_a_live_installer_step() {
        let Some(root) = crate::repo::root() else { return };
        let tree = super::super::read_github(&root);
        for (file, job, reason) in super::BLIND {
            assert!(!reason.trim().is_empty(), "{file}:{job} carries no reason");
            let live = tree.iter().any(|(label, text)| {
                label.ends_with(file)
                    && super::steps(text).iter().any(|step| {
                        step.uses().is_some_and(|uses| uses.starts_with(super::INSTALLER))
                            && super::super::job_of(text, step.line).as_deref() == Some(job)
                    })
            });
            assert!(
                live,
                "{file}:{job} is exempted from the store rule and no such installer step exists"
            );
        }
    }
}
