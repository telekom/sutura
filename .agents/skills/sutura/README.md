# sutura

This codebase's own reference material. `AGENTS.md` carries the rules that apply to every session;
these carry the depth, so it does not have to.

Open only the `SKILL.md` that matches the task.

| Skill | Entry point | When to open |
| --- | --- | --- |
| `invariants` | `invariants/SKILL.md` | Changing any mechanism, or about to claim something is enforced. Each guarantee beside its mechanism and, in the column that matters, the limit it does **not** reach. |
| `query-surface` | `query-surface/SKILL.md` | The tool surface, a plan, a dialect, a refusal, or the catalog read path - which mechanism fails, and which changes have none. Also what is built and not wired. |
| `identity` | `identity/SKILL.md` | Tokens, credentials, postures, brokers, provenance, audit records. Says exactly where impersonation stops. |
| `crate-map` | `crate-map/SKILL.md` | The rules a crate is subject to by its prefix, why a driver is a dev-dependency, and why a networked adapter must hide behind a default-off feature. |
| `dependencies` | `dependencies/SKILL.md` | Bumping, patching, vendoring, or resolving a version conflict. |
| `secure-by-design` | `secure-by-design/SKILL.md` | Shaping a parse, an error boundary, or an allocation on the query path - the reasoning under the type rules. |
| `gates` | `gates/SKILL.md` | Working out what a green run covered, why a check passed when it should not have, or why a local cargo line disagrees with CI. |

Two or more often apply: changing a refusal is `query-surface` **and** `invariants`. Open both.

For the *mechanics* of Rust here - the mistake tables with a **Caught by** column - open
`engineering/rust`. `secure-by-design` is the argument underneath it, not a replacement.
