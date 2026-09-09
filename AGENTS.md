# AGENTS.md

sutura is an identity-aware semantic data runtime: pluggable metadata and data sources, a compiled
semantic query plan, light federation, e2e impersonation. Security is the product, not a layer.

Root of trust. `CLAUDE.md` is `@AGENTS.md`. A skill refines *how* to work inside these rules and
never overrides them. Early stage - the plan is settled, the code is not.


## Public repository - `github.com/telekom/sutura`

Docs, comments, fixtures, commit messages and branch names are world-readable. **Never commit:**
internal product/platform/service names · non-public hostnames, domains, tracker URLs or page ids ·
paths outside this repo, other repo names · cloud project/account/tenant ids · people's names,
usernames or emails (use `user@example.com`) · internal classification schemes (use `internal` /
`confidential` / `restricted`) · references to documents that live elsewhere.

A description specific enough to identify one of those **is** disclosure - write the capability and
its constraint generically instead, and the point usually improves. The pattern backstop is
deliberately outside this repo (a list of what we avoid naming would itself be the disclosure) and
cannot catch a paraphrase. **The control is not writing it down here.**


## Principles

Hard breaking changes with clarity over legacy accumulation. Concise code, no AI-slop comments.
Security by design *and* by verification. Hermetic build with nix. Test most things, E2E in CI per
data source; a golden suite holds every adapter to conformance. Performance in mind - no clone or
`Arc` to escape the borrow checker, both where they earn it. Push computation and impersonation
down to the source system where the adapter can. Engage the ecosystem instead of monkeypatching:
propose upstream, vendor only to move fast and record it in `VENDOR.md`.

Use the compiler for communication - [newtypes that parse](https://www.howtocodeit.com/guides/ultimate-guide-rust-newtypes),
[typed errors](https://www.howtocodeit.com/guides/the-definitive-guide-to-rust-error-handling#structured-error-handling-in-rust),
[dependencies pointing inward](https://www.howtocodeit.com/guides/master-hexagonal-architecture-in-rust).
Those three plus secure-by-design are the definition of *correct* in review here.


## Non-negotiables

- **`just validate` is the only thing that counts as verified**, and passes before a change is
  done. Its nix checks build a GIT-DERIVED copy of the tree - so `git add -N` a new file at once,
  or it compiles under cargo and does not exist in the sandbox.
- **Never run a bare `cargo clippy` / `cargo nextest`. Run `just lint` / `just test`.** The task IS
  the gate's invocation: a hand-written line loses `-D warnings`, which turns a
  `restriction`-category finding into a success. Fixing that alone still fails the gate.
- **A new or changed test is red against base and green with your change.** One that passes both
  ways is worse than none, because it looks like coverage. `just causality` proves it mechanically;
  `just ship-check` before saying done.
- **Cite a `just` task, never a raw command line** - `check-guidance` fails a citation of a task
  that does not exist, or a cited `cargo` line missing `--all-features`.
- **Invariants are held by a type, a lint, a hook or a gate - never by recall.** Changing one is an
  architecture decision; a rule that loses its mechanism gets deleted, not demoted to advice. A
  change you cannot tie to a mechanism is unproven - say so rather than asserting it is fine, and
  prefer adding the missing check to adding a sentence.
- **State the limit next to the claim.** An overstated control is itself the defect. Leg 1 (knowing
  who is asking) is built; leg 2 (a source executing AS them) is not, on anything published.
- **Never commit or force-push unless asked.** Prefer stacked, individually reviewable PRs via stax.
- **This shell's cargo env leaks into other checkouts** - `CARGO_*CODEGEN_BACKEND=cranelift` and
  `DUCKDB_*_DIR` are unscoped, and a C++-linking crate built under them aborts. Unset them before
  building any other repo; a red run there is unexplained until you have.


## Working here

Read the source, run the tests, check the pinned versions - prompt text, task notes and memory are
routing context, not proof of current state. Verify external behaviour rather than asserting it: if
you claim a system rejects something, reproduce it and paste the error. Keep changes scoped; a
mechanical change repeated across files is one commit. Put a deterministic requirement in a task,
hook, lint or generated contract - a rule with no mechanism is a wish. Prove completion with the
command and its output; if tests fail say so, if you skipped a step say which, and correct a wrong
claim plainly. Ports get **fakes**, not mocked HTTP - a test asserting on source text proves
nothing. Rust 2024, conventional commits. If guidance here is wrong, fix this file - preferably by
adding a check rather than a sentence.

`ls crates/` is the layout, because the prefix is the role: `-domain` is the hexagon's interior,
everything else is an adapter, nothing depends on an adapter, and a port trait arrives with its
first implementor. `-semantic` query→plan · `-sql` plan→statement per dialect · `-app` service and
driving port · `-catalog-*` metadata · `-exec-*` data systems · `-http`/`-mcp` transports ·
`-config` settings and startup refusals · `-runtime` process globals · `-cli`/`-serve` composition
roots · `xtask` gates. `just` with no argument lists every task.


## Incremental discovery

**This file is a router, not a manual.** Context is the scarce resource, so knowledge lives as far
from the default path as it can while still being found. Guidance is a **tree**, never one blob:
`.agents/skills/README.md` → that group's `README.md` → only the `SKILL.md` it routes to. Three small
files, not the repo. A skill absent from `skill-router.json` is non-discoverable by policy, and
`cargo xtask check-skills` holds both directions.

What earns a place in the tree, and what does not:

- **Do NOT write down what a command answers.** `ls crates/`, `just --list`, `cargo xtask --help`,
  `cargo tree`, `grep` and the file itself are the source of truth for names, lists and structure. A
  copy of them is a second thing to keep true, and it rots first.
- **DO write down long-term value:** why a decision went the way it did, which way a mechanism fails,
  what a control does **not** cover, and anything measured at cost. If a green run once meant less
  than someone believed, that belongs in the tree.
- **DO write down a repeated challenge** - a mistake made twice is cheaper as one paragraph than as a
  third session. Name the trap and the fix, not the whole story.
- **Keep it token-efficient.** No preamble, no restating the code, no history of how a sentence used
  to read. Prefer a table row to a paragraph.
- **Split by task, not by topic size.** If a file only ever gets half-read, it is two skills.

Same rule applies to this file: if guidance here can be discovered in a second, delete it; if it is
task-specific, route to it.

This codebase's own reference is the `sutura/` group:

| Before you… | Open |
| --- | --- |
| change a mechanism, or claim anything is enforced | `sutura/invariants` |
| touch the tool surface, a plan, a dialect or a refusal | `sutura/query-surface` |
| touch tokens, credentials, postures, brokers or provenance | `sutura/identity` |
| add or change a crate, a feature, or what ships | `sutura/crate-map` |
| bump, patch, vendor or resolve a version conflict | `sutura/dependencies` |
| shape a newtype, an error, a clone or an `Arc` | `sutura/secure-by-design`, then `engineering/rust` |
| judge what a green run covered, or why a check passed | `sutura/gates` |
| split work into reviewable PRs | `git-ops/stacked-branches` |
| work out why a gate or test is red | `engineering/debugging` |

`CONTRIBUTING.md` hook tiers and the PR checklist · `SECURITY.md` what counts as a vulnerability
and what is design rather than guarantee · `docs/architecture.md` the narrative · `docs/adr/`
decisions in sutura's own numbering, citing nothing external ·
`docs/where-identity-is-proven.md` which venue may be cited for which identity claim.
