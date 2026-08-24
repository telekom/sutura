---
name: skill-policy
description: When an external skill belongs in this repo, how to record its provenance, and how to keep an adaptation maintainable. Use when importing, updating or removing a skill.
---

# Skill policy

## Provenance

- Upstream: `jubust`'s `agent-system/jubust-external-skill-policy` (private repo)
- Local status: **adapted** - the classification and provenance rules are upstream's idea; the
  inclusion bar, the two-tier layout and the enforcement are ours

Skills are context that gets loaded. A skill nobody needs is a permanent tax on every session
that touches its group, so the bar for keeping one in-repo is higher than "it is good".

## Two tiers

| Tier | Path | Discoverable? |
| --- | --- | --- |
| Active | `.agents/skills/<group>/<name>/SKILL.md` | Yes - routed via `skill-router.json` |
| Library | `.agents/skill-library/<group>/<name>/SKILL.md` | No - opened only when an active skill or a human points at it |

`cargo xtask check-skills` enforces the active tier in both directions: a route to a missing
skill fails, and a `SKILL.md` in no route fails. **An unrouted active skill is a rule nobody
reads while believing it is in force**, which is worse than not having it.

The library tier is deliberately outside that check. Its cost is only paid when opened, so the
bar is lower - but it still needs provenance.

## Classification

Every imported skill declares one, in a `## Provenance` section in the body:

| Status | Means |
| --- | --- |
| `mirror` | imported as-is |
| `normalized` | behaviour preserved, format changed for local validation |
| `adapted` | content or workflow changed for this repo |
| `local` | written here, no upstream |

State the upstream repo, the upstream path, the licence, the commit or date, and what changed
locally. Keep it in the body, not in frontmatter: `check-skills` allows only `name` and
`description` there, and adding keys would mean changing the gate.

If a copy has diverged so far that our behaviour is the source of truth, call it `adapted`.
Do not describe it as a mirror it no longer is.

## Keep it in-repo only if

- it supports recurring work in this repo, or
- it encodes a local adaptation that would otherwise be lost, or
- a gate, hook or workflow depends on it.

Otherwise prefer a user-local skill. Specifically **do not** import: generic reference
material an agent already knows, guidance for a stack this repo does not use, or anything
whose assets are large relative to how often it will be opened.

Ask the harder question too: does this skill overlap an existing one? Two skills covering the
same ground means routing has to guess, and the wrong one gets loaded. Merge instead.

## Importing

1. Read the upstream in full. A skill is instructions to an agent, so an unreviewed import is
   an unreviewed instruction.
2. Strip what does not apply here. Guidance for another repo's layout is worse than absent -
   it will be followed.
3. Add the `## Provenance` section.
4. Register it: active tier goes in `skill-router.json` **and** the group `README.md`; library
   tier goes in `.agents/skill-library/README.md`.
5. Note derived documentation in `VENDOR.md` when the licence requires attribution.
6. `cargo xtask check-skills`, then `prek run --files <touched>`.

## Updating

Compare against upstream, keep the local provenance note, re-apply the local adaptation
deliberately, then ask again whether it still belongs. A skill that no longer earns its place
gets deleted - a stale skill is read as current.

Where upstream is generated (see `ms-rust`), refresh through its generator and never hand-edit
the output: the recorded hash is what makes "is this current" answerable, and an edit makes it
a lie.
