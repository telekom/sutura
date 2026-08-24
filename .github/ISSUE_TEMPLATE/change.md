---
name: Change proposal
about: A capability, a change to the tool surface, or a change to an invariant
labels: proposal
---

## What

<!-- One sentence. The capability or the change, not the implementation. -->

## Why

<!-- The problem. What is impossible or unsafe today, and for whom. -->

## How

<!-- Sketch, not a design document. Which crate, which port, which adapter. If it needs a new
port trait or widens an existing one, say so here - that is the part with consequences. -->

## Does it touch the governance boundary?

<!-- Delete what does not apply. Any "yes" makes this an architecture decision and it needs an
ADR before code, per AGENTS.md. -->

- [ ] Adds or widens a tool input
- [ ] Adds a failure mode (must be a `RefusalReason`, not an `Err`)
- [ ] Reads from the catalog at request time
- [ ] Adds a second execution leg
- [ ] Stores or forwards rows
- [ ] Changes a definition or its anchor

## Which mechanism will enforce it?

<!-- A type, a lint, a hook, a gate, or a generated contract. "Review will catch it" is not a
mechanism. If the honest answer is that nothing will, say so - that is a real answer and it
belongs in the discussion, not hidden. -->
