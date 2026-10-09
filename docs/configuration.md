---
title: Configuration
description: How sutura reads its settings, and where each group of settings is described.
---

# Configuration

sutura reads its settings in four layers. A later layer overrides an earlier one:

1. The built-in defaults:
   [`crates/sutura-config/src/defaults.yaml`](https://github.com/telekom/sutura/blob/main/crates/sutura-config/src/defaults.yaml).
2. `<dir>/base.yaml`, where `SUTURA_CONFIG_DIR` names `<dir>`.
3. `<dir>/<environment>.yaml`. `SUTURA_ENVIRONMENT` selects the environment: `development`
   (the default), `test` or `production`.
4. One environment variable for each key: `SUTURA__`, then the key path in capitals with `__`
   between the parts. `SUTURA__SERVER__PORT` sets `server.port`.

Every layer refuses a key that sutura does not know. sutura also refuses to start with an unsafe
or incomplete configuration, and the error names the key.

## The settings

| Group               | What it sets                                                   | Described in                                                                                          |
| ------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `server`            | The bind address, the request timeout, the agent surface       | [Serving over HTTP](serving.md#run-it)                                                                |
| `security`          | Tokens, TLS termination, the deployment identity               | [Serving over HTTP](serving.md#who-is-asking)                                                         |
| `security.inbound`  | How sutura verifies a caller                                   | [Inbound identity](integrations/identity.md)                                                          |
| `security.outbound` | The TLS trust material of the DataHub and OpenMetadata readers | [DataHub](integrations/catalogs/datahub.md#settings)                                                  |
| `catalogs`          | The catalogs that supply the definitions                       | [Catalog settings](integrations.md#catalog-settings)                                                  |
| `sources`           | The data systems that run the questions                        | [Data system settings](integrations.md#data-system-settings)                                          |
| `rate_limit`        | Request rates per client                                       | [Serving over HTTP](serving.md#capacity)                                                              |
| `runtime`           | Concurrent queries, the engine memory and threads, shutdown    | [Serving over HTTP](serving.md#capacity)                                                              |
| `prompt`            | What the agent prompt includes                                 | [The agent prompt](agent-prompt.md)                                                                   |
| `tools`             | The raw SQL tool                                               | [Serving over HTTP](serving.md#the-raw-sql-tool)                                                      |
| `telemetry`         | The log filter and format                                      | [Serving over HTTP](serving.md#the-log)                                                               |
| `governance`        | Optional limits on spend and on the rows of a `top` question   | [`defaults.yaml`](https://github.com/telekom/sutura/blob/main/crates/sutura-config/src/defaults.yaml) |

## Defaults that matter first

| Key                              | Default      | Meaning                                                          |
| -------------------------------- | ------------ | ---------------------------------------------------------------- |
| `server.host`                    | `127.0.0.1`  | sutura listens on loopback only                                  |
| `server.port`                    | `8080`       | The HTTP port                                                    |
| `server.request_timeout_seconds` | `30`         | The longest time that one request may take                       |
| `security.tls_termination`       | `none`       | Where TLS ends. Declare it for a bind that other hosts can reach |
| `runtime.max_concurrent_queries` | `8`          | Questions that run at the same time                              |
| `runtime.working_set_max_bytes`  | `1073741824` | The memory that the engine may reserve                           |
| `prompt.catalog_prose`           | `quoted`     | Catalog descriptions reach the agent quoted, or `omitted`        |

`security.identity` has no default. When you declare a source, write `single-user` or
`multi-user`.
