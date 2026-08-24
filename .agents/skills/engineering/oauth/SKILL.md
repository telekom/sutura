---
name: oauth
description: OAuth 2.x and OIDC for this service - validating access tokens, authorizing on claims, and exchanging a token for a downstream identity without downgrading the subject.
---

# OAuth and OIDC here

sutura is a **resource server** that also acts as a **client** on the way down: it validates
an incoming token, then obtains a downstream credential *as the calling subject*. Both halves
have to be right, and the second is where governance is won or lost.

Derived in part from Curity's OAuth developer skills (Apache-2.0) - see `VENDOR.md`.

## Validating an incoming access token

Configuration, from the environment, never hardcoded:

| Setting | Meaning |
| --- | --- |
| `JWKS_URI` | the authorization server's key set |
| `ISSUER` | expected `iss` |
| `AUDIENCE` | this service's identifier, expected in `aud` |
| `ALGORITHM` | the accepted signing algorithm, e.g. `ES256` |

Rules, all of them:

- Verify the signature against the key selected by the token's `kid`.
- **Accept only `ALGORITHM`.** Let the library decide and it will honour the token's own
  `alg` - that is the `alg: none` and RS/HS confusion class.
- Check `exp` and `nbf` against current UTC.
- Check `iss` equals `ISSUER` exactly.
- Check `aud` **contains** `AUDIENCE`. A token minted for another API must not work here;
  this is the check people omit, and it is the one that makes tokens transferable.
- Cache the JWKS in memory keyed by `kid`, fetch on miss, and make it thread-safe. Do not
  fetch per request, and do not refetch on every unknown `kid` without a bound - that is a
  denial-of-service lever handed to the caller.

Keep validation out of business logic. On failure return `401` with a `WWW-Authenticate`
header, `error="invalid_token"`, and a description that does **not** echo token contents. Log
at `warn` with the library's technical reason; log the reason, never the token.

## Authorizing on claims

Two layers, and both are needed:

1. **Coarse - scope.** Missing required scope is `403` with `WWW-Authenticate`,
   `error="insufficient_scope"`, and the required `scope`.
2. **Fine - claims.** Authorize each operation against the subject's claims. Reads are
   *filtered* to what the subject may see, not checked after the fact. A `403` for business
   authorization carries a plain body and **no** `WWW-Authenticate` header - that header means
   "your token is wrong", which here it is not.

Build a claims principal once, at the edge, and pass it inward. Authorization that re-reads
the raw token deep in a call stack is authorization nobody can audit.

## The downstream leg - where this service is different

Every query runs as the calling principal. Concretely:

- `CredentialBroker::credential_for(&RequestContext, ..)` mints per request. There is no
  service account fallback: a leg that cannot run as the subject returns
  `RefusalReason::SourceIdentityUnavailable`. **Downgrading to a service identity is the
  failure, not the recovery** - it silently converts "this user may not see these rows" into
  "here are the rows".
- Use RFC 8693 token exchange for the downstream token, and RFC 8707 `resource` indicators so
  the exchanged token is audience-restricted to the leg it is for. An unrestricted downstream
  token is a bearer token for everything that trusts the issuer.
- Exchange per request, cache narrowly if at all, and key any cache by subject **first**.
  Under row-level security a query-keyed cache is a cross-user leak.
- Record the whole principal chain in the audit sink before returning - including refusals.
  A refusal nobody can see is indistinguishable from a request that never happened.

## Reviewing a change here

| Question | If the answer is no |
| --- | --- |
| Is `aud` checked against this service? | tokens for other APIs are accepted |
| Is the algorithm pinned by config, not by the token? | forgery via `alg` |
| Does every leg run as the subject? | a cross-user data leak, reported as success |
| Is the downstream token audience-restricted? | it is reusable wherever the issuer is trusted |
| Are refusals typed results rather than errors? | they will be caught and turned into a retry |
| Is anything cached keyed by query rather than subject? | a cross-user leak under RLS |

## Never

- Never log, trace, or include a token, assertion, or credential in an error. Credential-shaped
  types are newtypes with a hand-written `Debug`; keep new ones that way and unit-test that the
  secret is absent from `{:?}`.
- Never accept a token without `aud` verification "because the issuer is trusted".
- Never implement JWT verification by hand. Use the pinned library; check its version and
  advisories via `cargo-deny`.
