---
title: Drill across for several metrics
description: A question that names several metrics runs one arm per metric and joins the arms above on the answer's keys and time bucket. What the plan type holds today, what is not built, and the limits.
---

# Drill across for several metrics

Status: **accepted**. The plan type is built. The planner, the combine and the answer path are not.

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

## What is not built

- **No planner builds this plan.** Both refusals above still fire, and no question reaches it.
- **No combine.** The `FederationCombiner` port gains no method here. A method with no implementor
  is not a port, so the join arrives with the DataFusion combiner that implements it.
- **No conformance check, and no `QuestionNeedsTooManyLegs` refusal.** The plan type holds the cap;
  the refusal text a caller sees comes with the planner.
- **No `top` for the whole question.** A `top` on one arm is refused. A `top` for the whole question,
  for example by period after the join, is not built here.
- **No calendar per source.** A cross-model ratio whose calendar is on another source stays refused
  (`docs/adr/0002`). It is a separate decision with its own amendment.

All of this is tracked in `github.com/telekom/sutura#1471`.

## Limits

- **The cap counts legs, not bytes.** Each leg stays under its adapter's result budget, so the
  memory a question holds is at most the cap times that budget, plus the combiner's working set.
  The working-set ceiling is not a process memory bound.
- **Two arms that reach one remote dimension run its lookup twice.** The charge is conservative, not
  exact. Sharing a leg is a later change.
- **Arms run one after another,** so the latency is their sum. The question's deadline still bounds it.
- **The plan type cannot check that two arms read the same real-world thing.** Whether a dimension is
  conformed is the planner's check, on the catalog's declared model and column.
