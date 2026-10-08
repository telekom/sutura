---
title: OKF
description: Read table structure and descriptions from a directory of Frictionless Table Schema descriptors.
---

# OKF

The OKF catalog reads a directory of Frictionless Table Schema descriptors, in YAML. Each file
describes one table. The crate is `sutura-catalog-okf`, and the catalog kind is `okf`. It supplies
the structure of the tables and their descriptions. It supplies no metrics and no joins.

## When to use it

- Use it for single-player governance. One owner keeps the table descriptors as files in the
  repository. The [single player](../../examples/single-player.md) example shows this model with the markdown catalog.
- Your tables are already described as Frictionless Table Schema files.

## Settings

This catalog reads the [settings that every catalog has](../../integrations.md#catalog-settings).
It reads `name`, `version` and `dir`. It requires `data_dir`, but does not read it.

## Example

```yaml
catalogs:
  - name: "physical"
    kind: "okf"
    dir: "/srv/sutura/okf"
    data_dir: "/srv/sutura/okf"
    version: "v1"
sources:
  physical:
    kind: "files"
    data_dir: "/srv/sutura/okf"
    posture: "shared-service-user"
```

Every model binds to the source whose alias is the catalog `name`, so a `sources.<name>` entry
must exist. [`examples/okf`](https://github.com/telekom/sutura/tree/main/examples/okf) has two
descriptor files.

## How the files map

| Table Schema                    | In sutura                                         |
| ------------------------------- | ------------------------------------------------- |
| File name without the extension | The model and table name                          |
| `description`, else `title`     | The model description. One of the two is required |
| `fields[].name`                 | The columns                                       |
| `fields[].type`                 | The column type                                   |
| `fields[].description`          | The column description                            |
| `primaryKey`                    | Kept as evidence for the key                      |

sutura reads `foreignKeys`, `constraints`, `format`, `rdfType` and `missingValues`, and does not
use them. A foreign key declares no cardinality, so it does not become a join.

## Identity

The reader reads the files as the operating-system user of the sutura process. It uses no
credential.
