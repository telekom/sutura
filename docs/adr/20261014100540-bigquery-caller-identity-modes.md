---
title: BigQuery runs its caller in one of three declared modes
description: An impersonating BigQuery source declares how it runs its caller - the caller's pool principal, the caller's own Google login, or opt-in domain-wide delegation that the deployment vouches for.
---

# BigQuery runs its caller in one of three declared modes

Status: **accepted**. It amends `docs/adr/20261010193211`, whose first amendment points here, and
it reverses the rejection of a deployment-vouched mode for the sources that opt in to mode c.

## The question

`docs/adr/20261010193211` runs every `impersonation-at-source` BigQuery caller as their own pool
principal. Some organisations grant BigQuery to Google accounts, not to pool principals, and their
row access policies name those accounts. How does a caller run as their own Google account?

## Decision

An `impersonation-at-source` `bigquery` source declares exactly one of three blocks, and the block
name is the mode. Two blocks, or none, are refused at load. A `google_login` or `domain_delegation`
block on a shared source, or on a source that is not `bigquery`, is refused at load.

1. **`workload_identity` (mode a)** - unchanged: the caller's own pool principal, as
   `docs/adr/20261010193211` decides.
2. **`google_login` (mode b)** - the caller's own Google login. The caller signs in to Google
   through the identity provider, which keeps the Google token. sutura sends the caller's own
   inbound token to the identity provider's broker token endpoint and gets that caller's Google
   access token back. BigQuery runs the query as the caller's Google account.
3. **`domain_delegation` (mode c)** - one service account that an administrator authorized for
   domain-wide delegation acts as the caller's verified address. BigQuery runs the query as the
   Google account with that address.

**Mode c is opt-in per source, and never a fallback.** No mode falls back to another: a caller
that a mode cannot run is refused.

**Mode b uses Keycloak's Identity Brokering API v2, not the legacy token exchange.** The v2
endpoint is a supported feature that gives the stored token of an external identity provider; it
replaces the internal-to-external use of the legacy exchange, which is a preview feature. The
`broker-token` grant already sends this request. So mode b adds no `requested_issuer` hop, and
sutura takes no dependency on a preview feature.

**A caller with no linked Google account gets a distinct `identity_not_linked` answer code.** The
code is about the caller's own linking state, so the caller can act on it, and it tells nothing
about other callers.

**The scope is a constant, not a key.** The driver runs a query through `jobs.insert`, which
accepts the `https://www.googleapis.com/auth/bigquery` scope and not `bigquery.readonly`. So mode c
asks for the `bigquery` scope, and read-only access comes from IAM: the grants on the caller's
account, not the token's scope.

## The trust shift in mode c

In modes a and b the caller's own credential proves who runs the query. In mode c **the deployment
vouches for the caller**: sutura asserts an address, and Google trusts the delegating account to
assert any address in the domain.

- **Whoever controls a verified email at the identity provider controls that BigQuery identity.**
  sutura takes the address from the verified `email` claim, and only when `email_verified` is
  `true`.
- **`email_verified` is necessary, not sufficient.** The identity provider must not let a user
  edit their own email. If a user can set their email to another person's address, that user runs
  queries as that person. This is a requirement on the identity provider's configuration that sutura
  cannot check.
- **The delegating account can act as any user in the domain.** Its key, or the right to sign as
  it, is as strong as every account in the domain for the scope it was granted. Grant the
  delegation the `bigquery` scope only, and protect the account like the domain's own
  administrator.

## Options considered

- **Mode a only** - not taken. It cannot serve an organisation whose grants name Google accounts.
- **A `requested_issuer` hop on the legacy token exchange for mode b** - not taken. It depends on
  a preview feature that the Identity Brokering API v2 replaces.
- **Domain-wide delegation as the default, or as a fallback when a caller has no linked account**
  - not taken. It would move every caller to the weaker trust model without an operator's decision.
- **A `scope` key** - not taken. Only one scope runs a query, so a key would be a choice with one
  answer.

## Consequences

- Each new mode needs a change to the vendored BigQuery driver: the pinned driver takes no bare
  access token and no delegation subject. The change is proposed upstream first.
- An operator who opts in to mode c owns the identity provider's email policy and the delegating
  account's protection.
- A configuration for a mode is checked at load before sutura runs that mode, and sutura refuses to
  start with a mode it does not run.

## First amendment, 2026-10-14: how mode b reads the broker's answer and the not-linked refusal

**Status of the amendment: accepted** (issue #1439). Mode b is built, with these decisions the
record above does not hold.

- **The Google token is opaque, and its expiry is the identity provider's absolute
  `accessTokenExpiration`.** The broker endpoint returns the response the identity provider
  stored when the caller signed in, so `expires_in` counts from that sign-in and not from this
  request. An answer without `accessTokenExpiration`, or with one at or before now, is refused.
  sutura does not verify the Google token; BigQuery does, at `jobs.insert`.
- **sutura keeps no refresh token.** The stored response carries Google's refresh token and ID
  token. The answer is read into a zeroized secret and decoded into a value that has no field for
  either, so neither reaches a type. Refreshing stays with the identity provider. A static token
  cannot be refreshed during a query, so a job that outlives it fails.
- **The token option and the auth type that selects it are set together, always.** One function
  produces `bigquery.auth_type: access_token` and `bigquery.auth.access_token` as one pair, so no
  job carries the token under another auth type, and none carries the auth type with no token.
- **Not linked is classified from one fixed message, and the message is never kept.** The
  identity provider answers a caller with no linked account with `400 invalid_request` and the
  description `User not associated to identity provider`; the same status and code also mean a
  disabled feature or an unknown provider. sutura compares the description to that literal only
  to choose `identity_not_linked` (`403`), and every other `400 invalid_request` stays
  `identity_unavailable`. The description is not echoed, logged or stored. A changed message
  degrades to `identity_unavailable`: still a refusal, never an answer.
- **A `google_login` source needs a `direct` inbound deployment**, for the reason a delegation
  does: the broker request sends the caller's own inbound token, which only `direct` verifies.

## Second amendment, 2026-10-14: how mode c reads the address and runs the job

**Status of the amendment: accepted** (issue #1439). Mode c is built, with these decisions the
record above does not hold.

- **The address is parsed and never normalised.** sutura keeps `email` as written: at most 254
  bytes of printable ASCII, one `@` between a non-empty local part and a non-empty domain. No case
  fold, no alias and no trimming, so the account a query runs as is the one the identity provider
  named. A value that does not parse, or an `email_verified` that is not the JSON boolean `true`,
  gives no address. The token itself still verifies: the claim is read only by a
  `domain_delegation` source.
- **No address, no query.** A caller without a verified address is refused at a
  `domain_delegation` source. The `sub` and the caller's token are never used in its place.
- **One function sets the three options of a job together**: the delegating account as the
  impersonation target, the `bigquery` scope, and the address as the subject. No auth type is set,
  so the base credential is the deployment's own application default credentials, which need
  `iam.serviceAccounts.signJwt` on the delegating account. sutura's configuration holds no key, but
  the deployment holds a credential that can act as any user in the domain for that scope.
- **Only a `domain_delegation` source accepts a principal switch.** At every other BigQuery source a
  principal is still refused with `NoPrincipalSwitch`.

## Third amendment, 2026-10-15: each job is billed to its caller's project

**Status of the amendment: accepted** (issue #1439). A `bigquery` source has no `billing_project`
and no `dataset`; both keys are refused at load with a message naming the key that replaces them.

- **A caller-run source** (modes a, b and c) names each job's billing project in this order:
  1. the project of the first `billing.by_group` entry whose group the caller's verified `groups`
     claim holds. The list order is the operator's, so a caller in several mapped groups always
     gets the same project, and a group the list does not name adds nothing;
  2. else the one project of every table the query reads;
  3. else the project the caller names in the `Sutura-Bill-To` header or the MCP `bill_to`
     argument, read only when the source sets `billing.caller_may_name: true`. A value that is not
     a billing account name is refused before the question runs: on `/v1` with a 400
     `unusable_bill_to` that names the header, on MCP with an invalid-params error that names
     `bill_to`. A value that is one but is not a project id is refused with `bill_to_not_allowed`
     when this step is reached: a 403 on `/v1`, a refused tool result with that code on MCP;
  4. else the query is refused with `no_billing_project`, and nothing runs.

  A payer named on a source without `billing.caller_may_name: true` is refused with
  `bill_to_not_allowed`, also beside a mapped group or a one-project question: it is never dropped.
  Where the source does take one, steps 1 and 2 come first, so a named payer can be replaced
  without an answer that says so.
- **A shared source bills to its service account's project**, because there the service account is
  the caller. `billing.service_account_project` is required on a shared `bigquery` source and refused
  on a caller-run one; `billing.by_group` and `billing.caller_may_name` are refused on a shared one.
  sutura reads no credential to find that project: the operator declares it.
- **The deployment's own project never pays for a caller-run job.** No step above falls back to it.
- **No location is sent.** BigQuery runs a query job where the tables it reads are.

Why: one source project made every caller need `bigquery.jobs.create` in a project they do not
otherwise hold, and billed one project for every caller's queries.

## Fourth amendment, 2026-10-22: a declared address claim and a domain limit

**Status of the amendment: accepted** (issue #1554). Two issuers that sutura documents never send
`email_verified`: Entra ID, and a gateway that names the user in `sub`. With them mode c refused
every caller.

- **The claim is declared at `security.inbound.address_claim`**, because it is a property of the
  issuer, not of a source. Absent, empty or `email` is the default above: `email`, only beside
  `email_verified: true`. Any other claim is read with no verification flag, so it starts only
  beside `accept_address_claim: true`, and each start logs one `WARN`. The acknowledgement states
  that the issuer verifies the claim and users cannot change it; sutura cannot check either.
- **The declared claim is read from the signed payload a second time**, after the signature
  verified, by a reader that keeps only that one key. The default path deserializes what it did
  before. A claim that occurs more than once, escaped spellings included, gives no address: reading
  either value would choose which one to believe. The address is parsed as the second amendment
  states.
- **`domain_delegation.address_domains` limits the addresses a source runs as**, whatever claim
  they come from. A domain matches whole, ASCII case ignored (RFC 4343), never by suffix. An address
  outside the list is refused with its own refusal, `address_outside_domains`, decided by the
  broker before any credential is minted or exchanged for any source. An empty list is refused at
  load.

The limit: the address parse is the one the second amendment states, so `<a@example.com>` parses.
`address_domains` never matches its domain; without that key it reaches Google as the subject, and
no test here observes what Google answers.

## Fifth amendment, 2026-10-22: mode c asks for `bigquery.readonly` only

**Status of the amendment: accepted** (issue #1553). This replaces the decision **The scope is a
constant, not a key** above, the scope named in **The trust shift in mode c**, and the scope and
the count of options in the second amendment's third point.

- **Mode c asks for `https://www.googleapis.com/auth/bigquery.readonly`, still a constant.** A
  credential that can act as any user in the domain should carry the smallest scope that reads.
  With that scope Google refuses `jobs.insert` and `jobs.cancel`, and accepts `jobs.query`,
  `jobs.get`, `jobs.getQueryResults` and the Storage Read API. A probe measured this on 2026-10-22
  with a service account's own token, not a token from domain-wide delegation.
- **The driver starts a mode c query with `jobs.query`.** `nix/bigquery-adbc-jobs-query.patch`
  adds a database option that does this; sutura sets it only beside the read-only scope. The rows
  still come through the Storage Read API, and `maximumBytesBilled` and `jobTimeoutMs` go on the
  same request.
- **sutura cannot cancel a mode c job.** Google refuses `jobs.cancel` with the read-only scope. When
  sutura stops waiting at the caller's deadline, the job runs on to its end or to `jobTimeoutMs`.
  `maximumBytesBilled` caps its bytes. Nothing in sutura caps its slot time.
- **A statement that writes is refused through `jobs.query` with the read-only scope, for the four
  statements measured.** A second probe on 2026-10-22 sent `CREATE TABLE`, `INSERT`, `UPDATE` and
  `DROP TABLE`. Google refused each with `403 accessDenied`, and a `bigquery`-scope control ran all
  four. Not measured: `DELETE`, `MERGE`, `ALTER`, `LOAD DATA`, `EXPORT DATA`, and any of them with
  a token from domain-wide delegation. At a mode c source sutura sends only the queries it renders.

Why: the full `bigquery` scope let the delegating account start any job as any user in the domain.
