---
title: BigQuery grants the caller's workload identity principal directly
description: A BigQuery source that runs as the caller federates the caller's own token to a pool principal and stops there. No service account per caller, no second hop, no subject map.
---

# BigQuery grants the caller's workload identity principal directly

Status: **accepted**. It replaces `docs/adr/0032`'s option (1) with its option (3), which that
record rejected: see its fourth amendment.

## The question

`impersonation-at-source` on BigQuery federated the caller's assertion at Google STS and then
impersonated a service account that a per-source map declared for that subject. Every caller
needed a service account, a `roles/iam.workloadIdentityUser` binding and a map entry. Callers are
people at an identity provider, not service accounts. Which identity does BigQuery see?

## Decision

The caller's own principal in the workload identity pool. In this order:

1. sutura verifies the caller's token.
2. With `delegation`, sutura exchanges it at the identity provider for a token the pool accepts.
3. The driver sends that token to Google STS. STS accepts it only when the pool provider trusts
   its issuer and its signature and audience are valid, so the token proves who the caller is.
4. STS gives a federated token for
   `principal://iam.googleapis.com/projects/<number>/locations/global/workloadIdentityPools/<pool>/subject/<subject>`.
5. BigQuery runs the query as that principal. Its grants and row access policies decide what the
   caller reads.

**Each caller must exist in BigQuery, by design**: as their own pool principal, with their own
grants and row access policies. One principal per caller is the normal case, and the query runs
with that principal's own grants; `shared-service-user` is the one-identity mode. Callers may share
a dataset, because BigQuery applies each caller's own permissions. No step uses a service account. The credential document names no
`service_account_impersonation_url`. `workload_identity.impersonate` is deleted, and a
configuration that still writes it is refused as an unknown key.

**No subject allow-list.** A federating source presents every verified caller's own token. The
pool provider's attribute conditions and BigQuery IAM decide who reaches data. A caller with no
grant is refused by BigQuery, and sutura reports it as `source_refused`: the vendored driver gives
BigQuery's REST `403` access denial the status `Unauthorized`, as it already does for a job's
`accessDenied` (`VENDOR.md`). A `403` for a rate or a quota stays a failure a retry may answer. An
anonymous request is refused before any source is asked, and no question runs as the deployment.

## Options considered

- **A service account per caller, behind a declared map** (`docs/adr/0032` option (1)) - removed.
  It put a second identity, a service account, between the caller and BigQuery, and sutura's own
  subject-to-account map decided which account each caller became.
- **Grant the pool principal directly** - taken. Google recommends it for workload identity
  federation, and it needs no account, no binding and no map.
- **Keep a subject allow-list beside direct grants** - not taken. It duplicates a decision the
  pool and BigQuery already make, and a second list is a second thing to keep true.

## Consequences

- An operator grants `roles/bigquery.jobUser`, `roles/bigquery.readSessionUser` and
  `roles/bigquery.dataViewer`, and names row access policy grantees, as the pool principal.
- A principal is an opaque subject string in grants and logs, not a nameable account.
- `SESSION_USER()` reads the principal, so no address a deployment holds predicts it.
