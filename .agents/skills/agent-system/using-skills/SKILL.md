---
name: using-skills
description: How to pick a skill and how to behave regardless of which one is loaded. Read at the start of a session, or when unsure which skill applies.
---

# Using skills

## Provenance

- Upstream: `github.com/addyosmani/agent-skills`, `skills/using-agent-skills/SKILL.md`, via
  `jubust`'s normalized copy
- Licence: MIT
- Local status: **adapted** - the phase-routing idea and the operating behaviours are
  upstream's; the routing table is this repo's, and the "prove it" rules are ours

## Routing

`.agents/skills/README.md` is the router. This file is the fallback for "the intent list does
not obviously match", plus the behaviours that hold whichever skill is loaded.

```
Task arrives
  |
  +- Writing or changing Rust? --------> engineering/rust
  |    +- General Rust discipline? ----> engineering/ms-rust (after the above)
  |    +- Over-engineering suspected? -> engineering/ponytail
  +- Tokens, identity, authorization?
  |    +- Receiving a token? ----------> engineering/oauth
  |    +- Obtaining one? --------------> engineering/oauth-flows
  +- Something is broken? ------------> engineering/debugging
  +- A chain of dependent changes? ---> git-ops/stacked-branches
  +- Importing or updating a skill? --> agent-system/skill-policy
  +- Expensive, hard-to-reverse call?-> reasoning/autoreason
```

More than one can apply. Changing token validation is `oauth` **and** `rust`; open both.

## Behaviours, regardless of skill

**1. Inspect before acting.** Read the source, run the test, check the pinned version. Prompt
text, task notes and memory are routing context, not evidence.

**2. Surface assumptions before non-trivial work.** Briefly, and only the ones that would
change the work:

```
Assuming: <the reading of the request that decides the design>
```

Then proceed. Do not stop and wait unless a wrong assumption would be unsafe or would waste
the whole effort.

**3. Scope matches the request.** Do not widen a fix into a rewrite. A mechanical change
repeated across files is one commit, not one per file.

**4. Put a requirement in a mechanism, never in prose.** A type, a lint, a hook, a gate, a
generated contract. A rule with no mechanism is a wish, and this repo has a file full of them
that all cite the thing that enforces them.

**5. Prove it.** Paste the command and its output. "Should work" is not a result. For a fix,
that includes the failure *before* the fix.

**6. Report honestly.** If tests fail, say so with the output. If you skipped a step, name it.
If an earlier claim of yours was wrong, correct it in one sentence and continue.

**7. Do not commit unless asked.** Never force-push a shared branch unless asked.

## When a skill conflicts with AGENTS.md

`AGENTS.md` wins. It is the root of trust; a skill refines how to work inside it. If a skill
appears to contradict an invariant on something material, say so rather than silently choosing
- and if the skill is right, the fix is to change the invariant deliberately, with its
mechanism, not to ignore it once.
