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

The diagram shows one deployment. The agent reaches sutura over MCP. The key set of the identity
provider lets sutura verify the caller's token.

<iframe
  src="../assets/deployment.html"
  title="sutura deployment"
  loading="lazy"
  style="width: 100%; height: 570px; border: 0;"
></iframe>

[Open the diagram on its own page](assets/deployment.html).

The agent calls the model providers through an AI gateway. The MCP gateway is optional. If you use
it, it passes the caller's token to sutura. The token must name sutura as its audience.

sutura is the MCP server for agents. sutura verifies the token and runs each query in the data
system.

Users ask the access request portal for access. The portal grants roles in the identity provider
and grants in all data systems. sutura keeps no copy of the grants.

DataHub and Cube are the single source of truth. DataHub holds the definitions. Cube serves the
metrics to BI tools, APIs and sutura. Without Cube, sutura reads the definitions from DataHub and
runs each metric itself.

The metrics serving layer, for example Cube, is optional. We suggest it when BI tools or REST clients
also use the metrics. The serving layer can also cache results. sutura keeps no result cache.

Each part can change: another catalog (OpenMetadata, RDBMS, files), another data system (DuckDB,
Oracle, files), another OIDC issuer, with or without the MCP gateway, with or without Cube.

## Identity

sutura separates two parts of identity:

- **Leg 1: sutura knows who asks.** sutura verifies the caller's token before it answers, on HTTP and on MCP
  (`security.inbound`). [Inbound identity](integrations/identity.md) has the settings.
- **Leg 2: the data system runs the query as the caller.** BigQuery, ClickHouse and Oracle do this
  (secure-impersonation). sutura refuses a caller that the source does not declare. Every other data system runs each
  query as one identity that the operator declares for that source (`shared-service-user`).

Grants, row policies and masking stay in the data system. sutura keeps no result cache, because a cache keyed on the
question would leak rows between callers.
