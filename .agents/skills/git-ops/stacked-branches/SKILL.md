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
st sync                # rebase the whole stack onto its updated base
st submit              # open or update a PR per branch
st ls                  # what the stack looks like now
```

`st sync` after the base moves is the whole point. Doing it with `git rebase` per branch is
where mistakes happen, and the mistakes are the expensive kind - a force-push that drops a
commit from the middle of a chain.

## Rules here

- **One reviewable idea per branch.** If a branch needs "and" to describe it, split it.
- **Never force-push a shared branch unless asked** (`AGENTS.md`). `st sync` rewrites history
  by design, so on a branch someone else has, ask first.
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
not a replacement for `st sync`: it describes a stack, it does not rebase one.
