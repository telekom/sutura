//! Can a `pull_request` path write the Actions cache? The gate that answers no.
//!
//! **The rule this holds.** A workflow run restores caches from its own ref or the default branch
//! and from nowhere else, so an entry a pull request writes is readable by exactly one pull request
//! and is then deleted by `cache-prune.yml` when it closes. Measured on 2026-09-08: 6,393 active
//! entries, roughly 97% of them under `refs/pull/<n>/merge`. The shape ordinary CI is allowed is
//! therefore restore on every event, save only from a push to `main` - which is the asymmetry
//! `ci.yml`'s hosted populate step already applies, one layer up.
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
//! * Whether the entry FITS is not a question about text at all. GitHub refuses a single cache
//!   entry over 10 GB and nothing here can weigh a store, so the run reports its own size instead -
//!   see the summary step in `.github/actions/nix-store-cache`.

use super::reach::Closure;

/// The main-push condition, token for token.
///
/// Pinned rather than matched loosely, for `crate::workflows::step`'s reason: a `contains` check
/// passes on `github.event_name != 'push'` and on an inverted ternary. Extra `&&` conjuncts are
/// accepted - `ci.yml`'s hosted populate step adds `vars.NIX_CACHE_NAME != ''` and is still
/// strictly narrower - and a `||` anywhere in the remainder is refused, because it can widen.
const MAIN_PUSH: &str = "github.event_name == 'push' && github.ref == 'refs/heads/main'";

/// Actions that write a cache, and are therefore only allowed behind [`MAIN_PUSH`].
///
/// Matched on the `owner/repo` prefix so a version pin, a subpath (`actions/cache/save`) or a
/// trailing comment does not evade it.
const WRITERS: [&str; 4] = [
    "nix-community/cache-nix-action",
    "actions/cache",
    "cachix/cachix-action",
    "DeterminateSystems/flakehub-cache-action",
];

/// The one that carries `/nix/store`, and therefore the one the anchor below counts.
///
/// **Separate from [`WRITERS`] because a floor over the whole list does not hold anything.** The
/// anchor first counted every gated writer, and `ci.yml`'s two `cachix/cachix-action` steps - gated
/// correctly, and inert while unprovisioned - satisfied it on their own. Deleting the store cache
/// outright then left the gate GREEN, which is the shape this tree calls a floor counted off the
/// same derivation as its own loop. Measured by provocation, not reasoned.
const STORE_CACHE: &str = "nix-community/cache-nix-action";

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

/// The restore-only rule over ordinary CI, as a list of violations.
///
/// Over the whole closure rather than one file name, for `super::reach`'s reason: a hard line cap
/// moves steps between files, and a refusal that names a file stops covering the step that left it.
pub(super) fn problems(closure: &Closure) -> Vec<String> {
    let files: Vec<(&str, &str)> = closure.inspected().iter().map(|file| (file.label(), file.text())).collect();
    judge(&files)
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
    match rest.strip_prefix(" && ") {
        None => rest.is_empty(),
        Some(extra) => !extra.is_empty() && !extra.contains("||"),
    }
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
            let value = key
                .strip_prefix("if:")
                .or_else(|| key.strip_prefix("save:"))?
                .trim();
            let inner = value
                .strip_prefix("${{")
                .and_then(|rest| rest.strip_suffix("}}"))
                .unwrap_or(value);
            Some(inner.split_whitespace().collect::<Vec<_>>().join(" "))
        })
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
            .find(|(_, next)| {
                !next.trim().is_empty() && next.len().saturating_sub(next.trim_start().len()) <= depth
            })
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
        for accepted in [
            MAIN_PUSH.to_owned(),
            format!("{MAIN_PUSH} && vars.NIX_CACHE_NAME != ''"),
        ] {
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

        let gated = workflow(
            "nix-community/cache-nix-action",
            "save:",
            &format!("${{{{ {MAIN_PUSH} }}}}"),
        );
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
    fn a_gated_hosted_cache_does_not_satisfy_the_store_anchor() {
        // THE HOLE THIS GATE WAS FIRST WRITTEN WITH, FOUND BY PROVOCATION AND NOW HELD BY A TEST.
        // `ci.yml` gates two `cachix/cachix-action` steps correctly, and a floor written over every
        // writer counted them - so deleting the store cache outright left the gate green. That is a
        // floor counted off the same derivation as its own loop, and no provocation on the real
        // tree can show it, because the real tree has both.
        let hosted = step_using("cachix/cachix-action", "if:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        let found = super::judge(&[("ci.yml", &hosted)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found.first().is_some_and(|problem| problem.contains(super::STORE_CACHE)),
            "a correctly gated hosted cache is not a store cache: {found:#?}"
        );

        // The same file plus a gated store cache is the shape this repository has, and it passes.
        let store = step_using(super::STORE_CACHE, "save:", &format!("${{{{ {MAIN_PUSH} }}}}"));
        let both = format!("{hosted}\n{store}");
        let clean = super::judge(&[("ci.yml", &both)]);
        assert!(clean.is_empty(), "{clean:#?}");
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
        let back = format!(
            "{store}\n      - uses: DeterminateSystems/magic-nix-cache-action@908b263f # v14\n"
        );
        let found = super::judge(&[("ci.yml", &back)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found.first().is_some_and(|p| p.contains("has no restore-only mode")),
            "{found:#?}"
        );
    }

    #[test]
    fn the_committed_tree_writes_the_actions_cache_only_from_a_push_to_main() {
        // THE LIVE ASSERTION. The unit fixtures above are shapes; this is the repository, and it is
        // what fails when somebody adds a cache write to a pull-request path.
        let Some(root) = crate::repo::root() else { return };
        let ordinary = crate::workflows::contexts::OrdinaryCi::read(&root);
        let found = super::problems(ordinary.closure());
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
            .filter(|step| step.uses().is_some_and(|uses| uses.starts_with("nix-community/cache-nix-action")))
            .collect();
        assert_eq!(saving.len(), 1, "one restore/save step, not {}", saving.len());
        let gate = saving.first().and_then(super::Step::gate).unwrap_or_default();
        assert_eq!(gate, MAIN_PUSH, "the save condition must be the pinned main-push expression");
    }
}
