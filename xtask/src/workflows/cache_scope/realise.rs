//! Writer-EFFECTIVENESS for the daemon-mode cache publisher (issue #560).
//!
//! The containment gate over in `retired.rs` answered *may a cachix-action step exist here*; this
//! module answers *does the job it lives in push anything*. `cachix` in daemon mode (`useDaemon`,
//! the default) publishes ONLY paths nix BUILDS after it starts - the post-build hook - and does
//! NOT push the store it found restored. A publish step that is the last thing its job does fills
//! its cache with nothing and stays green, which is how three per-consumer caches came back EMPTY
//! under new names after the containment rules claimed to hold the retired inert cache.
//!
//! THE RULE, ONE SENTENCE, AS `#560` ASKS FOR IT: a `cachix/cachix-action` step that does not
//! declare `useDaemon: false` must be followed, in the SAME job, by a step that runs `nix build`.
//! `useDaemon: false` with explicit paths-to-push is the alternative spelling the issue names - the
//! only one whose point is to publish a RESTORED store rather than a built one - and it is exempt
//! because it does not need the hook. Everything else in this repository uses the daemon and is
//! held to the realise. Two jobs that legitimately realise (`push` for the shared `sutura` cache,
//! the per-consumer jobs for theirs) must keep the realise; refusing correct work is the shape that
//! gets a gate disabled rather than fixed, and `a_cachix_step_followed_by_a_realise_is_accepted`
//! pins that the gate cannot fire on them.
//!
//! Its own file rather than another block in `retired.rs` because `max-lines` splits at 1000 - the
//! same reason `retired.rs` exists as a sibling to `cache_scope.rs`.

use super::{key_of, steps, Step};

/// The same-job realise requirement for every `cachix/cachix-action` write step in `text`.
pub(super) fn problems(label: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let steps = steps(text);
    for (index, step) in steps.iter().enumerate() {
        let Some(uses) = step.uses() else { continue };
        if !uses.starts_with("cachix/cachix-action") {
            continue;
        }
        // The explicit-push spelling publishes the restored store on purpose and needs no hook.
        if step.lines.iter().any(|line| line.contains("useDaemon: false")) {
            continue;
        }
        let job_end = job_end(text, step.line);
        let realises_after = steps
            .iter()
            .skip(index.saturating_add(1))
            .take_while(|later| later.line < job_end)
            .any(realises);
        if !realises_after {
            out.push(format!(
                "{label}:{}  {uses} writes only what nix BUILDS while its daemon is alive, and no later step in this job realises anything - a job that restores and then ends pushes nothing and stays green (issue #560). Add a `nix build` realise step after this one, in the same job",
                step.line
            ));
        }
    }
    out
}

/// Whether a step's `run:` body executes `nix build` - the realise a daemon-mode publisher needs.
///
/// The recognise is over the whole step: the `run:` KEY is on its own line and the `nix build`
/// sits in the body BELOW it, so a single-line scan of the key would never see the realise.
fn realises(step: &Step<'_>) -> bool {
    let is_run = step
        .lines
        .iter()
        .any(|line| key_of(line).is_some_and(|k| k.trim_start().starts_with("run:")));
    is_run && step.lines.iter().any(|line| line.contains("nix build"))
}

/// The line just past the end of the job block owning `step_line`, or [`usize::MAX`].
///
/// A job is a two-space key ending in `:`. The scan mirrors `retired::in_pr_publish_job`'s span
/// walk so a step in one job can never be satisfied by a realise in a sibling job - `job_end` is
/// where the "same job" boundary is drawn.
fn job_end(text: &str, step_line: usize) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let mut spans = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.len().saturating_sub(line.trim_start().len()) == 2 && line.trim_start().ends_with(':') {
            let end = lines
                .iter()
                .enumerate()
                .skip(index.saturating_add(1))
                .find(|(_, next)| {
                    let next_trimmed = next.trim_start();
                    !next_trimmed.is_empty()
                        && next.len().saturating_sub(next_trimmed.len()) <= 2
                        && next_trimmed.ends_with(':')
                })
                .map_or(lines.len(), |(at, _)| at);
            spans.push((index.saturating_add(1), end));
        }
    }
    spans
        .iter()
        .find(|(start, end)| *start <= step_line && step_line < *end)
        .map_or(usize::MAX, |(_, end)| *end)
}

#[cfg(test)]
mod tests {
    const LABEL: &str = ".github/workflows/cachix-push.yml";

    /// The per-consumer publish block, cachix-action permitted in this file.
    fn publish_job(name: &str, tail: &str) -> String {
        format!(
            concat!(
                "  {name}:\n",
                "    runs-on: rust-mcp-32core\n",
                "    environment: cachix-push-mixed\n",
                "    steps:\n",
                "      - uses: cachix/cachix-action@38b082610b782e7e93e209c35fd730d399dee866 # v17\n",
                "        with:\n",
                "          name: sutura-{name}\n",
                "          authToken: ${{{{ secrets.CACHIX_AUTH_TOKEN }}}}\n",
                "          pushFilter: \"(-source$|nixpkgs)\"\n",
                "{tail}",
            ),
            name = name,
            tail = tail
        )
    }

    /// ISSUE #560, RED: a daemon-mode publish step that is its job's LAST step.
    ///
    /// This is the exact shape that shipped the three empty caches - restore a scope, run
    /// cachix-action, end the job. The daemon's post-build hook fires nothing, the cache stays
    /// empty, and the job reports green. Mutating the fix away (removing the realise step) must
    /// turn the gate RED.
    #[test]
    fn a_cachix_step_with_no_following_realise_is_refused() {
        let restored = concat!(
            "      - uses: ./.github/actions/nix-store-cache\n",
            "        with:\n",
            "          scope: bq\n",
        );
        let found = super::problems(LABEL, &publish_job("connectors", restored));
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found
                .first()
                .is_some_and(|problem| problem.contains("no later step in this job realises anything")),
            "{found:#?}"
        );
        assert!(
            found
                .first()
                .is_some_and(|problem| problem.contains("issue #560")),
            "{found:#?}"
        );
    }

    /// ISSUE #560, GREEN (restored): a realise step AFTER cachix-action satisfies the rule.
    ///
    /// The two legitimate jobs (`push` for the shared cache, `connectors`/`cross-build` for their
    /// own) must keep passing - a rule that refuses correct work is the shape that gets a gate
    /// disabled rather than fixed.
    #[test]
    fn a_cachix_step_followed_by_a_realise_is_accepted() {
        let realise = concat!(
            "      - name: Realise and publish the shared closure\n",
            "        run: |\n",
            "          set -eu\n",
            "          nix build --print-build-logs \\\n",
            "            .#checks.x86_64-linux.nextest \\\n",
            "            .#checks.x86_64-linux.clippy \\\n",
            "            .#checks.x86_64-linux.hygiene\n",
        );
        assert!(
            super::problems(LABEL, &publish_job("push", realise)).is_empty(),
            "a realise after cachix-action is the working shape"
        );
    }

    /// ISSUE #560, the same-job boundary: a realise in a SIBLING job does NOT satisfy this job's
    /// publisher. The post-build hook only fires for builds in the same daemon, so the realise has
    /// to live in the same job - a "fix" that just moved the realise elsewhere must stay red.
    #[test]
    fn a_realise_in_a_sibling_job_does_not_satisfy_the_publisher() {
        let two_jobs = concat!(
            "  push:\n",
            "    steps:\n",
            "      - uses: cachix/cachix-action@bbbb # v17\n",
            "        with:\n",
            "          name: sutura\n",
            "  connectors:\n",
            "    steps:\n",
            "      - name: Realise elsewhere\n",
            "        run: nix build .#checks.x86_64-linux.nextest\n",
        );
        let found = super::problems(LABEL, two_jobs);
        assert_eq!(found.len(), 1, "a sibling-job realise is not this job's realise: {found:#?}");
    }

    /// ISSUE #560, GREEN: the explicit-push spelling (`useDaemon: false`) publishes the RESTORED
    /// store on purpose and needs no following realise - the issue's named alternative.
    #[test]
    fn an_explicit_push_spelling_needs_no_following_realise() {
        let fixture = publish_job("push", "          useDaemon: false\n");
        assert!(
            super::problems(LABEL, &fixture).is_empty(),
            "useDaemon:false publishes the restored store and must not be refused"
        );
    }
}
