# Skills

Discovery is a **tree**, not a list, and the reason is token economy: you open three small
files instead of every skill in the repo.

## Protocol

1. Read this file.
2. Pick **one** intent below.
3. Open that group's `README.md`.
4. Open only the `SKILL.md` it routes you to.

Do not open a `SKILL.md` you were not routed to. A skill absent from `skill-router.json` is
non-discoverable by policy - `cargo xtask check-skills` fails if the router and the tree
disagree, in either direction.

Every agent reaches the same tree: `.claude/skills`, `.codex/skills` and `.opencode/skills`
are symlinks to `.agents/skills`. One canonical set, three products.

## Intents

| I need to… | Group |
| --- | --- |
| start a session, or find which skill applies | `agent-system/README.md` |
| write or change Rust in this workspace | `engineering/README.md` |
| cut over-engineering | `engineering/README.md` |
| find out why something fails | `engineering/README.md` |
| validate a token or authorize on claims | `engineering/README.md` |
| obtain a token: flow, PKCE, storage, logout | `engineering/README.md` |
| split dependent changes into reviewable PRs | `git-ops/README.md` |
| decide between approaches, or check my own reasoning | `reasoning/README.md` |

## Groups

| Group | Scope |
| --- | --- |
| `agent-system/` | How skills are discovered, imported and maintained. |
| `engineering/` | Rust in this workspace, debugging, and both halves of OAuth 2.x / OIDC. |
| `git-ops/` | Stacked branches: dependent changes as separate reviewable PRs. |
| `reasoning/` | Structured reasoning: making the argument explicit before acting on it. |

`AGENTS.md` remains the root of trust for repo invariants. A skill refines *how* to work
inside them; it never overrides them.
