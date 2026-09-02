# Multi player

**The deployment shape this example documents is not servable in this repository yet.** No binary
here links `sutura-catalog-datahub` - its only dependant is `sutura-app`, as a dev-dependency - and
`sutura-serve` refuses `catalog.kind: datahub` by name. So this page is the shape a deployment would
configure, and the proof the build gives about it, told in that order; the *What runs here and now*
section below says exactly where the proof stops.

This is the sibling of `single-player/` that single-player cannot be: there, every claim holds
because a local file has no login to present and there is nobody else to be. Here the deployment
declares a per-caller identity (`security.inbound`), the catalog is a metadata service rather than
markdown in git, and the answered identity is a difference the runtime knows about. It exists to show
the DataHub connector doing its job in that shape, and to say plainly which half of "multiplayer"
still cannot run here.

## The deployment

Two things make a deployment multi-player, and both are configuration rather than code:

- **`security.identity: "multi-user"`** - the operator's declaration that callers are not all one
  person. In multi-user mode a `shared-service-user` source needs its per-source operator
  acknowledgement (`sources.<alias>.acknowledged_because`); without it the process refuses to boot
  (`SharedSourceNotAcknowledged`).
- **`security.inbound`** - who is asking, verified from a signature: `direct` (a token from this
  deployment's own authorization server) or `behind-gateway` (a compact, short-lived assertion signed
  by the trusted component). Either way the deployment knows the caller's subject, and the caller
  cannot state their own identity in a request body - `docs/adr/0014`.

Those are two different refusals for two different configuration mistakes. A deployment that
*declares* an inbound mode but attaches no gate to it is refused as
`RouterNotBuilt::InboundIdentityNotAttached` before the listener opens; a multi-user deployment whose
`shared-service-user` source carries no acknowledgement is refused at boot as
`SharedSourceNotAcknowledged`. The reference is `docs/serving.md`.

The catalog is DataHub, read by `sutura-catalog-datahub` - the first **declaring** `SemanticCatalog`.
It provides `Structure`, `Descriptions` and `Relationships` unconditionally, and the metric kinds as
**declared-and-empty may-provide** declarations (`docs/adr/0016`, issue #202): a certified metric
comes from the deployment itself. `DataHub`'s `structuredProperty` is scalar-only, so a deployment
defines metric content as **one string-valued structured property named `sutura`** whose value is a
JSON document over the domain's closed vocabularies. What the deployment defines for the example's
metric looks like this - the canonical shape `sutura-catalog-datahub` decodes:

```json
{
  "name": "revenue",
  "dialect": "ANSI_SQL",
  "expression": "SUM(amount_cents)",
  "sutura": {
    "string_value": "{\"model\":\"orders\",\"description\":\"Net revenue in minor units, from active orders.\",\"measure\":{\"simple\":{\"aggregate\":\"sum\",\"column\":\"amount_cents\"}},\"time_column\":\"order_date\",\"grains\":[\"month\"],\"required_filters\":[{\"equals\":{\"column\":\"status\",\"value\":\"active\"}}],\"dimensions\":[{\"name\":\"segment\",\"column\":\"segment\",\"via\":\"orders_to_customer\",\"allowed_values\":[\"retail\",\"wholesale\"]}],\"anchor\":{\"range\":{\"start\":\"2026-06-01\",\"end\":\"2026-07-01\"},\"value\":\"412345\"}}"
  }
}
```

The inner document is structure, not text: `measure` is the domain `Measure` vocabulary, `grains`,
`required_filters` and `allowed_values` are the closed sets, and a dimension's `via` names a
relationship this snapshot carries. An unrecognised property is refused by name rather than guessed.
A metric **without** the property stays the promotion candidate `docs/adr/0016` describes - read,
never certified - and because the kinds are declared-and-empty, a DataHub whose metrics all lack it
still loads, as a bundle with models, prose and joins and no certified metrics.

## The shape, and where the data system sits

The whole point of a multi-player deployment is that the question runs as the caller who asked it.
Concretely: caller A and caller B ask the same certified question, the catalog is the same DataHub
bundle, and the data system evaluates each leg under the identity the runtime minted for *that*
caller - so two principals can legitimately read two different sets of rows. A leg that cannot run as
the subject is refused (`credential_unavailable`) rather than answered under some other identity,
because that silent downgrade is the failure mode the shape exists against. That is the shape's
intent; today no shipped deployment reaches the refusal, because no served source impersonates at
all - the shipped serve binary answers under the shared identity, and `docs/adr/0008` records the
port that changes that.

```
caller A ─┐                                   ┌─> shared-service-user or
caller B ─┼─ security.inbound ─> sutura ──────┼─> impersonation-at-source source
catalog   ─┘  (a signature)                   └─> the rows the leg's identity can read
```

## What runs here and now

There is no live DataHub in this repository and no warehouse with row-level grants, so the runnable
form of the example is the recorded fixture: `crates/sutura-catalog-datahub/tests/multi_player.rs`
loads the recorded corpus with the certified `revenue` metric above. The example question is a real
documented input rather than a copy - `examples/multi-player/question.json` holds it (`revenue in
June 2026`, monthly), the README names it, and the test reads it off disk - so the question, this
page and the suite cannot drift apart. The test compiles that question into a plan over the orders
table. It is in `checks.nextest`, needs no network, and is what CI uses to keep the integration
honest: the DataHub bundle, its certified metric and a question about it all agreeing, on every pull
request.

**What that test proves is: a bundle loads, certifies its metric, and a question about it compiles
to a plan.** It stops at the plan - nothing here reaches a warehouse, a surface or a settings file,
and no binary in this repository can open this catalog (`catalog.kind: datahub` is refused by
`sutura-serve`, and the crate has no other composition root). What the fixture **cannot** prove, and
what the example therefore says explicitly rather than pretending:

- **A served deployment** needs a composition root that links `sutura-catalog-datahub`; none exists
  today (the crate is a `sutura-app` dev-dependency), which is the *Built and not wired* entry
  `.agents/skills/sutura/query-surface/SKILL.md` records alongside the read-path paragraph below.
- **Two callers, two answers** needs a data system that evaluates two principals differently, and
  nothing here can present one to it. `docs/adr/0008` records the port (a credential per leg for the
  calling subject); the missing piece is a data system with grants and a served source that actually
  executes as the asker.
- **A real DataHub read path** needs a provisioned instance and the `AspectReader` over its versioned
  OpenAPI entity surface - the open measurement `docs/adr/0016` leaves, exactly as
  `sutura-exec-bigquery`'s acceptance leg was measured against a real system.

Until those land, "multiplayer" here means: the deployment shape is declared, the DataHub bundle
certifies its defined metric under the flat `sutura` property, and a question about it compiles to a
plan - with identity's per-row half and the served half stated as limits rather than elided.
