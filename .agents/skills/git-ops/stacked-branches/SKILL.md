---
name: stacked-branches
description: Ship a chain of dependent changes as separate reviewable PRs with stax, instead of one branch that grows until nobody can review it.
---

# Stacked branches

This plan is a chain of dependent changes by construction: a port, then an adapter, then the
transport that uses it. One branch holding all of it is unreviewable; three branches rebased
by hand is a full-time job. `stax` (`st`) automates the rebasing.

## Configuration, and why it is in the repo

`stax.toml` at the repo root overlays `~/.config/stax/config.toml`, setting only the forge
and a few conventions. A global config is per-machine, and a machine configured for a
different forge would otherwise submit this repo's stacks there. The overlay applies only
while `STAX_CONFIG_DIR` is unset - set it and you have taken over.

## The loop

```bash
st create <name>       # branch off the current one, entering the stack
# ... commit ...
st create <next>       # stack another on top
st refresh             # sync trunk, restack the stack, submit the updates
st ss                  # open or update a PR per branch
st ls                  # what the stack looks like now
st sync                # pull trunk and delete merged branches - NOT a restack
```

`st refresh` after the base moves is the whole point. Doing it with `git rebase` per branch is
where mistakes happen, and the mistakes are the expensive kind - a force-push that drops a commit
from the middle of a chain.

**`sync` is not the restack command**, which this file previously said it was. Checked against
stax's own `--help`: `sync` is "pull trunk, delete merged branches"; `refresh` is "sync trunk,
restack current stack, then submit updates"; and the lower-level form is `st stack restack`.
Running `sync` and expecting a rebase leaves the stack exactly as stale as it was.

**No version number is written here on purpose.** A skill that dates its quotes goes stale the
next time the pin moves, and it goes stale SILENTLY, because nothing in this tree reads a version
out of prose. The pin is `nix/stax.nix` and the dev shell echoes what it resolved to on entry, so
`st --help` is the answer to "is this still true" - run it rather than trusting this paragraph.

## Rules here

- **One reviewable idea per branch.** If a branch needs "and" to describe it, split it.
- **Do not run `st update` or `st skills update`.** Both are real subcommands. `update` is
  "upgrade the stax CLI and check for skill updates" - it would replace a nix-provided binary
  with one nothing here pins, on one machine only, and nix is the only pin for a tool whose
  version changes what it reports (`AGENTS.md`). A bump is an edit to `nix/stax.nix`.
  `skills update` is "download the latest skills from GitHub and update installed skill files",
  and a skill file that lands in this tree without a `skill-router.json` entry makes the router
  and the tree disagree - which is exactly what `cargo xtask check-skills` fails on.
- **Never force-push unless asked** (`AGENTS.md`). `st refresh` rewrites history and submits it,
  so obtain explicit authorization before rewriting any published branch, even one you alone own.
  Without it, ask rather than merging `main` as a workaround.
- **Bottom-up.** Land the base before the branches on top; merging out of order recreates by
  hand exactly the conflicts the tool exists to avoid.
- **`ship-check` per branch, not once for the stack.** Each PR is reviewed alone, so each has
  to be green alone. It judges the committed diff from the merge base, which for a stacked
  branch is the branch below - which is what you want.
- Conventional-commit subjects: the `commit-msg` hook enforces it, and stack tooling derives
  PR titles from them.

## gh-stack

`gh-stack` is also in the shell. It reads an existing stack and produces the PR descriptions
and cross-links, which is useful when the stack was built by hand rather than by `st`. It is
not a replacement for `st refresh`: it describes a stack, it does not restack one.
