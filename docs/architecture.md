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

## Identity

sutura separates two parts of identity:

- **Leg 1: sutura knows who asks.** sutura verifies the caller's token before it answers, on HTTP and on MCP
  (`security.inbound`). [Inbound identity](integrations/identity.md) has the settings.
- **Leg 2: the data system runs the query as the caller.** BigQuery and ClickHouse do this (secure-impersonation). sutura
  refuses a caller that the source does not declare. Every other data system runs each query as one identity that the
  operator declares for that source (`shared-service-user`).

Grants, row policies and masking stay in the data system. sutura keeps no result cache, because a cache keyed on the
question would leak rows between callers.
