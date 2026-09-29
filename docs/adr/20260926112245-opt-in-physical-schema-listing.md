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

The served MCP `initialize` prompt renders through the same caller-scoped `ScopedView` ADR 0028's
second amendment gives it, so an enabled deployment's `instructions` already carries this caller's
own physical listing at `initialize`, cut to their audience. The audience cut itself is held by
`sutura-domain`'s `pinned::tests::model_listing_requires_its_own_explicit_audience`, over
`ScopedView::models`, which both tools read; `served_initialize_does_not_publish_the_whole_physical_schema`
pins only the narrower case that an audience-less model is not listed to a verified caller at
`initialize`. `describe_catalog` renders the same listing through the same per-request view, so the
two tools agree. The stdio MCP and `sutura prompt` use an operator-side whole-bundle view, so an
operator sees every declared model regardless of audience.

## Limits

Source adapters that infer models from a data dictionary do not declare model audiences. Their
models stay absent from caller-scoped listings until an explicit audience channel is added for
those sources. `audience: open` exposes the whole model's metadata, including every column; this
decision adds no column-level visibility. Authored prose can still mention hidden metadata, and
quoting does not prevent persuasive prompt injection. `prompt.list_physical_schema` is a product
switch, not the security boundary: the audience cut applies whether or not it is enabled, but on
the stdio and CLI transports - which have no caller and no `ScopedView` to cut by - enabling it
lists every declared model to whoever holds that process.
