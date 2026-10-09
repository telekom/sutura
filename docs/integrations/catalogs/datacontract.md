---
title: Data Contract
description: Read tables, columns and joins from Open Data Contract Standard v3 documents.
---

# Data Contract

The Data Contract catalog reads a directory of Open Data Contract Standard (ODCS) v3 documents, in
YAML. The crate is `sutura-catalog-datacontract`, and the catalog kind is `datacontract`. It
supplies tables, columns, types, descriptions and joins. It supplies no metrics.

## When to use it

- Use it for single-player governance. One owner keeps the contracts as files in the repository, and
  sutura reads that directory. The [single player](../../examples/single-player.md) example shows this model with the markdown
  catalog.
- Your data contracts are ODCS v3 documents.

## Settings

This catalog reads the [settings that every catalog has](../../integrations.md#catalog-settings).
It reads `name`, `version` and `dir`. It requires `data_dir`, but does not read it.

## Example

```yaml
catalogs:
  - name: "physical"
    kind: "datacontract"
    dir: "/srv/sutura/contracts"
    data_dir: "/srv/sutura/contracts"
    version: "v1"
sources:
  physical:
    kind: "files"
    data_dir: "/srv/sutura/contracts"
    posture: "shared-service-user"
```

Every model binds to the source whose alias is the catalog `name`.
[`examples/datacontract`](https://github.com/telekom/sutura/tree/main/examples/datacontract) has
three contracts.

## How a contract maps

| ODCS                                   | In sutura                                                            |
| -------------------------------------- | -------------------------------------------------------------------- |
| `schema[]`                             | One model each                                                       |
| `physicalName`, else `name`            | The table                                                            |
| `properties[]`                         | The columns                                                          |
| `physicalType`, else `logicalType`     | The column type                                                      |
| `required`                             | Whether the column can be null                                       |
| `relationships[]` of type `foreignKey` | A many-to-one join, if the target column is `primaryKey` or `unique` |

sutura accepts `apiVersion` `v3.0.0` to `v3.2.0`, and `kind` must be `DataContract`. Relationships
need `v3.1.0` or later, and a reference uses the `table.column` form. sutura accepts `servers`,
`quality`, `team`, `support`, `slaProperties` and `context`, and does not use them.

## Identity

The reader reads the files as the operating-system user of the sutura process. It uses no
credential.
