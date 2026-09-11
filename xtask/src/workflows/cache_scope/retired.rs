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
//! * **Every write site declares an `environment:`.** [`write_environment`]. The credential behind
//!   a cachix write is an environment secret, so a job that names no environment cannot hold one -
//!   and the refusal was pinned on the PR writer alone, which left the three writers in
//!   `cachix-push.yml` free to drop theirs with the gate green. Measured by mutation, below.
//! * **The publish workflow's trigger is pinned, on its own terms.** [`PUBLISH_TRIGGER`]. Widening
//!   `cachix-push.yml` to `pull_request` used to redden only the required-contexts accounting, so
//!   the same widening plus three advisory rows there was green.
//! * **Which stores a line may name, and whether a job names any at all.** [`stores`], its own
//!   file: the substituter rules are a LINE rule and a STEP rule over the same two settings, and
//!   this file had reached the 1000-line cap that `crates/` and `xtask/` cannot exempt.
//! * **And the record itself.** Missing or empty is refused, for `sast`'s reason.
//!
//! # What this does NOT hold, and one sentence of it used to claim otherwise
//!
//! The substituter heading read *no store trusted* when it shipped while the rule read only
//! `<name> =`, so `nix build --extra-substituters https://…` was GREEN on merged `main` - an
//! overstated refusal on a supply-chain path is worse than none. Both spellings are refused now;
//! the route that survives is named on [`stores::trusted_stores`]: a value assembled from pieces
//! passes unseen.
//!
//! [`write_environment`] holds that an environment is NAMED, which is not the same as an
//! environment that is SCOPED. Measured against the forge on 2026-09-11: `cachix-push` carries a
//! deployment-branch policy admitting `main` and nothing else; `cachix-push-mixed`, which holds
//! the cross-build and connectors credentials, has no protection rules and no branch policy at
//! all, so for those two stores the only thing keeping a write on the default branch is
//! [`PUBLISH_TRIGGER`]. Two mechanisms for one store and one for the other two - and a gate cannot
//! read a branch policy, which is why both halves are held here and the difference is written
//! down rather than averaged into one sentence.
//!
//! [`HOSTED`] carries [`super::WRITERS`]'s limit - a name list, so a publisher nobody named passes
//! unseen; `zizmor` and review cover the rest.

use std::collections::BTreeSet;
use std::path::Path;

use super::{key_of, steps};

// Which stores a line may NAME, and whether an installer names any - one subject, two directions,
// and the pair that pushed this file past the 1000-line cap `crates/` and `xtask/` cannot exempt.
// A CHILD module rather than a sibling: the seam is inside this question, not beside it, and the
// entry point callers use (`problems`, `retired`) does not move.
mod stores;

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
/// narrow.** The publish workflow triggers only on a push to the default branch ([`PUBLISH_TRIGGER`])
/// and takes its credential from a named environment ([`write_environment`]); whether that
/// environment also carries a branch policy is a forge setting no gate reads, and for
/// `cachix-push-mixed` it does not - so the trigger is the binding half there. The same step added to
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
    if environment_of(text, step_line).as_deref() != Some(PUBLISH_PRS) {
        return false;
    }
    let lines: Vec<&str> = text.lines().collect();
    let Some((start, end)) = job_span(text, step_line) else {
        return false;
    };
    // The job's own `if:` must be PR-only, read ONLY at the job's own key depth (a sibling of its
    // `environment:`/`steps:` keys) - a step-level `if:` under a step marker must not stand in for
    // it, or deleting the job gate would re-open main with the gate still GREEN.
    let scope = lines
        .get(start)
        .map_or(2, |l| l.len().saturating_sub(l.trim_start().len()) + 2);
    (start..end).any(|at| {
        lines.get(at).is_some_and(|line| {
            line.len().saturating_sub(line.trim_start().len()) == scope
                && key_of(line).is_some_and(|k| {
                    k.trim_start()
                        .strip_prefix("if:")
                        .is_some_and(|v| stores::narrower_than_pr(v.trim()))
                })
        })
    })
}

/// The job block owning `step_line`, as a `start..end` line range over `text`.
///
/// A job is a two-space key ending in `:`; the block ends at the next key no deeper than two
/// spaces. Comment blocks at two spaces precede a job key and own no steps. **One definition of
/// *the same job*, because three rules here draw that boundary** - the PR-writer pairing, the
/// write-site environment and the coverage rule's exemption lookup - and three walks would be
/// three places for the boundary to drift.
fn job_span(text: &str, step_line: usize) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut spans = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if line.len().saturating_sub(trimmed.len()) == 2 && trimmed.ends_with(':') {
            let end = lines
                .iter()
                .enumerate()
                .skip(index.saturating_add(1))
                .find(|(_, next)| {
                    let nt = next.trim_start();
                    !nt.is_empty() && next.len().saturating_sub(nt.len()) <= 2 && nt.ends_with(':')
                })
                .map_or(lines.len(), |(at, _)| at);
            spans.push((index, end.min(lines.len())));
        }
    }
    spans.into_iter().find(|(start, end)| *start < step_line && step_line < *end)
}

/// The name of the job owning `step_line`, for a rule that has to name one.
pub(super) fn job_of(text: &str, step_line: usize) -> Option<String> {
    let (start, _) = job_span(text, step_line)?;
    let key = text.lines().nth(start)?.trim();
    Some(String::from(key.strip_suffix(':').unwrap_or(key)))
}

/// The `environment:` the job owning `step_line` declares, read at the job's own key depth.
///
/// Depth matters: a `with: { environment: … }` input to some step is not a deployment environment,
/// and reading one as if it were would hand a write site a credential boundary it does not have.
fn environment_of(text: &str, step_line: usize) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = job_span(text, step_line)?;
    let scope = lines
        .get(start)
        .map_or(2, |l| l.len().saturating_sub(l.trim_start().len()) + 2);
    (start..end).find_map(|at| {
        let line = lines.get(at)?;
        (line.len().saturating_sub(line.trim_start().len()) == scope)
            .then(|| {
                key_of(line)?
                    .trim_start()
                    .strip_prefix("environment:")
                    .map(|v| String::from(v.trim()))
            })
            .flatten()
    })
}

/// Contexts a workflow cannot observe, and therefore may not decide a step on.
///
/// `secrets` is not exposed to an `if:` at ALL, and `vars.X != ''` is `false` when unprovisioned -
/// either way the step reports `completed/skipped`: green, invisible. Matched as a bare prefix so
/// `vars.ANYTHING` is covered - the failure is the SHAPE, not the names.
const UNOBSERVABLE: [&str; 2] = ["secrets.", "vars."];

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
                        "{label}:{}  {publisher} publishes to a store outside this repository. Only `{}` may, and only in `{}` (or a pull_request ci.yml job declaring environment `{PUBLISH_PRS}`), whose trigger admits one branch and whose credential is an environment secret - see docs/adr/0027",
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
        out.extend(stores::problems(label, text));
        out.extend(write_environment(label, text));
        out.extend(publish_trigger(label, text));
        out.extend(super::realise::problems(label, text));
    }
    out
}

/// Every cachix write site whose job declares no `environment:`.
///
/// **THE REFUSAL EXISTED FOR ONE OF THE FOUR WRITE SITES, and mutation is what found that.**
/// Measured on 2026-09-11 with the gate green after each edit: deleting `environment: cachix-push`
/// from the `sutura` writer exited 0, deleting `environment: cachix-push-mixed` from the
/// cross-build writer exited 0, and the control - deleting `environment: cachix-push-pr` from
/// `ci.yml`'s PR writer - exited 1. [`in_pr_publish_job`] pinned the PR one by NAME, so the three
/// in `cachix-push.yml` were held by nothing.
///
/// **What it holds is that an environment is NAMED.** A cachix write needs a token, the token is an
/// environment secret, and a job naming no environment cannot read one - so a write site with no
/// `environment:` is either broken or reading a repository-wide secret, and both are refused. What
/// it does NOT hold is that the environment is SCOPED: a deployment-branch policy is a forge
/// setting outside every file here, `cachix-push-mixed` has none, and no text rule can see that.
/// [`PUBLISH_TRIGGER`] is the half that is in the tree.
fn write_environment(label: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for step in steps(text) {
        if !step.uses().is_some_and(|uses| uses.starts_with(PUBLISH.0)) {
            continue;
        }
        // A publisher in a file that may not host one is already refused by `retired`, harder than
        // this; judging it again would report one defect as two. So this speaks only to a write
        // site the pairing PERMITS - which is where the hole was.
        if !label.ends_with(PUBLISH.1) && !in_pr_publish_job(text, step.line) {
            continue;
        }
        if environment_of(text, step.line).is_some() {
            continue;
        }
        out.push(format!(
            "{label}:{}  {} writes a store and its job declares no `environment:` - the write credential is an environment secret, so a job without one either cannot publish or is reading a repository-wide token. Declare the job's environment (`{PUBLISH_PRS}` on the pull-request path)",
            step.line, PUBLISH.0
        ));
    }
    out
}

/// The trigger [`PUBLISH`]'s file must carry, line for line, comments and blank lines excluded.
///
/// **A PIN AND NOT A PREDICATE, because a predicate was what let the widening through.** Adding a
/// `pull_request:` trigger to `cachix-push.yml` did redden a run - but only through the
/// required-contexts accounting one module over, so the same widening plus three advisory rows
/// there was green, and the thing actually at stake (a pull request reaching a write credential)
/// was refused by nothing that names it. An exact sequence refuses `pull_request`,
/// `workflow_dispatch`, `schedule`, a second branch and a tag pattern in one comparison, and a
/// LEGITIMATE change to the trigger edits this constant in the same commit - which is the diff a
/// reviewer should be made to read.
const PUBLISH_TRIGGER: [&str; 4] = ["on:", "push:", "branches:", "- main"];

/// Whether [`PUBLISH`]'s file still triggers on exactly [`PUBLISH_TRIGGER`].
fn publish_trigger(label: &str, text: &str) -> Vec<String> {
    // A text with no `on:` at all declares no trigger to widen. Left to `actionlint`, which refuses
    // a workflow without one, and it keeps this rule off every fixture of the rules beside it.
    if !label.ends_with(PUBLISH.1) || !text.lines().any(|line| line.starts_with("on:")) {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line.starts_with("on:") {
            inside = true;
        } else if inside && !line.starts_with(char::is_whitespace) {
            break;
        }
        if inside {
            found.push(trimmed);
        }
    }
    if found == PUBLISH_TRIGGER {
        return Vec::new();
    }
    vec![format!(
        "{label}  triggers on {found:?}, not on {PUBLISH_TRIGGER:?} - this file holds the write credentials for every store CI trusts, and a trigger a pull request or a tag can reach hands them to a ref the environment policy may not bound. `cachix-push-mixed` has no branch policy at all, so this line is what holds those two stores to the default branch"
    )]
}

#[cfg(test)]
pub(super) mod tests {
    // The parent's pinned main-push condition and its step-fixture builder: the shapes these
    // refusals are provoked WITH belong to the write half, so they are borrowed rather than
    // duplicated - a second copy of `MAIN_PUSH` is a second thing to keep true.
    use super::super::tests::step_using;
    use super::super::{MAIN_PUSH, STORE_CACHE};

    /// One labelled file, for the absence rules - they read `(String, String)` off a directory walk
    /// rather than the closure's borrowed pairs.
    pub(super) fn owned(label: &str, text: &str) -> Vec<(String, String)> {
        vec![(String::from(label), String::from(text))]
    }

    /// An installer step naming the permitted pair, with `gate` spliced in as its own `if:` line.
    ///
    /// **The store rule is CROSS-CUTTING**: any fixture carrying an `install-nix-action` step has to
    /// satisfy it whatever that fixture is really about, so the pair is built from the constants
    /// rather than copied - a second literal is a second thing to keep true. The tests that judge
    /// the VALUE keep their literals, because a fixture built from the constant it asserts on is a
    /// tautology.
    pub(super) fn wired_installer(gate: &str) -> String {
        format!(
            "      - uses: cachix/install-nix-action@13d8dd58 # v31.11.1\n{gate}        with:\n          extra_nix_config: |\n            fallback = true\n            extra-substituters = {}\n            extra-trusted-public-keys = {}\n",
            super::stores::STORES,
            super::stores::KEYS
        )
    }

    /// The publisher permission is a PAIRING, and the file half is the half that matters: the same
    /// step on the pull-request path would have a same-repository pull request's secrets in reach.
    #[test]
    fn the_publisher_is_permitted_in_one_file_and_refused_in_every_other() {
        // A permitted publisher still realises after cachix-action (#560), as the live tree does.
        // In the job shell a write site really has: `write_environment` refuses one with no
        // `environment:`, so a fixture without it would be measuring that rule and not this one.
        let step = concat!(
            "jobs:\n",
            "  push:\n",
            "    environment: cachix-push\n",
            "    steps:\n",
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
        let pr_job = &format!(
            "{}{}{}",
            "  ci:\n    steps:\n",
            wired_installer("        if: github.event_name == 'pull_request'\n"),
            concat!(
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
            )
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
    fn the_committed_tree_names_no_substituter_in_either_spelling() {
        // THE LIVE ASSERTION for the widened rule. It is what would have caught
        // `--extra-substituters` on merged `main`, and it is the row that goes red if a shell body
        // in `.github` ever hands nix a store this repository does not control.
        let Some(root) = crate::repo::root() else { return };
        let found: Vec<String> = super::read_github(&root)
            .iter()
            .flat_map(|(label, text)| super::stores::trusted_stores(label, text))
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

    /// THE LIVE ASSERTION for the coverage half, and the row that goes red the moment a workflow
    /// installs nix and reads upstream alone - which four of them did until this rule existed.
    #[test]
    fn every_installer_in_the_committed_tree_names_the_published_stores() {
        let Some(root) = crate::repo::root() else { return };
        let found: Vec<String> = super::read_github(&root)
            .iter()
            .flat_map(|(label, text)| super::stores::problems(label, text))
            .collect();
        assert!(found.is_empty(), "{found:#?}");
    }

    /// A write site with no `environment:` is refused, and the control is that a declared one is
    /// not - the refusal was pinned on the PR writer by NAME, so the three in `cachix-push.yml`
    /// were held by nothing and dropping either of theirs exited 0.
    #[test]
    fn a_cachix_write_site_whose_job_declares_no_environment_is_refused() {
        let job = |env: &str| {
            format!(
                "jobs:\n  push:\n    runs-on: ubuntu-latest\n{env}    steps:\n      - uses: cachix/cachix-action@38b08261 # v17\n        with:\n          name: sutura\n"
            )
        };
        let named = job("    environment: cachix-push\n");
        assert!(
            super::write_environment(".github/workflows/cachix-push.yml", &named).is_empty(),
            "a declared environment is the shape this permits"
        );
        let bare = job("");
        let found = super::write_environment(".github/workflows/cachix-push.yml", &bare);
        assert!(
            found
                .first()
                .is_some_and(|problem| problem.contains("declares no `environment:`")),
            "{found:#?}"
        );

        // DEPTH IS THE RULE: a step input spelled `environment:` is not a deployment environment,
        // and reading one as if it were would credit a write site with a boundary it has not got.
        let as_input = named.replace("    environment: cachix-push\n", "").replace(
            "          name: sutura\n",
            "          name: sutura\n          environment: cachix-push\n",
        );
        assert!(
            !super::write_environment(".github/workflows/cachix-push.yml", &as_input).is_empty(),
            "an `environment:` under a step's `with:` is not the job's"
        );

        // And the committed tree carries one at all four write sites.
        let Some(root) = crate::repo::root() else { return };
        let live: Vec<String> = super::read_github(&root)
            .iter()
            .flat_map(|(label, text)| super::write_environment(label, text))
            .collect();
        assert!(live.is_empty(), "{live:#?}");
    }

    /// The publish trigger is PINNED, and the pin is the point: widening it used to redden only the
    /// required-contexts accounting, so the same widening with three advisory rows added there was
    /// green while a pull request could reach every write credential CI trusts.
    #[test]
    fn widening_the_publish_trigger_is_refused_on_its_own_terms() {
        let pinned = "name: cachix-push\n\non:\n  push:\n    branches:\n      - main\n\npermissions:\n  contents: read\n";
        assert!(
            super::publish_trigger(".github/workflows/cachix-push.yml", pinned).is_empty(),
            "the pinned trigger is the shape this permits"
        );
        for widened in [
            pinned.replace("      - main\n", "      - main\n  pull_request:\n"),
            pinned.replace("      - main\n", "      - main\n      - release/*\n"),
            pinned.replace("      - main\n", "      - main\n  workflow_dispatch:\n"),
            pinned.replace("  push:\n    branches:\n      - main\n", "  push:\n    tags: [\"v*\"]\n"),
        ] {
            let found = super::publish_trigger(".github/workflows/cachix-push.yml", &widened);
            assert!(!found.is_empty(), "a widened trigger must be refused: {widened}");
        }
        // Scoped to the publish file: another workflow's trigger is not this rule's business.
        assert_eq!(
            super::publish_trigger(".github/workflows/ci.yml", pinned),
            Vec::<String>::new()
        );

        // Comments and blank lines are skipped, so the pin is about the trigger and not about the
        // prose above it - that header argues at length why the environment lives in its own file.
        let commented = pinned.replace("on:\n", "on:\n  # only the default branch writes a store\n");
        assert_eq!(
            super::publish_trigger(".github/workflows/cachix-push.yml", &commented),
            Vec::<String>::new()
        );

        let Some(root) = crate::repo::root() else { return };
        let live: Vec<String> = super::read_github(&root)
            .iter()
            .flat_map(|(label, text)| super::publish_trigger(label, text))
            .collect();
        assert!(live.is_empty(), "{live:#?}");
    }
}
