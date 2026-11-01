---
title: Several metrics on one data system
description: A question over several metrics whose arms all read one data system runs as one statement on it. Each metric is aggregated in its own branch, the branches are stacked with UNION ALL and grouped again on the keys and bucket. The data system joins them, not the combiner and not a second node (github.com/telekom/sutura#828).
---

# Several metrics on one data system

Status: **accepted**, 2026-10-31. This is the owner's decision. Built (#1656): metrics on one data
system are planned as one stacked statement, every SQL adapter renders it, and DataFusion runs it as
one logical plan. What is not built stays below. No cell runs the statement on a live data system,
and ClickHouse, Oracle and BigQuery have not run it. This record decides and proves nothing.

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
  metric is aggregated in its own branch before any join.
- **The branches are stacked with `UNION ALL` and grouped again on every key and the bucket.** Each
  branch projects its own measure and a null under every other metric's label, and the outer
  statement takes each measure's `MAX`. A branch has at most one row per group, so `MAX` returns that
  row's value, and a metric with no rows in a group answers null. `GROUP BY` puts null keys in one
  group in every dialect, so a null key from two arms is one row, as in the in-process merge's
  null-safe full join (`crates/sutura-exec-datafusion/src/combine/merge.rs`).
- **The chasm trap stays impossible.** `docs/adr/0002` makes it impossible by never letting two facts
  share a `FROM` before each is aggregated. A branch per metric holds that property inside one
  statement, as two statements held it before. What `0002` ruled out is two facts in one `FROM`, not
  one statement.
- **A `top` for the whole question is the outer statement's.** Its order and limit apply after every
  arm joins, as the in-process merge applies them. A `top` by metric ranks by the last metric's
  measure, as that merge ranks. The row limit applies to the outer statement only.
- **The plan is a whole answer from one data system.** It is one more shape the `Warehouse` port
  executes. It is not a set of legs, so an adapter that runs no leg answers it too, and one broker
  mint, one pre-flight and one charge cover it.
- **The planner checks the arms before it emits the plan.** That every arm has the same keys in the
  same order, the same bucket label and grain, and the same range is the plan type's to refuse, as
  `DrillAcrossPlan::new` refuses it today.
- **Two or more data systems keep the arm per metric.** Their legs run where each source is, and the
  size rule on the legs decides whether the join runs on one node or across several
  (`docs/adr/20261030182759-shuffle-across-nodes.md`).

## One rendering, not one per dialect

A null-safe `FULL JOIN` needs a different key equality per dialect, and PostgreSQL refuses it. On
PostgreSQL 18.6 both `ON l.k IS NOT DISTINCT FROM r.k` and `ON (l.k = r.k OR (l.k IS NULL AND r.k IS
NULL))` fail with `FULL JOIN is only supported with merge-joinable or hash-joinable join conditions`.
`UNION ALL` and `GROUP BY` need no key equality, and one rendering serves every dialect.

Bind parameters are concatenated in arm order. A numbered placeholder in a later arm is shifted past
the arms before it, and the existing placeholder count check holds the whole statement.

## What this changes in older records

- `docs/adr/0002`'s second amendment says two statements on one source "are the only shape this
  amendment builds". That sentence is about a cross-model ratio's two facts, and it stands. This
  record changes only the several-metrics path.
- The drill-across record's "Arms run one after another" limit and "each leg runs in its own source"
  no longer describe a question whose arms all read one data system.

## What is not built

- **A cross-model ratio's two facts on one source** stay two statements combined in-process on the
  node that asked. Running them as one statement is the same branch shape. It is left for its own
  change, because the ratio divides across the two branches and applies `zero_denominator` above
  them.
- **A joined arm** (a metric that reaches another data system's dimension) keeps the arm per metric
  even when every other arm reads one source.

## Limits

- **The data system's type rules decide the join.** The in-process merge refuses a key or a bucket
  of another Arrow kind (`KeyKindMismatch`, `BucketKindMismatch`). One statement gets no such
  refusal. `UNION ALL` resolves two buckets of different SQL types to a common type as that data
  system resolves it, or refuses them. A conformed key ends on one model and column, so this limit is
  about the bucket.
- **A typeless null is the data system's to type.** A branch projects an untyped null under another
  metric's label. PostgreSQL 18.6 and DuckDB resolve it to the other branch's type. ClickHouse,
  Oracle and BigQuery have not been run here, and each dialect's tier decides.
- **The leg cap no longer bounds this shape's memory, and the source's own limits do.** The cap of
  legs bounded what sutura holds. Here sutura holds only the answer, bounded by the row limit, and the
  data system bounds the join.
- **BigQuery bills the one statement.** The dry run is of the whole statement, so the charge is the
  data system's estimate for the join, not the sum of the arms' estimates.
- **Provenance stays per source.** One data system is one `ExecutedAs` entry, as before.
