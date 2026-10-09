---
title: Wren
description: Convert a WrenAI MDL manifest into a markdown catalog that a person reviews and commits.
---

# Wren

sutura reads a WrenAI MDL manifest with the `sutura import wren` command. The command writes a
[markdown catalog](markdown.md) for a person to review and commit. The crate is
`sutura-catalog-wren`. Wren is not a catalog kind: sutura serves the output as `kind: markdown`.

## When to use it

- Use it for single-player governance. One owner reviews the converted files and commits them to the
  repository. The [single player](../../examples/single-player.md) example shows this model with the markdown catalog.
- Your semantic model is in WrenAI.

## Usage

```bash
sutura import wren <wren-project-dir> <out-dir>
```

- `<wren-project-dir>` contains `manifest.json`, an MDL manifest of layout version 2.
- `<out-dir>` must be empty. If it contains a file, the command writes nothing.

The command runs offline and uses no settings keys. It writes:

| Path                 | Contents                                                      |
| -------------------- | ------------------------------------------------------------- |
| `models/*.md`        | One model document for each Wren model                        |
| `relationships/*.md` | One relationship document for each Wren relationship          |
| `metrics/*.md`       | One `kind: cube` document for each Wren cube                  |
| `declaration.md`     | What the catalog supplies                                     |
| `report.txt`         | Every part of the manifest that the command could not convert |

## Serve the output

Every converted model has `source: wren`. Declare a data system with that alias, and point a
markdown catalog at the output:

```yaml
catalogs:
  - name: "model"
    kind: "markdown"
    dir: "/srv/sutura/wren-catalog"
    data_dir: "/srv/sutura/data"
    version: "wren-import-1"
sources:
  wren:
    kind: "files"
    data_dir: "/srv/sutura/data"
    posture: "shared-service-user"
```

## What the command converts

The command converts a column, one aggregate over one column, a ratio of two such aggregates, and
a join on equal columns. It refuses anything else by name in `report.txt`: for example SQL models,
calculated columns, access-control rules and many-to-many relationships. Wren has no audience and
no grain, so each converted metric gets `audience: open` and the grain `day`. Review both before
you commit the catalog.
