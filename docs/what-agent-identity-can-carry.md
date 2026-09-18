---
title: What an agent identity can carry for this deployment's BigQuery credential - and the decision that nothing changes
description: Issue 760 evaluated whether the deployment-held machine-to-machine credential behind SharedServiceUser should become a Google agent identity. Read against Google's own pages, an agent identity's only documented consumer is Vertex AI Agent Engine, not the BigQuery data APIs; the identity-federation support matrix - the page that says which services accept federated identities - is silent on agent identities. So there is no documented basis to claim BigQuery accepts one, none on offer for an external runner, and the federated path stays.
---

# What an agent identity can carry for this deployment's BigQuery credential

Status: an **evaluation**, and the deliverable of issue #760. The question was whether the
machine-to-machine credential this deployment holds for BigQuery - the one that sits behind
`sutura_domain::identity::credential::Presented::SharedServiceUser`, *"the deployment's own identity
for this source, which is not the asker's"* - should become a Google **agent identity** rather than a
federated service account. This record is the answer, read off Google's own documentation rather than
reasoned about, and the answer is **no, on the documented surface it cannot, so the federated path
stays.**

## What this is NOT, restated

An agent identity is a **workload** identity. It is tied to an *agent's* lifecycle and says which
**machine** is asking; it does not represent, impersonate or carry a **human** end user. It therefore
does not advance leg 2 - *a source executing as the asking subject* - at all. This evaluation is only
about the deployment's own credential, and it is the same distinction the issue drew:
`AGENTS.md`'s leg 2 belongs to #378 and #376, not to any change to `SharedServiceUser`.

## What was read, and what each source actually says

Two Google pages, both read on 2026-09-18 rather than quoted from memory.

**1. Identify the primitive - `docs.cloud.google.com/iam/docs/workload-identities`, "Identities for
workloads".** Under the heading *Agent identities* it says, verbatim:

> "An agent identity is a Google-managed identity for agentic workloads. An agent identity is attested
> and tied to the lifecycle of the agent, which provides a more secure way to manage agent access to
> Google Cloud resources than using service accounts. Existing access management controls through IAM
> support agent identity to enable strong governance."

And the page's only pointer for how to use one is:

> "To learn more about agent identities and how to use them, see [Use agent identity with **Vertex AI Agent Engine**]."

That single pointer is the decisive fact: **the only documented consumer of an agent identity is Vertex
AI Agent Engine** - a GCP-resident agent runtime. The page does not list BigQuery (or any data API) as
a consumer, and gives no API surface for obtaining or presenting an agent identity to one.

**2. Where services that accept federated identities are enumerated -
`docs.cloud.google.com/iam/docs/federated-identity-supported-services`, "Identity federation: products
and limitations".** This is the page the issue named as the place to read whether BigQuery accepts a
federated identity at all. Its **BigQuery** row records launch stage **GA** and, for the Google Cloud
API column, **"No known limitations"** for *identity federation*. But the page is about **Workforce
and Workload Identity Federation** - the federated-identity products - and it says nothing about
**agent identities**, which are a distinct, newer primitive. The matrix is silent on them.

## The two answers

**1. Does BigQuery accept an agent identity?** Not as documented. The read surface shows agent
identities are tied to Vertex AI Agent Engine; neither the workload-identities page nor the federation
support matrix states that BigQuery (or any data API this repository calls) can accept one as a
principal. This is the issue's own early-termination condition, and it fires: on the documented
surface there is no basis to claim BigQuery accepts an agent identity.

**2. Is it obtainable in the venues this runs in?** Independently of (1), an agent identity is
"attested and tied to the lifecycle of the agent" - an agent that lives in a GCP agent runtime.
This deployment's exchange path runs from a **GitHub Actions OIDC token** on an external runner
(`crates/sutura-exec-bigquery/src/sts.rs`'s one hop, `.exchange(workload.audience(),
workload.scope(), assertion)`). Nothing in the read documentation describes issuing an agent identity
to a runner outside Google's agent platform. Even if BigQuery accepted one, obtainability in CI is
unestablished, and the CI answer is the one the leg-2 harness depends on.

## The decision

**The federated path stays, and the change this issue offered is declined.** The deployment's
BigQuery credential remains `SharedServiceUser` over Workload Identity Federation - already keyless,
no service-account key file. The attestation-and-lifecycle improvement an agent identity would bring
is not available to this venue on the documented surface, and there is no false claim in the tree to
correct by shipping one. No code changes. `crates/sutura-exec-bigquery/src/sts.rs` is untouched, as
the issue required until step 1 was answered.

This does not close the door if Google later documents a BigQuery (or general data-API) acceptance of
agent identities: the gate is a documented consumer, and the moment one exists this evaluation is
worth re-reading, with the same two unknowns asked again against that new surface.

## Limits, stated

Written against two publicly readable Google pages on 2026-09-18. It does not test anything against a
live pool - it is the decision record the issue asked for, not a live verification. The claim "the
only documented consumer is Vertex AI Agent Engine" is a claim about what the **documented surface**,
as read on that date, ties agent identities to; a Google product change after that date is not covered
by it.
