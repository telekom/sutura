---
title: Opt in physical schema listing
description: The opt-in, caller-scoped physical model and column listing for agent surfaces.
---

# Opt in physical schema listing

Status: **accepted**. Amends ADR 0028 and ADR 0022 for physical metadata on agent surfaces.

## Decision

`prompt.list_physical_schema: true` enables a descriptive listing of model names, table paths,
column names and their descriptions in the operator-side prompt and the `describe_catalog` tool.
The default is `false`. It adds no query input and no data rows. Descriptions follow
`prompt.catalog_prose`: quoted as untrusted catalog text or omitted in both tool content blocks and
the prompt. The existing definition byte cap counts model names and table paths as well as columns
and descriptions, so a large collection of empty models cannot bypass it.

A model may declare `audience: open` or a nonempty restricted audience, using the same vocabulary
as a metric. `ScopedView::models` is the only reader for the new listing. A caller-scoped view
omits models without an explicit audience; it neither infers visibility from a metric nor silently
opens a model. An operator-side whole-bundle view can list every model after the opt-in. This lets
a physical-only authored catalog declare an audience without inventing a metric.

The served MCP `initialize` prompt is rendered before a caller-specific view exists, so it does
not include this listing. Authenticated callers read it through `describe_catalog`, where the
request's view filters the models. The stdio MCP and `sutura prompt` use an operator-side
whole-bundle view.

## Limits

Source adapters that infer models from a data dictionary do not declare model audiences. Their
models stay absent from caller-scoped listings until an explicit audience channel is added for
those sources. `audience: open` exposes the whole model's metadata, including every column; this
decision adds no column-level visibility. Authored prose can still mention hidden metadata, and
quoting does not prevent persuasive prompt injection. The existing whole-bundle served prompt's
metric and knowledge content remains an independent limit of ADR 0028.
