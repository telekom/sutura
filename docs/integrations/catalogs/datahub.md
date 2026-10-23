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

| Key                  | Type    | Default   | Meaning                                                                                                                                                                                                                                     |
| -------------------- | ------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `endpoint`           | string  | required  | `scheme://host[:port]`. `https` for any host, `http` only for a loopback IP address. No path and no `user@`.                                                                                                                                |
| `token_file`         | path    | required  | A file that holds a DataHub personal access token. sutura reads it once at startup.                                                                                                                                                         |
| `metric_property`    | string  | required  | The qualified name of the structured property that holds the metric document.                                                                                                                                                               |
| `platforms`          | map     | none      | Each DataHub platform, mapped to the `sources:` alias that serves its datasets. An alias that `sources:` does not declare, an empty map, a platform with no source, and a platform name that is empty or holds whitespace all stop startup. |
| `deadline_seconds`   | integer | `30`      | One time limit for all requests of one catalog read. `0` is refused.                                                                                                                                                                        |
| `max_response_bytes` | integer | `8388608` | The size limit for one response page. `0` is refused.                                                                                                                                                                                       |

`sutura serve` reads all catalog entries again at the shortest `refresh_seconds` among them. The default is 900, and `0` is refused. See [Serving](../../serving.md).

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

A model is bound to the source that `platforms` maps its platform to. Without `platforms`, every
model must be on the `bigquery` platform, and sutura binds it to the source whose alias is the
catalog `name`. A model on a platform that is not mapped is refused at load, by name.
The [federated](../../examples/federated.md) example maps `postgres` and `clickhouse`.

A model is named by its platform and the whole path of its dataset, joined by `__`, with `_` for
each character a model name refuses. The dataset `acme-prod.sales.orders` on `bigquery` is the
model `bigquery__acme_prod__sales__orders`. A metric document names its model by this name in its
`model` field. A name longer than 63 characters keeps its first 54 characters, then `_` and eight
hex digits of a hash of the whole name. Two datasets that map to one name are refused as a
duplicate model.

A relationship endpoint (`from` or `to` of a semantic model) joins only through the exact dataset urn
it names, and that urn must name a dataset the same read returned. An endpoint that names any other
dataset, on any platform or environment, is refused, and the error names the endpoint and the urn.

## Identity

The reader sends the token from `token_file` on every request. It is one token for every caller,
because sutura loads a catalog without a caller's identity.
