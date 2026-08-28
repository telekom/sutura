---
title: Federating across different data systems
description: The two tracks for BigQuery, Postgres and Oracle - one source is a whole-query pushdown in that system's dialect and involves no federation, several sources are per-source subplans rendered by sutura-sql and combined by DataFusion - why the combiner is ours and not an unparser, why a compiled dialect is a golden and not a feature flag, which of the six single-source assumptions each track forces in what order, and why attaching several databases was declined outright rather than sequenced.
---

# Federating across different data systems

Status: accepted, and **nothing here is built.** It decides a shape and an order; it adds no code and
no dependency.
[Several databases behind one data system](0006-several-databases-behind-one-data-system.md) declined
both a federation crate and attaching several databases below the port. This record decides
what is built instead, for the systems that are three separate logins: BigQuery, Postgres and Oracle.

**Amended.** This record decided that a measure which cannot be re-aggregated across legs is
REFUSED. [The plan](0009-the-plan-from-one-source-to-many.md) decides the opposite and supersedes it:
push what descends, and otherwise retrieve finer-grained rows and compute above. Three things below
are therefore superseded and marked in place - step 1's `RefusalReason` variant, the consequence that
six of eleven metrics refuse a cross-source dimension, and the open question of whether `Avg` is
rewritten or refused. The reasoning that produced the refusal is kept, because it is why the pull-up
needs a bound.

## The decision

**There are two tracks, and only the second one is federation.**

**Track 1 - one data source: query it directly.** Compile the semantic plan to that system's dialect
and push the whole query down. One `QueryPlan`, one statement, one `Warehouse`, one result. This is
what `sutura-sql` and the two existing adapters already do; adding BigQuery, Oracle or Postgres to it
adds a `Dialect` variant, a golden family and an adapter, and relaxes **none** of the six
single-source assumptions. No federation is involved and none should be implied.

**Track 2 - several data sources: our renderer per source, DataFusion as the combiner.** The
semantic plan splits into per-source subplans, **each of which is itself a whole mono-source
`QueryPlan`**; each is rendered by `sutura-sql` in its own dialect with its bind parameters and forced
quoting intact; each executes through its own `Warehouse` adapter under its own credential; and
DataFusion joins and re-aggregates the results. This is the architecture.

**The combiner is DataFusion. The generator is never DataFusion.** `datafusion-federation`'s route -
DataFusion's unparser rendering the pushed statement - is declined, and this is the sentence that
keeps the invariants: a value from a question never reaches a statement as text, because every leg is
a `GeneratedQuery` with statement and parameters in separate fields; and no SECOND parser enters the
closure. Stating that precisely, because an overstated control is itself the defect here:
`polyglot-sql` IS a parser and is already in the closure, the golden suite parses every statement it
pins, and `sutura_sql::expression` parses at load. What holds is narrower and still worth having -
nothing parses on the QUERY PATH, and `datafusion`'s `sql` feature stays off, so no second parser and
no unparser arrive. `polyglot-sql` occupies
the position `sqlglot` occupies in comparable systems: **generation per dialect, never
transpilation.**

**Attaching several databases is declined, DuckDB included.** Every source reaches the semantic
compiler and standard credentialed access; there is no shape in which one connection stands in for
several sources. It builds none of the track-2 machinery, and the cheapest continuation of it -
attaching a Postgres instead of a file - is the composing-adapter-with-a-synthetic-source-name that
ADR 0006 refused in advance. **Two DuckDB files as two `SourceName`s are the first instance of
track 2**, and that is where the machinery gets built. The argument is in its own section below.

**And the first thing built is neither track.** It is a decision about which aggregates survive being
computed at a finer grouping and added back up, because two of the six in the vocabulary do not, and
**six of the eleven metrics in the shipped corpus use one of those two.** That is track 2's version of
the change that compiles, passes and answers wrongly.

### The connectors this is built for

Recorded because the shape above is chosen to fit it, not to be exhaustive or scheduled.

| Side | Now | Intended |
| --- | --- | --- |
| Metadata | a directory of markdown documents with YAML frontmatter | a second markdown/YAML convention, Datahub, OpenMetadata, a catalog held in an RDBMS carrying prompt instructions, BPMN, RDF |
| Data | DuckDB files and local CSV/Parquet through the engine | DuckDB, Postgres, BigQuery, Oracle |

Both sides are ports that already exist, and neither list changes the tool surface. A metadata
provider is a `SemanticCatalog`; a data system is a `Warehouse`. Track 2 changes how many
`Warehouse`s a service holds and nothing about how many `SemanticCatalog`s.

## Which claims were run, and which were reasoned

The distinction is kept per claim rather than declared once, because it is the whole credibility of a
record that recommends work nobody has done.

**Run here:** every dialect-rendering fact, from a throwaway crate with its own `[workspace]` table
that renders this repository's own statement shape through the same three calls
`crates/sutura-sql/src/generate.rs:302-307` makes. Every DuckDB fact, against the pinned
`v1.5.5 (Variegata) d8cdaa33fd` from `nix/duckdb.nix`. Every count over the corpus, the aggregate
vocabulary and the dialect features, from the files.

**Reasoned, not run:** what BigQuery and Oracle do with a statement, because neither is reachable
from here. Whether Arrow Flight SQL's prepared-statement binding carries our parameters. Each is
labelled where it is used.

**Taken from elsewhere and not re-derived:** the six single-source assumptions and their citations,
from the survey behind ADR 0006, and that record's measurements of the declined federation crate.

## Track 1: one data source, queried directly

The cheap, high-value track, and the one to build first because it is where the dialect work lives
and track 2 needs the same dialects.

### It is a dialect-coverage question before it is an adapter question

`Cargo.toml:102-110` compiles **three** dialect features: `dialect-duckdb`,
`dialect-postgresql`, `dialect-clickhouse`. `sutura_sql::Dialect` is the matching three-variant closed
enum, and its own doc comment states the rule this record leans on:

> A closed set rather than a passthrough of the dialect layer's thirty-three, because each entry here
> is a claim that we generate correct SQL for it and have a golden that says so.

**BigQuery and Oracle are not compiled.** So track 1 for either is not "write an adapter"; it is
"make the claim, with the golden behind it", and the adapter comes after.

### What enabling a dialect costs, verified

`polyglot-sql` 0.9.2 declares 33 dialect features, `dialect-oracle` and `dialect-bigquery` among
them, and both have real implementation modules. Checked in the registry source and again in the
upstream checkout, which is the same version.

**Each feature is `[]` - an empty feature list.** It gates a module and pulls no dependency, so
enabling one adds no crate, no licence, no `deny.toml` entry and nothing for `check-boundaries` to
see. The cost is compile time, binary size, and the goldens.

That last one is the whole cost, and it is not small. Rendered through the same call this repository
makes - `Dialect::get(t).generator_config()`, `always_quote_identifiers = true` - the statement shape
from `recurring-revenue-by-region__sql@postgres.snap` came back like this:

| Target | Quoting | Time bucket | Row cap | Placeholder |
| --- | --- | --- | --- | --- |
| DuckDB | `"x"` correct | `DATE_TRUNC('month', c)` correct | `LIMIT` correct | `?` correct |
| Postgres | `"x"` correct | correct | correct | `$1` correct, ours |
| ClickHouse | `"x"` correct | `dateTrunc(...)` correct | correct | `?` correct |
| **BigQuery** | `` `x` `` correct | `DATE_TRUNC('month', c)` - **wrong argument order and a quoted part** | `LIMIT` correct | `?` correct |
| **Oracle** | `"x"` correct | `DATE_TRUNC(...)` - **no such function** | `LIMIT` - **no such clause** | `?` / `$1` - **neither** |

The BigQuery row is a defect **this repository already predicted, in code, from the other side.**
`crates/sutura-sql/src/expression/refusal.rs:115-118` gives the reason `DATE_TRUNC` is a refused
construct in the authored-SQL hatch:

> every dialect spells the rest differently: `generate_date_trunc` special-cases exactly two dialect
> families, which means **a third compiled dialect would silently get its argument order wrong**

That was written about the hatch and it is a statement about the generator. The probe confirms it from
the generate path: the generic-function branch special-cases ClickHouse and nothing else, and the
semantic-node branch special-cases two dialect families. BigQuery is the third compiled dialect, and
it silently gets its argument order wrong. Reasoned, not run, for the consequence at BigQuery itself:
its signature takes the part as an unquoted keyword in second position, so that call is an error
there rather than a wrong number - a better failure, and still a defect the first golden catches.

### Why a dialect is a golden and not a flag: three dead config fields

The reason none of this can be delegated to the dialect layer's configuration is that its per-dialect
fields are not reliably read. Three are now known, and this repository found two of them
independently before this record:

| Field | Set by | Read by |
| --- | --- | --- |
| `parameter_token` | per dialect | **nothing.** Recorded at `crates/sutura-sql/src/dialect.rs:8-11`, which is why `PlaceholderStyle` exists |
| `aggregate_filter_supported` | six dialects set it false | **nothing.** Recorded at `crates/sutura-sql/src/expression/refusal.rs:122-125` |
| `limit_fetch_style`, with a `FetchFirst` variant documented for Oracle | four dialects | **nothing.** Zero read sites in the generator - verified by grep, and verified by setting it and watching the output not change |

And the semantic rewrites that *would* fix Oracle exist and are unreachable. `dialects/oracle.rs:298`
maps `DATE_TRUNC` to Oracle's `TRUNC`. That mapping lives in `transform_function`, reached from
`transform_expr`, which is `#[cfg(feature = "transpile")]` - and `transpile` is deliberately not
compiled here. **The invariant that keeps a transpiler out also keeps the per-dialect rewrite layer
out**, which is a cost of that invariant worth stating plainly rather than a reason to revisit it:
the layer's default `unsupported_level` discards its own diagnostic, which is why it is off.

Two of Oracle's three defects are repairable on our side, and the repair was run. Rendering
`TRUNC(col, 'MM')`, `ParameterStyle::Colon`, and taking the row limit into our own hands:

```sql
SELECT "dim_customer"."region" AS "region",
       CAST(TRUNC("fct_subscription_monthly"."month", 'MM') AS DATE) AS "period",
       SUM("fct_subscription_monthly"."mrr_cents") AS "recurring_revenue"
FROM "fct_subscription_monthly" LEFT JOIN "dim_customer" ON ...
WHERE ... >= :1 AND ... < :2 AND ... = :3
GROUP BY ... ORDER BY ... LIMIT 10001
```

Everything correct except the clause the dead knob was supposed to write. So **the row-limit clause
joins placeholder style and forced quoting as a third thing `dialect.rs` owns per dialect**, and the
counted `LIMIT 10001` claim that `check-guidance` holds becomes dialect-aware on the day Oracle lands.

One more thing forced quoting does, reasoned and not run: Oracle folds an unquoted identifier to
upper case when the table is created, and we quote every identifier always. A model declaring
`table: dim_customer` therefore names `"dim_customer"`, which an Oracle schema built the ordinary way
does not have. Forced quoting is an invariant, so the catalog carries the stored case - which is a
documentation and validation matter, not a generator one, and it is the sort of thing an anchor
catches on the first day rather than a golden.

### Oracle: the driver is the blocker, not the dialect

There is no Oracle driver this workspace can take. Every Rust option wraps Oracle's own client
library, which nixpkgs cannot supply freely, so there is no analogue of `nix/duckdb.nix`'s single path
from nixpkgs to a library, and `just validate` - which builds in the nix sandbox and is the only thing
that counts as verified here - could not build it. Reasoned from the dependency situation, not
attempted.

Which is why `docs/architecture.md:104-105` already has the answer and this record adopts it:
**Arrow Flight SQL, uniformly, rather than a linked native driver each.** Under that transport a
leg's adapter is one crate parameterised by an endpoint and a `Dialect` rather than one crate per data
system; the proprietary client lives in a gateway process outside our artifact; and the type mapping
is decided once instead of per adapter. Whether Flight SQL's prepared-statement parameter binding
carries our parameters is **reasoned and not run**, and it is the first thing to verify.

### The order for track 1

Postgres first, and it is nearly free: the dialect is compiled, 21 statement goldens and 21 parameter
goldens exist, and every statement is already parse-checked. What is missing is an adapter and a real
Postgres to point it at - which is where docker compose arrives, below.

Then BigQuery: one dialect feature, one corrected bucket function, one golden family, and a transport
that is HTTP rather than a wire protocol. Then Oracle, which is the transport decision plus the
row-limit clause plus the case question.

## Track 2: several data sources, combined by DataFusion

### The shape

A plan in this system is always one fact table, at most `MAX_DIMENSIONS` = 4 group-by keys, and one
measure. Every join is exactly one hop from the metric's own model, because `check_dimension` refuses
a relationship that does not start there, and no join may duplicate rows, because it refuses a
declared `OneToMany`. That shape is what makes the split expressible: the measure and the bucket
always live on the fact model, and a dimension is always a one-hop lookup beside it.

- **One aggregate leg**, on the metric's own model's source. A whole `QueryPlan`, grouped by the time
  bucket, the local keys **and the join key of every remote dimension**, carrying every filter whose
  column lives in that source.
- **One lookup leg per remote dimension model.** The join key and the needed columns, distinct, with
  any filter that belongs there.
- **The combine, in DataFusion, above the port.** Join each lookup onto the aggregate leg - INNER where
  that dimension carries a filter, LEFT where it does not, and *A second finding* below is why that
  distinction is not cosmetic - re-aggregate, then apply a ratio's division and its zero handling, then
  project into the labels `QueryPlan::result_labels` already fixes.

Four dimensions and one hop mean **at most five legs**, and that bound is a consequence of checks that
already exist rather than a new budget.

### The finding that decides the cost: two of six aggregates do not survive the split

Grouping the fact leg by a remote join key is a strictly finer grouping than the answer, so the
combine has to aggregate again. Whether that is correct depends entirely on the aggregate.
`Aggregate` is the six-variant closed enum at `crates/sutura-domain/src/model.rs:190-197`:

| `Aggregate` | Combine step | Correct? |
| --- | --- | --- |
| `Sum` | `SUM` | yes |
| `Count` | `SUM` | yes, and note the function is not the same one |
| `Min` | `MIN` | yes |
| `Max` | `MAX` | yes |
| `Avg` | `AVG` | **no.** Correct only if the leg is rewritten into a `SUM` and a `COUNT` divided after the combine, which is a different `PlanMeasure` than the metric declares |
| `CountDistinct` | anything | **no, and no rewrite exists.** Two join keys can share a subscription, so adding two exact distinct counts over-counts. An exact answer needs the distinct values shipped rather than a count |
| `CountIf`, the other `PlanTerm` | `SUM` | yes |

Counted over `examples/single-player/catalog/metrics/`, which is both the quickstart and the test
corpus:

| Survives the split | Does not |
| --- | --- |
| `recurring_revenue` (sum) | `active_subscriptions` (count_distinct) |
| `voice_minutes` (sum) | `subscription_base` (count_distinct) |
| `subscriptions_churned` (count_if) | `mean_subscription_mrr` (avg) |
| `subscription_months_billed` (count) | `churn_rate` (ratio, denominator count_distinct) |
| `revenue_per_churned_subscription` (ratio: sum / count_if) | `data_per_subscription` (ratio, denominator count_distinct) |
| | `revenue_per_customer` (ratio, denominator count_distinct) |

**Six of eleven, and five of the six are blocked by `count_distinct` alone** - which is, after `sum`,
the most-used aggregate in the corpus. Three of those six declare `region` and `segment`, which are
exactly the dimensions a federated deployment would move to a second system. So the headline
demonstration - *revenue by region, across two systems* - works, and *active subscriptions by region,
across two systems* does not, and nothing in the code as it stands would say so.

A `Ratio` adds a second trap in the same place. `ZeroDenominator::Null` renders as `NULLIF(d, 0)`.
Applied inside a leg, a subgroup whose denominator is zero becomes null, `SUM` skips nulls, and that
subgroup's numerator is silently dropped from the answer instead of nulling it. **The division and
the zero handling belong in the combine, on the re-aggregated totals, and only there** - and `fails`
must fail on the final denominator, not on a leg's.

### A second finding, and this one is a wrong number rather than a refusal

*Left-join each lookup onto the aggregate leg*, with *any filter that belongs there* pushed into the
lookup leg, is **incorrect together**. Either half alone is fine. The pair silently drops a filter.

Walk it. A question filters on a remote dimension - `region = 'south'` - and that column lives in the
lookup source, so the filter goes to the lookup leg, which returns only southern customer keys. The
aggregate leg is grouped by customer key and carries no such filter, because the column is not in its
source. Left-join the lookup onto the aggregate and **every non-southern key survives**, matched to
nothing, its dimension column null. Re-aggregate and those rows land in a null `region` bucket - or,
worse, get projected into the answer's `region` column as a null label beside the real ones. The
filter the caller asked for did not reduce the answer; it added a bucket.

**What single-source rendering does, which is the specification.** The rendered statement is
`FROM fct LEFT JOIN dim ON ... WHERE dim.region = ?`. A `WHERE` on the null-producing side of a left
join is applied **after** the join and eliminates exactly those unmatched rows, so the statement's real
semantics are an inner join. That is not a quirk to work around; it is the answer the federated path
has to reproduce, because *rows identical, any source* is the conformance property.

So the rule, and it is derived rather than chosen:

> **A remote dimension whose column carries a filter is joined as an INNER join. A remote dimension
> with no filter on it is joined as a LEFT join.** The join kind is a function of where the filters
> went, computed by the splitter, and never a default that a leg's contents can contradict.

The left case has to stay left, for the same reason: a fact row whose join key is missing from the
dimension table survives single-source rendering with a null dimension, and an inner join everywhere
would silently drop it. Both kinds are needed and each is wrong in the other's place.

**Two consequences worth writing down.** The aggregate leg does work that the inner join then throws
away - it groups keys that no surviving lookup row matches - and that is acceptable rather than
regrettable: pushing the dimension filter into the fact leg is not available, since the column is not
in that source. And this is the case the conformance packs must contain by name: **a filter on a remote
dimension, plus an orphan key in the fact table**, asserted against the single-source rows. Neither
half alone catches it - an unfiltered question passes with either join kind, and a filtered question
with no orphans passes with an inner join everywhere.

### What it does to the plan, the port and the goldens, which is less than it looks

The important move: **a leg is a whole mono-source plan, not a fragment.** So

- `QueryPlan` keeps its one `source` field. The invariant *a plan cannot silently span two sources* is
  not relaxed, re-worded or re-keyed; it applies to each leg unchanged, and what the combine adds is a
  new thing to refuse rather than an old thing to weaken.
- `Warehouse::execute(&plan) -> RowSet` keeps its signature. A fragment protocol on the port is
  avoidable, and avoiding it is most of the saving: a fragment protocol is what gives up the property
  that push-down is complete.
- `GeneratedQuery` stays one statement, one source, one parameter list. **No SQL golden moves**, so
  AGENTS.md's counted claim about 63 goldens reading `LIMIT 10001` does not move for track 2 either.

What is genuinely new, stated as cost rather than hidden in the shape:

- **A second plan type for a lookup leg**, because `QueryPlan` requires a bucket, a measure and a
  measure label and a dimension table has none of the three. A domain type, a `sutura-sql` entry
  point, a golden family, and a port method or a second port.
- **A per-leg row cap, and it is not the answer's cap.** `row_limit()` is `max_rows + 1` so a result
  at the cap is distinguishable from one cut off by it. Grouped by a customer key rather than a
  region, the aggregate leg's row count is the *key* cardinality: `revenue by region` over 50,000
  customers refuses at 10,001 while its answer is twelve rows. The honest default is to refuse with
  the cap that exists, and raising it is the first time this process would hold more rows than it
  certifies - which is AGENTS.md's *anything that stores or forwards rows* row, and a human review
  question rather than an agent's.
- **The join key's type is constrained.** `Value` has four variants and one is `Real`. Joining on a
  float is a wrong answer waiting for a rounding difference, so a cross-source join key is `Integer`
  or `Text`, refused otherwise.
- **The combine forces the `RowSet` question.** Joining two row-oriented `RowSet`s inside the engine
  means converting to Arrow and back - exactly the conversion `docs/architecture.md:113-127` records
  as the reason a port returning Arrow IPC bytes is the direction. The combine is where that stops
  being a note.
- **`differential.rs` changes shape**, and its own module doc says so: when the engine moves above the
  port, DataFusion stops being a peer of a data source. That test is the cheap net for a statement
  that is valid SQL with different semantics, and it needs a replacement in the new arrangement rather
  than a deletion.
- **A leg's failure is not the question's failure.** Five legs mean five ways to be slow and five ways
  to be unavailable, and a partial result that must never become an answer. `Warehouse::execute`
  returns a `RowSet` or its error and `answer` holds one warehouse; a failing leg has to refuse the
  whole question, and saying *which* leg failed is a diagnostic the refusal enum does not carry.

## The attach route, declined

The direct answer, because it is the question this record was asked to settle.

**Build track 2 directly, with two DuckDB files as its first instance.** Two files as two
`SourceName`s, two `DuckDbWarehouse`s, two legs rendered in the already-golden `duckdb` dialect, one
combine. That is the smallest possible instance of the real machinery: **no new dependency, no new
dialect, no network, no credential, no licence review** - and every track-2 mechanism gets built and
tested against it, from the decomposability refusal to the keyed warehouse set to the per-leg row cap.

Being precise so this is not over-claimed: by ADR 0006's own definition two files under one process
and one set of file permissions are **one** data system storing its tables in two places. Declaring
them two sources is a *test-bed* choice that exercises the plumbing. It does not deliver the identity
property and must never be described as doing so.

What attaching would buy instead: the same user-facing capability, delivered below the port, at a
tenth of the runtime cost measured in ADR 0006's spike - 0.01s and 65 MiB against 0.10s and 123 MiB
for the per-source shape on five million rows, in debug builds. That is a real number and it is not
decisive at these sizes, and the memory ratio is two rather than the order of magnitude the time
ratio suggests.

What it does not buy: any of the machinery above. And the hazard, which is the argument: the cheapest
next step from *attach two files* is *attach a Postgres* - one extension, no new Rust, the same code
path - and that step is the composing adapter with a synthetic source name that ADR 0006 refused in
advance. Under it a catalog declares one source, the plan names one source, the plan-stage set has one
member and passes, and *a plan cannot silently span two sources* reads green over a two-identity
question. **Attaching two files is fine. Attaching a Postgres is the refused shape, and the distance
between them is one line of configuration.**

There is no longer a condition that reverses this. Attaching was held open against one case - several
DuckDB files as a committed near-term deliverable - and that case is now declined explicitly: every
source uses the semantic compiler and standard access, DuckDB no differently from the rest. The
attach affordance and its one-view-per-model registration are therefore work nobody should start.
ADR 0006 records the same decline from the other side, including why it is not a good test fixture
either: two connections need no new adapter code, while attaching needs `attach_database`, a view
registration and a collision refusal - production code for a shape nothing ships.

Three things survive the attach milestone either way, and they are the expensive ones, needed
identically by track 2 because every network adapter is a connection-holding warehouse with the same
two problems: **which artifact ships a native driver** (undecided in ADR 0006, with the missing musl
`libduckdb` as the blocker); **how a serving surface holds a connection** (`duckdb::Connection` is
`Send` and not `Sync` while the service needs `Send + Sync + 'static`); and **the multi-source fixture
plus the two startup-refusal tests.**

## Which of the six single-source assumptions each track forces, in what order

The survey found six, in the order a question meets them: the startup refusal in both composition
roots; the compile-time `ENGINE_SOURCE` constant; `LocalService<W>` holding one warehouse; the
plan-stage `PlanSpansTwoSources`; the plan's one `source` field beside joins that carry only a table
name; and `answer` refusing when the plan's source is not the warehouse's.

| # | Assumption | Track 1 | Attach | Track 2 |
| --- | --- | --- | --- | --- |
| 1 | startup refusal, >1 source | untouched | untouched | **retired, step 5** |
| 2 | `ENGINE_SOURCE` constant | **its wrong-name branch is what a second dialect meets**; the constant itself stays | untouched | **retired, step 5** |
| 3 | one warehouse per service | untouched | untouched | **replaced by a keyed set, step 4** |
| 4 | plan-stage `PlanSpansTwoSources` | untouched | untouched | **re-keyed to identity, step 8 - last** |
| 5 | plan has one `source` | untouched | untouched | **stays. Permanently** |
| 6 | `answer`'s source comparison | untouched | untouched | **becomes per-leg, step 7** |

The attach column is why that route is tactical: moving zero assumptions is what makes it cheap and
what makes its networked continuation the refused shape. The checks an honest federation has to relax
are the checks attaching leaves reading green.

Assumption 5 never moves, and that is deliberate. Making a two-source `QueryPlan` representable would
change a domain type whose serialized form is pinned by snapshots and hashed into the definition
digest. Keeping every leg mono-source is what keeps that type, that digest and 126 statement and
parameter goldens still.

## The ordered plan

Dependent steps, so they stack: `stax` is in the dev shell and
`.agents/skills/git-ops/stacked-branches/SKILL.md` is the guidance. Each step names the evidence it
owes, because a step whose test passes against the base behaviour proves nothing.

1. **The decomposability decision.** `Aggregate::combine_with() -> Option<Aggregate>` as an exhaustive
   `const fn`. ~~plus a `RefusalReason` variant~~ - **SUPERSEDED by 0009: a non-decomposable measure is
   pulled up, not refused, so there is no variant here.** Before anything else, because it is
   the change that answers wrongly if it comes last. Pure domain addition; evidence is the per-variant
   test and the two total matches that will not compile until agent-facing guidance exists.
2. **The multi-source fixture in `examples/multi-player`, and the two missing startup-refusal tests.**
   The fixture makes step 1's refusal provokable from a question file. The tests cover the
   **more-than-one** branch, which is the untested one, and they are owed *before* the thing they
   guard is dismantled. Not the single-player corpus: moving a model there was costed and flips 7
   questions from planned to refused, deletes 21 SQL goldens, and moves a sentence a gate counts.
3. **Track 1 for Postgres**: the adapter, and a real Postgres to point it at. **Integration testing
   arrives via docker compose** - `compose.dev.yaml` exists and carries no such service yet, and Datahub on the metadata side
   is the other case it serves. Evidence is the anchor check and `differential.rs` against a real
   server, which is what the DuckDB adapter exists to do for DuckDB.
4. **Per-source configuration and a keyed set of warehouses** - assumption 3, and with it the single
   `catalog.data_dir` and the source-to-adapter selection `docs/architecture.md:595-600` records as
   deliberately absent. This is where `SourceName` stops being compared for equality and starts
   selecting.
5. **The two startup refusals retired** - assumptions 1 and 2, one at a time, each with its test
   already written in step 2.
6. **The lookup-leg plan type, its generator entry point and its goldens.** The first step that moves
   the definition digest, because a new domain type changes a serialized form.
7. **The combine, above the port, with two DuckDB files as its first instance** - assumption 6 becomes
   per-leg. Where the per-leg row cap lands, where the `RowSet`-to-Arrow question is answered rather
   than deferred, and where `differential.rs` gets its new shape.
8. **Only then the plan-stage refusal** - assumption 4, re-keyed from *a plan spanning two sources* to
   *a plan spanning two identities*, which is the property that was always meant. It cannot be written
   before a principal type exists, and moving it first is the diff that produces a wrong number.

Track 1 for BigQuery and Oracle runs beside this and blocks none of it: a dialect feature, a corrected
bucket function, a row-limit clause, a golden family, and a transport decision.

## Identity

Not designed here, deliberately: the credential port is another record's subject and duplicating it
would give two shapes to reconcile. What this record owes it is one sentence and one commitment.

**The per-source-adapter shape is what keeps per-subject execution reachable**, and it is the only one
of the shapes considered that does. An adapter per source holds a credential per source today and can
hold a credential per request when the port carries one. One connection with several attachments
cannot hold two credentials at all, which is the foreclosure ADR 0006 recorded. A federation layer
whose executing method receives a string, a schema and physical filters and no session cannot either.

And the honest statement of where this stands, written the way the `SourceUnavailable` doc comment is
written: **no security property currently held is at stake, because none is held.** There is no
per-caller identity, `describes_identity()` is a `const fn` returning `false` printed on every boot,
and the bearer token authenticates the deployment. What changes with this milestone is that the gap
acquires a victim: against three systems with grants, reading under a service identity is what
`examples/multi-player/README.md` calls turning a row-level security policy into decoration. **This
is the milestone that cannot honestly ship without the credential port**, and single-player DuckDB
federation is not.

One disclosure this record found rather than inherited, and it is why a semi-join is declined below:
pushing one source's key values into another source's statement puts source B's data into source A's
query log and plan cache. Every value is still a bind parameter, so no invariant is broken, and a
governance boundary is crossed that nothing in this system models.

## What does not change

| Guarantee | Still held by |
| --- | --- |
| No SQL, table, predicate or row-id on the tool surface | Unchanged, and there is nothing to widen: a federated question is the same question. Every source is derived from the pinned bundle, and `Query`'s five `deny_unknown_fields` fields carry no source |
| Refusal is a result, not an error | Unchanged. Everything new here refuses: a measure that cannot be combined, a leg over its cap, a join key that is a float |
| No value from a question reaches the statement as text | Unchanged, per leg. It is the specific thing the declined unparser route could not offer |
| The executed SQL is owned by `sutura-sql` | Unchanged, and this is the point of the decision: every statement that runs anywhere is one this repository rendered and pinned |
| We never translate SQL, and the one thing we parse is parsed at load | Unchanged, and it is what Oracle costs: the per-dialect rewrites live behind `transpile`, which stays uncompiled |
| A definitional filter is always applied | Unchanged. `required_filters` compile into the leg that owns the fact table, marked `PredicateOrigin::Definition` |
| A join cannot silently change a measure | Unchanged and **now load-bearing in a new way.** The `OneToMany` refusal is what stands between a cross-source join and a wrong sum, on a declaration nothing compares against the data - and across sources neither side can see the other's key distribution. The reconciliation test needs a fixture whose unmatched key sits on the far side |
| A catalog edit cannot change what executes | Unchanged. The digest covers every model's `source`, so moving a model to a second system already moves it |
| A plan cannot silently span two sources | Unchanged, per leg, and this is the row the attach continuation would have left true and meaningless |
| A result that hit the row cap is refused, not truncated | Held, and it gains a second number: a per-leg cap as well as the answer's. Both asserted |
| No result cache | Unchanged. The combine buffers legs for one question; nothing is keyed and nothing is reused, and reuse across questions is what would make it a cache - at which point it is keyed on subject first or not at all |
| No panic path reachable from input | Unchanged, and the combine is where to watch it: an arithmetic re-aggregation is where the overflow lints earn their keep |
| The domain acquires no framework dependency | Unchanged. Nothing in this decision adds a dependency to the domain, and a dialect feature adds no dependency anywhere |
| Every query runs as the calling principal | **Still not held**, and now with a named consequence rather than a placeholder |

## Consequences

- **Track 1 is worth shipping on its own, and should not wait for track 2.** One BigQuery deployment
  and one Oracle deployment are single-source deployments, which is most of the value and none of the
  federation risk.
- ~~**The corpus is the demonstration and also the counter-example.** Six of eleven metrics refuse a
  cross-source dimension.~~ **SUPERSEDED by 0009.** Those six are the metrics whose aggregate is pulled
  up instead, so the corpus demonstrates the pull-up and its cost rather than a refusal. What survives
  is the count: six of eleven use `Avg` or `CountDistinct`, which is why the bound matters.
- **`count_distinct` is the single biggest functional gap.** ~~There is no exact fix inside this
  shape.~~ **SUPERSEDED by 0009:** the exact fix is transporting the distinct keys and counting above,
  which is correct and is the most expensive thing the pull-up does - the leg's row count becomes the
  key cardinality. An approximate one exists in every dialect and is not available here: an answer certified
  under a definition digest cannot be approximate without saying so on the wire, which is a tool
  surface change rather than an implementation.
- **Adding a dialect moves counted claims.** Oracle's row cap is not a `LIMIT`, so the sentence
  AGENTS.md holds with a counting gate becomes dialect-aware; and a golden family per dialect
  multiplies the snapshot count that gate reads.
- **The `PLAN_SPANS_TWO_SOURCES` prompt guide becomes wrong** when step 8 lands, and one AGENTS.md
  invariant row is deleted rather than demoted, per the rule at the head of that table. Two prose
  statements - in ADR 0003 and `docs/concepts.md` - say the one-source rule is currently enforced and
  move with it.
- **`dry_run` becomes more valuable and more awkward.** It is defaulted because an in-process engine
  cannot check more cheaply than it runs. Across a network it is worth a round trip per leg, and five
  dry runs before five executions is ten round trips for one question.
- **Docker compose becomes part of the test story**, and with it the question of which gates require
  a running service. `just validate` builds in a sandbox with no network, so a compose-backed test is
  a tier above it rather than inside it, and saying which tier is a CI decision this record does not
  make.
- **Nothing here needs the tool surface to change**, now or later, and that is worth restating
  because it is the strongest fact in the survey and the easiest to forget under a heading about
  federation.

## What is explicitly not decided

- **Whether the per-leg row cap is the answer's cap or a larger one.** Refusing at 10,001 grouped rows
  makes correct questions unanswerable; raising it is the first time this process holds more rows than
  it certifies.
- ~~**Whether `Avg` is rewritten or refused.**~~ **DECIDED by 0009: rewritten.** Rewriting it into a `SUM` and a `COUNT` is exact and
  means a leg's `PlanMeasure` is not the metric's, which is a second place a measure can be
  represented. Refusing it is honest and loses a shipped metric.
- **The transport, in the concrete.** Flight SQL is adopted as the direction and not built. Whether
  its prepared-statement binding carries our parameters is unverified and is the first thing to check.
- **Which artifact ships which adapter**, inherited unchanged from ADR 0006 and multiplied by the
  number of data systems.
- **How a cross-source join key is declared and compared.** A relationship names two models and their
  columns and nothing says whether the two sides are comparable across systems - an `INTEGER` here and
  a `NUMBER(38)` there is a join that works until it does not. That is the strongest argument for one
  wire format deciding types once.
- **Identifier case for Oracle.** Whether the catalog carries the stored case, or the loader learns to
  ask, or a deployment is required to create quoted lower-case objects.
- **Which tier runs a compose-backed test**, and whether a missing service is a skip or a failure. A
  skipped cell that reads as coverage is the failure mode this repository already names.
- **What a per-leg credential is.** Another record's subject.
- **Anything that stores or forwards rows.** Flagged, not resolved, for the same reason ADR 0006
  flagged it.

## Alternatives considered

**`datafusion-federation`, with DataFusion's unparser rendering the pushed statement.** The route the
goal's framing invites and the one this decision replaces. Declined on ADR 0006's measurements - it
returned a transposed aggregate with no error, does not support the pinned DataFusion, and brings a
second SQL parser and a disallowed licence - and declined again here on the point that survives all of
those being fixed: its executing method takes a string with no parameter list, so every value from the
question is inside that string as text. **Our renderer per source with DataFusion as the combiner
keeps the combiner and loses the unparser**, which is the whole of the decision.

**`datafusion-table-providers`.** Requires `datafusion ^54.0` and `arrow ^58.0`, both the previous
major of what this workspace pins, and ADR 0006 built the consequence: two majors resolve and then
fail to compile. Sharper than the federation crate on one point, from its own manifest: it asks for
`datafusion` with `default-features = false` **and `features = ["sql"]`**, so it avoids `bzip2` and
enables by name the one feature `Cargo.toml` turns off with the strongest sentence in that file - that
not calling a parser is a convention and not having one is a property.

**DuckDB's Postgres and BigQuery extensions, as a federation engine.** Verified against the pinned
DuckDB: `postgres_scanner` is a 31 MB runtime download from the core repository, `bigquery` a 56 MB
one from the **community** repository, `odbc_scanner` 522 KB, and **Oracle does not exist** -
`HTTP 404` at `community-extensions.duckdb.org/v1.5.5/osx_arm64/oracle.duckdb_extension.gz`, with
DuckDB itself suggesting `odbc` as the candidate. Declined on four counts: no Oracle path; 87 MB of
signed native code that no gate here can see, because `cargo deny` reads `Cargo.lock`, `unused-deps`
and `check-boundaries` read manifests, and `VENDOR.md` records what is vendored; a connection-global
credential; and moving none of the six assumptions, which is the synthetic-source-name shape. Two
settings verified and worth recording: `allow_unsigned_extensions` is `false`, so an extension we
built ourselves is refused and only the signed per-version per-platform blob loads; and
`allow_parser_override_extension` exists at all, described as allowing an extension to override the
parser. Also a fifth mirror for `docs/enterprise-mirrors.md`, which covers four today.

**A semi-join: push the far side's keys into the near side's statement as bind parameters.** The only
shape that makes `count_distinct` exact across a boundary, and the one a careful reader will ask
about. Declined on three counts: the parameter count is the far side's key cardinality, so the
statement is no longer one of a fixed set of golden shapes; it needs a `VALUES` list or a temporary
table, and the second is a write on a connection that should be read-only; and it moves one source's
key values into another source's query log and plan cache.

**A hand-written pushdown renderer per source, without `sutura-sql`.** What the provider-per-source
route looks like if the generator is not reused, and ADR 0006 measured it failing silently: an
uncovered literal type turned a filtered scan into a five-million-row one, right answer, no
diagnostic. `sutura-sql` renders a whole plan or returns a `GenerateError`; there is no
partial-pushdown path to degrade along. **That is the difference between this decision and the
direction ADR 0006 could only name.**

**Enable `transpile` and let the dialect layer rewrite per target.** It would fix Oracle's
`DATE_TRUNC` in one line. Declined: its default `unsupported_level` returns the SQL and discards the
diagnostic, and the alternative setting errors on every non-count aggregate targeting ClickHouse while
staying silent on the real breakages. The invariant stands and the rewrite work is ours.

**Relax the plan-stage refusal and let the engine sort it out.** Traced in the survey: with both
guards gone the plan carries only the metric's own model's source, the engine resolves the joined
table in a flat single-level namespace out of one directory, and a question reaching across two nominal
sources is *answered* under the metric's certified name and the bundle's digest. Not a refusal - a
number. It is step 8 for a reason.

**Export every source to Parquet and answer over the exports.** Works today with no code and is the
baseline every option has to beat. It fails on freshness, it doubles the storage, and a copy is read
under whoever made it - the same objection ADR 0003 raised against an acceleration layer, arriving by
hand. Against three systems with grants it is decoration in its most literal form.

**Track 1 only, and no federation ever.** The honest minimum, and it stays correct for as long as a
deployment is one data system. It is what this record recommends *until* step 1 exists, because the
one thing worse than no federation is a federated answer nobody can tell is wrong.
