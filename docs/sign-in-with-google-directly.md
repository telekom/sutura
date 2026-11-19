---
title: Sign in with Google directly
description: Use a dedicated Google OAuth client and caller-owned read-only access tokens without a server-side refresh-token store.
---

# Sign in with Google directly

This optional mode admits an opaque Google access token at one MCP mount and presents that same
request's token to BigQuery. A dedicated OAuth client binds admission to this deployment. sutura
receives no refresh token and cannot renew a job's credential. Real MCP-client sign-in and Google
account execution through this served mode remain unmeasured; fixture policy and adapter cells do
not establish them.

## Register the client

Create an OAuth consent screen for your organization with an **internal** audience. Register a
separate OAuth client used only for this deployment. Configure the exact redirect URIs required by
each MCP client; do not reuse another application's OAuth client.

Request `openid`, `email` and `https://www.googleapis.com/auth/bigquery.readonly`. Google does not
offer dynamic client registration: configure the pre-registered client ID and, where required,
client secret in the MCP client. An installed-app client's secret is not confidential. Keep the
Google client's secret in the MCP client; sutura needs only its client ID.

Two client behaviours remain unmeasured: whether Google accepts the RFC 8707 `resource` parameter
sent by the client, and whether the client requests `access_type=offline` and obtains a refresh
token. Without client-side renewal the user signs in again when the access token expires. sutura
stores no refresh token, irrespective of the client's behaviour.

## Configure admission and the source

```yaml
security:
  inbound:
    - id: google
      path: /google/mcp
      mode: direct
      verifier: google_tokeninfo
      resource: https://sutura.example.com/google/mcp
      authorization_server: https://accounts.google.com
      client_id: dedicated.apps.googleusercontent.com
      hosted_domains: [example.com]
      required_scopes: [https://www.googleapis.com/auth/bigquery.readonly]
sources:
  warehouse:
    kind: bigquery
    posture: impersonation-at-source
    max_bytes_billed: 1073741824
    admitted_by: [google]
    caller_google_token:
      acknowledged_because: the dedicated OAuth client is used only by this deployment
```

`hosted_domains` is optional. When present, the tokeninfo `hd` must name a listed domain; an email
suffix alone does not satisfy it. All declared `required_scopes` must be present. Both `aud` and
`azp` must equal the dedicated `client_id`, and admission requires a positive remaining lifetime,
a subject and a verified email. The caller's identity is `(verifier id, sub)`; records mask email.
An email rename does not create another caller identity.

A successful check of BigQuery's read-only scope maps to catalog reads, metric questions and raw
SQL reads. Raw SQL still requires the existing operator enablement and source restrictions. This
map never grants remote-leg execution, and arbitrary sutura scope text in tokeninfo grants
nothing. The Google grant does not override source admissions or catalog audience restrictions.
With several verifiers, sources must explicitly declare `admitted_by`. With a sole verifier,
omitting `admitted_by` admits that verifier under the existing single-verifier rule. The mapped
operations can therefore reach other sources, including acknowledged shared sources;
BigQuery's grants protect only the BigQuery leg, not another source's data.

The source refuses a missing or empty acknowledgement and any non-Google-tokeninfo admitting
verifier. Startup logs one warning naming this explicit token-forwarding opt-in. Existing billing
selection applies; no payer falls back to the deployment's project.

## Transport and expiry limits

Tokeninfo defaults to `https://oauth2.googleapis.com/tokeninfo`. Set `tokeninfo_endpoint` on the
verifier, or process-wide `google_endpoints.tokeninfo`, for a declared private Google endpoint.
Endpoint parsing requires HTTPS and a `googleapis.com` host without credentials, query or fragment.
The request sends the token in a POST form body, refuses redirects, has a ten-second timeout and
reads at most 16 KiB. `security.outbound` anchors and optional client identity are loaded at
startup for tokeninfo; they do not rotate on this path.

Only successful checked identities are cached, under the token's SHA-256 digest, with at most
2048 entries per verifier and no entry beyond the token's remaining lifetime. Refusals and
unavailable tokeninfo responses are not cached. Revocation can therefore remain unnoticed until a
positive entry expires; replicas have separate caches. Tokeninfo failure answers
`503 identity_unavailable`, without revealing the request or response body.

BigQuery receives `access_token`, the caller's token and the `jobs.query` route. No credential file,
service-account impersonation or refresh token is added. A job that outlives the token can fail.
BigQuery's grants, row policies and policy tags on that Google account decide its data access;
these checks do not prove that a particular deployment's account has the intended grants.

## The explicit MCP exception

MCP authorization forbids accepting tokens for other audiences and forwarding client tokens to an
upstream API. This opt-in deliberately forwards a Google token. Exact `aud` and `azp` binding to a
dedicated client constrains admission, but does not turn the Google token into an audience-bound
sutura token or make this pattern generally MCP compliant. The token remains usable at Google's
APIs for its consented scopes until expiry. Leaking it exposes that caller's granted rights.

Use this only when the dedicated-client constraint and token-forwarding exception are accepted.
The acknowledgement records that decision; it cannot verify the client's exclusive use. Prefer
workload identity or brokered Google login where their identity and token-storage constraints fit.
