---
title: Drill across for several metrics
description: A question that names several metrics runs one arm per metric and joins the arms above on the answer's keys and time bucket. What the plan type holds today, what is not built, and the limits.
---

# Drill across for several metrics

Status: **accepted**. The plan type, the DataFusion merge, the leg loop, the planner and the answer path are built. A question over several metrics that cannot share one statement is planned as an arm per metric, answered, and joined above.

## The question

A question that names two metrics on different models, or two metrics with a dimension on another
data system, is refused today (`MetricsSpanDifferentModels`, `MultiMetricFederationNotExecutable`).
The combiner joins two legs, or three for a cross-model ratio, and returns one measure column.
Can one question return a metric from each of two data systems, by one shared dimension?

## Decision

**Each metric is one arm. Every arm is aggregated on its own, and the arms are joined above.**
Two metrics never share a `FROM`, so the chasm trap stays impossible, and no new leg shape is needed.

- **A joined arm** is a metric that reaches a remote dimension: the `FederatedPlan` the splitter
  already builds.
- **A local arm** is a metric whose dimensions are all on its own source: one fact leg, grouped by
  the answer's keys and the bucket.
- **The join is above the legs and in the combiner.** It is a FULL join on every key and the bucket,
  null-safe on the keys, so a null key from two arms is one row. DataFusion and Arrow do the join,
  as they do every other combine. Each leg runs in its own source as the caller, as today.
- **A metric with no rows in a merged group answers null.** "No rows" stays visible, and a ratio arm
  cannot fail the answer by dividing an invented zero. Inside one arm, `docs/adr/0002`'s empty-set
  rule still applies to each leaf. Across arms, a metric with no rows in a merged group answers null.
- **Two metrics share a dimension when it ends on the same model and column.** A declared type is
  not enough, and a replicated dimension (one copy per source) is not conformed by this rule.
- **Each metric buckets by its own time column.** The grain and the label must agree. Two bucket
  columns of different Arrow kinds are refused by the combine, never cast. Every fact leg reads the
  same time range.
- **A question may run at most six legs, over every arm.** A leg's result is held whole before the
  combine, so the held memory grows with the leg count. The cap is a property of the plan type.

## What is built

`DrillAcrossPlan` in `sutura-domain` (`plan::drill_across`). It holds two or more arms and refuses
a set of arms that cannot be joined, with a typed error each:

| Refusal                                                   | Why                                       |
| --------------------------------------------------------- | ----------------------------------------- |
| one metric in two arms                                    | one measure column would be emitted twice |
| an arm with other keys, or the same keys in another order | the join columns would not line up        |
| an arm with another bucket label or grain                 | the join would compare unlike periods     |
| a fact leg over another time range                        | a period one arm never read would be null |
| a joined arm carrying a `top`                             | it cuts one arm, so that arm gets a null  |
| more than `MAX_LEGS` (6) legs                             | bounds the memory the legs hold           |

A local arm refuses a leg that is not a fact leg, terms that are not its federation's leaves, a leaf
on another model, and an answer key (or a distinct count's key) the leg does not project.

## The combiner

`DrillAcrossCombiner::merge` (`sutura-domain`) is a supertrait of `FederationCombiner`, so the error
and the two classifying predicates are the two-leg combine's. `DataFusionCombiner` implements it.

- **One session, one plan.** Each arm is cut to a logical plan (a joined arm by the plan the two-leg
  combine runs, a local arm by one aggregate over its fact leg), and the plans are joined left to
  right into one plan that runs once. The working-set ceiling is the question's, not each arm's.
- **The join is FULL on every key and the bucket, a null equal to a null.** It is
  `NullEquality::NullEqualsNull` on `join_detailed` at the pinned DataFusion; a plain `=` would answer
  a null key two arms share as two rows. A group one arm lacks answers null under that arm.
- **A key or a bucket of another Arrow kind is refused.** `FederatedAnswerRefusal::KeyKindMismatch` for
  a key, `BucketKindMismatch` for the bucket; never a cast, because a text `"01"` and an integer `1`
  are one group under a cast. The types are read off each arm's cut plan before the join, so a key on a
  joined arm's lookup leg is compared too, and the refusal carries the two Arrow type names and the
  label, never a key value. A kind is the join key's own, so `Utf8` and `LargeUtf8` are one.
- **Leg results that are not the plan's arms** are `ArmsMiswired`: this workspace's defect. A count
  that is not the plan's, a variant that is not the arm's, and a local arm's result tagged with another
  metric are checked. A joined arm's two legs carry a side and no metric, so two joined arms' results
  are not told apart by the port.

Limits: a key type that is not a join-key type (a float, a timestamp) must be identical across arms,
so two timestamp units are refused though they could join. A declared cast across arms is not honoured.
The ceiling bounds the merge's operators, as
for the two-leg combine, not the leg results the caller already holds. Only local arms and one joined
arm over in-memory legs are measured.

## The planner and the answer path

`sutura_semantic` resolves each metric's own dimensions, so one dimension name can reach another
model or column for another metric. A question over several metrics stays one grouped statement when
every metric is on one model and one time column, every dimension is reached by the same
relationships, no dimension is on another source and no metric names another model. Otherwise it is
an arm per metric.

- `DimensionNotConformed` is raised at resolve time for a grouped or filtered dimension that does not
  end on the same model and column for every metric.
- `QuestionNeedsTooManyLegs` is raised when the arms run more legs than `MAX_LEGS`.
- `PlanSpansTooManySources` holds over the whole plan: more than four distinct data systems between
  all arms are refused, where the limit was two before the twentieth amendment of
  `0007-federating-across-different-data-systems.md`. An arm may read up to three lookup legs, and
  `PlanNeedsTooManyLookupLegs` refuses a fourth.
- `MetricsSpanDifferentModels` and `MultiMetricFederationNotExecutable` are retired: both shapes are
  planned. An adapter that runs no leg now refuses such a question as `FederationNotExecutable`.

`answer_drill_across` (`sutura-app`) does one lookup of every leg's adapter, one mint over every
distinct source, a pre-flight of every leg and one summed charge before any leg runs, then
`DrillAcrossCombiner::execute_across`.

## What is not built

- **No `top` for the whole question.** A `top` on one arm is refused. A `top` for the whole question,
  for example by period after the join, is not built here. `github.com/telekom/sutura#1566` tracks it.

## Limits

- **The cap counts legs, not bytes.** Each leg stays under its adapter's result budget, so the
  memory a question holds is at most the cap times that budget, plus the combiner's working set.
  The working-set ceiling is not a process memory bound.
- **Two arms that reach one remote dimension run its lookup twice.** The charge is conservative, not
  exact. Sharing a leg is a later change.
- **Arms run one after another,** each arm's legs in `Arm::legs` order, so the latency is the sum of the
  legs'. A deadline is the leg runner's to hold.
- **Provenance is per source, not per leg.** `ExecutedAs` records how each data system was reached; a
  source two arms read is one entry.
- **The plan type cannot check that two arms read the same real-world thing.** Whether a dimension is
  conformed is the planner's check, on the catalog's declared model and column.

## First amendment, 2026-10-23: the cap of six legs is a setting

The decision's "at most six legs, over every arm" becomes `governance.leg_ceiling`, six by default
and sixteen (`MAX_LEGS`) at most, and it now bounds a one-metric plan too. The planner counts the legs
of every arm before `DrillAcrossPlan::new` and refuses `QuestionNeedsTooManyLegs` naming the
configured number; `MAX_LEGS` is only the plan type's own cap. Each arm's lookup legs are bounded by
`governance.lookup_leg_ceiling`, three by default. `docs/adr/0007-federating-across-different-data-systems.md`'s
twenty-third amendment records the three plan bounds together.
