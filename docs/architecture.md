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

sutura separates two claims about identity:

- **Leg 1: sutura knows who asks.** This is built. With `security.inbound`, sutura verifies the
  caller's token before it answers. [Inbound identity](integrations/identity.md) has the settings.
- **Leg 2: a data system runs the query as the caller.** This is built for BigQuery
  (secure-impersonation). The BigQuery adapter sends the caller's verified assertion through the
  account that the source maps for that caller. sutura refuses a caller that the map does not
  declare.

Every other data system runs as one identity that the deployment declares for that source. An
operator acknowledges that shared identity in the configuration, and the answer reports it.

sutura keeps no copy of who may see which rows. Grants, row policies and masking stay in the data
system. For this reason there is no result cache: under row-level security, a cache keyed on the
question would leak rows between callers.
