---
title: DataHub
description: Read models, joins and certified metrics from a DataHub deployment.
---

# DataHub

<span class="sutura-badge sutura-badge--recommended">Recommended</span>

The DataHub catalog reads the semantic model from a DataHub deployment over its OpenAPI v3 entity
API. The crate is `sutura-catalog-datahub`, and the catalog kind is `datahub`. It reads datasets
as models, semantic models for their joins, and metrics. A metric becomes a certified metric when
the deployment writes the sutura metric document into a structured property.

## When to use it

- Use it for multi-player governance. Many owners keep their definitions in one shared DataHub, and
  one reviewed catalog is the source of truth. The [multi player](../../examples/multi-player.md) example uses this catalog.
- Your organisation already keeps its datasets and their descriptions in DataHub.

## Settings

This catalog reads the [settings that every catalog has](../../integrations.md#catalog-settings)
and these keys. It requires `dir` and `data_dir`, but does not read them.

| Key                  | Type    | Default   | Meaning                                                                                                      |
| -------------------- | ------- | --------- | ------------------------------------------------------------------------------------------------------------ |
| `endpoint`           | string  | required  | `scheme://host[:port]`. `https` for any host, `http` only for a loopback IP address. No path and no `user@`. |
| `token_file`         | path    | required  | A file that holds a DataHub personal access token. sutura reads it once at startup.                          |
| `metric_property`    | string  | required  | The qualified name of the structured property that holds the metric document.                                |
| `deadline_seconds`   | integer | `30`      | One time limit for all requests of one catalog read. `0` is refused.                                         |
| `max_response_bytes` | integer | `8388608` | The size limit for one response page. `0` is refused.                                                        |

For TLS, the reader uses `security.outbound`: `transport_anchors` (a PEM file, or `system`) and an
optional `client_certificate` and `client_key`. The BigQuery source does not use them: a PEM file or a
client certificate beside a `bigquery` source stops startup.

## Example

This is `conf/base.yaml` of the [multi player](../../examples/multi-player.md) example. The
catalog `name` is also the name of the BigQuery source that the models bind to.

```yaml
--8<-- "examples/multi-player/conf/base.yaml"
```

## How the model maps

| DataHub entity                        | In sutura                                                      |
| ------------------------------------- | -------------------------------------------------------------- |
| `dataset` (`schemaMetadata`)          | A model and its columns, with descriptions                     |
| `semanticModel` relationships         | Joins. A join needs a cardinality, and many-to-many is refused |
| `metric` with the structured property | A certified metric                                             |
| `metric` without the property         | Read and not used                                              |

Every model must be on the `bigquery` platform. sutura binds those models to the source whose
alias is the catalog `name`.

## Identity

The reader sends the token from `token_file` on every request. It is one token for every caller,
because sutura loads a catalog without a caller's identity.
