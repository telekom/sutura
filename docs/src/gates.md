# The gates

The gates live in one binary, `xtask`, rather than in a script per check: one thing to
install, one language to review, and they are unit-tested by `cargo test --workspace` like
any other code. A gate with no test is a gate nobody has seen fail.

Run one directly:

```bash
cargo xtask <task>              # inside the dev shell
cargo run -q -p xtask -- <task> # anywhere with cargo
```

`cargo xtask --help` prints the same list. If a task below is missing from that output, the
output is right and this page is stale - `cargo xtask check-guidance` fails on prose naming
a task that no longer exists, so it should not stay wrong for long.

## What each one rejects

| Task | Rejects |
| --- | --- |
| `check-boundaries` | A framework dependency reaching `sutura-domain` - tokio, axum, rmcp, datafusion, arrow, or anything else that makes the domain crate expensive to compile and hard to test |
| `max-lines` | Any file over 1000 lines. Generated and vendored output can be exempted in `.max-lines-ignore`; a pattern under `crates/` or `xtask/` cannot, and the gate fails on such a pattern rather than honouring it |
| `unused-deps` | A declared dependency nothing references, and a `[workspace.dependencies]` entry nobody inherits. An entry nothing inherits pins nothing |
| `line-endings` | CRLF in a text file. Nix keeps a carriage return inside an `''...''` string, where it becomes part of a shell argument and produces an error naming a flag that looks byte-identical to the correct one |
| `text-hygiene` | Merge-conflict markers, trailing whitespace, a missing or duplicated final newline, and a tracked file over 512 kB. `--fix` repairs everything except a conflict marker, which needs a judgement |
| `check-skills` | Disagreement between `.agents/skills/skill-router.json` and the skill tree, in either direction: a route to a skill that does not exist, and a skill no route reaches. Also a frontmatter name that does not match its directory, and an imported skill whose hash no longer matches the lock |
| `check-guidance` | Prose that no longer describes this repo: a forbidden phrase, a version contradicting the pin it names, a `cargo xtask` task that was renamed or deleted, and a dead `.agents/` path in backticks |
| `check-docs` | A chapter listed in `docs/src/SUMMARY.md` with no file behind it, and a page under `docs/src/` that `SUMMARY.md` never links to. The second direction is the one that matters: an unreachable page is read by nobody while looking published |
| `commit-msg` | A commit subject that is not a conventional commit. The hook passes the message file |
| `test-causality` | A changed test that is not red against the base behaviour and green with the change. A test that passes both ways proves nothing and is worse than no test, because it looks like coverage. `--since <ref>` |

## Deciding what a diff requires

Three tasks classify rather than reject. They exist so a prose-only change does not pay for
a release build, and they **fail open**: an unmapped path, a bad base ref or an empty diff
all run everything and say why. The expensive failure is a new directory being skipped
silently, not a wasted CI minute.

| Task | Answers |
| --- | --- |
| `classify` | What does this diff require? `--since <ref>`, or explicit paths. Emits the flags CI gates its steps on |
| `changed-packages` | Which cargo packages own the given `.rs` paths? |
| `check-changed` | `cargo check`, narrowed to those packages. A path belonging to no package widens to `--workspace` rather than checking less than was asked |

## Where they run

The same binary runs in all four places, so they cannot drift.

| Entry point | What it is |
| --- | --- |
| `hygiene` | The dev-shell script: the cheap structural gates, grouped so a 1200-line file fails in seconds rather than after the test suite |
| `gates` | `hygiene`, plus fmt, clippy, tests and `cargo deny` |
| `ship-check` | The finishing sequence over the committed branch diff: the hooks, the gates' own unit tests, and the causality proof |
| `prek run --all-files` | The hooks, configured in `.pre-commit-config.yaml` and tiered by cost - fast checks on commit, tests and `cargo deny` on push |
| `nix build .#checks.x86_64-linux.hygiene` | What CI runs. It needs `nix` and nothing else: CI does not enter the dev shell |

Two checks are deliberately not xtask tasks. `cargo deny` fetches the RustSec advisory
database, so it is a flake app (`nix run .#deny`) rather than a sandboxed check that could
only fail or pass while auditing nothing. Workflow static analysis runs from pixi, as
`zizmor` with `--frozen`, because it ships as a conda package and needs no Rust build.
