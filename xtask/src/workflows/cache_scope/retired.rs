//! Is the RECORDED ABSENCE of a third-party binary cache still the truth?
//!
//! `docs/adr/0026` retired a binary cache that was wired into `ci.yml` and `cross-link.yml` and
//! gated on `secrets.NIX_CACHE_SUBSTITUTER`, `secrets.NIX_CACHE_PUBLIC_KEY`, `vars.NIX_CACHE_NAME`
//! and `secrets.NIX_CACHE_AUTH_TOKEN` - **none of which has ever existed in this repository.** So
//! its populate step was `completed/skipped` on every `main` push it ran on (run 34247863640,
//! step 6), silently and green, and a deletion that nothing witnesses is one editor away from
//! coming back the same way.
//!
//! **Its own file for `crate::workflows::sast`'s reason, and the seam is the question.** The parent
//! asks *may a `pull_request` path WRITE the Actions cache*; this asks *is a recorded absence still
//! absent*, which is the shape that module holds one file over for Scorecard's SAST zero. The split
//! was forced by the unexemptable 1000-line cap when the flag-form refusal below landed, and it is
//! the right seam regardless: nothing here counts, gates or anchors a cache write.
//!
//! # The refusals, and why each is the shape it is
//!
//! * **No hosted publisher while the record stands.** [`HOSTED`] over the whole `.github` tree, not
//!   just the pull-request closure: re-adding it to `release.yml` is the same return. The direction
//!   is deliberate - provisioning a binary cache later is a GOOD change, and it makes
//!   `docs/adr/0026` wrong the moment it lands, so the refusal names the record to edit rather than
//!   forbidding the tool.
//! * **No step decided by state the workflow cannot see.** [`UNOBSERVABLE`] refuses a gate reading
//!   `secrets.` or `vars.`. `secrets` is not available to an `if:` at all - `cross-link.yml` said
//!   so in prose while depending on it - and a `vars.X != ''` test in one is exactly the shape that
//!   skipped invisibly here. It is a rule about SHAPE and not a name list, so a *different*
//!   secret-gated step is caught without anybody remembering to list it.
//! * **No line NAMES a store outside this repository**, in either spelling nix accepts.
//!   [`SUBSTITUTER`] is the one the other two cannot reach: the retired wiring's trust half was two
//!   `${{ }}` interpolations inside `install-nix-action`'s `extra_nix_config` VALUE, neither a gate
//!   nor a step. That half is the supply-chain surface - a substituter plus a trusted key means CI
//!   fetches paths signed by a key held elsewhere - so it is refused as a LINE.
//! * **And the record itself.** Missing or empty is refused, for `sast`'s reason: a refusal
//!   enforcing a decision nobody wrote down is a rule with no reason.
//!
//! # What this does NOT hold, and one sentence of it used to claim otherwise
//!
//! The [`SUBSTITUTER`] heading read *no store outside this repository trusted* when it shipped,
//! while the rule read `<name> =` and nothing else - so `nix build --extra-substituters https://…`
//! was GREEN on merged `main`. **An overstated refusal on a supply-chain path is worse than no
//! refusal, because a reader stops looking.** Both spellings are refused now, and the route that
//! genuinely survives is named on [`trusted_stores`]: a value assembled from pieces, or one
//! reaching the runner through some other action's input, passes unseen, and no text rule closes
//! that.
//!
//! [`HOSTED`] carries [`super::WRITERS`]'s limit - it is a name list, so a publisher nobody has
//! named passes unseen. The `hosted:` INPUT the deletion also removed carried no decision and is
//! held by nothing. And nothing here can tell whether a store a workflow names is trustworthy, only
//! that it is named. `zizmor` and review cover the rest.

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
/// rule rather than a step rule: one of these names anywhere in a workflow or a composite action,
/// comments excluded.
///
/// **TWO SPELLINGS, AND THE FIRST DRAFT HELD ONLY ONE.** As shipped this read `<name> =` and
/// nothing else, so `nix build --extra-substituters https://…` and `--option substituters …` were
/// GREEN on the merged tree while the module doc claimed no store outside this repository is
/// trusted - found by provoking the gate rather than by reading it. Nix accepts a setting from its
/// configuration (`nix.conf`, `NIX_CONFIG`, `install-nix-action`'s `extra_nix_config`), where it is
/// spelled `name = value`, AND from a command line, where it is spelled `--name value`,
/// `--extra-name value` or `--option name value`. Both are refused now; see [`Names`].
///
/// Matched as a SUBSTRING, which is what makes `extra-substituters` and
/// `extra-trusted-public-keys` fall out of two entries rather than four.
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
                let here = publisher == PUBLISH.0 && label.ends_with(PUBLISH.1);
                if !here {
                    out.push(format!(
                        "{label}:{}  {publisher} publishes to a store outside this repository. Only `{}` may, and only in `{}`, where the trigger and the environment both admit one branch - see docs/adr/0027",
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

/// The one store outside this repository that CI trusts, with the key that signs it.
///
/// **Committed rather than held in a secret or a variable, deliberately.** A committed value is one a
/// reviewer sees in the diff and one this gate can compare against; a variable can be swapped with no
/// diff at all and nothing would notice. The write credential is the only half that is a secret, and
/// it is an *environment* secret so only a push to the default branch can reach it.
const ALLOWED: [(&str, &str); 2] = [
    ("substituters", "https://sutura.cachix.org"),
    (
        "trusted-public-keys",
        "sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0=",
    ),
];

/// Whether one line assigns `setting` to exactly the permitted value and to nothing else.
///
/// **Only the additive assignment form is permitted** - `extra-substituters`, never plain
/// `substituters`, and never a flag. Three narrowings, each for its own reason: the allowance exists
/// for the installer's own `extra_nix_config`, where the workflow diff shows it; a substituter passed
/// on a command line stays refused even when it names the allowed store, because that spelling is how
/// a step reaches past the configuration a reviewer read; and the **destructive** spelling is refused
/// because it replaces nix's default substituter list instead of adding to it. The comparison is
/// against the WHOLE value list, so appending a second store to an otherwise-permitted line is still
/// a refusal.
fn permitted(code: &str, setting: &str) -> bool {
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
    ALLOWED
        .iter()
        .find(|(named, _)| *named == setting)
        .is_some_and(|(_, allowed)| value.split_whitespace().eq(std::iter::once(*allowed)))
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
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(code) = key_of(line) else { continue };
        for setting in SUBSTITUTER {
            let Some(named) = names(code, setting) else { continue };
            if named == Names::Assigned && permitted(code, setting) {
                continue;
            }
            let how = match named {
                Names::Assigned => format!("assigns `{setting}`, so every nix invocation in the job trusts it"),
                Names::Flag => format!("passes `{setting}` as a command-line option, so that nix invocation trusts it"),
            };
            out.push(format!(
                "{label}:{}  {how} - a store outside this repository, and {RECORD} records the Actions cache as the only carrier, so update that record rather than this gate",
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
        let step = "      - uses: cachix/cachix-action@38b082610b782e7e93e209c35fd730d399dee866 # v17\n";
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

    /// The allowance is the whole risk of enabling a binary cache: it has to admit exactly one store
    /// and refuse every neighbouring shape, or it reads as coverage while trusting anything.
    #[test]
    fn exactly_one_store_is_permitted_and_every_neighbour_is_still_refused() {
        let allowed = "      extra_nix_config: |\n        extra-substituters = https://sutura.cachix.org\n        extra-trusted-public-keys = sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0=\n";
        assert!(
            super::trusted_stores("ci.yml", allowed).is_empty(),
            "the committed store and its key are the one permitted pair"
        );

        for (why, text) in [
            (
                "a second store appended to the permitted line",
                "        extra-substituters = https://sutura.cachix.org https://example.invalid\n",
            ),
            ("a different store", "        extra-substituters = https://example.invalid\n"),
            (
                "the DESTRUCTIVE spelling, which replaces nix's default list and cost a bootstrap build",
                "        substituters = https://sutura.cachix.org\n",
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
        ] {
            assert!(!super::trusted_stores("ci.yml", text).is_empty(), "still refused: {why}");
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
        assert_eq!(found.len(), 1, "{found:#?}");
        let problem = found.first().map_or("", String::as_str);
        assert!(
            problem.starts_with(".github/workflows/ci.yml:1  cachix/cachix-action"),
            "{problem}"
        );
        assert!(problem.contains("docs/adr/0027"), "{problem}");

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
