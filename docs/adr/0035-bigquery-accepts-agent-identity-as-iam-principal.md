---
title: WIF stays the load-bearing mechanism for BigQuery; agent identity is an optional addition
description: sutura's primary use case is a human asking through an AI harness, and that path executes as the asking subject through the WIF/STS exchange in every deployment scenario, GitHub Actions CI included. Google Cloud IAM separately accepts a Google-managed agent identity as a first-class allow-policy principal - the agent-identity auth page lists BigQuery as an example SERVICE for `add-iam-policy-binding` - but that is an optional credential available on three Google Cloud runtimes, and it changes WHO asks rather than whose identity the source executes as.
---

# WIF stays the load-bearing mechanism for BigQuery; agent identity is an optional addition

Status: **accepted.** Evidence gathered 2026-09-18 from Google Cloud documentation.

## Why WIF is load-bearing, and what agent identity does not change

sutura's primary use case is a human asking a question through an AI harness (an MCP client), and
leg 2 for that path - a source executing AS the asking subject - already runs on WIF: the STS +
`iamcredentials` exchange in `sutura-exec-bigquery/src/sts.rs`, per source, in every deployment
scenario. That is the load-bearing mechanism, not a CI-only fallback.

Agent identity, examined below, does not compete for that role. **It changes WHO asks, not WHOSE
identity the source executes as.** An agent identity is a workload identity tied to the agent's own
lifecycle - functionally analogous to a service account or a WIF-exchanged credential - so it
occupies the same slot as WIF rather than filling one WIF is missing. It is an optional addition,
available only on the three Google Cloud runtimes below, never the direction of travel for the
asking-subject path. The rest of this record establishes that BigQuery accepts it as a principal at
all, which is a real finding, and keeps it scoped to what it is.

## Decision 1: BigQuery accepts agent identity as a principal (quotable)

Google's IAM documentation establishes agent identities as first-class principals for allow policies. This is the authoritative passage:

> **Principal identifiers for allow policies** (IAM Principal Identifiers page):

> | Principal type     | Identifier                                                 |
> | ------------------ | ---------------------------------------------------------- |
> | **Agent identity** | `principal://TRUST_DOMAIN/resources/SERVICE/RESOURCE_PATH` |

> Examples:
>
> - Vertex AI Agent Engine (organization): `principal://agents.global.org-123456789012.system.id.goog/resources/aiplatform/projects/9876543210/locations/us-central1/reasoningEngines/my-test-agent`
> - Gemini Enterprise: `principal://agents.global.org-123456789012.system.id.goog/resources/discoveryengine/projects/9876543210/locations/global/collections/default_collection/engines/my-test-agent`

Source: [Principal identifiers](https://docs.cloud.google.com/iam/docs/principal-identifiers), last updated 2026-09-16.

The Agent Identity overview further states:

> Agent Identity is fully integrated with Google's policy systems like IAM, Principal Access Boundary (PAB), and VPC Service Controls, which allow for enhanced security and governance.

Source: [Agent Identity overview](https://docs.cloud.google.com/iam/docs/agent-identity-overview), section "Security and governance", last updated 2026-09-16.

Critically, the page on authenticating with an agent's own authority uses BigQuery as an **explicit example** of a Google Cloud service that accepts agent identity bindings:

> To grant an agent access to a resource, run the following command:
>
> ```
> gcloud SERVICE add-iam-policy-binding RESOURCE_NAME \
>     --member="PRINCIPAL_IDENTIFIER" \
>     --role="ROLE"
> ```
>
> Replace the following:
>
> - *SERVICE*: The Google Cloud service (for example, `storage` or `bigquery`).

Source: [Authenticate using an agent's own authority](https://docs.cloud.google.com/iam/docs/auth-agent-own-identity), section "Grant access to agents", last updated 2026-09-16.

This is the closest Google comes to naming BigQuery as a service that accepts agent identities. The phrase "for example, `storage` or `bigquery`" in a section explicitly about granting agent identity access to Google Cloud services means: **BigQuery accepts agent identity as an IAM principal for allow policies.**

**Limitation:** Google does not have a dedicated BigQuery page stating "agent identities are supported on BigQuery" - the only explicit BigQuery mention is the generic example in the agent auth page. No BigQuery IAM documentation page cross-references agent identities. The claim rests on the general IAM agent identity framework plus the concrete BigQuery example in the auth guide. A reader seeking a BigQuery-specific page confirming this will find none, but the general framework covers all Google Cloud services that implement allow policies, and BigQuery does.

### What this does NOT decide

This record does not decide **which Google Cloud service hosts the agent**. The following services explicitly support Agent Identity (Agent Identity overview, section "The following services support Agent Identity"):

- Gemini Enterprise Agent Platform Runtime (*Agent Runtime*)
- Gemini Enterprise
- Cloud Run

An agent identity is **tied to the resource where the agent is hosted** (the SPIFFE ID encodes the resource path). If the agent runs on an unsupported runtime, it does not have an agent identity to present - not a BigQuery limitation, but a platform limitation.

## Decision 2: where agent identity exists, and what the asking-subject path needs

The split below is **not** CI-versus-production. A GitHub Actions runner and a supported Google
Cloud runtime both run the same asking-subject path on WIF; what differs by runtime is only
whether an agent identity exists at all as an *alternative credential for the agent's own calls*.
Agent identity is never a substitute for the per-subject exchange leg 2 needs, on any runtime.

### 2a. CI (GitHub Actions runner) - agent identity is unavailable

An agent identity is a Google-managed cryptographic identity that is **assigned when you deploy an agent to a supported Google Cloud service** (Vertex AI Agent Engine, Gemini Enterprise, or Cloud Run). A GitHub Actions runner is **not** a supported runtime:

> When you deploy an agent to Google Cloud, ensure that Agent Identity is enabled.
> If you're deploying to Agent Runtime, use the `identity_type=AGENT_IDENTITY` flag.

Source: [Authenticate using an agent's own authority](https://docs.cloud.google.com/iam/docs/auth-agent-own-identity), section "Deploy the agent".

A GitHub Actions runner has no Google Cloud-attached SPIFFE ID, no auto-provisioned X.509 certificate, and no metadata server that can issue agent-bound access tokens. The agent identity credential surface lives entirely on Google Cloud infrastructure.

**Therefore: a GitHub Actions runner cannot hold or present an agent identity. The WIF (workload identity federation) path remains the correct mechanism for CI.** WIF (via identity pools and OIDC tokens) is specifically designed for non-Google-Cloud workloads to authenticate to Google Cloud - exactly the GitHub Actions → GCP pattern.

### 2b. A supported Google Cloud runtime - agent identity is available for the agent's own calls

If the agent runs on Vertex AI Agent Engine (or Gemini Enterprise, or Cloud Run with agent-platform features), it has an agent identity by default:

> When you deploy an agent, Google Cloud assigns it a unique SPIFFE identity and an X.509 certificate. Each X.509 certificate is valid for 24 hours, and Google Cloud automatically keeps it current to maintain security.

Source: [Agent Identity overview](https://docs.cloud.google.com/iam/docs/agent-identity-overview), section "How Agent Identity works".

In this case, BigQuery accepts that agent identity as a principal via the `principal://` identifier pattern established in the IAM allow policy framework. That principal is the **agent's own**, not the asking subject's - it authenticates the agent to Google Cloud, and does not answer who a BigQuery query then executes as.

## Decision 3: WIF is the load-bearing mechanism everywhere; agent identity is optional where it exists

The current sutura path for BigQuery (WIF via STS exchange in `sutura-exec-bigquery/src/sts.rs`) authenticates **as a workload identity** - a service account obtained by exchanging a federated token for a Google Cloud access token. This is the correct path for **all** deployment scenarios, including the primary one - a human asking through an AI harness:

| Deployment                     | Mechanism the deployment authenticates ITSELF with  | Touches the asking-subject exchange (leg 2)?                                     |
| ------------------------------ | --------------------------------------------------- | -------------------------------------------------------------------------------- |
| GitHub Actions CI              | WIF pool + OIDC token → STS → service account token | No - orthogonal to leg 2, which runs WIF/STS regardless                          |
| Cloud Run (agent)              | Agent identity (SPIFFE + X.509) → access token      | No - agent identity is the deployment's own credential; leg 2 still runs WIF/STS |
| Vertex AI Agent Engine (agent) | Agent identity (SPIFFE + X.509) → access token      | No - agent identity is the deployment's own credential; leg 2 still runs WIF/STS |
| Compute Engine (non-agent)     | Service account (attached to VM) → access token     | No - legacy service-account path, also orthogonal to leg 2                       |

The agent identity does not replace WIF; it is an **alternative credential mechanism available on different deployment surfaces**. The fundamental distinction:

- **WIF** answers: "How does a non-Google-Cloud workload authenticate to Google Cloud?"
- **Agent identity** answers: "How does an agent hosted on Google Cloud authenticate to other Google Cloud services?"

These are non-competing paths that serve different deployment geometries, and neither is about the
asking-subject exchange this product's leg 2 needs. The current WIF/STS path (keyless, no
service-account key material) already handles every deployment, CI included. If sutura ever runs
on a supported agent runtime, agent identity could replace WIF as the way the **deployment itself**
authenticates to Google Cloud - a separate credential-acquisition detail on the runtime side - but
that is an optional addition available there, never a change to the per-subject exchange in
`sutura-exec-bigquery/src/sts.rs`. The asking-subject path still runs on WIF regardless.

**What sutura already does is correct for its current scope, and for the primary use case.** The
`SharedServiceUser` credential path via `sutura-exec-bigquery/src/sts.rs` is the right mechanism for
a human asking through an AI harness, on every deployment surface including CI. Agent identity does
not change that; it is orthogonal to it.

## Limits

1. **No BigQuery-specific agent identity page exists.** Google has not published a BigQuery page that explicitly lists "agent identities supported." The claim rests on: (a) the general IAM agent identity framework covering all Google Cloud allow-policy services, and (b) the explicit `bigquery` example in the agent auth page. This is a documentation gap, not a functional gap.

2. **Agent identity is only available on three Google Cloud services.** Vertex AI Agent Engine, Gemini Enterprise, and Cloud Run (with agent-platform features). A deployment on any other surface (including GitHub Actions, bare Compute Engine, or external infrastructure) cannot use agent identity.

3. **Agent identity changes WHO asks, not WHOSE identity the source executes as.** The agent identity is a workload identity tied to the agent's lifecycle - it is functionally analogous to a service account or a WIF-exchanged credential. The current WIF path (keyless, no keys on disk) and the agent identity path (keyless, SPIFFE-bound) have the same property: neither holds long-lived secret material. The honest gain in switching from WIF to agent identity is attestation/lifecycle binding (per-agent SPIFFE ID, X.509 auto-rotation, PAB support), not secret removal.

4. **sutura-exec-bigquery/src/sts.rs is untouched.** The exchange hop in this crate verifies #376 against real STS and is explicitly out of scope for this decision. This ADR does not change any code.

## Sources consulted

| Page                                          | URL                                                                         | Key claim                                                                                                       |
| --------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| Agent Identity overview                       | https://docs.cloud.google.com/iam/docs/agent-identity-overview              | Agent identity integrated with IAM; list of supported services; SPIFFE identity format; mTLS credential binding |
| Authenticate using an agent's own authority   | https://docs.cloud.google.com/iam/docs/auth-agent-own-identity              | Agent identity for Google Cloud services; BigQuery named as example SERVICE for `add-iam-policy-binding`        |
| Principal identifiers                         | https://docs.cloud.google.com/iam/docs/principal-identifiers                | Agent identity as first-class allow-policy principal (`principal://` format)                                    |
| BigQuery control access to resources with IAM | https://docs.cloud.google.com/bigquery/docs/control-access-to-resources-iam | BigQuery IAM; no mention of agent identities (documentation gap)                                                |
| BigQuery authentication                       | https://docs.cloud.google.com/bigquery/docs/authentication                  | Standard auth paths (service accounts, ADC, OAuth); no mention of agent identities (documentation gap)          |
| Workforce identity federation (contrast)      | https://cloud.google.com/iam/docs/workforce-identity-federation             | Not agent identity - about workforce pools for human users                                                      |
| Workload identity federation (contrast)       | https://cloud.google.com/iam/docs/workload-identity-federation              | Not agent identity - about workload pools for non-Google-Cloud workloads (CI, Kubernetes)                       |

Note: The "federation support matrix" page originally linked from issue #760 concerns workload and workforce identity federation, **not** agent identities. Agent identity is a distinct mechanism from both.
