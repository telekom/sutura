# Vendored and derived material

Third-party material adapted into this repo. "Inspired by" is not a licence position, so each
entry names the upstream, the commit, the date and what changed locally.

The `cargo-deny` licence gate covers dependencies. This file covers everything else -
documentation, guidance, fixtures - which no dependency check can see.

| Local path | Upstream | Licence | Commit | Date | Local changes |
| --- | --- | --- | --- | --- | --- |
| `.agents/skills/engineering/oauth/SKILL.md` | `github.com/curityio/oauth-developer-skills` | Apache-2.0 | `d411f195ab0d03bc68de6b16504036b6f7533244` | 2026-05-27 | Rewritten, not copied. Retains the upstream's JWT-validation rule set (pin the algorithm from configuration, verify `iss` and `aud`, cache JWKS by `kid`, RFC 6750 error shapes) and its two-layer scope-then-claims authorization model. Added: the downstream token-exchange leg (RFC 8693 / RFC 8707), the refuse-rather-than-downgrade rule, and the subject-keyed cache constraint - none of which are upstream, since it addresses a plain resource server rather than one that also acts as a client. Dropped the Curity-product-specific setup and deployment instructions. |

| `.agents/skills/engineering/oauth-flows/SKILL.md` | Public OAuth/OIDC agent skills, notably `github.com/0xfurai/claude-code-subagents` (`agents/oauth-oidc-expert.md`) | MIT | fetched from `main` | 2026-08-24 | Rewritten, not copied. Retains the grant-selection table, the PKCE-always rule, the `state`/`nonce`/exact-redirect-URI checks, the token-storage split and refresh-token rotation with reuse detection. Added: the `iss` callback check for provider mix-up, RFC 8707 resource indicators, the reason `state` and `nonce` are not interchangeable, and the negative-case test list. Dropped the provider-specific integration walkthroughs (Auth0, Okta, Keycloak). |

| `.agents/skills/engineering/ms-rust/[0-9][0-9]_*.md` | `microsoft.github.io/rust-guidelines` (`agents/all.txt`) | MIT | content sha256 `c9f1ac1c` | 2026-08-24 | **Generated, not written.** `generate.py` splits the upstream file by section and records the sha256, so "is this current" has an answer. Verbatim text; the em-dash sweep and other repo-wide edits skip these files deliberately. Section names are read from the source rather than hardcoded, because upstream reorganises - `Safety` became `Correctness`, and `Macros` and `Project` are new since the snapshot this was taken from. `SKILL.md` and `README.md` here are ours: the routing table, the precedence rules, and the two places where sutura deliberately departs (`M-APP-ERROR`, `M-TARGET-CPU`). |

| `.agents/skills/engineering/ponytail/SKILL.md`, `.claude/hooks/ponytail-session-start.sh` | `github.com/DietrichGebert/ponytail` | MIT | `2ed6c52c9d7e5e56942508591085fd45dea277d3` | 2026-08-07 | **adapted.** The ladder and the persistence model are upstream's. Added: where laziness stops in this repo, since ponytail is about volume of code and the governance mechanisms are not volume. The activation hook is a shell reimplementation of upstream's Node one - a Rust repo should not acquire a Node dependency to run an agent hook - and it reads the ruleset from the skill file so there is one copy. OpenCode uses upstream's own plugin via `opencode.json`, which it supports properly. |
| `.agents/skills/agent-system/using-skills/SKILL.md` | `github.com/addyosmani/agent-skills` (`skills/using-agent-skills`) | MIT | via a normalized private copy | 2026-08-24 | **adapted.** Phase routing and the operating behaviours are upstream's; the routing table is this repo's, and the prove-it rules are ours. |
| `.agents/skills/agent-system/skill-policy/SKILL.md` | a private repo's external-skill policy | - | - | 2026-08-24 | **adapted.** Classification and provenance rules are the upstream idea; the two-tier layout, the inclusion bar and the `check-skills` enforcement are ours. |
| `.github/pull_request_template.md`, `.github/ISSUE_TEMPLATE/*` | a private repo's PR template | - | - | 2026-08-24 | **adapted.** The review-snapshot / what / why / how-to-review / validation shape is the upstream idea. Dropped its visual-evidence machinery (no UI here); added the test-causality and invariants sections, which are this repo's distinctive requirements. |

No source code is vendored at present. When it is, record the upstream commit here and keep
the upstream licence text alongside the code, not only in this table.
