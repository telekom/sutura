---
title: BigQuery pool exchange in process
description: sutura exchanges a caller's assertion at Google's token service itself and hands the BigQuery driver the access token, replacing the loopback credential source the driver fetched it from. Reverses one point of ADR 0018's eighth amendment (github.com/telekom/sutura#828).
---

# BigQuery pool exchange in process

Status: **accepted**, 2026-11-02. This is the owner's decision. Built: an `impersonation-at-source`
`bigquery` source with a `workload_identity` pool exchanges each subject's assertion in this process.
No hosted run has measured it yet, so `docs/where-identity-is-proven.md` records the venue as `wired`.

## The question

[0018](0018-what-the-bigquery-wire-is-built-from.md)'s sixth amendment handed the driver an
`external_account` credential document whose `credential_source` was a loopback URL in this process.
The driver's Go auth library fetched the caller's assertion from it and performed the RFC 8693
exchange at Google's token service. Its eighth amendment recorded the result: *Google's token
service - not this process - performs the exchange.*

Federation across nodes (`github.com/telekom/sutura#828`) needs every worker to read with a
credential the coordinator already holds. Forwarding the caller's assertion to each worker would
spread it to every node and multiply the token-service calls. What should a worker be handed?

## Decision

**sutura performs the exchange and hands the driver the access token.** The domain has a
`PoolExchange` port beside `DelegationExchange`. `sutura-http-client`'s `GoogleSts` implements it,
and `sutura-cli` composes it for a `ThroughPool` source. The adapter calls the port before the
driver loads and sets `bigquery.auth_type=access_token` and `bigquery.auth.access_token`. These are
the two options a `google_login` source already uses, added by `nix/bigquery-adbc-caller-credentials.patch`.
A failed exchange is a refusal, and no connection opens.

The request is the one the driver's library sent. It was read from the pinned
`cloud.google.com/go/auth@v0.24.1`, the version the driver's module graph resolves:

- **Form fields.** `audience`, `grant_type`, `requested_token_type`, `subject_token_type`,
  `subject_token` and `scope` (`credentials/internal/stsexchange/sts_exchange.go`).
- **No client authentication and no `options` member.** The library sends neither without a client
  id and secret or a workforce project, and a source here declares none.
- **The `BigQuery` client's own scope, not read-only.** A federated token without an impersonation
  hop asks for the client's scopes (`externalaccount.go`), and the `BigQuery` client prepends its
  scope.

The answer is shape-checked:

- `issued_token_type` is the access-token type;
- `token_type` is `Bearer`;
- `access_token` is non-empty;
- `expires_in` is a positive integer.

The token is opaque and is never decoded. A refusal keeps a registered OAuth error code and nothing
else from the body, so the assertion never reaches a message.

`DelegationExchange` is not reused. Its client decodes the answer as a JWT and checks `aud`, and
Google's federated token is not a JWT.

**The loopback source is deleted**, with its listener, its two 128-bit request secrets, its tests and
the `getrandom` dependency. So is the cost the sixth amendment's addendum stated: any local process
could connect, and the serial accept loop could delay the driver's fetch.

## What it reverses, and what it does not

**Reversed:** the eighth amendment's *not this process*. That sentence was true of who initiates the
call. Now sutura initiates it.

**Unchanged:**

- **Who verifies the assertion.** Google's token service verifies it against the declared pool.
  sutura only carries it there.
- **The principal.** The token is the subject's own pool principal. No service account is
  impersonated and no second hop is asked for.
- **The other modes.** `GoogleLogin`, `DomainDelegation`, `Disabled`, the boot path's `Transport`
  arm and the jobs-query patch are not touched.

## Costs and limits

- **No mid-query refresh.** The driver never refreshes an `access_token`, so a job that runs past the
  token's `expires_in` fails mid-query. The loopback document let the driver's cached provider fetch
  again; this does not, and no refresh is built.
- **One more outbound call in sutura's own budget.** The exchange runs over the shared outbound agent
  under the delegation exchange's read bounds, inside the question's request timeout.
- **The answer is checked for shape, not verified.** Google issued it over TLS, and `BigQuery`
  verifies the token.
- **The refusal classification is not measured.** A `401`, a `403` or `invalid_grant` reaches the
  caller as `source_refused`, read off RFC 6749 section 5.2 rather than from a recorded token-service
  answer.
- **Storage Read is not measured.** No local run shows the Storage Read API accepting a token asked
  for with the `BigQuery` scope alone.
- **The exchanger can be missing.** A `ThroughPool` transport built without one refuses every
  subject as uncovered rather than running the question as the deployment. No type forbids building
  it that way.
- **The venue is unmeasured.** Run `38116387739` measured the deleted loopback path, so the BigQuery
  identity venue is `wired` until this path has a run of its own.
