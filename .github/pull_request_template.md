## Review snapshot

<!-- Short. It should tell a reviewer how to spend attention in five seconds. -->

| Size | Risk | Focus | Evidence |
| --- | --- | --- | --- |
|  |  |  |  |

## What changed?

<!-- 1-3 bullets. Name the behaviour, the port, the invariant or the boundary that changed. -->

-

## Why?

<!-- The problem this solves. Link the ADR if there is one. -->

-

## How to review

<!-- Files in the order they should be read. Say which diffs to skip. -->

1.
2.

Generated or mechanical, skip:

-

## Validation

<!-- Exact commands and whether they passed. Say what you did NOT run. "Should work" is not a result. -->

| Check | Result |
| --- | --- |
| `gates` |  |
| `ship-check` |  |

## Test causality

<!-- A changed test must be red against base behaviour and green with the change. A test that
passes both ways proves nothing. `just causality` checks it; where impl and test share a file it
cannot, and then the evidence goes here: the command, the failure before the fix, the pass after.
EXIT 3 IS "I MEASURED NOTHING" and is neither a pass nor a violation - the base tree did not
build, or the base run named no failure, which a harness move and a changed public signature a
kept-at-HEAD test file calls both reach legitimately. A green CI step over exit 3 is not
evidence: say which substitute you used - a mutation run, or the gate scoped per commit - and
paste the verdict line, never the step's colour. Delete this section only if no test changed. -->

-

## Invariants

<!-- Which mechanism would fail if this change were wrong? See the table in AGENTS.md. If the
answer is "nothing mechanical", say so - that is a review question, not something to certify. -->

-

## Risk / notes

<!-- Breaking changes, migrations, a widened tool surface, a new dependency, or "None". -->

-
