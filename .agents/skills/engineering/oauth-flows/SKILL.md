---
name: oauth-flows
description: Choosing and implementing an OAuth 2.x / OIDC flow - grant selection, PKCE, state and nonce, redirect URIs, token storage, refresh rotation, discovery and logout.
---

# Choosing and implementing a flow

The companion to `oauth/SKILL.md`. That one is the **resource server** - validating an
incoming token and authorizing on its claims. This one is the **client** side: obtaining a
token in the first place, and the six decisions that go with it.

Derived in part from public OAuth/OIDC agent skills - see `VENDOR.md`.

## Work in this order

Skipping to step 2 is how the wrong flow gets chosen and then defended.

1. **Actors and client type.** Who authenticates, and can the client keep a secret? A native
   app, a browser SPA and a server-side service are three different answers.
2. **Flow and PKCE.**
3. **Tokens and validation.**
4. **Scopes and consent.**
5. **Session and logout.**
6. **Operational hardening.**

## 1–2. Grant selection

| Situation | Grant |
| --- | --- |
| Web app, mobile app, native app, SPA, CLI | **Authorization Code + PKCE** |
| Service to service, no user present | **Client Credentials** |
| Input-constrained device (TV, headless) | **Device Authorization** |
| Acting on behalf of a subject downstream | **Token Exchange** (RFC 8693) - see `oauth/SKILL.md` |
| - | **Implicit**: do not. Tokens in the URL fragment, no refresh, deprecated |
| - | **Resource Owner Password**: only a legacy system with no alternative, and record why |

**PKCE on every Authorization Code flow, including confidential clients.** It costs nothing
and removes the authorization-code interception class outright. `S256`, never `plain`.

## 3. The request and callback

Four checks, and each one is load-bearing:

- **`state`** - always sent, random, single-use, bound to the user's session and verified on
  callback. Without it the callback accepts a code the user never requested: CSRF.
- **`nonce`** - always sent for OIDC, and verified inside the ID token. `state` protects the
  callback; `nonce` protects the token. They are not interchangeable.
- **Redirect URI** - registered and matched **exactly**. No wildcards, no prefix matching, no
  open path suffix. This is the single most exploited misconfiguration in OAuth, because a
  loose match turns the authorization server into a code-delivery service for an attacker.
- **`iss`** on the callback where the provider supports it - it defeats mix-up attacks when
  more than one provider is configured.

## 4. Scopes

Request the least that works, per operation rather than one union at login. A token minted
with everything is a token whose compromise costs everything. Where the API supports it, add
RFC 8707 `resource` indicators so the token is audience-restricted to the API it is for.

## 5. Tokens, storage and rotation

| Token | Where it goes |
| --- | --- |
| Access token | memory, or a `Secure` `HttpOnly` `SameSite` cookie. Short-lived |
| Refresh token | server side, encrypted at rest. **Never** in a browser-accessible store |
| ID token | consumed and discarded; it is proof of authentication, not an API credential |

- **Never `localStorage` or `sessionStorage`.** Any XSS becomes credential theft, permanently.
- **Rotate refresh tokens**, and treat reuse of a rotated token as theft: revoke the whole
  family and require re-authentication. Rotation without reuse detection is bookkeeping.
- Cache the discovery document and JWKS with a sane TTL. Do not fetch per request; do not
  cache forever, or a key rotation becomes an outage.
- Use **OIDC Discovery** for endpoints instead of hardcoding them, so a provider can rotate
  its own URLs.

## 6. Logout

Revoke, do not merely forget. Clearing local state leaves a valid token alive for its full
lifetime. Call the revocation endpoint for the refresh token, clear the session cookie, and
use RP-initiated logout where the provider offers it if the IdP session must end too.

## Never

- Never write your own token validation or crypto. Use the pinned, maintained library.
- Never skip exact redirect-URI matching, including in a development configuration - dev
  configuration is what gets copied to production.
- Never omit `state`, and never reuse one.
- Never log a token, a code, or a `code_verifier`.
- Never accept an ID token as an API access token. Different audience, different purpose.

## Testing it

Prove the negative cases, not only the happy path: a tampered `state`, a replayed code, an
expired token, a rotated-then-reused refresh token, a token for the wrong audience, and a
redirect URI that differs by one character. Each should be refused, and refused as a typed
result rather than an unhandled error.
