---
title: Architecture
description: "How sutura is built: the ports, the adapters and identity."
---

# Architecture

sutura is one binary built from a set of Rust crates. The crates follow a ports-and-adapters
(hexagonal) design. The domain crate defines the ports. Each adapter implements one port for one
metadata source or one data system.

<iframe
  src="../assets/architecture.html"
  title="sutura crates and ports"
  loading="lazy"
  style="width: 100%; height: 820px; border: 0;"
></iframe>

[Open the diagram on its own page](assets/architecture.html). The diagram works without
JavaScript; the script only highlights the connections of the crate you point at.

## Deployments

Two deployments show how the parts connect. In both, the agent reaches sutura over MCP, and the key
set of the identity provider lets sutura verify the caller's token.

### Direct

<iframe
  src="../assets/deployment-direct.html"
  title="sutura with an agent that calls it directly"
  loading="lazy"
  style="width: 100%; height: 540px; border: 0;"
></iframe>

[Open the diagram on its own page](assets/deployment-direct.html). The agent calls sutura over MCP
with the caller's token.

### Through an AI gateway

<iframe
  src="../assets/deployment-gateway.html"
  title="sutura behind an AI gateway"
  loading="lazy"
  style="width: 100%; height: 540px; border: 0;"
></iframe>

[Open the diagram on its own page](assets/deployment-gateway.html). The gateway routes the model
calls and the MCP calls. It passes the caller's token to sutura. The token must name sutura as its
audience.

In both deployments, sutura verifies the token, reads the definitions from DataHub and runs each
query in the data system.

Users ask the access request portal for access. The portal grants roles in the identity provider and
grants in the data system. sutura keeps no copy of the grants.

DataHub holds each metric definition once. The metrics serving layer serves it to BI tools and APIs,
and sutura serves it to agents, so every consumer gets the same number.

Each part can change: another catalog (OpenMetadata, RDBMS, files), another data system (DuckDB,
Oracle, files), another OIDC issuer, direct or through a gateway.

## Identity

sutura separates two parts of identity:

- **Leg 1: sutura knows who asks.** sutura verifies the caller's token before it answers, on HTTP and on MCP
  (`security.inbound`). [Inbound identity](integrations/identity.md) has the settings.
- **Leg 2: the data system runs the query as the caller.** BigQuery and ClickHouse do this (secure-impersonation). sutura
  refuses a caller that the source does not declare. Every other data system runs each query as one identity that the
  operator declares for that source (`shared-service-user`).

Grants, row policies and masking stay in the data system. sutura keeps no result cache, because a cache keyed on the
question would leak rows between callers.
