---
title: OpenMetadata
description: Read tables, descriptions and joins from an OpenMetadata deployment.
---

# OpenMetadata

The OpenMetadata catalog reads tables and metrics from an OpenMetadata deployment over its REST
API. The crate is `sutura-catalog-openmetadata`, and the catalog kind is `openmetadata`. It
supplies models, column types, descriptions and joins.

## When to use it

- Your organisation keeps its tables and their descriptions in OpenMetadata.
- You want sutura to use the table constraints that OpenMetadata records as joins.

## Settings

This catalog reads the [settings that every catalog has](../../integrations.md#catalog-settings)
and these keys. It requires `dir` and `data_dir`, but does not read them.

| Key                  | Type    | Default   | Meaning                                                                                                      |
| -------------------- | ------- | --------- | ------------------------------------------------------------------------------------------------------------ |
| `endpoint`           | string  | required  | `scheme://host[:port]`. `https` for any host, `http` only for a loopback IP address. No path and no `user@`. |
| `token_file`         | path    | required  | A file that holds an OpenMetadata bearer token. sutura reads it once at startup.                             |
| `deadline_seconds`   | integer | `30`      | One time limit for both requests of one catalog read. `0` is refused.                                        |
| `max_response_bytes` | integer | `8388608` | The size limit for one response. `0` is refused.                                                             |

For TLS, the reader uses `security.outbound`: `transport_anchors` and an optional
`client_certificate` and `client_key`.

## Example

```yaml
catalogs:
  - name: "metrics"
    kind: "openmetadata"
    dir: "/unused-for-openmetadata"
    data_dir: "/unused-for-openmetadata"
    version: "v1"
    endpoint: "https://openmetadata.example.com"
    token_file: "/run/sutura/openmetadata-token"
sources:
  metrics:
    kind: "files"
    data_dir: "/srv/sutura/data"
    posture: "shared-service-user"
```

## How the model maps

sutura reads `/api/v1/tables` with their columns and constraints, and `/api/v1/metrics`.

| OpenMetadata                                             | In sutura                               |
| -------------------------------------------------------- | --------------------------------------- |
| A table of the service `warehouse`                       | A model. A description is required      |
| Its columns                                              | The columns, with type and description  |
| A table constraint: one-to-one, many-to-one, one-to-many | A join                                  |
| A metric                                                 | Read and not used as a certified metric |

The tables must belong to the service named `warehouse`. sutura binds them to the source whose
alias is the catalog `name`.

## Identity

The reader sends the token from `token_file` on every request. It is one token for every caller,
because sutura loads a catalog without a caller's identity.

## Sizing

The reader reads one page of at most 1000 tables and 1000 metrics. A response that reports more is
refused.
