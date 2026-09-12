---
name: Bug report
about: Something behaves differently from what it should
labels: bug
---

## What happens

<!-- Observed behaviour. Paste the actual error, not a paraphrase - a message naming a lint or
flag that looks correct is often an encoding bug, and that is invisible unless quoted. -->

## What should happen

## Reproduction

<!-- The narrowest command that shows it. If it only reproduces in CI, say so and link the run:
the usual differences are no `.git` in the Nix sandbox, no network in a Nix build, and
`--all-features` in the gates but not in a local inner loop. -->

```
```

## Environment

|                   |                                       |
| ----------------- | ------------------------------------- |
| Version or commit |                                       |
| How it was built  | `nix build` / `cargo` / the container |
| Platform          |                                       |

## Which mechanism should have caught this?

<!-- The useful question, and often the real bug. If an invariant in AGENTS.md covers this and
did not fire, the missing or broken check is what to fix - not just the symptom. If nothing
covers it, say that: the fix probably includes a new gate. -->
