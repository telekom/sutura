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
