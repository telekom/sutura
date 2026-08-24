# Vendored and derived material

Third-party material adapted into this repo. "Inspired by" is not a licence position, so each
entry names the upstream, the commit, the date and what changed locally.

The `cargo-deny` licence gate covers dependencies. This file covers everything else —
documentation, guidance, fixtures — which no dependency check can see.

| Local path | Upstream | Licence | Commit | Date | Local changes |
| --- | --- | --- | --- | --- | --- |
| `.agents/skills/engineering/oauth/SKILL.md` | `github.com/curityio/oauth-developer-skills` | Apache-2.0 | `d411f195ab0d03bc68de6b16504036b6f7533244` | 2026-05-27 | Rewritten, not copied. Retains the upstream's JWT-validation rule set (pin the algorithm from configuration, verify `iss` and `aud`, cache JWKS by `kid`, RFC 6750 error shapes) and its two-layer scope-then-claims authorization model. Added: the downstream token-exchange leg (RFC 8693 / RFC 8707), the refuse-rather-than-downgrade rule, and the subject-keyed cache constraint — none of which are upstream, since it addresses a plain resource server rather than one that also acts as a client. Dropped the Curity-product-specific setup and deployment instructions. |

| `.agents/skills/engineering/oauth-flows/SKILL.md` | Public OAuth/OIDC agent skills, notably `github.com/0xfurai/claude-code-subagents` (`agents/oauth-oidc-expert.md`) | MIT | fetched from `main` | 2026-08-24 | Rewritten, not copied. Retains the grant-selection table, the PKCE-always rule, the `state`/`nonce`/exact-redirect-URI checks, the token-storage split and refresh-token rotation with reuse detection. Added: the `iss` callback check for provider mix-up, RFC 8707 resource indicators, the reason `state` and `nonce` are not interchangeable, and the negative-case test list. Dropped the provider-specific integration walkthroughs (Auth0, Okta, Keycloak). |

No source code is vendored at present. When it is, record the upstream commit here and keep
the upstream licence text alongside the code, not only in this table.
