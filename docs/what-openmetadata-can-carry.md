---
title: What an OpenMetadata catalog can carry, and the two half-a-definition spots it keeps
description: A research spike in the method of ADR 0016, measured against the SemanticCatalog port - OpenMetadata is the DataHub shape with a richer entity model: a typed Metric entity (decidable aggregation, granularity and dimensions) and declared relationship cardinality, but with the measure-to-column binding and the filter left as free-text SQL and expression strings, so an adapter over it is a declaring source that provides the physical model, descriptions, typed metrics, a declared grain and non-duplicating joins, and reports-not-defines the free-text half.
---

# What an OpenMetadata catalog can carry

Status: **a finding, and the first deliverable** of issue #152's plan. The roadmap lists OpenMetadata
among the metadata connectors, and the issue is explicit that it is *"the DataHub shape with a
different API - the cheapest of the remaining four that need a service"*, with #114's HTTP-client +
bearer-credential + typed-declaration template. This record is the ADR-0016-shaped answer to the
question that decides the adapter's whole shape: **can an OpenMetadata deployment express a complete
semantic model as this repository defines one, or only part of one?** The answer, measured against the
published entity schema rather than guessed, is *most of one but with two slots that stay
half-a-definition* - which is a different and richer answer than ADR 0016 gave DataHub or the OKF
finding gave Frictionless Table Schema.

## What was measured against

Read out of the port, exactly as ADR 0016 and the OKF finding do. `SemanticCatalog::load` returns a
`PinnedDefinitions`; its halves are a `Definitions` and a `Knowledge`, and `Definitions::assemble` is
the only constructor of the first. So *can an OpenMetadata source express our model* is really *can it
fill these fields and survive these checks*:

| What a complete semantic model supplies           | The type in `sutura-domain`                                                                                                                                |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| a physical table and the column set it exposes    | `Model` - `ModelName`, `SourceName`, `TableName`, `BTreeSet<ColumnName>`, `Description`                                                                    |
| a join                                            | `Relationship` - a name, two `(ModelName, ColumnName)` endpoints, and a `JoinType` of `OneToOne`, `ManyToOne` or `OneToMany`                               |
| what a metric measures                            | `Measure` - `Simple(Term)` or `Ratio { numerator, denominator, zero_denominator }`, over a `Term` of `Aggregate(AggregatedColumn)` or `CountIf { column }` |
| a predicate that is part of what the metric means | `Vec<RequiredFilter>` - `Equals`, `NotEquals`, `IsTrue`, `IsNotNull`                                                                                       |
| when, and at what resolution                      | `ColumnName` plus a `BTreeSet<Grain>`                                                                                                                      |
| what it may be broken down by                     | `Dimension` - a column, optionally `via` one relationship, optionally an allowlist of at most `MAX_VALUES_PER_DIMENSION` values                            |
| the number it produced when it was certified      | `Option<Anchor>` - a `TimeRange` and a value as text                                                                                                       |
| what a reader has to know                         | `Knowledge` - phrases, caveats, reviewed absences and worked examples, each with a `Referent`                                                              |

`Definitions::assemble` refuses a dimension reached through a relationship whose declared cardinality
may duplicate rows, as `JoinWouldDuplicateRows`; and `Measure` has no free-text SQL at any depth - its
`Term` is `Aggregate` or `CountIf` over a validated column, never an `expression:` string.

## The model this names, and its version

OpenMetadata is a hosted metadata-management platform (`open-metadata.org`). This record reads the
entity model it serves as its own source of truth - the published JSON Schema under
`openmetadata-spec/src/main/resources/json/schema`, at the `main` revision of 2026-09-18: the `Table`
entity (`entity/data/table.json`, `title: "Table"`) and the `Metric` entity (`entity/data/metric.json`,
`title: "Metric"`). A version tag is deliberately not pinned the way a file-spec vocabulary would be:
OpenMetadata ships as a service whose served schema is whatever the running server carries, so *the
deployment is the spec* - the same reason the read path below treats a drifted instance as the finding's
limit rather than as an error in the record. Two entity kinds carry the whole answer.

## What the model actually carries, field by field

**`Table`** (`entity/data/table.json`): `name` / `fullyQualifiedName` (the model name and service path),
`columns: [Column]`, `tableConstraints: [TableConstraint]`, `description`, `owners`, `tags`. There is
no `foreignKeys` field - an earlier revision of this finding invented one, and the schema (read
again against `open-metadata/OpenMetadata@main`) has no such array on `Table`; `TableConstraint` is
the only carrier of a foreign key. Each `Column` carries `name`, a `description`, a `constraint`
(`NULL`, `NOT_NULL`, `UNIQUE`, `PRIMARY_KEY`) and - the notable part - a `dataType` whose enum is a
**column type system that already contains the semantic kinds**: `NUMBER`, `STRING`, …, but also
`MEASURE`, `MEASURE VISIBLE`, `MEASURE HIDDEN` and `KPI`. A `TableConstraint` carries exactly
`constraintType` (`UNIQUE`, `PRIMARY_KEY`, `FOREIGN_KEY`, …), `columns`, `referredColumns` (the
target's fully qualified column names - `additionalProperties: false`, so there is no `name` and no
nested `referencedTable`) and a `relationshipType` that **enumerates cardinality**: `ONE_TO_ONE`,
`ONE_TO_MANY`, `MANY_TO_ONE`, `MANY_TO_MANY`. The `TableResource`'s own `FIELDS` list (Java source)
shows `columns` and `tableConstraints` are populated only when a caller's list request names them
with `?fields=` - **the adapter has to ask**, or a real list answers without either.

**`Metric`** (`entity/data/metric.json`): `name`, `description` (markdown), `metricType`
(`COUNT`, `SUM`, `AVERAGE`, `RATIO`, `PERCENTAGE`, `MIN`, `MAX`, `MEDIAN`, `MODE`,
`STANDARD_DEVIATION`, `VARIANCE`, `SIMPLE`, `CUMULATIVE`, `DERIVED`, `CONVERSION`, `OTHER`),
`metricExpression: { language: SQL|Java|JavaScript|Python|External, code: string }`,
`unitOfMeasurement` / `customUnitOfMeasurement`, `granularity`
(`SECOND`, `MINUTE`, `HOUR`, `DAY`, `WEEK`, `MONTH`, `QUARTER`, `YEAR`), `dimensions: [{
name, type: CATEGORICAL|TIME, expression: string }]`, `measures: [{ name, aggregation: string,
description, expression: string }]`, `filters: [{ where: SQL }]`, `relatedMetrics`, `assets`.

## The two half-a-definition spots

**A measure is carried - richer than OKF - but the aggregation-to-column binding is free text.**
`metricType` is a decidable aggregation kind and `granularity`/`dimensions[].type` are typed, which is
already more than DataHub (`MetricInfo.expression`, a raw string with no aggregation function) and
immeasurably more than Frictionless Table Schema (no metric entity at all). But the domain's `Measure`
is not a bare aggregation kind: its `Term` is `Aggregate(AggregatedColumn)` or `CountIf { column }`, a
*column-bound* aggregation. In OpenMetadata that binding lives in the loose layer - `measures[].name`
plus `measures[].aggregation` and `measures[].expression` as free strings, and `metricExpression.code`
in a dialect set (`SQL`/`Java`/`JavaScript`/`Python`/`External`) that does not intersect the typed
`Term`. ADR 0016's refusal fires here, once: *a measure carried as a raw expression string in a foreign
dialect is half a definition*. Taking `metricType` supplies the aggregation but not a validated column;
taking the expression certifies a foreign dialect's free text. So the honest `capabilities()` is
**provides `Measure` from the typed `metricType` + granularity where a column binding also resolves**,
and **reports-not-defines** the expression/aggregation strings - the shape of issue #152's own cell
name, *a metric whose measure is an expression string is reported and not defined*.

**The filter is a raw SQL `where`, not a typed predicate.** `metricFilter` carries only `where: string`.
There is no `Equals` / `IsTrue` / `IsNotNull` vocabulary to map onto `RequiredFilter`; a `where` is a
predicate fragment in a dialect this repository does not parse. **Declared, not provided** - loaded or
ignored, but never minted into a `RequiredFilter` without executing it.

And cardinality, the third thing ADR 0016 taught us to check, is the pleasant surprise: it is **declared
when present, never defaulted**. `TableConstraint.relationshipType` explicitly distinguishes
`ONE_TO_ONE` / `MANY_TO_ONE` / `ONE_TO_MANY` (non-duplicating, license a `JoinType`) from
`MANY_TO_MANY` (row-duplicating, refused by `JoinWouldDuplicateRows`). DataHub defaulted a cardinality
(`N_N`) that the assembler had to refuse; Frictionless Table Schema omitted the property entirely
(licenses nothing); OpenMetadata names it when it is there and stays silent when it is not - the case a
deployment leaves a relationship unconstrained licenses no dimension, exactly as the assembler demands.

## The mapping

| The semantic-model field           | What OpenMetadata offers                                                                              | Verdict                                                                                                                                           |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Model` (table + columns)          | `Table` + `columns[]` (`name`, `dataType`, `description`)                                             | **provides**                                                                                                                                      |
| `Description`                      | `description` markdown on Table, Column, Metric                                                       | **provides**                                                                                                                                      |
| `Relationship` (join)              | `tableConstraints[constraintType=FOREIGN_KEY]` with a `relationshipType`                              | **provides** where cardinality is declared `ONE_TO_ONE`/`MANY_TO_ONE`/`ONE_TO_MANY`; declares-and-does-not-license where absent or `MANY_TO_MANY` |
| `Measure`                          | `Metric.metricType` (decidable) + `granularity`, with a loose `expression`/`aggregation` string layer | **provides from the typed enum; reports-not-defines the expression strings**                                                                      |
| `RequiredFilter`                   | `metricFilter.where`: raw SQL                                                                         | **declared, not provided**                                                                                                                        |
| `Grain` + time                     | `Metric.granularity` enum (`SECOND`..`YEAR`)                                                          | **provides**                                                                                                                                      |
| `Dimension` with `via` / allowlist | `Metric.dimensions[]` (`name`, `type` `CATEGORICAL`\|`TIME`, `expression`)                            | **partial** - a typed dimension name and kind, but no allowlist and no resolved `via`-relationship                                                |
| `Anchor`                           | none in the metric or table entities                                                                  | **absent, declared**                                                                                                                              |
| `Knowledge` with `Referent`        | `description`, `owners`, `tags` free text                                                             | **partial** - prose travels; referent-bearing phrases/caveats/worked examples do not                                                              |

## Conclusion, and what it costs

**The richest of the three measured sources, and still a `declaring` source - because it keeps two
half-a-definition slots rather than eliminating them.** OpenMetadata is not strictly narrower than
Wren the way Table Schema is, nor raw the way DataHub's metric is: it carries a typed metric layer and
declared cardinality, and the two spots that stay unresolved are the measure's free-text binding and the
SQL filter - exactly the places ADR 0016 found half-a-definition in DataHub. An adapter over it is a
`declaring` `SemanticCatalog` that provides the physical model, descriptions, typed metrics (with a
decidable `metricType` and `granularity`), and non-duplicating declared joins, and **reports-not-defines**
the expression strings and the `where` filters, exactly as issue #152's cell anticipates. Because it
needs a service, the cost is #114/#886's HTTP-client + bearer-credential arm plus a recorded-document
fixture - the "cheapest of the remaining four that need a service", with a richer model to show for it
than DataHub.

## Alternatives considered

**Was the `MEASURE` column `dataType` a metric?** No. A column typed `MEASURE`/`KPI` marks a column
as measuring something but carries no aggregation kind, no expression and no grounding - it is a tag on a
column in the physical layer, not the `Metric.metricType` of the semantic entity, and a `declaring`
adapter would treat column-level `MEASURE`/`KPI` types as the same reported-not-defined slot rather than
as a supplied `Measure`.

**Should the `Metric` entity be loaded at all if its binding is loose?** Yes, for the same reason any
declared provisioning is worth shipping: `metricType` + `granularity` + `dimensions[].type` are
decidable and validate, the bundle agrees that the free-text half never becomes a `Measure`, and the
cost is a bounded parse - while refusing the whole entity would throw away the only typed metric layer
of the three sources measured.

## Limits, stated

Written read-only, against the published OpenMetadata entity schema at the `main` revision read on
2026-09-18 and the `SemanticCatalog`/`Definitions` port in this repository, **not** against a
provisioned instance (the nix sandbox has no network, and no instance was booted for this record). Two
claims are open to a live check and should be before an adapter ships: (a) the metric entity is new to
the served surface and a real deployment's `/api/v1/metrics` may not be populated at all - the
"what does a real instance actually populate" question issue #152 names - which only strengthens the
narrow/normal case (Structure + Descriptions) and the declared-not-provided verdict; and (b) the read
path's cost (#114's own first question): how many `GET`s a whole bundle takes against the REST API when
metric-to-asset references are followed, which the finding does not measure. Neither changes the shape
decision this record makes.
