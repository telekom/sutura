# Contributing

The contributor guide is [`CONTRIBUTING.md`](https://github.com/telekom/sutura/blob/main/CONTRIBUTING.md)
in the repository root, where GitHub looks for it when somebody opens a pull request. It is
linked rather than copied here: two copies of the same prose drift, and the copy that has to be
right is the one beside the code.

It covers the three routes to a working environment, what `just setup` does, the nightly-versus-
stable split that makes a bare `cargo clippy` misleading, which gates run on commit, on push and
in CI, the conventional-commit subject the `commit-msg` hook enforces, the rule that a changed
test must be red against the base behaviour and green with the change, stacked pull requests with
`stax`, and the conventions that fail a pull request.

To set a machine up in the first place, start with [Getting started](getting-started.md). For
what is guaranteed and by which mechanism, read `AGENTS.md` in the repository.
