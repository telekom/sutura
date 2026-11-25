---
title: Catalog visibility per caller
description: A source with foundry_login may narrow each caller's catalog view to the object types that caller's own Foundry token lists. The definitions stay deployment-scoped; the narrowing hides names, it is not a data control (github.com/telekom/sutura#1857).
---

# Catalog visibility per caller

Status: **accepted**, 2026-11-25. Built, held by fakes only: no Foundry tenant has run it.

## The question

A catalog loads without a caller: one bundle per deployment, one digest, one provenance. A
`foundry` source with `foundry_login` runs each question as its caller, so Foundry refuses a
question on an object type that caller cannot read and no rows leak. But the catalog still shows
every caller every model: its name, columns, joins and the metrics over it. Where object-type names
are themselves sensitive, a caller should not see models they cannot read. How is that done without
a second catalog per caller?

## Decision

**The definitions stay deployment-scoped. The VIEW is narrowed per caller, as a filter.**

- `sources.<alias>.catalog_visibility: per-caller` declares it on one source. Off by default. It is
  refused at load on a source that declares no `foundry_login`, because nothing else brokers a token
  to list with.
- For `GET /v1/catalog`, the agent surface's `describe_catalog` tool and its `initialize` prompt,
  sutura lists the source's object types with **the caller's own brokered Foundry token** and hides
  every model on that source whose object type the list does not contain. A metric that reads a
  hidden model - its own, a measure term's, or one a dimension's relationship chain passes through -
  is hidden with it, and so is a relationship ending at one.
- The token comes from the broker the source already uses for its questions
  (`DeclaredPrincipalBroker` over the `foundry_login` hop). There is no second broker and no token
  mint. What the broker presents is checked as a question's leg is: `Minted::agreeing_with`, then
  the leg against the source's posture.
- The port is `sutura_domain::visibility::CallerListing`, implemented by
  `sutura_catalog_foundry::caller::CallerObjectTypes` over the same object-type list the catalog
  reader reads. `sutura_app::visibility::CatalogVisibility` holds the ports and a cache; the driving
  port gains `Surface::hidden_models`, required with no default so a forwarding wrapper cannot forget
  it.
- **Cache.** One entry per caller, keyed by the whole `PrincipalChain` (never a bare subject, for
  `docs/adr/0031`'s second amendment's reason), reused for 60 seconds, at most 1024 callers; a full
  cache drops expired entries, then the oldest. Only a listing every declared source granted is
  stored. Nothing from one caller's entry is read for another.
- **A miss is bounded.** A transport takes an execution slot - the one a question takes - before
  narrowing that would dial out for this caller (`Surface::catalog_dials`, required on every
  surface so no wrapper can drop it), and sheds it like a question; a cached listing or a held
  refusal takes no slot. `catalog_dials` is read just before the call, so a listing that expires in
  between dials without a slot. Concurrent
  misses for one chain share one listing: the first lists, the others wait inside their own deadline
  and read what it stored. A chain the broker answered *not linked* is answered so again for 10
  seconds without asking, keyed by the same whole chain. **The limit:** only *not linked* is held;
  a source refusing the listing, or an outage, is asked again on the next miss.
- **Fail closed.** A caller with no linked account gets `403 identity_not_linked`, as on a
  question. Any other broker failure is `503 identity_unavailable`; a source that did not list is
  `503 unavailable`; a credential that does not fit the source is `500`. None of them returns the
  unnarrowed view. A broker that answers *no credential at this source* hides every model on it.
  The agent surface's `initialize` answers the same cases as JSON-RPC errors in the server range,
  one code per thing a client can act on: `-32011` not linked, `-32012` identity provider
  unavailable, `-32013` source did not list, `-32014` shed; a wiring defect is `-32603`. It never
  renders the prompt over the unnarrowed bundle.

## What this is not

**It narrows what a caller SEES. It is not a data control.** Leg 2 - Foundry evaluating each
question under the caller's own token - stays the data control. Questions compile against the whole
bundle: a caller who names a hidden metric still asks it, and Foundry's refusal under their token
(`source_refused`) is what keeps the rows away. That answer differs from the one for a metric that
does not exist, so a caller can still probe for a name.

It does not cover:

- **The bundle's own text.** The digest, the version and the provenance are the whole bundle's.
  A metric's prose, a note, or the operator's instructions that name a hidden model still reach
  every caller.
- **Models on other sources.** Only models bound to a declaring source are filtered.
- **SQL-expression metrics.** A metric over catalog-authored SQL is judged by its own model alone;
  the text is not read for the tables it names.
- **Staleness.** A permission Foundry revokes stays visible for up to 60 seconds of a cached entry.
- **The `sutura` command.** `query` and `mcp` attach no broker and refuse an impersonating Foundry
  source, so only `sutura serve` narrows.
- **A tenant.** Whether a Foundry third-party application's token may list object types (its
  scopes) is unverified, as is whether two users see two different lists.

## Alternatives

- **A catalog per caller.** Rejected: one bundle, one digest and one provenance is the only
  consistent snapshot there is, and anchors re-run against the bundle at boot, with no caller.
- **Filter at compile as well.** Not taken: it would add a broker round trip and a Foundry listing to
  every question for no data protection - leg 2 already refuses the rows.
- **A listing method on the `Warehouse` port.** Rejected: it would widen a port with ten
  implementors for one adapter.
