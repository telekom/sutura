# Engineering

Open only the `SKILL.md` that matches the task.

| Skill | Entry point | When to open |
| --- | --- | --- |
| `rust` | `rust/SKILL.md` | Writing or changing Rust here: the lint table, the panic ban, feature gating, what the gates will reject. |
| `ms-rust` | `ms-rust/SKILL.md` | General Rust discipline (Microsoft Pragmatic Rust Guidelines), after `rust`. Load only the topic files the task touches. |
| `debugging` | `debugging/SKILL.md` | A failing test, a red gate, a build error, or behaviour you cannot explain. |
| `oauth` | `oauth/SKILL.md` | **Receiving** a token: validation, claims authorization, and the downstream token-exchange leg. |
| `oauth-flows` | `oauth-flows/SKILL.md` | **Obtaining** a token: grant selection, PKCE, state/nonce, redirect URIs, storage, refresh rotation, logout. |

Two or more may apply - changing token validation is `oauth` **and** `rust`. Open both; do
not guess which one wins.
