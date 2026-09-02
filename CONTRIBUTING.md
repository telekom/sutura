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

Run it before your first commit, because **an uninstalled hook does not complain - it silently never
fires.** This repo spent a whole session believing hooks ran that had never run: `core.hooksPath`
pointed at a `.githooks/` directory that had been deleted, so git looked somewhere that did not
exist while `prek install` wrote to `.git/hooks` where git was not looking. `just setup` now unsets
a `core.hooksPath` pointing at a missing directory for exactly that reason. If you are unsure
whether yours are live, `just setup` again and read what it prints.

## Working in parallel worktrees

Stacked branches mean several worktrees at once, so the development service tier is built to run
**one independent instance per worktree** rather than one shared set you have to take turns on.

```bash
just worktree feat/thing   # an isolated worktree for a stacked change
just worktrees             # every worktree and its scope
just ports                 # this worktree's compose project and service ports
just dev-up                # this worktree's services, provisioned and health-gated
just dev-endpoints         # where they are listening - the readable table
just dev-endpoint clickhouse   # one host:port on stdout, for shell substitution
just dev-down              # remove this worktree's services, network and volumes. Nothing else
just dev-down-dry          # what that would remove, and what it would spare
```

**No host port is written anywhere, and that is the load-bearing part.** `compose.services.yaml`
names only container ports, so the host port is ephemeral and chosen by docker and the OS;
`dev-up` reads back what they chose and writes it into a discovery file the harness reads, which is
why `just dev-endpoint` is the only way to learn a port and there is no constant to hardcode. A hash
into a port range cannot promise disjoint blocks, and "check whether the port is free, then bind" is
a race whose window belongs to whatever else is on the host.

Isolation comes from the compose **project name**, derived from the worktree's canonical path, so
containers, network and named volumes are all per-worktree and two worktrees running the same file
share nothing. Nobody has to remember to change a value. Every service declares a healthcheck,
because provisioning gates on health rather than sleeping.

The tier provisions ClickHouse today, plus Keycloak behind an `identity` profile that is off by
default (`just dev-up-identity`). Postgres is not here: `just test` provisions it from nix instead.
A missing docker **skips** locally and **fails** in CI, both from one flag. Adding a service is a
registration - a row in `sutura_dev::scope::SERVICES` and a block in `compose.services.yaml` - and
the file itself records which services are deferred and why. Orchestration lives in `xtask` and the
worktree and port commands in `sutura-dev`, deliberately: docker orchestration inside a release
artifact would be test scaffolding shipped to users, and neither of those crates is packaged.

**Two other routes, if that one is closed to you.** The dev container is the same shell in Docker -
`docker compose -f compose.dev.yaml run --rm dev` - and is the answer on Windows without WSL2. Bare
`rustup` compiles and tests (`rust-toolchain.toml` is the pin and rustup honours it), but `just`,
the hooks, the secret scan and the supply-chain gate all come from Nix and pixi, so on that route
you find out on the pull request instead of before it. On a network with no direct egress, read
[Building without direct internet egress](https://github.com/telekom/sutura/blob/main/docs/enterprise-mirrors.md)
first: nothing that fetches is hardcoded, and every location is read from the environment.

**The dev shell's bare `cargo` is nightly**, because the cranelift backend is what makes the inner
loop fast. Every gate that lints, tests or ships is stable. So a bare `cargo clippy` reports lints
stable has never heard of, and **you cannot conclude your branch is red from one** - run `just
lint`, which sources `nix/stable-env.sh` and adds `-D warnings`. That trap and its siblings are in
[the `gates` skill](https://github.com/telekom/sutura/blob/main/.agents/skills/sutura/gates/SKILL.md).

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

| | |
| --- | --- |
| Types | `feat`, `fix`, `refactor`, `chore`, `test`, `docs`, `perf`, `ci`, `build`, `style`, `revert` |
| Length | at most 72 characters, where `git log --oneline` starts truncating |
| Breaking | `!` after the type or scope |
| Shape | a space after the colon, no trailing full stop |
| Exempt | subjects git writes itself: `Merge `, `Revert `, `fixup!`, `squash!`, `amend!` |

```
feat(semantic): compile a bounded date predicate
fix(catalog-local): reject a dimension absent from the pinned bundle
refactor!: rename the Warehouse port's execute method
```

The body is not checked. Use it for *why*. The subject also decides the next version: `feat` a
minor, `fix` and the rest a patch, `!` or a `BREAKING CHANGE` footer a major.

## Testing

```bash
just test           # the gate's own invocation - never hand-write the cargo line
just check-changed  # does what I touched compile
just causality      # red-before-green proof
just ship-check     # the finishing sequence; run this before saying done
just validate       # THE gate: the nix checks, which build their own copy of the tree
```

**A new or changed test must be red against the base behaviour and green with your change.** A test
that passes both ways proves nothing and is worse than no test, because it looks like coverage.
`just causality` checks that mechanically. Where the change is not separable - impl and test in one
file, or a rename with no behavioural difference - the gate says so and asks for evidence instead:
the command you ran, the failure before the fix, the pass after. That goes in the pull request. **Do
not skip it silently.**

Ports get **fakes**, not mocked HTTP. That is what lets the whole tool surface, refusals included,
be tested without a warehouse, and a test asserting on source text proves nothing.

`just validate` is the only thing that counts as verified, because its nix checks build a
git-derived copy of the tree - so `git add -N` a new file immediately, or it compiles locally and
does not exist in the sandbox. Gate stages, what each one covers and where a green run means less
than it looks are all in
[the `gates` skill](https://github.com/telekom/sutura/blob/main/.agents/skills/sutura/gates/SKILL.md).

## Pull requests

**`main` is the sole trunk.** Pull requests are squashed, leaving one conventional commit for
`git-cliff`. A branch behind `main` is **rebased**, never merged: a merge commit from `main` into a
topic branch comes back as a rebase request in review.

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
  design, so ask before force-pushing a branch somebody else has.

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

An approved manual dispatch of `version-bump` on `main` is the **only** release entry point. It
derives the version from the commit subjects above, writes `CHANGELOG.md` in the release commit, and
pushes that commit and its `v*` tag through the narrow release App; the tag is what starts
`release.yml` and `docs.yml`, and the release workflow has no manual trigger of its own. Nothing is
tagged if no commit since the last tag would move the version, so a dispatch over chores publishes
nothing.

If a tagged release fails, its commit and tag stay for diagnosis - no GitHub Release, no image
manifest, no moving tag. Fix the source and approve a new dispatch. **Never rewrite a release tag.**
[Verifying a release](https://github.com/telekom/sutura/blob/main/docs/verifying-a-release.md) is
what a consumer of the artifacts reads.

## Conventions that will fail your pull request

- **LF line endings, no trailing whitespace, one final newline.** `just fmt` fixes what is
  mechanically fixable.
- **No em dashes.** Plain hyphens, in prose and in comments. A gate rather than a request, because
  the convention was stated from the start and one shipped anyway.
- **No first-party `unsafe`.** `forbid` in the workspace lint table, so a crate cannot re-allow it
  locally.
- **`#[expect(.., reason = "..")]`, never `#[allow]`**, so a suppression cannot outlive its cause.
- **No file over 1000 lines**, and no exemption under `crates/` or `xtask/` - the only way past it
  is to split the file.
- **No dependency declared and unused.** Declaring one to satisfy a document is what that gate
  exists to stop.
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
is explicit rather than derived, and `cargo xtask check-docs` fails on a page in no nav entry, a nav
entry with no file, or an asset that stopped resolving. `just validate` does **not** render the
site, so `just docs` is owed by any change that touches a doc comment.
[Publishing the docs](https://github.com/telekom/sutura/blob/main/docs/publishing.md) covers the
versioning and the one repository setting it needs.
