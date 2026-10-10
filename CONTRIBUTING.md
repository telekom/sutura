<!--
Every link to a file in this repository is written as an absolute github.com URL, and that
is deliberate. This file is rendered twice: by GitHub from the repository root, and by
mkdocs as `docs/contributing.md`, which is a pymdownx.snippets include of this file and
nothing else. A relative link cannot satisfy both - it resolves against the root in one
case and against `docs/` in the other - and the site builds with `--strict`, so a link that
does not resolve fails the build. An absolute URL resolves in both.
-->

# Contributing

Contributions are welcome. The fastest way to get one merged is to arrive with the gates green.

**This repository is public and everything in it is world-readable** - comments, fixtures, branch
names, commit messages. Read the first section of
[AGENTS.md](https://github.com/telekom/sutura/blob/main/AGENTS.md) before you write anything down.
It is also the root of trust for how this codebase is built, and it routes to the rest.

## Feedback and reporting

Questions, feature ideas and bug reports go to the
[issue tracker](https://github.com/telekom/sutura/issues). Please do not open a public issue for a
suspected vulnerability - report it privately through
[SECURITY.md](https://github.com/telekom/sutura/blob/main/SECURITY.md) instead.

## Development

Nix with flakes, plus `devenv` and `direnv`. Everything a gate uses comes from there at the pinned
version, so you cannot get a different compiler than CI.

```bash
curl -L https://nixos.org/nix/install | sh -s -- --daemon
printf 'experimental-features = nix-command flakes\n' >> ~/.config/nix/nix.conf
nix profile install nixpkgs#devenv nixpkgs#direnv
echo 'eval "$(direnv hook bash)"' >> ~/.bashrc   # or the zsh / fish equivalent

direnv allow    # once per clone: consents to .envrc loading devenv.nix
just setup      # installs the git hooks and the pixi env, warms xtask
just doctor     # what is present, what is missing, what would fail
```

`direnv allow` is a trust boundary, not a formality, and hooking `direnv` into your shell is the
step people skip. `just` with no argument lists every task.

**`just setup` is what installs the git hooks, and it is not optional.** It installs all three
stages (`pre-commit`, `pre-push`, `commit-msg`) through `prek`, installs the pixi environment the
hook runner lives in, warms `xtask` so your first commit is not a cold compile, and ends with
`doctor`. It is idempotent, so re-run it whenever an environment file changes.

**Hook tiers.** A hook that is too slow for its stage gets switched off, so each stage runs only what
it can afford.

| Stage        | Runs                                                                                           |
| ------------ | ---------------------------------------------------------------------------------------------- |
| `pre-commit` | fmt, clippy, `cargo check` of the changed packages, the structural gates, text and chart gates |
| `pre-push`   | the whole-tree secret scan and the supply-chain gate, nothing else                             |
| `commit-msg` | the conventional-commit subject                                                                |

The suite, the doctests and the CRAP score run in `just validate` and in CI. `just ship-check` also
runs CRAP for a diff that reaches the scored crate, and the fuzz replay (`just fuzz-smoke`) for a diff
that touches the fuzzed tree. A local commit can hold a failing test until `just validate` or CI
runs it.

Run `just setup` before your first commit, because **an uninstalled hook does not complain - it
silently never fires.** `just setup` unsets a `core.hooksPath` that points at a missing directory.
If you are unsure whether your hooks are live, run `just setup` again and read what it prints.

## Working in parallel worktrees

Each worktree runs its own development services. Two worktrees share no container, network, volume or port.

```bash
just worktree feat/thing       # a new worktree for a stacked change
just dev-up                    # start this worktree's services
just dev-endpoint clickhouse   # print one host:port
just dev-down                  # remove this worktree's services
```

Docker chooses each host port. Read a port with `just dev-endpoint`, and never write one into a file.

The services are ClickHouse, and Keycloak with `just dev-up-identity`. `just test` provisions PostgreSQL
from Nix. Without docker, the services skip locally and fail in CI. To add a service, add a row to
`sutura_dev::scope::SERVICES` and a block to `compose.services.yaml`.

**Other routes.** The dev container runs the same shell in Docker:
`docker compose -f compose.dev.yaml run --rm dev`. Use it on Windows without WSL2. Bare `rustup` compiles
and tests, but the hooks and gates come from Nix, so you see their failures only on the pull request.
With no direct internet egress, read
[Building without direct internet egress](https://github.com/telekom/sutura/blob/main/docs/enterprise-mirrors.md).

Run `just lint`, not a bare `cargo clippy`: the task adds `-D warnings`.

## Using AI-generated code

This codebase is written largely with coding agents, and that is not something to be coy about.
`AGENTS.md` is their contract; the tree under `.agents/skills/` is the reference material they load
on demand. If you work with an agent here, point it at `AGENTS.md` first and let it route.

Used thoughtfully these tools are a real multiplier. What they do not do is transfer ownership:

- **You own the diff.** A generated patch is a draft until you can explain why each part of it is
  there. If you cannot answer a review question about a line, it is not ready.
- **A claim needs a mechanism.** The failure mode here is not bad code, it is confident prose - a
  comment or a pull request describing a control that does not exist, or one stronger than the code
  delivers. An overstated claim is treated as a defect in its own right, because it spends trust a
  reviewer needed elsewhere.
- **A test has to be shown to test something.** See below. Generated tests are especially prone to
  passing against the base behaviour too.
- **Do not add prose where a check belongs.** A rule with no mechanism is a wish, and this
  repository would rather have a gate than a paragraph.
- **Keep guidance discoverable rather than duplicated.** If you add to `.agents/skills/`, add what a
  command cannot answer: a reason, a limit, a trap that cost somebody a session. Not a list of
  crates or tasks that `ls` and `just --list` already print.

**Attribution.** Where an agent co-authored a commit, say so with a trailer - the convention here is
`Co-Authored-By: <model> <noreply@anthropic.com>` - and note in the pull request what you verified
yourself rather than what was generated for you.

## Commit messages

The `commit-msg` hook runs `cargo xtask commit-msg`, which judges the subject line and nothing else:

```
<type>[(scope)][!]: <subject>
```

|          |                                                                                              |
| -------- | -------------------------------------------------------------------------------------------- |
| Types    | `feat`, `fix`, `refactor`, `chore`, `test`, `docs`, `perf`, `ci`, `build`, `style`, `revert` |
| Length   | at most 72 characters, where `git log --oneline` starts truncating                           |
| Breaking | `!` after the type or scope                                                                  |
| Shape    | a space after the colon, no trailing full stop                                               |
| Exempt   | subjects git writes itself: `Merge`, `Revert`, `fixup!`, `squash!`, `amend!`                 |

```
feat(semantic): compile a bounded date predicate
fix(catalog-local): reject a dimension absent from the pinned bundle
refactor!: rename the Warehouse port's execute method
```

The body is not checked. Use it for *why*. The subject also decides the next version: `feat` a
minor, `fix` and the rest a patch, `!` or a `BREAKING CHANGE` footer a major - a minor while the
version is 0.x (`breaking_always_bump_major = false` in `cliff.toml`).

## Testing

```bash
just test           # the gate's own invocation - never hand-write the cargo line
just check-changed  # does what I touched compile
just causality      # a new test fails on the base and passes with your change
just ship-check     # the finishing sequence; run this before saying done
just validate       # THE gate: the site build, then the nix checks, which build their own tree copy
```

**A new or changed test must be red against the base behaviour and green with your change.** A test
that passes both ways proves nothing and is worse than no test, because it looks like coverage.
`just causality` checks that mechanically. Where the change is not separable - impl and test in one
file, a rename with no behavioural difference, or every added test `#[ignore]`d so no run here
reaches one - the gate says so and asks for evidence instead: the command you ran, the failure
before the fix, the pass after. That goes in the pull request. **Do not skip it silently.** An
added or modified test PINNING behaviour the base tree already provides is the one shape the base
run can never redden, so its place is a declared claim cell: add a `Claim-Cell: <test-fn-name>`
commit trailer AND a committed killing mutation at `devco/claim-mutations/<test-fn-name>.patch`.
The gate applies the mutation and requires the cell to fail. A declared cell with no killing
mutation is refused. An added test the base run produced no result for (`not run at base`) is
refused by name: make it run at base, or list it with a reason in
`devco/causality-no-base-exemptions`.

**Exit 3 means the gate measured nothing**, and it is neither a pass nor a violation: the base tree
did not build, the base run named no failure, or every test in scope was one the base tree already
had - a MOVED test, which the gate reads out of the base tree because a diff cannot tell a move from
an addition. A harness move and a changed public signature that a test file kept at HEAD calls both
land there too. Substitute a mutation run, or scope the gate per commit, and say which in the pull
request. Read the verdict line, never a step's colour.

**Splitting a test file that hit the 1000-line cap has two answers, and the cheap one comes first.**
Move the assertions into a new module and declare it `#[cfg(test)] mod <name>;` - that form keeps
the declaring file at HEAD, so the base tree still has the declaration and the new module is not
orphaned. A bare `mod <name>;` reverts the declarer instead, and that is the whole difference
between exit 0 and *the tests this diff added did not run on base*. Only where the split has to land
beside a real implementation change, add `Cleanup-Split: <path>` as a commit trailer naming the file
you split: the gate then verifies the commit moved test code and changed none - every changed line
except a blank one is test code in its own image, and the code-line multiset is equal on both
sides - and answers `nothing to measure`. Only three shapes may be in surplus, and only on the
added side: a `#[cfg(test)]`, a `mod` declaration that resolves into the same diff, and a
`super::`-relative `use`. So move the imports the new module needs from `super::`, keep every other
line byte-identical, and put anything else in a second commit. **The trailer is a claim, not a
permission**: one added, deleted or reworded line - a comment and an attribute included - and the
gate fails and names it.

**The gate measures the MERGE BASE, it says which commit that was, and on a stack it derives it.**
It resolves `git merge-base <ref> HEAD` itself, so a base branch that has moved on cannot put other
people's commits into the diff - and on a stacked branch it takes the fork point from the parent
branch `st` recorded instead, because the trunk's merge base there is the fork point of the whole
stack. The printed line names the commit, the parent branch and the commit it replaced; a stale or
retargeted parent falls back to the ref you named rather than moving the base off your history.
`SHIP_CHECK_BASE_REF`, or a commit as the recipe's argument, still overrides both.

Ports get **fakes**, not mocked HTTP. That is what lets the whole tool surface, refusals included,
be tested without a warehouse, and a test asserting on source text proves nothing.

`just validate` builds a git-derived copy of the tree in its nix checks - so `git add -N` a new file
immediately, or it compiles locally and does not exist in the sandbox. `just gates` runs
`check-default-features` and `check-default-feature-tests`. Each gate stage is described in
[the `gates` skill](https://github.com/telekom/sutura/blob/main/.agents/skills/sutura/gates/SKILL.md).

## Pull requests

**`main` is the sole trunk.** Pull requests are squashed, leaving one conventional commit for
`git-cliff`. A branch behind `main` is **rebased**, never merged: a merge commit from `main` into a
topic branch comes back as a rebase request in review.

That history requirement is not authorization to rewrite a published branch. Obtain explicit
authorization before rebasing or restacking any published branch and force-pushing its replacement,
even if you alone own it (`AGENTS.md`). Without it, ask; do not merge `main` as a workaround.

**We prefer stacked pull requests, and we recommend [`stax`](https://github.com/cesarferreira/stax)
(`st`) for them.** A chain of dependent changes ships as one reviewable pull request per link, not
one branch that grows until nobody can review it. `stax` is in the dev shell at a pinned version
(`nix/stax.nix`), and `stax.toml` at the repo root overlays your global config with this repo's
forge, so a machine set up for a different one cannot submit stacks there.

```bash
st create feat/thing   # branch off the current one, entering the stack
st modify              # stage and amend into the branch tip
st ss                  # push the stack, opening or updating a PR per branch
st ls                  # what the stack looks like now
st refresh             # sync trunk, restack, submit the updates
```

Two rules, and both exist because the mistakes are the expensive kind:

- **Create stacked branches and their PRs with `st`, never `git checkout -b` or `gh pr create`.**
  stax only knows about branches and pull requests it made itself. One made with plain git is not
  merely invisible to it - it draws the stack **wrong** and then offers a destructive restack for
  that wrong picture, and `st stack submit` will plan a *duplicate* PR for a branch that already has
  one. Always `st ss --dry-run` before submitting a stack you did not create with `st`.
- **Never hand-`git rebase` a branch that belongs to a stack.** Restacking rewrites history by
  design; published branches need the explicit authorization above.

Run `just ship-check` **per branch** rather than once for the stack, since each pull request is
reviewed alone and so has to be green alone, and land bottom-up. Each link can sit in its own
worktree with its own service instances - see
[Working in parallel worktrees](#working-in-parallel-worktrees). A single independent change is fine
as one ordinary pull request; the preference is about dependent work, not ceremony. The mechanics,
including recovery when branches were made the wrong way, are in
[the `stacked-branches` skill](https://github.com/telekom/sutura/blob/main/.agents/skills/git-ops/stacked-branches/SKILL.md).

`.github/pull_request_template.md` is the form, and it is short because a reviewer's first five
seconds decide how they spend the rest. It asks for a review snapshot, what changed *as behaviour*
rather than as a list of files, the files in reading order, the exact validation commands and
whether they passed (**including what you did not run** - "should work" is not a result), the
causality evidence, and which mechanism would have failed if the change were wrong. "Nothing
mechanical" is an acceptable answer there and a useful one: it tells the reviewer the judgement is
theirs.

**One reviewable idea per branch. If describing it needs an "and", split it.**

## Releasing

An approved manual dispatch of `version-bump` on `main` is the **only** release entry point;
`release.yml` has no manual trigger of its own.

1. Approve the dispatch. It derives the version from the commit subjects since the last tag, writes
   `CHANGELOG.md` in the release commit, and pushes that commit and its `v*` tag. The tag starts
   `release.yml` and `docs.yml`.
2. Nothing is tagged if no commit since the last tag would move the version, so a dispatch over
   chores publishes nothing.
3. A failed release keeps its commit and tag for diagnosis: no GitHub Release, no image manifest,
   no moving tag. Fix the source and approve a new dispatch. **Never rewrite a release tag.**

[Verifying a release](https://github.com/telekom/sutura/blob/main/docs/verifying-a-release.md) is
what a consumer of the artifacts reads.

## Conventions that will fail your pull request

- **LF line endings, no trailing whitespace, one final newline.** `just fmt` fixes what is
  mechanically fixable.
- **No em dashes.** Plain hyphens, in prose and in comments.
- **No first-party `unsafe`.** `deny` in the workspace lint table and `#![forbid(unsafe_code)]` at
  every crate root but `sutura-adbc`'s.
- **`#[expect(.., reason = "..")]` over `#[allow]`**, so a suppression cannot outlive its cause -
  except on the three count-threshold lints, where `check-expect-thresholds` refuses an `#[expect]`:
  split the function or raise the threshold in `clippy.toml`. A workspace-wide exception is an
  `= "allow"` row in the workspace lint table instead.
- **No file over 1000 lines**, and no exemption under `crates/` or `xtask/` - the only way past it
  is to split the file.
- **No dependency declared and unused.**
- **`--all-features` on every lint and test entry point.** Several crates declare features now, so
  an entry point missing the flag lints and tests nothing behind them. The scoped
  `cargo check -p sutura-domain --no-default-features` is the fast inner loop, never the gate.
- **The domain crate acquires no framework dependency**, checked over the whole transitive tree, so
  a framework reached through an innocuous crate fails too.

## Building the docs

```bash
just docs          # render to site/
just docs-serve    # live reload
just docs-deploy   # one version to gh-pages
```

The site is mkdocs-material versioned by [mike](https://github.com/jimporter/mike), built with
`--strict`, and every command goes through pixi's isolated `docs` environment. `nav` in `mkdocs.yml`
is explicit rather than derived, so a new page needs an entry. A page that is deliberately not part
of the site - the implementation plans are the case - is named in `exclude_docs` instead (`/adr/`
keeps the decision records off the site). A published page does not link an excluded one, and does
not name a path inside the decision-record directory or cite a decision record by its number.
**This file is one of those published pages:** `docs/contributing.md` pulls it in with
`pymdownx.snippets`, so a relative link written here resolves against that page's URL rather than
the repository root. `just validate` renders the site as its first step, and `just docs` is the
same build, reached on its own.
[Publishing the docs](https://github.com/telekom/sutura/blob/main/docs/publishing.md) covers the
versioning and the one repository setting it needs.
