---
title: Several metrics on one data system
description: A question over several metrics whose arms all read one data system runs as one statement on it. Each metric is aggregated in its own derived table and the tables are FULL joined in the statement. The data system joins them, not the combiner and not a second node (github.com/telekom/sutura#828).
---

# Several metrics on one data system

Status: **accepted**, 2026-10-31. This is the owner's decision. Nothing is built. This record decides
and proves nothing.

## The question

A question over several metrics that cannot share one grouped statement becomes an arm per metric
(`docs/adr/20261018144926-drill-across-for-several-metrics.md`). Two metrics on two models, or on two
time columns, take that path even when every arm reads the same data system. Each arm's leg then runs
as its own statement, and the rows travel to sutura and are joined in-process. Only the join runs in
sutura, yet every leg's grouped rows are held in its memory.

Should the data system that holds every arm also join them?

## Decision

**When every arm is local and every leg reads one data system, the question is one statement on that
data system.** sutura holds no leg's rows and joins nothing. Its node never goes across to another
node for it.

- **Each metric is the whole-answer plan it would be if asked alone.** Its own model, joins,
  filters, bucket and measure, rendered by the generator that renders a single metric today. Each
  metric is aggregated in its own derived table before any join.
- **The derived tables are FULL joined left to right on every key and the bucket, null-safe.** Each
  joined key is the coalesce of the two sides, the same shape as the in-process merge
  (`crates/sutura-exec-datafusion/src/combine/merge.rs`). A null key from two arms is one row, and a
  metric with no rows in a merged group answers null.
- **The chasm trap stays impossible.** `docs/adr/0002` makes it impossible by never letting two facts
  share a `FROM` before each is aggregated. A derived table per metric holds that property inside one
  statement, as two statements held it before. What `0002` ruled out is two facts in one `FROM`, not
  one statement.
- **A `top` for the whole question is the outer statement's.** Its order and limit apply after every
  arm joins, as the in-process merge applies them. The row limit applies to the outer statement only.
- **The plan is a whole answer from one data system.** It is one more shape the `Warehouse` port
  executes. It is not a set of legs, so an adapter that runs no leg answers it too, and one broker
  mint, one pre-flight and one charge cover it.
- **The planner checks the arms before it emits the plan.** That every arm has the same keys in the
  same order, the same bucket label and grain, and the same range is the plan type's to refuse, as
  `DrillAcrossPlan::new` refuses it today.
- **Two or more data systems keep the arm per metric.** Their legs run where each source is, and the
  size rule on the legs decides whether the join runs on one node or across several
  (`docs/adr/20261030182759-shuffle-across-nodes.md`).

## Per dialect

| Dialect    | Null-safe key equality                                                              |
| ---------- | ----------------------------------------------------------------------------------- |
| PostgreSQL | `IS NOT DISTINCT FROM`                                                              |
| DuckDB     | `IS NOT DISTINCT FROM`                                                              |
| BigQuery   | `IS NOT DISTINCT FROM`                                                              |
| ClickHouse | `isNotDistinctFrom`, with `join_use_nulls = 1` so an unmatched side is null         |
| Oracle     | `(a = b OR (a IS NULL AND b IS NULL))`, unless the pinned server takes the operator |

Each row is a decision to verify, not a fact measured here. Each dialect's golden parse check and,
where a tier runs, the differential decide which rendering ships. Bind parameters are concatenated
in arm order. A numbered placeholder in a later arm is shifted past the arms before it, and the
existing placeholder count check holds the whole statement.

## What this changes in older records

- `docs/adr/0002`'s second amendment says two statements on one source "are the only shape this
  amendment builds". That sentence is about a cross-model ratio's two facts, and it stands. This
  record changes only the several-metrics path.
- The drill-across record's "Arms run one after another" limit and "each leg runs in its own source"
  no longer describe a question whose arms all read one data system.

## What is not built

- **A cross-model ratio's two facts on one source** stay two statements combined in-process on the
  node that asked. Running them as one statement is the same derived-table shape. It is left for its
  own change, because the ratio divides across the two derived tables and applies `zero_denominator`
  above them.
- **A joined arm** (a metric that reaches another data system's dimension) keeps the arm per metric
  even when every other arm reads one source.

## Limits

- **The data system's comparison rules decide the join.** The in-process merge refuses a key or a
  bucket of another Arrow kind (`KeyKindMismatch`, `BucketKindMismatch`). One statement gets no such
  refusal. Two buckets of different SQL types are compared as that data system compares them, and
  that comparison may be an implicit cast. A conformed key ends on one model and column, so this
  limit is about the bucket.
- **The leg cap no longer bounds this shape's memory, and the source's own limits do.** The cap of
  legs bounded what sutura holds. Here sutura holds only the answer, bounded by the row limit, and the
  data system bounds the join.
- **BigQuery bills the one statement.** The dry run is of the whole statement, so the charge is the
  data system's estimate for the join, not the sum of the arms' estimates.
- **Provenance stays per source.** One data system is one `ExecutedAs` entry, as before.
