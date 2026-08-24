# Skills

Discovery is a **tree**, not a list, and the reason is token economy: you open three small
files instead of every skill in the repo.

## Protocol

1. Read this file.
2. Pick **one** intent below.
3. Open that group's `README.md`.
4. Open only the `SKILL.md` it routes you to.

Do not open a `SKILL.md` you were not routed to. A skill absent from `skill-router.json` is
non-discoverable by policy — `cargo xtask check-skills` fails if the router and the tree
disagree, in either direction.

## Intents

| I need to… | Group |
| --- | --- |
| write or change Rust in this workspace | `engineering/README.md` |
| find out why something fails | `engineering/README.md` |
| touch tokens, identity, or authorization | `engineering/README.md` |
| decide between approaches, or check my own reasoning | `reasoning/README.md` |

## Groups

| Group | Scope |
| --- | --- |
| `engineering/` | Rust in this workspace, debugging, OAuth 2.x / OIDC and token exchange. |
| `reasoning/` | Structured reasoning: making the argument explicit before acting on it. |

`AGENTS.md` remains the root of trust for repo invariants. A skill refines *how* to work
inside them; it never overrides them.
