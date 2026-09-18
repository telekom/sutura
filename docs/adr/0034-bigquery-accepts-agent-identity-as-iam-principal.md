---
title: BigQuery accepts a Google-managed agent identity as an IAM principal
description: Google Cloud IAM supports agent identities as first-class allow-policy principals; the agent-identity auth page lists BigQuery as an example SERVICE for `add-iam-policy-binding`. The WIF path stays for GitHub Actions CI because agent identity is unavailable outside supported Google Cloud runtimes.
---

# BigQuery accepts a Google-managed agent identity as an IAM principal

Status: **accepted.** Evidence gathered 2026-09-18 from Google Cloud documentation.

## Decision 1: BigQuery accepts agent identity as a principal (quotable)

Google's IAM documentation establishes agent identities as first-class principals for allow policies. This is the authoritative passage:

> **Principal identifiers for allow policies** (IAM Principal Identifiers page):

> | Principal type | Identifier |
> | --- | --- |
> | **Agent identity** | `principal://TRUST_DOMAIN/resources/SERVICE/RESOURCE_PATH` |

> Examples:
> - Vertex AI Agent Engine (organization): `principal://agents.global.org-123456789012.system.id.goog/resources/aiplatform/projects/9876543210/locations/us-central1/reasoningEngines/my-test-agent`
> - Gemini Enterprise: `principal://agents.global.org-123456789012.system.id.goog/resources/discoveryengine/projects/9876543210/locations/global/collections/default_collection/engines/my-test-agent`

Source: [Principal identifiers](https://docs.cloud.google.com/iam/docs/principal-identifiers), last updated 2026-09-16.

The Agent Identity overview further states:

> Agent Identity is fully integrated with Google's policy systems like IAM, Principal Access Boundary (PAB), and VPC Service Controls, which allow for enhanced security and governance.

Source: [Agent Identity overview](https://docs.cloud.google.com/iam/docs/agent-identity-overview), section "Security and governance", last updated 2026-09-16.

Critically, the page on authenticating with an agent's own authority uses BigQuery as an **explicit example** of a Google Cloud service that accepts agent identity bindings:

> To grant an agent access to a resource, run the following command:
> ```
> gcloud SERVICE add-iam-policy-binding RESOURCE_NAME \
>     --member="PRINCIPAL_IDENTIFIER" \
>     --role="ROLE"
> ```
> Replace the following:
> - *SERVICE*: The Google Cloud service (for example, `storage` or `bigquery`).

Source: [Authenticate using an agent's own authority](https://docs.cloud.google.com/iam/docs/auth-agent-own-identity), section "Grant access to agents", last updated 2026-09-16.

This is the closest Google comes to naming BigQuery as a service that accepts agent identities. The phrase "for example, `storage` or `bigquery`" in a section explicitly about granting agent identity access to Google Cloud services means: **BigQuery accepts agent identity as an IAM principal for allow policies.**

**Limitation:** Google does not have a dedicated BigQuery page stating "agent identities are supported on BigQuery" — the only explicit BigQuery mention is the generic example in the agent auth page. No BigQuery IAM documentation page cross-references agent identities. The claim rests on the general IAM agent identity framework plus the concrete BigQuery example in the auth guide. A reader seeking a BigQuery-specific page confirming this will find none, but the general framework covers all Google Cloud services that implement allow policies, and BigQuery does.

### What this does NOT decide

This record does not decide **which Google Cloud service hosts the agent**. The following services explicitly support Agent Identity (Agent Identity overview, section "The following services support Agent Identity"):

- Gemini Enterprise Agent Platform Runtime (*Agent Runtime*)
- Gemini Enterprise
- Cloud Run

An agent identity is **tied to the resource where the agent is hosted** (the SPIFFE ID encodes the resource path). If the agent runs on an unsupported runtime, it does not have an agent identity to present — not a BigQuery limitation, but a platform limitation.

## Decision 2: CI vs. production

### 2a. CI (GitHub Actions runner) — agent identity is unavailable

An agent identity is a Google-managed cryptographic identity that is **assigned when you deploy an agent to a supported Google Cloud service** (Vertex AI Agent Engine, Gemini Enterprise, or Cloud Run). A GitHub Actions runner is **not** a supported runtime:

> When you deploy an agent to Google Cloud, ensure that Agent Identity is enabled.
> If you're deploying to Agent Runtime, use the `identity_type=AGENT_IDENTITY` flag.

Source: [Authenticate using an agent's own authority](https://docs.cloud.google.com/iam/docs/auth-agent-own-identity), section "Deploy the agent".

A GitHub Actions runner has no Google Cloud-attached SPIFFE ID, no auto-provisioned X.509 certificate, and no metadata server that can issue agent-bound access tokens. The agent identity credential surface lives entirely on Google Cloud infrastructure.

**Therefore: a GitHub Actions runner cannot hold or present an agent identity. The WIF (workload identity federation) path remains the correct mechanism for CI.** WIF (via identity pools and OIDC tokens) is specifically designed for non-Google-Cloud workloads to authenticate to Google Cloud — exactly the GitHub Actions → GCP pattern.

### 2b. Production (agent on a supported runtime) — agent identity is available

If the agent runs on Vertex AI Agent Engine (or Gemini Enterprise, or Cloud Run with agent-platform features), it has an agent identity by default:

> When you deploy an agent, Google Cloud assigns it a unique SPIFFE identity and an X.509 certificate. Each X.509 certificate is valid for 24 hours, and Google Cloud automatically keeps it current to maintain security.

Source: [Agent Identity overview](https://docs.cloud.google.com/iam/docs/agent-identity-overview), section "How Agent Identity works".

In this case, BigQuery accepts that agent identity as a principal via the `principal://` identifier pattern established in the IAM allow policy framework.

## Decision 3: The WIF path stays; agent identity is a production-side concern only

The current sutura path for BigQuery (WIF via STS exchange in `sutura-exec-bigquery/src/sts.rs`) authenticates **as a workload identity** — a service account obtained by exchanging a federated token for a Google Cloud access token. This is the correct path for **all** deployment scenarios:

| Deployment | Mechanism | Agent identity needed? |
| --- | --- | --- |
| GitHub Actions CI | WIF pool + OIDC token → STS → service account token | No — agent identity is unavailable on non-Google-Cloud runtimes |
| Cloud Run (agent) | Agent identity (SPIFFE + X.509) → access token | No — the agent identity itself is the credential; WIF is not needed |
| Vertex AI Agent Engine (agent) | Agent identity (SPIFFE + X.509) → access token | No — the agent identity itself is the credential; WIF is not needed |
| Compute Engine (non-agent) | Service account (attached to VM) → access token | No — this is the legacy service-account path |

The agent identity does not replace WIF; it is an **alternative credential mechanism available on different deployment surfaces**. The fundamental distinction:

- **WIF** answers: "How does a non-Google-Cloud workload authenticate to Google Cloud?"
- **Agent identity** answers: "How does an agent hosted on Google Cloud authenticate to other Google Cloud services?"

These are non-competing paths that serve different deployment geometries. The current WIF/STS path (keyless, no service-account key material) already handles the CI case. For production deployments on a supported agent runtime, the agent identity would be the simpler path (no external exchange needed), but that is a **future option**, not a current replacement.

**What sutura already does is correct for its current scope.** The `SharedServiceUser` credential path via `sutura-exec-bigquery/src/sts.rs` is the right mechanism for WIF-based deployments (including CI). If sutura ever deploys directly as a Vertex AI Agent, the agent identity would be a separate credential acquisition path on the runtime side — not a change to the exchange logic.

## Limits

1. **No BigQuery-specific agent identity page exists.** Google has not published a BigQuery page that explicitly lists "agent identities supported." The claim rests on: (a) the general IAM agent identity framework covering all Google Cloud allow-policy services, and (b) the explicit `bigquery` example in the agent auth page. This is a documentation gap, not a functional gap.

2. **Agent identity is only available on three Google Cloud services.** Vertex AI Agent Engine, Gemini Enterprise, and Cloud Run (with agent-platform features). A deployment on any other surface (including GitHub Actions, bare Compute Engine, or external infrastructure) cannot use agent identity.

3. **Agent identity changes WHO asks, not WHOSE identity the source executes as.** The agent identity is a workload identity tied to the agent's lifecycle — it is functionally analogous to a service account or a WIF-exchanged credential. The current WIF path (keyless, no keys on disk) and the agent identity path (keyless, SPIFFE-bound) have the same property: neither holds long-lived secret material. The honest gain in switching from WIF to agent identity is attestation/lifecycle binding (per-agent SPIFFE ID, X.509 auto-rotation, PAB support), not secret removal.

4. **sutura-exec-bigquery/src/sts.rs is untouched.** The exchange hop in this crate verifies #376 against real STS and is explicitly out of scope for this decision. This ADR does not change any code.

## Sources consulted

| Page | URL | Key claim |
| --- | --- | --- |
| Agent Identity overview | https://docs.cloud.google.com/iam/docs/agent-identity-overview | Agent identity integrated with IAM; list of supported services; SPIFFE identity format; mTLS credential binding |
| Authenticate using an agent's own authority | https://docs.cloud.google.com/iam/docs/auth-agent-own-identity | Agent identity for Google Cloud services; BigQuery named as example SERVICE for `add-iam-policy-binding` |
| Principal identifiers | https://docs.cloud.google.com/iam/docs/principal-identifiers | Agent identity as first-class allow-policy principal (`principal://` format) |
| BigQuery control access to resources with IAM | https://docs.cloud.google.com/bigquery/docs/control-access-to-resources-iam | BigQuery IAM; no mention of agent identities (documentation gap) |
| BigQuery authentication | https://docs.cloud.google.com/bigquery/docs/authentication | Standard auth paths (service accounts, ADC, OAuth); no mention of agent identities (documentation gap) |
| Workforce identity federation (contrast) | https://cloud.google.com/iam/docs/workforce-identity-federation | Not agent identity — about workforce pools for human users |
| Workload identity federation (contrast) | https://cloud.google.com/iam/docs/workload-identity-federation | Not agent identity — about workload pools for non-Google-Cloud workloads (CI, Kubernetes) |

Note: The "federation support matrix" page originally linked from issue #760 concerns workload and workforce identity federation, **not** agent identities. Agent identity is a distinct mechanism from both.
