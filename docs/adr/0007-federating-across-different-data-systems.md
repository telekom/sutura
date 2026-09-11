---
title: Federating across different data systems
description: The two tracks for BigQuery, Postgres and Oracle - one source is a whole-query pushdown in that system's dialect and involves no federation, several sources are per-source legs rendered by sutura-sql and combined above the port - the three leg shapes and the two-variant plan type that expresses them, why the combiner is ours and not an unparser, why a compiled dialect is a golden and not a feature flag, where the RowSet-to-Arrow boundary lives and what the leg size bound does and does not reach, which of the six single-source assumptions each branch forces in what order, and why attaching several databases was declined outright rather than sequenced.
---

# Federating across different data systems

Status: accepted, and **partly built** - amended twice, in place, by the two blocks below. *Nothing
here is built* was the status when this was written and is corrected rather than left: the splitter,
the leg types, the per-dialect rendering, the combine and the orchestrating call all exist, and
`sutura-exec-datafusion` executes a leg. What is still unbuilt is track 1 beyond the dialects that
ship and a deployment holding two KINDS of data system. It decides a shape and an order; the code it
led to is cited beside each amendment.
[Several databases behind one data system](0006-several-databases-behind-one-data-system.md) declined
both a federation crate and attaching several databases below the port. This record decides
what is built instead, for the systems that are three separate logins: BigQuery, Postgres and Oracle.

**Amended, and the amendment is finished rather than announced.** This record decided that a measure
which cannot be re-aggregated across legs is REFUSED.
[The plan](0009-the-plan-from-one-source-to-many.md) decides the opposite and supersedes it: push what
descends, and otherwise retrieve finer-grained rows and compute above. Decision 3 of the same record
retires the per-leg **row** cap in favour of a working set bounded in bytes.

**Corrected, and this is the one worth reading before citing either record: the two are not
opposites, and the refusal was never superseded.** What the plan changed is the DEFAULT - push what
descends, retrieve and compute above - and it kept this record's refusal for the measures that cannot
be recombined at all. `RefusalReason::MeasureDoesNotFederate` is live and is raised at two sites in
`crates/sutura-semantic/src/plan.rs`; `docs/adr/0005`'s status table still carries its row. So the
behaviour this record decided is the behaviour the tree has, and the paragraph above disowned it - the
shape that makes a reader trust the wrong document, which is exactly what the next paragraph warns
about one revision too late.

A previous revision of this page put those two facts in this paragraph and left four later parts
arguing the withdrawn design: an ordered step whose evidence was a refusal that no longer exists
(**and it does exist - see the correction above; only that step's ordering moved**), a
guarantee table asserting both that a non-combinable measure refuses and that a per-leg cap is
asserted, and two open questions that were closed. **Each is rewritten below rather than struck.** A
banner over a live design is the shape this repository has already paid for once, and a reader who
opens this record in the middle has no way to know a banner exists at the top.

**Amended a second time, and again in place: the combiner is NOT DataFusion.** This record decided
*the combiner is DataFusion* and that the engine belongs above the port. The combine as built is
`sutura_domain::plan::FederatedPlan::combine`, a pure domain function that no adapter is on the path
of - so the second driven port this record designed does not exist, and the sentence that named the
framework is rewritten in *Track 2* below rather than left standing beside the code. **What survives
unchanged is the part that decided the shape:** per-source legs, each a whole mono-source plan, each
rendered or built in its own adapter, joined and re-aggregated above the port. Only the identity of
the thing above the port moved.

That amendment is what permits `sutura-exec-datafusion` to declare
`Warehouse::EXECUTES_LEGS`, and it is this record's own sentence that permits it rather than a new
one: *`sutura-exec-datafusion` keeps its `Warehouse` impl for local files, **because the engine is
also a data source**, and the combiner is a separate implementor of a separate port.* With the
combine in the domain, the engine's only role is the data source's, and whether that data source can
run one source's share of an answer is an ordinary capability question. The adapter's own comment
said a leg arriving there *would mean the composition is wrong* and gave *there is no splitter* as
its reason; the splitter has existed since `sutura_semantic::federated_plan`, and that comment is
retired with the arm it guarded.

**What the reversal costs, said where the withdrawal is.** The refusal was cheap and the pull-up is
not. A measure that does not descend is answered by transporting finer-grained rows, so the resource a
refusal used to protect is now protected by a bound - and it also forces a shape decision the refusal
let this record avoid, because a leg carrying distinct keys is not an aggregate and cannot be
represented by the type an aggregate leg uses. *Three leg shapes, and what each costs* below is the
section the reversal wrote, and it is the part of this record the plan has to be reconciled against.

## The decision

**There are two tracks, and only the second one is federation.**

**Track 1 - one data source: query it directly.** Compile the semantic plan to that system's dialect
and push the whole query down. One `QueryPlan`, one statement, one `Warehouse`, one result. This is
what `sutura-sql` and the two existing adapters already do; adding BigQuery, Oracle or Postgres to it
adds a `Dialect` variant, a golden family and an adapter, and relaxes **none** of the six
single-source assumptions. No federation is involved and none should be implied.

**Track 2 - several data sources: our renderer per source, a combiner above the port.** The
semantic plan splits into per-source legs, **each of which is itself a whole mono-source plan** -
never a fragment, and *not* a `QueryPlan`, for the reason *Three leg shapes* gives below: two of the
three leg shapes are things `QueryPlan` cannot say. Each leg is rendered by `sutura-sql` in its own
dialect with its bind parameters and forced quoting intact, or built as a logical plan where the
adapter is the engine itself; each executes through its own `Warehouse` adapter under its own
credential; and the results are joined and re-aggregated above the port. This is the architecture.

**Amended: the combiner is the DOMAIN's, not DataFusion's.** This paragraph said *DataFusion joins
and re-aggregates the results above the port* and the next one said *the combiner is DataFusion*.
Neither is how it landed: `sutura_domain::plan::FederatedPlan::combine` is a pure domain function
and no adapter is on the combine path, so the second driven port this record designed below - *a
crate above that port implements it over DataFusion* - was never built and `LocalService` is generic
in one adapter type. **This is a correction of fact rather than a change of direction:** the reason
the combiner had to be ours is unchanged and is the paragraph that follows; what moved is only where
it lives, and it moved to the layer with the fewest dependencies rather than to a framework.

**The consequence worth naming, because it is the whole of `telekom/sutura#112`'s second blocker.**
With the combine in the domain, *the engine belongs above the port* stops being a reason the engine
may not be a leg-executing data source - and this record already says it is a data source, in the
sentence quoted in the amendment at the top. So `sutura-exec-datafusion` declaring
`Warehouse::EXECUTES_LEGS` is permitted by this record, and the constant's own documentation is what
keeps that narrow: it is *a missed-optimisation default rather than a missed-security one*, so one
adapter opting in is a capability statement while defaulting every adapter in would be a change of
that argument. **The limit that travels with it:** two data systems are not two identities. The
engine's `IMPERSONATION` is `NoPlaceForASubject`, so every leg it runs runs under one
operating-system identity - *Identity* below is unchanged, and leg 2 is not what this buys.

**The generator is never DataFusion.** `datafusion-federation`'s route -
DataFusion's unparser rendering the pushed statement - is declined, and this is the sentence that
keeps the invariants: a value from a question never reaches a statement as text, because every leg is
a `GeneratedQuery` with statement and parameters in separate fields; and no SECOND parser enters the
closure. Stating that precisely, because an overstated control is itself the defect here:
`polyglot-sql` IS a parser and is already in the closure, the golden suite parses every statement it
pins, and `sutura_sql::expression` parses at load. What holds is narrower and still worth having -
nothing parses on the QUERY PATH, and `datafusion`'s `sql` feature stays off, so no second parser and
no unparser arrive. `polyglot-sql` occupies the position `sqlglot` occupies in comparable systems:
**generation per dialect, never transpilation.**

**Attaching several databases is declined, DuckDB included.** Every source reaches the semantic
compiler and standard credentialed access; there is no shape in which one connection stands in for
several sources. It builds none of the track-2 machinery, and the cheapest continuation of it -
attaching a Postgres instead of a file - is the composing-adapter-with-a-synthetic-source-name that
ADR 0006 refused in advance. **Two DuckDB files as two `SourceName`s are the first instance of
track 2**, and that is where the machinery gets built. The argument is in its own section below.

**And the first thing built is neither track.** It is a decision about which aggregates survive being
computed at a finer grouping and added back up, because two of the six in the vocabulary do not, and
**six of the eleven metrics in the shipped corpus use one of those two.** That is track 2's version of
the change that compiles, passes and answers wrongly. The answer is a pull-up rather than a refusal -
finer-grained rows, computed above - and it is the pull-up that makes a leg a shape today's plan type
cannot express, which is the next-to-last section of the decision below.

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

- **One fact leg**, on the metric's own model's source. Grouped by the time bucket, the answer's local
  keys, **the join key of every remote dimension**, and - for a term that cannot descend - **the
  distinct key that term needs**; projecting one column per term that CAN descend; carrying every
  filter whose column lives in that source. One leg per source and not one per term, and *Three leg
  shapes* below is why fusing them is free.
- **One lookup leg per remote dimension model.** The join key and the needed columns, distinct, with
  any filter that belongs there.
- **The combine, above the port.** Join each lookup onto the fact leg - INNER where that dimension
  carries a filter, LEFT where it does not, and *A second finding* below is why that distinction is not
  cosmetic - re-aggregate the descended columns, count distinct over the key columns that were carried
  instead, then apply a ratio's division and its zero handling, then project into the labels
  `QueryPlan::result_labels` already fixes.

Four dimensions and one hop mean **at most five legs** - one fact leg plus one lookup per remote
dimension model - and that bound is a consequence of checks that already exist rather than a new
budget. **The leg COUNT is the easy bound and it is not the one that matters;** the leg SIZE bound is
below, with the number it has and the number it does not.

**What the first slice builds is the low end of that bound: exactly two legs.** Every remote dimension
the splitter reaches must join through **one** relationship to **one** remote model, so the lookup leg
is one - a fact leg plus a single lookup leg, linked on one column. The bound of two is therefore a
number somebody chose, and the reason is written where the choice is made: `sutura_semantic::plan`
refuses two remote dimensions joined through two relationships on one source as
`FederationLinkAmbiguous`, because the two legs link on a single column. The path from two to five is
the path that lets the lookup leg carry a second link - one lookup per remote dimension model on its
own data system - which is the shape the count above describes. Adding that is a change to the
`FederatedPlan` shape (the combiner's single link column) and to the splitter's relationship check,
not a change to the leg vocabulary.

### Three leg shapes, and what each costs

**This is the decision this record was missing, and it is why the plan could not be executed.** Naming
*an aggregate leg* and *a lookup leg* was sufficient while a non-decomposable measure was refused. It
is not sufficient now: pulling `CountDistinct` up means the fact leg has to transport the distinct
**keys** rather than a count, and neither an aggregate leg nor a dimension lookup represents that. So
the shapes are enumerated here against the types that exist, and each is priced. **The plan executes;
this record decides.**

**Read the existing type first, because it cannot express any of the three.** `QueryPlan` at
`crates/sutura-domain/src/plan.rs:319-332` holds twelve fields and four of them decide this section:
`bucket: PlanBucket` and `measure: PlanMeasure` are required and not `Option`; `measure_label` is one
label; and `max_rows` is set to `MAX_ROWS` by the constructor, which takes no parameter for it.
`generate` at `crates/sutura-sql/src/generate.rs:233` projects the keys, then one bucket expression,
then **exactly one** measure expression; groups by the keys and the bucket; and always emits
`LIMIT plan.row_limit()`.

| Shape | What it is | Can today's `QueryPlan` say it? |
| --- | --- | --- |
| **Aggregate fact leg** | grouped by the bucket, the local keys and every remote join key, projecting one column per PUSHED term | **No.** A ratio must not be divided per leg and a decomposed `Avg` is a sum beside a count, so a leg projects a LIST of terms. `measure` is one `PlanMeasure` and `measure_label` is one label. `measure_expression` renders `Ratio` as a division with a `NULLIF`, which is exactly the thing 0009's Decision 2 forbids per leg |
| **Distinct-key fact leg** | the same grouping plus the distinct key itself, with **no aggregate over that column** | **No.** There is no `PlanMeasure` that means "nothing", and adding one would put an absent case on the closed measure vocabulary - the shape AGENTS.md's row about `Option<String>` on `Measure` exists to keep out |
| **Dimension lookup leg** | the join key and the needed columns, distinct, off the dimension table | **No.** A dimension table has no time column and no measure, and `bucket` and `measure` are both required |

And one row that is not a shape but fails the same way for all three: **no leg carries an answer-shaped
row cap.** 0009's Decision 3 retires it, and `QueryPlan::new` has no parameter that could omit it.

**So the decision, in one sentence: `QueryPlan` is not touched, and a leg is a new domain type with two
variants.**

#### The type

`LegPlan`, in `sutura_domain::plan`, and the reason it is an enum rather than a struct with four
`Option`s is the habit this repository states as *prefer unrepresentable to checked*: there are exactly
two legal shapes and the illegal combinations - a measure with no bucket, a range on a table with no
time column - should not be constructible at all.

| Variant | Fields | Reads |
| --- | --- | --- |
| `Fact` | `source`, `metric`, `table`, `joins` (same-source hops only), `bucket`, `keys`, `terms`, `filters`, `params`, `range` | the metric's own model |
| `Lookup` | `source`, `table`, `keys`, `filters`, `params` - no bucket, no terms, no range | one remote dimension model |

**Two variants and not three, and the factoring is the decision rather than a preference.** There are
three SHAPES and two axes to split them along, and only one of the two splits is worth a variant:

- **Split by which model is read** and four fields move together: a dimension model has no time
  column, so no bucket and no range; it is not the metric's own model, so no metric name; and a
  one-hop dimension join does not start from it, so no joins. Four absences that always co-occur are
  what a variant is for.
- **Split by whether an aggregate is applied** and exactly one field moves: `terms` is empty or it is
  not. A variant for that would duplicate the other nine fields to express one bit, and both halves
  would render identically - project the key list, group by the key list.

So the aggregate and distinct-key shapes are one variant differing in one field, and the lookup shape
is the other. **The alternative collapse was considered and rejected:** putting the two DISTINCT shapes
together as one variant and the aggregate on its own needs `Option<PlanBucket>` and
`Option<TimeRange>` on the shared variant, which makes *a dimension leg with a time range* and *a fact
leg with no bucket* both constructible - the checked shape, where this one is the unrepresentable one.

Three things about `Fact` carry the whole decision:

- **`keys` holds three kinds of column and the type does not distinguish them:** the answer's local
  dimension keys, every remote dimension's join key, and every distinct key a non-descending term
  needs. It does not distinguish them because the RENDERING does not care - all three are grouped by
  and projected - and the combine reads which is which from the `FederatedPlan` beside the legs rather
  than from the leg. Bounded by arithmetic that already exists: `MAX_DIMENSIONS` is 4, so at most four
  dimension keys, plus one bucket, plus at most two distinct keys, because a `Measure` is one term or a
  ratio of two. **Seven grouping columns, worst case, derived rather than budgeted.**
- **`terms` is a `Vec<LegTerm>` and it may be EMPTY, and the empty case is the whole of the
  distinct-key leg.** A `Fact` leg with no terms groups by its key list and projects it, which is
  a distinct set of keys - so the third shape in the table above needs no third variant, only an empty
  vector. At most four entries, because a `Measure` is one term or a ratio of two and `Avg` expands one
  term into two.
- **`LegTerm` holds a `PlanTerm` and a label, and NOT a `PlanMeasure`. That is the mechanism, and it is
  a type rather than a convention.** `PlanMeasure` has exactly two variants and the only one that
  carries two terms is the one that DIVIDES them: `measure_expression` at
  `crates/sutura-sql/src/generate.rs:200` emits `CAST(numerator AS DOUBLE) / NULLIF(denominator, 0)`
  for a `Ratio`. So **a decomposed `Avg` travelling as a sum and a count, and a ratio travelling as an
  undivided numerator and denominator, are not expressible by `PlanMeasure` at all** - which makes
  Decision 2 a domain change rather than a generator one, and is the single sharpest reason a leg
  needs its own type. With `LegTerm` there is no `Ratio` shape a leg can carry, so `ZeroDenominator`
  cannot reach a leg's statement and a division per leg is not something a reviewer has to notice.
  `PlanTerm` is reused unchanged, and that is free because `plan.rs:180-190` already mirrors `Term`
  at the plan level for exactly this reason - *"so an adapter that renders one half of a ratio and
  one that renders a whole measure reach for the same function"*.

**One fact leg per source, fused across terms, and the fusion is what keeps the leg count at five.** A
ratio of a `Sum` and a `CountDistinct` could be two fact legs against the same table - one grouped
coarsely with the sum, one grouped finely with the key - and it is one leg instead, grouped at the
finest grouping any term needs, because `Sum`, `Count`, `Min`, `Max` and `CountIf` all survive being
computed at a finer grouping and added back up. That is the same property Decision 2 turns on, applied
one level down. It costs the sum's column extra rows and saves a second scan of the same table, and
which of those is cheaper depends on the data. **The shape would permit either and this record fixes
one**, so that the leg count is a property of the shape rather than of an optimiser's mood - and so
that a reviewer counting legs in a log has a number to compare against.

#### What each shape does to `sutura-sql`

**One new entry point, and "nothing new" is wrong.** `generate_leg(&LegPlan, Dialect) ->
Result<GeneratedQuery, GenerateError>` beside `generate`, sharing `column`, `aliased`, `aggregate`,
`term_expression` and `predicate`, and differing from `generate` in exactly four ways: it projects a
list of term columns rather than one measure expression; it omits the bucket for a `Lookup`; it emits
**no `LIMIT`**; and it never calls `measure_expression`, because there is no `PlanMeasure` in a
`LegPlan` to hand it. One golden family, statement and parameters, per leg per dialect.

**No existing golden moves, and that is checkable rather than asserted.** `QueryPlan` is untouched, so
the 63 statement snapshots, the 63 parameter snapshots and the 21 `plan@markdown` snapshots are all
renderings of a type this change does not edit. AGENTS.md's counted claim about 63 goldens reading
`LIMIT 10001` therefore stands as written, and `check-guidance`'s counter -
`xtask/src/guidance/claims.rs:273`, which holds the literal `LIMIT 10001` - does not need to move
either. **The limit of that claim:** it holds because the new legs are new snapshots, so the first
person to add a leg golden must not put a `LIMIT` in it, and nothing mechanical stops them. The
generator emitting none is what makes it hard to get wrong.

**And a correction this record owes, because it claimed a mechanism it does not have.** An earlier
version of the ordered plan said the lookup-leg plan type would be *"the first step that moves the
definition digest, because a new domain type changes a serialized form"*. **That is false.**
`DefinitionDigest::of` at `crates/sutura-domain/src/definitions.rs:91` takes the `Definitions` and the
`Knowledge` and nothing else. `QueryPlan` is not under the digest, `LegPlan` will not be either, and no
plan-shape change moves a digest. What a new plan type moves is its own new snapshots, and what moves a
digest is a catalog edit - including moving a model to a second source, which is the fact that actually
matters here and is stated correctly in *What does not change* below.

#### What each shape does to the port

**`Warehouse::execute` does NOT keep its signature, and an earlier version of this record said it
would.** That claim was true only while a leg was a `QueryPlan`, and the section above is why it is
not. Corrected in the direction that costs us:

> `execute` takes an `Executable` - a two-variant enum over a whole `QueryPlan` and a `LegPlan` - so
> every adapter's top-level match is exhaustive and a third leg shape cannot be added without every
> adapter stating what it does with it.

The alternative considered and rejected was a second port method for legs. It is a smaller diff and it
is worse in the way that matters: a second method invites a default, a default that returns an error
lets an adapter be silently non-federating, and "adding a data system is a registration" then stops
being true in the one direction nobody would notice. An exhaustive match is the mechanism this
repository reaches for elsewhere - *a new shape is a domain variant plus a plan variant plus a
generator arm plus a golden* - and this is the same case.

What that costs, named rather than implied: **the signature of the one method every adapter and every
fake implements changes**, across `sutura-exec-datafusion`, `sutura-exec-duckdb`,
`crates/sutura-app/tests/support/oracle.rs` and `crates/sutura-http/src/testing.rs`, and `dry_run`
changes with it. It is a wide mechanical diff and it lands in the same commit as the type, which is
what keeps it a diff rather than a drift. And it is the same signature 0008's credential and 0009's
deadline arrive on, which is an argument for one method rather than two.

**What does NOT change, and it is the property that was worth protecting all along: the port still
carries a WHOLE plan and returns a WHOLE result. It never carries a fragment.** A fragment protocol is
what gives up the guarantee that push-down is complete, and no shape here needs one. The number of
plan shapes the port carries went from one to two; the protocol did not move.

**A second driven port was designed here and is NOT what was built.** The design: the combine needs
DataFusion, `sutura-app` may not name a framework, so the domain declares a port beside `Warehouse`
and `SemanticCatalog` and a crate above it implements the combine over DataFusion, with
`LocalService` generic in both. **What landed instead is a pure domain function** -
`sutura_domain::plan::FederatedPlan::combine`, taking the legs' `RowSet`s and the `FederatedPlan` and
returning the answer's `RowSet`, exactly the signature this port was to carry. It needs no framework,
so it needs no port and no second implementor: `LocalService` is still generic in one adapter type,
and no adapter is on the combine path. The refusals this paragraph wanted a fake combiner for - the
working-set ceiling, the deadline, the answer's row cap - are provokable without a data system
because the function is in the domain, which is the same property reached one layer lower.

**One sentence of this paragraph survives the correction and is load-bearing:**
`sutura-exec-datafusion` keeps its `Warehouse` impl for local files, because the engine is also a data
source, and the combiner is separate from it. That is what makes the engine's leg capability an
ordinary question about a data source rather than a contradiction of *the engine belongs above the
port* - there is nothing above the port for it to be. An adapter never calls another adapter, and
that is unchanged: the combine is called by `sutura-app`, above every adapter.

#### Where the `RowSet`-to-Arrow boundary lives

**Every leg crosses it, so the record has to say where it is rather than leaving it to whoever writes
the combine.** The decision: **the port's currency stays `RowSet` for the first federated milestone,
and the conversion to Arrow lives in exactly one function inside the combiner crate.**

Why not move the port to Arrow first, which is the direction `docs/architecture.md:113-127` records:
`arrow` is a framework the domain may not name, `sutura-arrow` does not exist, and the pinned `duckdb`
and `datafusion` disagree on the Arrow major - `arrow 58.4.0` under the driver and `59.2.0` under the
engine, both in `Cargo.lock` today - so an Arrow-typed boundary between them is either IPC bytes,
which copies every buffer, or the C data interface, which `unsafe_code = "forbid"` puts out of reach.
ADR 0006 built both of those walls. Deciding an Arrow port here would be deciding a record 0009
already says wants its own.

**What that costs, and it is a real number rather than a shrug.** The engine's own leg pays Arrow into
`RowSet` in `sutura-exec-datafusion`'s `collect.rs` and then `RowSet` back into Arrow in the combiner -
two conversions for data that never left the process. Every SQL leg pays one. **One function, named,
in one crate, is what makes that measurable and what makes it a single place to delete** when the Arrow
record lands.

**And that one function is where a leg's bytes get counted, which is the second decision this section
owes.** 0009's Decision 3 says the working-set bound is the engine's memory pool and that the pool
counts what its operators reserve - not a row set a driver handed back, and not a leg's buffers before
conversion. So a leg held as a finished `RowSet` is **invisible to the bound that is supposed to
protect the process from it**, and a bound that bites only when the combine reserves bites after the
rows are already in memory. The decision:

> **The byte budget is applied AS rows are converted, inside that one function, rather than to a
> finished `RowSet`.** A leg is refused while it is arriving, which is the only place a bound can
> refuse before the memory has already been spent.

That is what makes *one function, in one crate* load-bearing rather than tidy: it is the single place
that sees every leg's rows before anything holds all of them, so it is the only place the bound can be
enforced at all. **The limit, stated with it:** this bounds the CONVERSION and the pool bounds the
COMBINE, which are two bounds over two quantities, and neither covers what a driver buffered inside
itself before handing rows back. `feat/query-bounds` owes a measurement of the third.

#### The leg SIZE bound, which is the one that matters

The count bound is five. **The size bound has one number and it is not per leg, and saying so is
better than implying there is one.**

| Leg | Rows it returns | Bounded by |
| --- | --- | --- |
| `Fact`, with terms only | the distinct (bucket x local keys x remote join keys) tuples in the range | the **remote join key's** cardinality, which is the cardinality driver: `revenue by region` over 50,000 customers and 12 months is up to 600,000 rows of a key, a month and a sum |
| `Fact`, carrying a distinct key | the above, times the distinct key | the FACT table's own grain. `active subscriptions by region` carries (month, customer key, subscription key) - one row per subscription per month in the range. Over five million subscription-months that is five million rows of three narrow columns, roughly 120 MB before any framing |
| `Lookup` | the distinct keys and columns surviving its own filters | the dimension table's cardinality |

**There is no per-leg number, deliberately, and 0009's Decision 3 is why:** a count of rows per leg
protects nothing that is scarce, because it cannot tell 50,000 rows of two integers from 50,000 rows of
wide text. The number that exists is the **per-query working set**, provisionally 1 GB, and it is
provisional in the strict sense - nobody measured it, `feat/query-bounds` does, and it is written down
so it cannot harden into a decision by having appeared in a record.

Two consequences of that bound being per QUERY rather than per leg, and both are decisions rather than
notes. **A question with a distinct-key leg and four lookups shares one budget**, so the legs compete
rather than each having room. And **the distinct-key leg is the shape that reaches the ceiling first**,
by roughly the ratio of the fact grain to the answer's grain - which is the price of the pull-up,
stated as a quantity rather than as "we pay the processing cost".

### The finding that decides the cost: two of six aggregates do not descend

Grouping the fact leg by a remote join key is a strictly finer grouping than the answer, so the
combine has to aggregate again. Whether that is correct depends entirely on the aggregate.
`Aggregate` is the six-variant closed enum at `crates/sutura-domain/src/model.rs:190-197`, and the
third column is what each one costs a leg:

| `Aggregate` | How it travels | What the leg carries |
| --- | --- | --- |
| `Sum` | descends as written, `SUM` above | one term column |
| `Count` | descends as written, `SUM` above - note the function is not the same one | one term column |
| `Min` | descends as written, `MIN` above | one term column |
| `Max` | descends as written, `MAX` above | one term column |
| `Avg` | **descends decomposed:** a `SUM` and a `COUNT` per leg, divided once above. `AVG` of `AVG`s is wrong | **two** term columns |
| `CountDistinct` | **does not descend at all.** Two join keys can share a subscription, so adding two exact distinct counts over-counts, and no re-aggregating function repairs it. The exact answer is the distinct KEYS in the leg and the count above | **zero** term columns and **one** grouping column |
| `CountIf`, the other `PlanTerm` | descends as written, `SUM` above | one term column |

**That third column is why `Aggregate::combine_with() -> Option<Aggregate>` is the wrong signature, and
an earlier version of this record's ordered plan named it.** `Option<Aggregate>` has two outcomes and
there are three: descends as itself, descends as two columns, does not descend and travels as a
grouping key. So the exhaustive `const fn` returns a three-variant enum naming those three, and the
compile error a new aggregate produces is *you have not said which of the three you are* rather than
*you have not said whether you can*. That is the whole content of `feat/federation-decomposability`.

Counted over `examples/single-player/catalog/metrics/`, which is both the quickstart and the test
corpus:

| Descends whole | Must be pulled up |
| --- | --- |
| `recurring_revenue` (sum) | `active_subscriptions` (count_distinct) |
| `voice_minutes` (sum) | `subscription_base` (count_distinct) |
| `subscriptions_churned` (count_if) | `mean_subscription_mrr` (avg - decomposed, so the cheap kind) |
| `subscription_months_billed` (count) | `churn_rate` (ratio, denominator count_distinct) |
| `revenue_per_churned_subscription` (ratio: sum / count_if) | `data_per_subscription` (ratio, denominator count_distinct) |
| | `revenue_per_customer` (ratio, denominator count_distinct) |

**Six of eleven, and five of the six turn on `count_distinct` alone** - which is, after `sum`, the
most-used aggregate in the corpus. Three of those six declare `region` and `segment`, which are exactly
the dimensions a federated deployment would move to a second system. So the headline demonstration -
*revenue by region, across two systems* - works, and **so does *active subscriptions by region, across
two systems*, at the cost of a leg carrying one row per subscription per month rather than one row per
customer per month.** That is the pull-up, and the corpus is the demonstration of its price rather than
of a refusal.

Worth stating precisely, because the two are easy to run together: `mean_subscription_mrr` and the five
`count_distinct` metrics are both "does not descend" and they are not the same cost. `Avg` decomposed
returns exactly as many rows as a `Sum` would, in two columns instead of one. `CountDistinct` gives as
many rows as the fact grain. **The expensive half of the pull-up is one aggregate, and it is the one
five of the six metrics use.**

A `Ratio` adds a second trap in the same place. `ZeroDenominator::Null` renders as `NULLIF(d, 0)`.
Applied inside a leg, a subgroup whose denominator is zero becomes null, `SUM` skips nulls, and that
subgroup's numerator is silently dropped from the answer instead of nulling it. **The division and
the zero handling belong in the combine, on the re-aggregated totals, and only there** - and `fails`
must fail on the final denominator, not on a leg's.

### A second finding, and this one is a wrong number rather than a refusal

*Left-join each lookup onto the fact leg*, with *any filter that belongs there* pushed into the
lookup leg, is **incorrect together**. Either half alone is fine. The pair silently drops a filter.

Walk it. A question filters on a remote dimension - `region = 'south'` - and that column lives in the
lookup source, so the filter goes to the lookup leg, which returns only southern customer keys. The
fact leg is grouped by customer key and carries no such filter, because the column is not in its
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

**Two consequences worth writing down.** The fact leg does work that the inner join then throws
away - it groups keys that no surviving lookup row matches - and that is acceptable rather than
regrettable: pushing the dimension filter into the fact leg is not available, since the column is not
in that source. And this is the case the conformance packs must contain by name: **a filter on a remote
dimension, plus an orphan key in the fact table**, asserted against the single-source rows. Neither
half alone catches it - an unfiltered question passes with either join kind, and a filtered question
with no orphans passes with an inner join everywhere.

### What it does to the rest of the tree

*Three leg shapes* above priced the plan type, the generator entry point, the port and the Arrow
boundary. What is left is everything else the shape touches, and the first bullet is the one that keeps
the invariant table intact.

- **A leg is a whole mono-source plan, not a fragment**, and *a plan cannot silently span two sources*
  is therefore not relaxed, re-worded or re-keyed. It applies to `QueryPlan` unchanged and to each
  `LegPlan` variant by construction, since neither carries more than one `SourceName`. What the combine
  adds is a new thing to refuse rather than an old thing to weaken.
- **`GeneratedQuery` stays one statement, one source, one parameter list.** Both generator entry points
  return one, so the no-value-as-text row keeps its mechanism per leg with nothing added to it.
- **`sutura-semantic` returns two shapes.** `compile` produces a `QueryPlan` for a mono-source question
  and a `FederatedPlan` - the legs, plus which key is which, plus the join kinds, plus the metric's own
  `PlanMeasure` for the combine to apply above - for a question that spans sources. **`PlanMeasure`
  stops being a thing a leg renders and becomes a thing the combine reads**, which is how a ratio's
  `ZeroDenominator` survives the split without being duplicated: the metric's measure travels whole and
  is applied once, on the re-aggregated totals.
- **The splitter is analysis over the join graph, not a `BTreeMap` keyed by `SourceName`.** Two lookup
  models on one remote source with no declared relationship between them would fuse into a cross
  product, so the rule is *same source AND connected by a declared relationship in this plan*. 0009
  states it and this record is where the shape it constrains lives; the test that pins it is named for
  the relationship rather than for the source.
- **The answer's row cap moves location and not value.** `MAX_ROWS + 1` is applied by the combine on
  the combined result, and `answer` still refuses `ResultTooLarge` above it. A leg carries no cap and
  the answer carries the same one it always did.
- **The join key's type is constrained.** `Value` has four variants and one is `Real`. Joining on a
  float is a wrong answer waiting for a rounding difference, so a cross-source join key is `Integer`
  or `Text`, refused otherwise.
- **`differential.rs` changes shape**, and its own module doc says so: when the engine moves above the
  port, DataFusion stops being a peer of a data source. That test is the cheap net for a statement
  that is valid SQL with different semantics, and it needs a replacement in the new arrangement rather
  than a deletion.
- **A leg's failure is not the question's failure.** Five legs mean five ways to be slow and five ways
  to be unavailable, and a partial result that must never become an answer. A failing leg has to refuse
  the whole question, and saying *which* leg failed is a diagnostic the refusal enum does not carry
  today.
- **The startup refusal's replacement is a per-source posture, not a count.** A federated deployment
  holds several sources, each declaring how it establishes the identity a query runs as, so what
  refuses at boot is a source whose declared posture the deployment cannot deliver rather than a
  catalog that named more than one source. That is ADR 0008's and ADR 0011's mechanism and this record
  only says which assumption it displaces - assumptions 1 and 2 in the table below.

## The attach route, declined

The direct answer, because it is the question this record was asked to settle.

**Build track 2 directly, with two DuckDB files as its first instance.** Two files as two
`SourceName`s, two `DuckDbWarehouse`s, two legs rendered in the already-golden `duckdb` dialect, one
combine. That is the smallest possible instance of the real machinery: **no new dependency, no new
dialect, no network, no credential, no licence review** - and every track-2 mechanism gets built and
tested against it, from the decomposability match to `LegPlan`'s two variants to the keyed warehouse
set to the working-set ceiling.

Being precise so this is not over-claimed: by ADR 0006's own definition two files under one process
and one set of file permissions are **one** data system storing its tables in two places. Declaring
them two sources is a *test-bed* choice that exercises the plumbing. It does not deliver the identity
property and must never be described as doing so.

What attaching would buy instead: the same user-facing capability, delivered below the port, buffering
less. **65 MiB against 123 MiB for the per-source shape on five million rows** - a factor of two on
peak resident set, which is not decisive at these sizes.

**And a correction, because this record used to quote a ratio ADR 0006's table cannot support.** The
sentence here said *a tenth of the runtime cost*. That came from 0.01s against 0.10s in the same table,
and those two cells are not comparable: the attach leg is DuckDB's own release C++ inside the library
and the per-source leg is first-party Rust built without optimisation. **Withdrawn rather than
qualified.** ADR 0006 now says so at the table, the three Rust legs remain comparable to each other
where the useful finding lives - a rendered pushdown and the federation layer are within noise, and
pulling whole tables is six times the memory - and **no time ratio involving the attach row is quoted
anywhere, here included.** Rerunning the three in release is the cheap way to make that column mean
something and nobody has.

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

**The fourth column names the branch that moves each one, and no longer a step number.** An earlier
version of this table indexed into an ordered list further down this page, which made this record a
second owner of a step number while `docs/implementation-plan.md` was the first
- the exact shape the *one owner per artefact* rule exists to prevent. The plan owns the numbering;
this record names branches.

| # | Assumption | Track 1 | Attach | Track 2 |
| --- | --- | --- | --- | --- |
| 1 | startup refusal, >1 source | untouched | untouched | **retired in `feat/source-registry`**, where more than one source becomes configurable and each declares its posture. The test is owed first, in `test/startup-source-refusals` |
| 2 | `ENGINE_SOURCE` constant | **its wrong-name branch is what a second dialect meets**; the constant itself stays | untouched | **retired in `feat/source-registry`**, with the same test owed first |
| 3 | one warehouse per service | untouched | untouched | **replaced by a keyed set, in `feat/source-registry`** |
| 4 | plan-stage `PlanSpansTwoSources` | untouched | untouched | **re-keyed to identity, and it is the LAST thing to move** - after `feat/credential-port`, because a principal type has to exist for it to key on |
| 5 | plan has one `source` | untouched | untouched | **stays. Permanently** |
| 6 | `answer`'s source comparison | untouched | untouched | **becomes per-leg, in `feat/two-source-execution`** |

The attach column is why that route is tactical: moving zero assumptions is what makes it cheap and
what makes its networked continuation the refused shape. The checks an honest federation has to relax
are the checks attaching leaves reading green.

Assumption 5 never moves, and that is deliberate - **but for one reason rather than the two this record
used to give.** Making a two-source `QueryPlan` representable would change a domain type whose
serialized form is pinned by 21 `plan@markdown` snapshots, and keeping every leg mono-source is what
keeps that type and the 126 statement and parameter goldens still.

**The second reason was false and is withdrawn:** this paragraph also said such a change would move the
definition digest. It would not. `DefinitionDigest::of` at
`crates/sutura-domain/src/definitions.rs:91` hashes the `Definitions` and the `Knowledge`, and
`QueryPlan` is in neither. The plan and the digest are pinned by different snapshots on purpose, and
saying otherwise made a plan-shape change sound like a provenance change - which spends exactly the
trust this record needs for the shape decisions above. What is true about the digest and federation is
one sentence, and it is about the catalog rather than the compiler: **the digest covers every model's
`source`, so moving a model to a second system already moves it.**

## The order, by branch

Dependent work, so it stacks: `stax` is in the dev shell and
`.agents/skills/git-ops/stacked-branches/SKILL.md` is the guidance. Each entry names the evidence it
owes, because work whose test passes against the base behaviour proves nothing.

**Named by branch and NOT numbered, deliberately.** An earlier version of this section numbered eight
steps while `docs/implementation-plan.md` numbered its own branches differently,
and the table above indexed into this list - two owners for one artefact, and a third reader having to
reconcile them. The plan owns the numbering and the dependency graph. What is below is this record's
*ordering argument*: which work must precede which, and why, in the vocabulary the plan already uses.

- **`feat/federation-decomposability` - the decomposability decision, and no refusal in it.** An
  exhaustive `const fn` over `Aggregate` returning the three-outcome enum *The finding that decides the
  cost* derives: descends as itself, descends as two columns, does not descend and travels as a
  grouping key. **There is no `RefusalReason` variant here and there must not be one** - 0009's
  Decision 2 pulls a non-decomposable measure up rather than refusing it, and a variant no question
  can provoke is what that enum refuses to carry. First, because it is the change that answers wrongly
  if it comes last. Pure domain addition; evidence is the per-outcome test and the total matches that
  will not compile until every aggregate has stated which of the three it is.
- **`test/startup-source-refusals` - the two missing startup-refusal tests, plus the multi-source
  corpus in `examples/multi-player` they and everything after them are asserted against.** The tests
  cover the **more-than-one** branch, which is the untested one, and are owed *before* the arm they
  guard is dismantled: red against a build with that arm removed. The corpus exists here rather than
  later because every subsequent entry needs a real on-disk two-source catalog, and because in this
  entry it demonstrates the refusal that holds TODAY - which is what makes the later flip from refusal
  to answer a visible behaviour change in a diff rather than a new test appearing. *The home this
  entry names was later repurposed:* `examples/multi-player` is the served-deployment DataHub example
  (`docs/adr/0016`'s amendment, issue #202), so the two-source corpus it asserts against remains
  unbuilt and is a prerequisite this entry still owns.
  **Not the single-player corpus:** moving a model there was costed and flips 7 questions from planned
  to refused, deletes 21 SQL goldens, and moves a sentence a gate counts.
- **`feat/source-registry` - per-source configuration, a keyed set of warehouses, and the two startup
  refusals retired.** Assumptions 1, 2 and 3 together, because they are one change seen from three
  places: the single `catalog.data_dir` becomes per source, the source-to-adapter selection
  `docs/architecture.md:595-600` records as deliberately absent arrives, and `SourceName` stops being
  compared for equality and starts selecting. Each retirement lands with the test written in the entry
  before it. This is also where each source declares its identity posture, which is what replaces the
  count-based refusal rather than merely removing it.
- **`feat/query-bounds` - the working-set ceiling and the deadline, before there is a combine to
  overrun them.** It comes before the combine rather than inside it, because a bound built after the
  thing it bounds is a bound nobody has watched bite. Evidence is each bound provoking its own typed
  refusal, and the working-set one shown biting rather than described.
- **`feat/leg-plan-types` - the leg types, their rendering and their goldens, and nothing that executes
  them.** `LegPlan`, `FederatedPlan` and `generate_leg` with its golden family have evidence of their
  own that needs no executor: a statement rendered, snapshotted and parse-checked in its target dialect
  is exactly the evidence the mono-source corpus runs on. So this is its own branch, and the reason is
  the evidence rather than the digest - **the definition digest does NOT move here**, per the
  correction above. What may NOT happen is that the stack stops at this branch: a `LegPlan` nothing
  renders and a `generate_leg` nothing calls is the *Built And Not Wired* shape, and AGENTS.md has a
  section named after that mistake. The branch after it is not optional.
- **`feat/two-source-execution` - the execution half, with two DuckDB files as its first instance.**
  The splitter in `sutura-semantic`, keyed on the relationship graph rather than on `SourceName`; the
  `Executable` enum on the port and the mechanical diff at every implementor and fake; the combiner
  port and its DataFusion implementor above the `Warehouse` port; assumption 6 becoming per-leg. Where
  the `RowSet`-to-Arrow conversion gets its one function and its byte budget, and where
  `differential.rs` gets its new shape. Evidence: rows equal to the single-source corpus, each leg's
  statement snapshotted, the filtered-remote-dimension-over-an-orphan-key case correct, and an exact
  `CountDistinct` across sources correct - **not "correct or refused"**, because Decision 2 chose the
  keys.
- **`feat/credential-port`, and only then the plan-stage refusal.** Assumption 4, re-keyed from *a plan
  spanning two sources* to *a plan spanning two identities*, which is the property that was always
  meant. It cannot be written before a principal type exists, and moving it first is the diff that
  produces a wrong number. **The plan's table has no row for this re-key** and needs one.

Track 1 - `feat/postgres-adapter` and, after it, BigQuery and Oracle - runs beside all of this and
blocks none of it: a dialect feature, a corrected bucket function, a row-limit clause, a golden family,
and a transport decision. Where it sits in the plan's dependency graph is the plan's decision, and this
record's only constraint on it is that nothing in track 2 waits for it.

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
acquires a victim: against three systems with grants, reading under a service identity is exactly the silent downgrade the multi-player deployment shape
exists against (`examples/multi-player/README.md`). **This
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
| Refusal is a result, not an error | Unchanged, and **narrower than this row used to claim.** It said a measure that cannot be combined refuses; 0009's Decision 2 pulls it up instead, so that is not a refusal and no variant may be added for it. What DOES refuse, each as a `RefusalReason` inside `ToolOutcome`: a cross-source join key that is a float, a query over the working-set ceiling, a query over the deadline, and a leg whose data system failed. **Ownership splits, and this row used to get it wrong:** the float join key is the SPLITTER's refusal and belongs to `feat/two-source-execution`; `feat/query-bounds` owes provoking tests for the working-set ceiling and the deadline, which is two rather than three, and its own section lists no join-key test because it should not |
| No value from a question reaches the statement as text | Unchanged, per leg. It is the specific thing the declined unparser route could not offer |
| The executed SQL is owned by `sutura-sql` | Unchanged, and this is the point of the decision: every statement that runs anywhere is one this repository rendered and pinned |
| We never translate SQL, and the one thing we parse is parsed at load | Unchanged, and it is what Oracle costs: the per-dialect rewrites live behind `transpile`, which stays uncompiled |
| A definitional filter is always applied | Unchanged. `required_filters` compile into the leg that owns the fact table, marked `PredicateOrigin::Definition` |
| A join cannot silently change a measure | Unchanged and **now load-bearing in a new way.** The `OneToMany` refusal is what stands between a cross-source join and a wrong sum, on a declaration nothing compares against the data - and across sources neither side can see the other's key distribution. The reconciliation test needs a fixture whose unmatched key sits on the far side |
| A catalog edit cannot change what executes | Unchanged. The digest covers every model's `source`, so moving a model to a second system already moves it |
| A plan cannot silently span two sources | Unchanged, per leg, and this is the row the attach continuation would have left true and meaningless |
| A result that hit the row cap is refused, not truncated | Held for the ANSWER, unchanged in value and moved in location: the combine applies `MAX_ROWS + 1` to the combined result and `answer` still returns `ResultTooLarge` above it. **This row used to say the cap gains a second number, a per-leg cap, and that both are asserted. Withdrawn:** 0009's Decision 3 retires the per-leg row cap outright, because a row count cannot tell 50,000 narrow rows from 50,000 wide ones, and a leg is bounded in BYTES by the working-set ceiling instead. So there is one row cap and it is the answer's; the leg's bound is a different bound counting a different thing, and *The leg SIZE bound* above states what it does and does not reach |
| No result cache | Unchanged. The combine buffers legs for one question; nothing is keyed and nothing is reused, and reuse across questions is what would make it a cache - at which point it is keyed on subject first or not at all |
| No panic path reachable from input | Unchanged, and the combine is where to watch it: an arithmetic re-aggregation is where the overflow lints earn their keep |
| The domain acquires no framework dependency | Unchanged. Nothing in this decision adds a dependency to the domain, and a dialect feature adds no dependency anywhere |
| Every query runs as the calling principal | **Still not held**, and now with a named consequence rather than a placeholder |

## Consequences

- **Track 1 is worth shipping on its own and blocks nothing in track 2, and that is a weaker claim
  than this bullet used to make.** It said track 1 should not wait for track 2, which reads as an
  ordering instruction - and `docs/implementation-plan.md` puts
  `feat/postgres-adapter` after the compose tier, for a reason this record agrees with: an adapter that
  lands before there is a real server to point it at is tested against a fake asserting our own code
  back to us. So the surviving claim is about **independence rather than precedence**: one BigQuery
  deployment and one Oracle deployment are single-source deployments, which is most of the value and
  none of the federation risk, and nothing in track 2 waits on either. Where track 1 sits in the
  dependency graph is the plan's decision.
- **The corpus is the demonstration of the pull-up and of its price, and no longer a counter-example.**
  This bullet used to say six of eleven metrics refuse a cross-source dimension. **They do not** -
  0009's Decision 2 pulls those six up instead. What survives is the count and what it now measures:
  six of eleven use `Avg` or `CountDistinct`, five of those `CountDistinct`, so five of eleven metrics
  demonstrate a fact leg carrying rows at the FACT grain rather than at the answer's. That is what the
  bound is for, and it is why *active subscriptions by region across two systems* is the corpus
  question worth watching rather than *revenue by region*.
- **`count_distinct` is the most expensive thing this shape does, and it is exact.** The exact fix is
  transporting the distinct keys and counting above - correct, reproducing the single-source rows
  exactly, and priced in *The leg SIZE bound* at one leg row per distinct key per bucket. An
  approximate count exists in every dialect and is **not** available here, and the reason is the tool
  surface rather than the arithmetic: an answer certified under a definition digest cannot be
  approximate without saying so on the wire, which is a tool surface change rather than an
  implementation.
- **Adding a dialect moves counted claims.** Oracle's row cap is not a `LIMIT`, so the sentence
  AGENTS.md holds with a counting gate becomes dialect-aware; and a golden family per dialect
  multiplies the snapshot count that gate reads.
- **The `PLAN_SPANS_TWO_SOURCES` prompt guide becomes wrong** when the plan-stage refusal is re-keyed
  to identity, and one AGENTS.md invariant row is deleted rather than demoted, per the rule at the head
  of that table. Two prose statements - in ADR 0003 and `docs/concepts.md` - say the one-source rule is
  currently enforced and move with it.
- **The port's signature change lands in one commit with the type, and every fake moves with it.**
  `Executable` on `execute` and `dry_run` touches `sutura-exec-datafusion`, `sutura-exec-duckdb`,
  `crates/sutura-app/tests/support/oracle.rs` and `crates/sutura-http/src/testing.rs`. It is a wide
  mechanical diff and AGENTS.md's own rule covers it: a mechanical change repeated across files belongs
  in one commit, not one per file.
- **`dry_run` becomes more valuable and more awkward.** It is defaulted because an in-process engine
  cannot check more cheaply than it runs. Across a network it is worth a round trip per leg, and five
  dry runs before five executions is ten round trips for one question.
- **Docker compose becomes part of the test story**, and with it the question of which gates require
  a running service. Precisely, because a looser sentence here would not survive a grep:
  `compose.dev.yaml` exists at the repository root and is the dev-container wrapper - **there is no
  compose file for services**, and standing one up is `feat/compose-tier`'s work. `just validate` runs
  in a sandbox with no network, so a compose-backed test is a tier above it rather than inside it, and
  saying which tier is a CI decision this record does not make.
- **Nothing here needs the tool surface to change**, now or later, and that is worth restating
  because it is the strongest fact in the survey and the easiest to forget under a heading about
  federation.

## What is explicitly not decided

**Two entries that used to be here are decided and are gone from this list rather than struck through**
- whether the per-leg row cap is the answer's cap or a larger one, and whether `Avg` is rewritten or
refused. 0009's Decisions 2 and 3 answer both: the per-leg row cap is retired in favour of a working
set in bytes, and `Avg` travels as a sum and a count divided once above. Both answers are argued where
they now belong, in *The finding that decides the cost* and *The leg SIZE bound*, so this list carries
what is genuinely open and nothing that is closed.

- **The transport, in the concrete.** Flight SQL is adopted as the direction and not built. Whether
  its prepared-statement binding carries our parameters is unverified and is the first thing to check.
- **The value of the working-set ceiling, and of the deadline.** 0009 carries a provisional 1 GB and a
  provisional three minutes and says plainly that nobody measured either. `feat/query-bounds` measures
  them on this corpus, and the number this record most wants back is the one the section above names:
  how much of a distinct-key leg the engine's memory pool can actually see.
- **Whether the `RowSet`-to-Arrow conversion survives the first federated answer or is replaced before
  it.** The boundary is decided - one function, in the combiner crate - and its lifetime is not. The
  measurement above is what decides it, and an Arrow-typed port is a record 0009 already says is
  wanted.
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

**A semi-join: push the far side's keys into the near side's statement as bind parameters.** The shape
a careful reader will ask about, because it pushes the dimension filter into the fact leg and so avoids
the fact leg grouping keys the combine will discard.

**Not, however, "the only shape that makes `count_distinct` exact across a boundary" - this record said
that and it was wrong.** Carrying the distinct keys in the fact leg and counting above is exact too,
reproduces the single-source rows, and is what 0009's Decision 2 chose. The false claim matters because
it made the semi-join look like the price of correctness when it is only a possible optimisation, and a
declined alternative described as the only correct route is an invitation to reopen it for the wrong
reason.

Declined on three counts, none of which is correctness: the parameter count is the far side's key
cardinality, so the statement is no longer one of a fixed set of golden shapes; it needs a `VALUES`
list or a temporary table, and the second is a write on a connection that should be read-only; and it
moves one source's key values into another source's query log and plan cache.

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
number. That is why re-keying it is the last thing to move and not the first.

**Export every source to Parquet and answer over the exports.** Works today with no code and is the
baseline every option has to beat. It fails on freshness, it doubles the storage, and a copy is read
under whoever made it - the same objection ADR 0003 raised against an acceleration layer, arriving by
hand. Against three systems with grants it is decoration in its most literal form.

**Track 1 only, and no federation ever.** The honest minimum, and it stays correct for as long as a
deployment is one data system. It is what this record recommends *until
`feat/federation-decomposability` exists*, because the one thing worse than no federation is a
federated answer nobody can tell is wrong.
