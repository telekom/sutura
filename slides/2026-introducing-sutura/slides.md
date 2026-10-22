---
theme: ./theme
title: "sutura - secure, pluggable, multiplayer AI + semantic data experience"
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
  <div class="eyebrow-light" style="font-size:1rem">
    Multiplayer semantic layer
  </div>
  <div
    class="grid w-full gap-10"
    style="grid-template-columns:1.6fr 0.4fr;align-items:center"
  >
    <div class="space-y-5">
      <h1 class="slide-title text-white" style="font-size:7rem">sutura</h1>
      <p class="lead-dark" style="font-size:2.75rem;line-height:1.25">
        Secure, pluggable, multiplayer <span style="color:#5eead4">AI</span><br>
        <span style="color:#5eead4">semantic data</span> experience
      </p>
    </div>
    <div class="flex justify-center">
      <img
        src="/shared/sutura.svg"
        alt="sutura"
        class="h-auto object-contain"
        style="max-height:15rem"
      />
    </div>
  </div>
  <div
    class="mono-label"
    style="color:rgba(94,234,212,0.78);line-height:1.6;letter-spacing:0.04em;font-size:1rem"
  >
    <a
      href="https://telekom.github.io/sutura/latest/"
      class="hover:text-teal-300"
    >telekom.github.io/sutura</a> · Apache-2.0
  </div>
</div>

<!--
sutura is a secure, pluggable, multiplayer AI + semantic data experience: answers about data, as the person asking.
Chat with your data, securely and multiplayer.
Certified metrics from your semantic layer, no lock-in. HTTP + MCP, many data sources.
-->

---

```yaml
layout: light
```

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex items-center">
  <div class="w-full space-y-8">
    <div class="eyebrow" style="font-size:1rem">
      Agent + database = its own SQL
    </div>
    <h1 class="slide-heading" style="text-wrap:balance;font-size:3.6rem">
      Agentic trust and context problem
    </h1>
    <div class="grid gap-6" style="grid-template-columns:1fr 1fr">
      <TealCallout label="Governance misalignment">
        <ul
          class="text-neutral-700 list-disc pl-6 space-y-2 mt-2"
          style="font-size:1.35rem"
        >
          <li>Nobody certified the number</li>
          <li>"Revenue" ≠ finance's revenue</li>
          <li>Data quality unclear</li>
        </ul>
      </TealCallout>
      <TealCallout label="Security challenges">
        <ul
          class="text-neutral-700 list-disc pl-6 space-y-2 mt-2"
          style="font-size:1.35rem"
        >
          <li>Service credential, not the caller's</li>
          <li>Row-level security: no effect</li>
          <li>IAM re-implemented in the service</li>
        </ul>
      </TealCallout>
    </div>
  </div>
</div>

<!--
An agent with a database connection writes its own SQL, and runs it with the credential of the service.
Two things go wrong, governance misalignment and security challenges:
- "Revenue" can mean something different from what finance means.
- Data quality is unclear: governance is missing.
- The query reads what the service may read, not what the caller may read. Row-level security has no effect.
- Each service re-implements IAM instead of relying on the grants of the data system.
-->

---

```yaml
layout: white
```

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex items-center">
  <div class="w-full space-y-8">
    <div class="eyebrow" style="font-size:1rem">
      Enterprise data
    </div>
    <h1 class="slide-heading" style="text-wrap:balance;font-size:3.6rem">
      Why sutura
    </h1>
    <div class="grid gap-6" style="grid-template-columns:1fr 1fr">
      <TealCallout label="Why">
        <ul
          class="text-neutral-700 list-disc pl-6 space-y-2 mt-2"
          style="font-size:1.35rem"
        >
          <li>Cost</li>
          <li>Integration</li>
          <li>Sovereignty</li>
        </ul>
      </TealCallout>
      <TealCallout label="Enterprise has many data platforms">
        <ul
          class="text-neutral-700 list-disc pl-6 space-y-2 mt-2"
          style="font-size:1.35rem"
        >
          <li>Independent</li>
          <li>Sensitive data</li>
          <li>Bridging the gaps</li>
        </ul>
      </TealCallout>
    </div>
  </div>
</div>

<!--
Why: cost, integration, sovereignty. Enterprise has many data platforms: independent, sensitive data, bridging the gaps.
-->

---

```yaml
layout: dark
```

<div class="relative z-10 h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow-light" style="font-size:1rem">
      Pluggable, no lock-in semantic data experience
    </div>
    <h1
      class="slide-heading text-white"
      style="text-wrap:balance;font-size:3.6rem"
    >Certified answers - or a refusal</h1>
  </div>
  <div class="grid gap-4" style="grid-template-columns:repeat(3,1fr)">
    <StageCard label="Pluggable" name="Catalog + data">
      <div class="mt-3 text-lg text-slate-300">
        DataHub · OpenMetadata · Cube · files (OKF, ODCS)
      </div>
      <div class="mt-1 text-lg text-slate-300">
        BigQuery · ClickHouse · Oracle · Postgres · DuckDB
      </div>
      <div class="mt-1 text-lg text-slate-300">Data stays in place</div>
    </StageCard>
    <StageCard label="Verified metrics" name="No SQL from the model">
      <div class="mt-3 text-lg text-slate-300">
        Only certified metrics parse · values are bind parameters
      </div>
      <div class="mt-1 text-lg text-slate-300">Less SQL/prompt injection</div>
    </StageCard>
    <StageCard label="DataFusion + ADBC" name="Federation">
      <div class="mt-3 text-lg text-slate-300">
        One question, several systems · bind parameters
      </div>
      <div class="mt-1 text-lg text-slate-300">
        ADBC: Arrow-native drivers per data system
      </div>
    </StageCard>
  </div>
  <p class="text-base text-slate-400">
    Apache-2.0 · open standards: DataFusion · Arrow · ADBC · MCP
  </p>
</div>

<!--
sutura answers with certified definitions. When it cannot answer correctly, it refuses.
- Pluggable: catalog and data systems. Definitions arrive as a pinned, hashed snapshot from the semantic layer you already have - DataHub, OpenMetadata, Cube, data contracts, files or a database.
- Verified metrics: the question type has no field for SQL, a table or a filter. Only certified metrics and dimensions parse, an extra field is refused, and every value becomes a bind parameter. That closes the SQL-injection path and shrinks what a prompt injection can make the agent do - it cannot stop the injection itself. Definitions arrive pinned and hashed, and every anchor runs again before sutura serves.
- DataFusion federation: a question is compiled into one statement per data system, with bind parameters. A question can combine several data systems: BigQuery, ClickHouse, Oracle, Postgres, DuckDB, files, Cube.
No lock-in: sutura reads definitions from where they already live.
- Definitions: DataHub, OpenMetadata, Cube, Open Data Contract Standard documents, Frictionless Table Schema descriptors, a database dictionary, or plain markdown files with YAML frontmatter.
- Swap by config: one setting, catalog.kind, selects the source. A Wren MDL manifest converts with sutura import wren. The question an agent asks does not change.
- Data stays in place: sutura runs each query in your data system, as the caller. It keeps no copy, and your grants and row policies still apply.
Everything is Apache-2.0, on open standards.
-->

---

```yaml
layout: light
```

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow" style="font-size:1rem">Inspiration</div>
    <h1 class="slide-heading" style="text-wrap:balance;font-size:3.2rem">
      Standing on the shoulders of giants
    </h1>
  </div>
  <div class="grid gap-6" style="grid-template-columns:repeat(3,1fr)">
    <TealCallout label="Wren">
      <ul
        class="text-neutral-700 list-disc pl-6 space-y-2 mt-2"
        style="font-size:1.35rem"
      >
        <li>Semantic model → SQL</li>
        <li>On DataFusion</li>
      </ul>
    </TealCallout>
    <TealCallout label="Spice">
      <ul
        class="text-neutral-700 list-disc pl-6 space-y-2 mt-2"
        style="font-size:1.35rem"
      >
        <li>Federated SQL on DataFusion</li>
      </ul>
    </TealCallout>
    <TealCallout label="sutura adds">
      <ul
        class="text-neutral-700 list-disc pl-6 space-y-2 mt-2"
        style="font-size:1.35rem"
      >
        <li>Polyglot adapters</li>
        <li>Pluggable connectors</li>
      </ul>
    </TealCallout>
  </div>
  <p class="lead" style="font-size:1.35rem">
    <strong>Pushing the ecosystem forward:</strong> upstream contributions (OIDC, fuzzing)
  </p>
  <p class="text-base text-neutral-500">Built on DataFusion · Arrow · ADBC</p>
</div>

<!--
Inspiration: two Apache-2.0 projects. sutura uses no code from either; the shape is theirs.
- Wren compiles a question over a semantic model into SQL, on DataFusion. From Wren, sutura takes the idea that the model, not the prompt, makes a number defensible. `sutura import wren` converts a Wren MDL manifest into a sutura catalog.
- Spice federates one SQL query across many data systems and accelerates it locally, also on DataFusion.
sutura adds:
- Polyglot adapters: one plan, rendered in the SQL dialect of each data system.
- Pluggable connectors: catalogs and data systems are adapters behind ports, chosen by config.
And identity: what a question means, who asks it, and whether they may see the answer.
Pushing the ecosystem forward: fixes go upstream as contributions - OIDC, fuzzing - rather than staying as local patches.
- Definitions come from your catalog as a pinned snapshot, and the question is a type with no field for SQL.
- Each leg of the plan runs as the caller. When it cannot, sutura refuses and gives the reason. Where a data system cannot run as the caller, the operator declares one identity for it.
It runs on Apache DataFusion, Apache Arrow and ADBC.
-->

---

```yaml
layout: white
```

<div class="h-full px-10 py-6 flex flex-col gap-2">
  <div class="eyebrow">Architecture · 1/2</div>
  <h1 class="slide-heading" style="font-size:1.9rem">Transports and service reach the domain's ports</h1>
  <iframe src="./shared/architecture.html#top" title="sutura callers, transports and domain ports" class="w-full flex-1 rounded-lg border border-neutral-200" />
</div>

<!--
The agent speaks MCP, an app speaks HTTP. Both transports verify the token with the key set of the identity provider, then hand the question to the service. The service compiles it into a plan and calls the ports of the domain.
-->

---

```yaml
layout: white
```

<div class="h-full px-10 py-6 flex flex-col gap-2">
  <div class="eyebrow">Architecture · 2/2</div>
  <h1 class="slide-heading" style="font-size:1.9rem">Domain defines ports · adapters implement them</h1>
  <iframe src="./shared/architecture.html#bottom" title="sutura domain ports and adapters" class="w-full flex-1 rounded-lg border border-neutral-200" />
</div>

<!--
The domain defines the ports. Each adapter implements one. Nothing depends on an adapter.
-->

---

```yaml
layout: white
clicks: 4
```

<div class="h-full px-10 py-6 flex flex-col gap-2">
  <div class="eyebrow">Deployment{{ ['', ' · request path', ' · identity', ' · data systems', ' · semantics'][$clicks] }}</div>
  <h1 class="slide-heading" style="font-size:1.9rem">Agent → MCP → sutura</h1>
  <iframe :src="'./shared/deployment.html#fit' + ['', '/path', '/identity', '/data', '/semantics'][$clicks]" title="sutura deployment" class="w-full flex-1 rounded-lg border border-neutral-200" />
</div>

<!--
Press → to zoom through request path, identity, data systems and semantics; the audience window follows. Ad hoc: the + / − buttons, scroll or pinch, drag to pan, double-click to reset - these stay in the window you use.
The agent reaches sutura over MCP, optionally through an MCP gateway that passes the caller's token.
sutura verifies the caller with the OIDC issuer, then runs each query in the data system; BigQuery through the workload identity pool.
DataHub holds the definitions. Cube serves the metrics to BI tools and APIs.
-->

---

```yaml
layout: dark
```

<div class="relative z-10 h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow-light">Identity</div>
    <h1 class="slide-heading text-white" style="text-wrap:balance">
      Two legs: who asks, who runs it
    </h1>
  </div>
  <div class="grid gap-6" style="grid-template-columns:1fr 1fr">
    <DarkCallout label="Leg 1 - who asks · per deployment">
      <ul class="lead-dark list-disc pl-5 space-y-1">
        <li><b>direct</b> - the caller's own token</li>
        <li><b>behind-gateway</b> - the gateway's signed assertion</li>
        <li><b>none</b> - single-user only, with a reason</li>
      </ul>
    </DarkCallout>
    <DarkCallout label="Leg 2 - who runs it · per source">
      <ul class="lead-dark list-disc pl-5 space-y-1">
        <li>
          <b>as the caller</b>
          <ul class="list-[circle] pl-5">
            <li><b>multi-hop</b> exchange → caller signs in</li>
            <li>federation → caller's own principal</li>
            <li>session switch → statement runs as caller</li>
          </ul>
        </li>
        <li><b>shared</b> - one declared identity</li>
      </ul>
    </DarkCallout>
  </div>
  <p class="lead-dark max-w-5xl">
    Any OIDC issuer · grants, row policies, masking stay in the data system
  </p>
</div>

<!--
Leg 1 is set once for the deployment. Leg 2 is set on each source, so one answer can mix postures. executed_as names each one.
- Leg 1, direct: the caller sends their own access token. sutura verifies signature, issuer, audience and expiry against the key set of the issuer.
- Leg 1, behind-gateway: a gateway signs the caller in and sends a signed assertion. sutura verifies the assertion.
- Leg 1, none: only in a single-user deployment, and only with a written reason. A source that runs as the caller needs leg 1, or sutura does not start.
- Leg 2, as the caller: the adapter carries the caller to the data system. Three ways:
  - multi-hop token exchange: a declared chain of hops (RFC 8693, on-behalf-of) exchanges the caller's token, hop by hop, until it has the audience the data system expects - for example issuer token, then Keycloak, then the pool. The data system signs in with the result.
  - federation: a cloud identity pool turns the caller's token into the caller's own principal. No service account.
  - session switch: sutura signs in as its service user and runs each statement as the caller.
- Leg 2, shared: one identity that the operator declares. In a multi-user deployment, each shared source needs a written reason, or sutura does not start.
Which data system supports which posture is in its integration page, and more data systems will follow.
Grants, row policies and masking stay in the data system.
-->

---

```yaml
layout: white
```

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow">Showcase 1 - Federated</div>
    <h1 class="slide-heading" style="text-wrap:balance">
      One question, two data systems
    </h1>
  </div>
  <div class="grid gap-4" style="grid-template-columns:repeat(3,1fr)">
    <PhaseCard phase="Catalog" title="DataHub" output="metric + relationship">
      <li class="text-sm text-neutral-700">
        <code style="color:#336791">recurring_revenue</code>
      </li>
      <li class="text-sm text-neutral-700">
        <code>subscription_customer</code>
      </li>
    </PhaseCard>
    <PhaseCard
      phase="Fact"
      title="Postgres"
      output="fct_subscription_monthly"
      accent="#336791"
    >
      <li class="text-sm text-neutral-700">TLS, one declared identity</li>
      <li class="text-sm text-neutral-700">Holds the metric</li>
    </PhaseCard>
    <PhaseCard
      phase="Dimension"
      title="ClickHouse"
      output="dim_customer"
      accent="#b45309"
    >
      <li class="text-sm text-neutral-700">
        Dimension <code style="color:#b45309">segment</code>
      </li>
      <li class="text-sm text-neutral-700">Joined by sutura</li>
    </PhaseCard>
  </div>
  <p
    class="slide-heading text-neutral-700"
    style="font-size:1.55rem;font-style:italic"
  >
    “What was our <span style="color:#336791">recurring revenue in June 2026</span>, <span style="color:#b45309">by customer segment</span>?”
  </p>
</div>

<!--
DataHub holds the certified metric recurring_revenue and the relationship subscription_customer.
The metric is on the Postgres fact table, reached over TLS with one declared identity.
The dimension segment is on ClickHouse; sutura joins it to the fact rows.
Example question: what was our recurring revenue in June 2026, by customer segment? That is the request on the next slides.
-->

---

```yaml
layout: dark
```

<div class="relative z-10 h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-5">
  <div class="space-y-2">
    <div class="eyebrow-light">Federated - run it</div>
    <h1 class="slide-heading text-white" style="font-size:2.4rem">Three commands, one container</h1>
  </div>

```sh
cd examples/federated  # the federated example
./setup.sh  # generate local secrets
docker compose up -d --wait postgres clickhouse datahub  # start the three systems
docker compose run --rm federated-loader  # load the DataHub catalog
```

```sh
image=ghcr.io/telekom/sutura:${SUTURA_VERSION:-latest}  # the released image
args=(  # container options
  --rm --network host --user "$(id -u):$(id -g)"  # throwaway, host network, you
  -v "$PWD/conf:/conf:ro"  # config, read-only
  -v "$PWD/.local:/run/sutura:ro"  # secrets, read-only
  -e SUTURA_CONFIG_DIR=/conf  # where base.yaml lives
)
docker run "${args[@]}" "$image" serve  # start sutura
```

</div>

<style>
.slidev-code { font-size: 1rem !important; line-height: 1.5 !important; }
</style>

---

```yaml
layout: white
```

<div class="h-full max-w-6xl mx-auto px-16 py-8 flex flex-col justify-center gap-4">
  <div class="space-y-2">
    <div class="eyebrow">Federated - ask it</div>
    <h1 class="slide-heading" style="font-size:2.6rem">No SQL in the question</h1>
  </div>

```sh
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month",
       "range":{"start":"2026-06-01","end":"2026-07-01"},
       "dimensions":["segment"]}' | jq
```

```json
{
  "outcome": "answer",
  "executed_as": [{ "source": "billing", "posture": "shared-service-user" },
                  { "source": "crm", "posture": "shared-service-user" }],
  "columns": ["segment", "period", "recurring_revenue"],
  "rows": [["business", "2026-06-01", "66630"], ["consumer", "2026-06-01", "123119"], ...]
}
```

  <p class="lead" style="font-size:1.3rem">Metric → <span style="color:#336791">Postgres</span> · <code style="color:#b45309">segment</code> → <span style="color:#b45309">ClickHouse</span> · join → sutura</p>
</div>

<style>
.slidev-code { font-size: 0.95rem !important; line-height: 1.45 !important; }
</style>

<!--
The metric is on Postgres. The dimension segment is on ClickHouse. sutura reads both and joins them.
The answer names both sources in executed_as: billing (Postgres) and crm (ClickHouse). Amounts are in cents.
-->

---

```yaml
layout: white
```

<div class="h-full max-w-6xl mx-auto px-16 py-8 flex flex-col justify-center gap-4">
  <div class="space-y-2">
    <div class="eyebrow">Showcase 2 - Single player</div>
    <h1 class="slide-heading" style="font-size:2.4rem">One command · the answer says who read the data</h1>
  </div>

```sh
docker compose -f examples/single-player/compose.yaml up
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month",
       "range":{"start":"2026-06-01","end":"2026-07-01"},
       "dimensions":["region"]}' | jq
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

<style>
.slidev-code { font-size: 1rem !important; line-height: 1.45 !important; }
</style>

<!--
Point at executed_as: every answer names the identity each source ran as.
-->

---

```yaml
layout: light
```

<div class="h-full max-w-6xl mx-auto px-16 py-8 flex flex-col justify-center gap-4">
  <div class="space-y-2">
    <div class="eyebrow" style="font-size:1rem">Uncertified grain → refused, with a remedy</div>
    <h1 class="slide-heading" style="font-size:3.2rem">A refusal is an answer</h1>
  </div>

```text
refused: GrainNotSupported
  the metric exists and does not declare that time resolution
  metric: recurring_revenue
  grain: day
  remedy: Ask at a grain the metric lists -
          a finer grain is a number nobody certified.
```

  <p class="lead" style="font-size:1.5rem">HTTP: <code>422</code> · <code>grain_not_supported</code></p>
</div>

<style>
.slidev-code { font-size: 1.35rem !important; line-height: 1.5 !important; }
</style>

<!--
A grain nobody certified is refused, with the way forward. Over HTTP, a refusal has its own status: 422 with the code grain_not_supported.
The remedy is one line in the real output; the slide wraps it.
-->

---

```yaml
layout: white
```

<div class="h-full max-w-6xl mx-auto px-16 py-10 flex flex-col justify-center gap-6">
  <div class="space-y-2">
    <div class="eyebrow" style="font-size:1rem">Safety patterns</div>
    <h1 class="slide-heading" style="font-size:3.2rem">
      Secure by design, and by verification
    </h1>
  </div>
  <div class="grid gap-5" style="grid-template-columns:1fr 1fr">
    <TealCallout label="Rust">
      <ul
        class="text-neutral-700 list-disc pl-6 space-y-1 mt-1"
        style="font-size:1.2rem"
      >
        <li>No <code>unsafe</code> in all crates but one</li>
        <li>Clippy pedantic + restriction lints</li>
        <li>No <code>unwrap</code>, <code>panic</code> or indexing</li>
      </ul>
    </TealCallout>
    <TealCallout label="Types">
      <ul
        class="text-neutral-700 list-disc pl-6 space-y-1 mt-1"
        style="font-size:1.2rem"
      >
        <li>Newtypes that parse · typed errors</li>
        <li>A question has no field for SQL</li>
      </ul>
    </TealCallout>
    <TealCallout label="Verification">
      <ul
        class="text-neutral-700 list-disc pl-6 space-y-1 mt-1"
        style="font-size:1.2rem"
      >
        <li>Fuzzing: tokens, keys, catalogs, SQL</li>
        <li>Golden suite per adapter</li>
        <li>Every new test red before green</li>
      </ul>
    </TealCallout>
    <TealCallout label="Supply chain">
      <ul
        class="text-neutral-700 list-disc pl-6 space-y-1 mt-1"
        style="font-size:1.2rem"
      >
        <li>Hermetic nix build · static musl</li>
        <li>cargo-deny · osv-scanner</li>
        <li>Signed images · SBOM · SLSA</li>
      </ul>
    </TealCallout>
  </div>
</div>

<!--
Security is the product, not a layer, so each rule is held by a type, a lint, a hook or a gate.
- Rust 2024. unsafe_code is forbidden at every crate root but one. dead_code, unused results and unreachable pub items are errors. Clippy runs the all, nursery, cargo, pedantic and restriction groups with warnings denied, and unwrap, expect, panic and slice indexing are refused outside tests.
- Types: newtypes parse at the boundary, errors are typed, and dependencies point inward. The question type has no field for SQL, a table or a filter, so an uncertified question cannot be expressed.
- Verification: five fuzz targets - tokens, key sets, catalog documents, question bodies, SQL expressions. A golden suite holds every adapter to the same answers. A new test must fail on the base and pass with the change.
- Supply chain: the build is hermetic with nix, and every release ships statically linked musl binaries and images for x86_64 and aarch64 beside the glibc ones. cargo-deny and osv-scanner check dependencies, a secret sweep runs before every push, and zizmor checks the CI workflows. Release images are signed with cosign, with a CycloneDX SBOM and SLSA provenance.
-->

---

```yaml
layout: dark-closing
```

<div
  class="relative z-10 h-full max-w-6xl mx-auto px-16 py-12 grid gap-8"
  style="grid-template-columns:1fr auto"
>
  <div class="flex flex-col justify-between">
    <div class="space-y-5">
      <div class="eyebrow-light" style="font-size:1rem;letter-spacing:0.16em">
        Join the sovereign semantic data experience
      </div>
      <h1 class="slide-title text-white" style="font-size:4.5rem">
        Build it with us
      </h1>
      <p class="lead-dark" style="font-size:1.6rem">
        Run an example · open an issue · send a pull request
      </p>
      <a
        href="https://telekom.github.io/sutura/latest/"
        class="block text-teal-300 hover:text-teal-200"
        style="font-size:2rem"
      >telekom.github.io/sutura</a>
      <p class="lead-dark" style="font-size:1.2rem">
        <span class="text-teal-300 font-semibold">Roadmap:</span> agentic BI - BI tool (persistent object model) + MCP + sutura
      </p>
    </div>
    <div class="mono-label" style="color:rgba(94,234,212,0.78);font-size:1rem">
      Apache-2.0
    </div>
  </div>
  <div class="flex flex-col items-end justify-between">
    <img
      src="/shared/sutura.svg"
      alt="sutura"
      class="object-contain"
      style="height:9rem"
    />
    <a
      href="https://telekom.github.io/sutura/latest/"
      class="block bg-white rounded-lg p-3"
    >
      <img
        src="/docs-qr.svg"
        alt="QR code: telekom.github.io/sutura"
        style="width:10rem;height:10rem;image-rendering:pixelated"
      />
    </a>
  </div>
</div>

<!--
Build it with us: run an example, open an issue, send a pull request. Everything starts at the docs.
Roadmap thought, not a commitment: agentic BI - a BI tool with an agent over MCP, and sutura underneath, so every chart comes from the certified metrics, read as the person asking. The idea is a persistent object model: charts and dashboards are objects the agent creates and edits over MCP, and they stay. Help shape it: ideas and use cases are welcome.
-->
