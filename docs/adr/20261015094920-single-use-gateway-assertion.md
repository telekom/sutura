---
title: A gateway assertion may be declared single use
description: Behind a gateway, an opt-in key refuses a second presentation of one assertion by its jti, in a bounded store per process. Off by default, and what it does not cover.
---

# A gateway assertion may be declared single use

Status: **accepted**. It amends the replay paragraph of `docs/adr/0014`, whose tenth amendment
points here.

## The question

In `behind-gateway`, a signed assertion says that the component issued it, not that this request
carried it first. `docs/adr/0014` bounds its lifetime and leaves the replay open: an intercepted
assertion is accepted again until it expires. Today that window is every presentation until `exp`
plus the 30-second leeway, and `exp` is at most `transit_max_lifetime_seconds` after `iat`: 150
seconds after `iat` by default, 3630 at most. Can a deployment refuse a second presentation?

## Decision

**`security.inbound.transit_single_use: true` makes each assertion single use, by its `jti`.** The
key is `false` by default, and with it off the behaviour and the replay window above are unchanged.
The documentation of the setting states that window while the key is off.

- **Off by default, because only the operator knows how the component mints.** A component that
  reuses one assertion across requests gets every request after the first refused. A component
  that mints one assertion per request loses nothing. Whether a given component mints one per
  request is not known here, so the documentation of the setting says to confirm it first.
- **`behind-gateway` only.** A `direct` access token is reused across requests by design, so `true`
  in a `direct` block is refused at load, not ignored.
- **With it on, an assertion without a `jti`, or with a `jti` over the length bound, is refused.**
  So is a `jti` this process has seen and still holds. Each refusal is the same `401` body as a bad
  signature or an expired assertion, so a caller cannot tell which check failed. A `jti` is never
  written to a log or a body.
- **An assertion is recorded only after its signature and every claim check pass.** A forged
  assertion, an expired one or one over the lifetime ceiling never takes a place in the store.
- **The check and the record are one step under one lock.** Of two concurrent presentations of one
  assertion, exactly one is admitted.

## Why `jti` once, and not a binding to the request

A binding proves more: the component signs the request's method, its URL and a hash of its body,
and a captured assertion fails on any other request. It also needs every component to compute that
proof, and most components mint a plain assertion. `jti` once needs only one assertion per request,
which a component can do today. A binding is not built, and nothing here rules it out later.

## The store and its bound

- **In memory, one per process.** A `jti` is held until its own `exp` plus the leeway, which is the
  moment the validator would refuse the assertion anyway. The store's own clock decides, and it never
  moves back: an assertion whose `exp` plus the leeway has passed by that clock is refused and not
  recorded. No entry outlives `transit_max_lifetime_seconds` plus 60 seconds.
- **Expired entries are swept only when the store is full and the earliest expiry it knows has
  passed.** No timer and no background task, and a store full of unexpired entries is not swept
  again on each request.
- **The number of entries has a fixed cap, and a full store fails closed.** When the store is full
  after the sweep, a new assertion is refused with `503 identity_unavailable`, and it is never
  admitted unrecorded. **No entry is evicted before its `exp` plus the leeway**, because an eviction
  reopens that assertion's replay. The refusal is the typed `SeenStoreFull`, and it is logged once
  per window of `transit_max_lifetime_seconds` plus 60 seconds, not once per refused request.
- **The cap is 262,144 entries and a `jti` is at most 128 bytes.** Both are constants in the code,
  not keys, and start-up prints both when the key is on.
- **The cap is a throughput limit.** An entry keeps its place until its assertion expires, so a
  component that mints at the full lifetime gets at most the cap's number of requests per lifetime,
  per process. A deployment that needs more shortens `transit_max_lifetime_seconds` or adds
  replicas.

## What it does not cover

- **More than one replica.** The store is per process. With N replicas, one captured assertion is
  accepted up to N times inside its window. A store shared across replicas is a separate decision,
  and it is the same open decision as the shared counter in `docs/adr/0030`.
- **A restart.** The store is not kept, so an assertion seen before a restart is accepted once more
  after it, inside its window.
- **The first presentation.** Single use refuses a second presentation, not a first one by the wrong
  party. An assertion captured before it reaches this process is admitted once for whoever presents
  it first, and the request that carried it is then refused. The hop between the component and this
  process stays a trusted transport boundary, as `docs/adr/0014` says.
- **A `jti` the component repeats.** Two different assertions with one `jti` are one assertion here,
  so the second is refused. The component must mint a unique `jti` for each assertion.
