---
name: ponytail
description: Forces the laziest solution that actually works - YAGNI, stdlib before custom code, one line before fifty. Use when the user says ponytail, lazy mode, simplest solution, yagni, or complains about over-engineering, bloat or an unnecessary dependency.
---

# Ponytail

## Provenance

- Upstream: `github.com/DietrichGebert/ponytail`, `.openclaw/skills/ponytail/SKILL.md`
- Licence: MIT. Commit `2ed6c52c9d7e5e56942508591085fd45dea277d3`, 2026-08-07
- Local status: **adapted** - the ladder and persistence model are upstream's; the
  interaction with this repo's invariants is not, and is the section that matters most here

Lazy means efficient, not careless. The best code is the code never written.

## Persistence

Active for every response once switched on. Off only on "stop ponytail" or "normal mode".
Default intensity **full**; `lite` and `ultra` exist.

## The ladder

Stop at the first rung that holds:

1. **Does this need to exist at all?** A speculative need is a no. Say so in one line.
2. **Does the standard library do it?** Use it.
3. **Does a platform or language feature cover it?** A type invariant over a runtime check, a
   DB constraint over application code, a `NewType` over a validator.
4. **Does an already-present dependency solve it?** Use that. Never add a dependency for what
   a few lines can do - here a new dependency also costs a `cargo-deny` licence review and an
   `unused-deps` entry.
5. **Can it be one line?** One line.
6. **Only then:** the minimum code that works.

Two rungs both work? Take the higher one and move on. The first lazy solution that works is
the right one.

## Where laziness stops, in this repo

Ponytail is about *volume of code*, not about coverage of the mechanisms. These are not
over-engineering and do not get skipped:

| Not negotiable | Why it is not bloat |
| --- | --- |
| The typed refusal surface | `ToolOutcome::Refusal` is what makes an uncertified question unrepresentable. Collapsing it to an error is not simpler, it is a different guarantee |
| Per-request identity | A service-account shortcut is fewer lines and a cross-user data leak |
| A gate for a new rule | A rule in prose costs nothing and enforces nothing. Adding the check *is* the lazy option, because the alternative is enforcing it by memory forever |
| Red-before-green for a test | A test that passes both ways is less than no test |
| A test for a new gate | A gate nobody has seen fail is not known to work |

Conversely, ponytail is right about: speculative traits with one implementor, a builder for a
two-field struct, a config knob nobody asked for, an abstraction layer over one call, a
dependency for string formatting, and a wrapper crate that only re-exports.

## Verdict format

When declining work, one line and move on:

```
Skipping the <thing>: <reason it is not needed>. Doing <the smaller thing> instead.
```

Do not write a design document explaining what you did not build.
