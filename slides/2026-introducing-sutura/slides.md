---
theme: ./theme
title: "sutura - answers about data, as the person asking"
info: sutura, a semantic data runtime. Documentation at telekom.github.io/sutura.
transition: slide-left
mdc: true
favicon: ./shared/sutura.svg
fonts:
  provider: none
layout: dark
drawings:
  persist: false
routerMode: hash
---

<div class="relative z-10 h-full max-w-6xl mx-auto px-16 py-12 flex flex-col justify-between">
  <div class="eyebrow-light"><a href="https://github.com/telekom/sutura" class="hover:text-teal-300">github.com/telekom/sutura</a></div>
  <div class="grid w-full gap-10" style="grid-template-columns:1.35fr 0.65fr;align-items:center">
    <div class="space-y-5">
      <h1 class="slide-title text-white" style="text-wrap:balance">
        Answers about data, <span style="color:#5eead4">as the person asking</span>
      </h1>
      <div class="lead-dark space-y-1">
        <p>sutura is a semantic data runtime written in Rust.</p>
        <p>Certified metric definitions. HTTP and MCP. Several data sources.</p>
      </div>
    </div>
    <div class="flex justify-center">
      <img src="/shared/sutura.svg" alt="sutura" class="h-auto object-contain" style="max-height:15rem" />
    </div>
  </div>
  <div class="mono-label" style="color:rgba(94,234,212,0.78);line-height:1.6;letter-spacing:0.04em">
    <a href="https://telekom.github.io/sutura/latest/" class="hover:text-teal-300">telekom.github.io/sutura</a> · Apache-2.0
  </div>
</div>

---
layout: light
---

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex items-center">
  <div class="space-y-6">
    <div class="eyebrow">The problem</div>
    <h1 class="slide-heading" style="text-wrap:balance">An agent with a database connection writes its own SQL</h1>
    <p class="lead max-w-4xl">
      It runs that SQL with the credential of the service. Two things go wrong.
    </p>
    <div class="grid gap-6" style="grid-template-columns:1fr 1fr">
      <TealCallout label="Nobody certified the number">
        <p class="text-neutral-700">"Revenue" can mean something different from what finance means.</p>
      </TealCallout>
      <TealCallout label="The rows are the service's">
        <p class="text-neutral-700">The query reads what the service may read, not what the caller may read. Row-level security has no effect.</p>
      </TealCallout>
    </div>
  </div>
</div>

---
layout: dark
---

<div class="relative z-10 h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow-light">What sutura is</div>
    <h1 class="slide-heading text-white" style="text-wrap:balance">It answers with certified definitions. When it cannot answer correctly, it refuses.</h1>
  </div>
  <div class="grid gap-4" style="grid-template-columns:repeat(3,1fr)">
    <StageCard label="Catalog" name="Definitions arrive as a pinned, hashed snapshot from DataHub, OpenMetadata, files or a database." />
    <StageCard label="Plan" name="A question is compiled into one statement per data system, with bind parameters." />
    <StageCard label="Data systems" name="BigQuery, ClickHouse, Oracle, Postgres, DuckDB, files, Cube. A question can combine several." />
  </div>
</div>

---
layout: white
---

<div class="h-full max-w-6xl mx-auto px-16 py-8 flex flex-col justify-center gap-4">
  <div class="space-y-2">
    <div class="eyebrow">The four properties</div>
    <h1 class="slide-heading" style="font-size:2.4rem">What the design guarantees</h1>
  </div>
  <div>
    <div class="grid gap-4 border-t border-neutral-300 py-3" style="grid-template-columns:240px 1fr">
      <div class="text-sm font-semibold uppercase tracking-[0.18em] text-teal-700">No SQL in the question</div>
      <div class="text-sm leading-relaxed text-neutral-700">The question type has no field for SQL, a table or a filter. An extra field is refused, and every value becomes a bind parameter.</div>
    </div>
    <div class="grid gap-4 border-t border-neutral-300 py-3" style="grid-template-columns:240px 1fr">
      <div class="text-sm font-semibold uppercase tracking-[0.18em] text-teal-700">Definitions from a catalog</div>
      <div class="text-sm leading-relaxed text-neutral-700">sutura does not edit them. Every anchor, a date range with the value the metric must return, runs again before sutura serves.</div>
    </div>
    <div class="grid gap-4 border-t border-neutral-300 py-3" style="grid-template-columns:240px 1fr">
      <div class="text-sm font-semibold uppercase tracking-[0.18em] text-teal-700">A refusal is an answer</div>
      <div class="text-sm leading-relaxed text-neutral-700">A refusal is a result with a reason, not an error to retry. Every outcome goes to the audit sink before sutura returns it.</div>
    </div>
    <div class="grid gap-4 border-t border-b border-neutral-300 py-3" style="grid-template-columns:240px 1fr">
      <div class="text-sm font-semibold uppercase tracking-[0.18em] text-teal-700">Every query runs as the caller</div>
      <div class="text-sm leading-relaxed text-neutral-700">A leg that cannot run as the caller is refused. Where a data system cannot do this, it runs as one identity that the operator declares.</div>
    </div>
  </div>
</div>

---
layout: light
---

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow">What it borrows</div>
    <h1 class="slide-heading" style="text-wrap:balance">Two Apache-2.0 projects did parts of this first</h1>
  </div>
  <div class="grid gap-6" style="grid-template-columns:1fr 1fr">
    <TealCallout label="Wren">
      <p class="text-neutral-700">Compiles a modelled question into SQL over DataFusion. sutura takes the idea of a plan compiled from a model.</p>
    </TealCallout>
    <TealCallout label="Spice">
      <p class="text-neutral-700">Federates queries across data systems, also on DataFusion.</p>
    </TealCallout>
  </div>
  <p class="lead max-w-5xl">
    sutura adds identity: what a question means, who asks it, and whether they may see the answer.
    It runs on Apache DataFusion, Apache Arrow and ADBC.
  </p>
</div>

---
layout: white
---

<div class="h-full px-10 py-6 flex flex-col gap-2">
  <div class="eyebrow">Architecture</div>
  <h1 class="slide-heading" style="font-size:1.9rem">The domain defines the ports. Each adapter implements one.</h1>
  <iframe src="./shared/architecture.html" title="sutura crates and ports" class="w-full flex-1 rounded-lg border border-neutral-200" />
  <p class="text-xs text-neutral-500">Diagram rendered with archify (MIT).</p>
</div>

---
layout: white
---

<div class="h-full px-10 py-6 flex flex-col gap-2">
  <div class="eyebrow">Deployment</div>
  <h1 class="slide-heading" style="font-size:1.9rem">The agent reaches sutura over MCP</h1>
  <iframe src="./shared/deployment.html" title="sutura deployment" class="w-full flex-1 rounded-lg border border-neutral-200" />
  <p class="text-xs text-neutral-500">Diagram rendered with archify (MIT).</p>
</div>

---
layout: light
---

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-5">
  <div class="space-y-2">
    <div class="eyebrow">Deployment</div>
    <h1 class="slide-heading" style="font-size:2.4rem">How to read the diagram</h1>
  </div>
  <div class="grid gap-6" style="grid-template-columns:1fr 1fr">
    <div class="space-y-3 text-neutral-700">
      <p>The MCP gateway is optional. If you use it, it passes the caller's token to sutura. The token must name sutura as its audience.</p>
      <p>sutura verifies the token with the key set of the identity provider, then runs each query in the data system.</p>
      <p>sutura keeps no copy of the grants and no result cache.</p>
    </div>
    <div class="space-y-3 text-neutral-700">
      <p>DataHub holds the definitions. Cube, optionally, serves the metrics to BI tools and APIs as well.</p>
      <p>Each part can change: another catalog, another data system, another OIDC issuer, with or without the gateway, with or without Cube.</p>
    </div>
  </div>
</div>

---
layout: dark
---

<div class="relative z-10 h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow-light">Identity</div>
    <h1 class="slide-heading text-white" style="text-wrap:balance">Two legs: who asks, and who runs the query</h1>
  </div>
  <div class="grid gap-6" style="grid-template-columns:1fr 1fr">
    <DarkCallout label="Leg 1">
      <p class="lead-dark">sutura knows who asks. It verifies the caller's token before it answers, on HTTP and on MCP.</p>
    </DarkCallout>
    <DarkCallout label="Leg 2">
      <p class="lead-dark">The data system runs the query as the caller: BigQuery, ClickHouse and Oracle. Every other data system runs as one identity that the operator declares.</p>
    </DarkCallout>
  </div>
  <p class="lead-dark max-w-5xl">Grants, row policies and masking stay in the data system.</p>
</div>

---
layout: white
---

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow">Showcase 1 - Federated</div>
    <h1 class="slide-heading" style="text-wrap:balance">One question reads two data systems</h1>
  </div>
  <div class="grid gap-4" style="grid-template-columns:repeat(3,1fr)">
    <PhaseCard phase="Catalog" title="DataHub" output="the certified metric and its relationship">
      <li class="text-sm text-neutral-700">Metric <code>recurring_revenue</code></li>
      <li class="text-sm text-neutral-700">Relationship <code>subscription_customer</code></li>
    </PhaseCard>
    <PhaseCard phase="Fact" title="Postgres" output="fct_subscription_monthly">
      <li class="text-sm text-neutral-700">TLS, one declared identity</li>
      <li class="text-sm text-neutral-700">The metric is on this table</li>
    </PhaseCard>
    <PhaseCard phase="Dimension" title="ClickHouse" output="dim_customer">
      <li class="text-sm text-neutral-700">The dimension <code>segment</code></li>
      <li class="text-sm text-neutral-700">Joined to the fact rows by sutura</li>
    </PhaseCard>
  </div>
</div>

---
layout: dark
---

<div class="relative z-10 h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-5">
  <div class="space-y-2">
    <div class="eyebrow-light">Federated - run it</div>
    <h1 class="slide-heading text-white" style="font-size:2.4rem">Three commands, then one container</h1>
  </div>

```sh
cd examples/federated
./setup.sh
docker compose up -d --wait postgres clickhouse datahub
docker compose run --rm federated-loader
```

```sh
docker run --rm --network host --user "$(id -u):$(id -g)" \
  -v "$PWD/conf:/conf:ro" -v "$PWD/.local:/run/sutura:ro" -e SUTURA_CONFIG_DIR=/conf \
  ghcr.io/telekom/sutura:${SUTURA_VERSION:-latest} serve
```

</div>

---
layout: white
---

<div class="h-full max-w-6xl mx-auto px-16 py-8 flex flex-col justify-center gap-4">
  <div class="space-y-2">
    <div class="eyebrow">Federated - ask it</div>
    <h1 class="slide-heading" style="font-size:2.2rem">The question has no SQL in it</h1>
  </div>

```sh
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month",
       "range":{"start":"2026-06-01","end":"2026-07-01"},
       "dimensions":["segment"]}'
```

  <p class="lead" style="font-size:1rem">The metric is on Postgres. The dimension <code>segment</code> is on ClickHouse. sutura reads both and joins them.</p>
</div>

---
layout: white
---

<div class="h-full max-w-6xl mx-auto px-16 py-8 flex flex-col justify-center gap-4">
  <div class="space-y-2">
    <div class="eyebrow">Showcase 2 - Single player</div>
    <h1 class="slide-heading" style="font-size:2.2rem">One command, and the answer says who read the data</h1>
  </div>

```sh
docker compose -f examples/single-player/compose.yaml up
curl -s -X POST http://127.0.0.1:8080/v1/query -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}'
```

```json
{
  "outcome": "answer",
  "executed_as": [{ "source": "local", "posture": "shared-service-user" }],
  "columns": ["region", "period", "recurring_revenue"],
  "rows": [["central", "2026-06-01", "51739"], ["east", "2026-06-01", "32598"], ...]
}
```

</div>

---
layout: light
---

<div class="h-full max-w-6xl mx-auto px-16 py-8 flex flex-col justify-center gap-4">
  <div class="space-y-2">
    <div class="eyebrow">A refusal is an answer</div>
    <h1 class="slide-heading" style="font-size:2.2rem">A grain nobody certified is refused, with the way forward</h1>
  </div>

```text
refused: GrainNotSupported
  the metric exists and does not declare that time resolution
  metric: recurring_revenue
  grain: day
  remedy: Ask at a grain the metric lists - a finer grain is a number nobody certified.
```

  <p class="lead" style="font-size:1rem">Over HTTP, a refusal has its own status: <code>422</code> with the code <code>grain_not_supported</code>.</p>
</div>

---
layout: dark-closing
---

<div class="relative z-10 h-full max-w-6xl mx-auto px-16 py-12 flex flex-col justify-center gap-8">
  <div class="space-y-4">
    <div class="eyebrow-light">Try it</div>
    <h1 class="slide-title text-white" style="font-size:4rem">Start with the examples</h1>
  </div>
  <div class="grid gap-4" style="grid-template-columns:repeat(3,1fr)">
    <DarkCallout label="Documentation"><a href="https://telekom.github.io/sutura/latest/" class="hover:text-teal-300">telekom.github.io/sutura</a></DarkCallout>
    <DarkCallout label="Source and issues"><a href="https://github.com/telekom/sutura" class="hover:text-teal-300">github.com/telekom/sutura</a></DarkCallout>
    <DarkCallout label="Report a vulnerability">Privately, through SECURITY.md in the repository</DarkCallout>
  </div>
  <div class="mono-label" style="color:rgba(94,234,212,0.78)">Apache-2.0</div>
</div>
