---
title: Markdown catalog
description: A directory of markdown files with YAML frontmatter, in git. The catalog kind that supplies every part of the model.
---

# Markdown catalog

The markdown catalog is a directory of `.md` files. Each file has YAML frontmatter with a `kind:`
key, and the body is prose. The crate is `sutura-catalog-local`, and the catalog kind is
`markdown`. It is the only catalog that can supply every part of the model: models,
relationships, metrics, cubes and knowledge.

## When to use it

- Use it for single-player governance. One owner writes the model as files in the repository and
  reviews it in git like code. The [single player](../../examples/single-player.md) example and
  [Getting started](../../getting-started.md) use this catalog.

## Settings

This catalog reads only the [settings that every catalog has](../../integrations.md#catalog-settings).
It reads `name`, `version` and `dir`. It requires `data_dir`, but does not read it.

`kind: markdown` is also the value when `kind` is absent. The built-in default is this entry:

```yaml
catalogs:
  - name: "model"
    kind: "markdown"
    dir: "catalog"
    data_dir: "data"
    version: "unversioned"
```

A relative `dir` resolves against the working directory of the process.

## The files

The reader walks `dir` recursively, in sorted order. It skips symbolic links. Each file has one of
these kinds: `model`, `relationship`, `metric`, `cube`, `glossary`, `caveat`, `not_defined`,
`example` or `declaration`. [Concepts](../../concepts.md) describes what each kind means. The
[single player catalog](https://github.com/telekom/sutura/tree/main/examples/single-player/catalog)
has one file of most kinds.

A metric document names its model with `model:`. A model document names its data system with
`source:`, and `sutura serve` refuses to start if no `sources.<alias>` entry has that name.

## Identity

The reader reads the files as the operating-system user of the sutura process. It uses no
credential. It loads the catalog without the caller's identity, so every caller gets the same
definitions.

## Size

One catalog holds at most 1000 documents and 16 MiB of text. The reader visits at most 10 000
directory entries. An empty directory is an error.
