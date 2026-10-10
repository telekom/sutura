---
title: Examples
description: Complete sutura setups you can run, each with its configuration and the commands to use it.
---

# Examples

Each example is a directory in
[`examples/`](https://github.com/telekom/sutura/tree/main/examples) with a catalog, data and the
configuration to serve it.

| Example                           | Metadata         | Data            | Identity                                     |
| --------------------------------- | ---------------- | --------------- | -------------------------------------------- |
| [Single player](single-player.md) | Markdown catalog | Local CSV files | One user, shared service user                |
| [Multi player](multi-player.md)   | DataHub          | BigQuery        | Keycloak users, a service account per caller |
| [The local chat demo](../demo.md) | Markdown catalog | Local CSV files | One user, a chat client over MCP             |
