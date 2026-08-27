---
kind: metric
name: mean_subscription_mrr
model: subscriptions
measure:
  simple: { aggregate: avg, column: mrr_cents }
required_filters:
  - equals: { column: status, value: active }
time_column: month
grains: [month]
---
What the average active subscription was worth in the month, in minor units.

The mean of a COLUMN, which is a different definition from every ratio in this catalog and is
declared here to make that difference executable rather than argued. `revenue_per_customer` is
the same instinct written as a sum over a distinct count, and its own document says why: a
customer holding three subscriptions is one customer and three rows, so `avg(mrr_cents)` averages
subscription-months while that one averages customers. The two answers differ by however much the
base fans out. Reading the two files together is the shortest way to see that average revenue is
not a definition until somebody has said average over what.

Per SUBSCRIPTION-MONTH, then, and the name says so rather than saying per subscription. A
subscription that existed for all six months contributes six rows to a six-month question, and
this metric weights it six times. That is the right behaviour for the question it answers - what
a row of this snapshot is typically worth - and the wrong behaviour for a question about
subscriptions, which is why the metric that answers that one is a distinct count and not this.

`avg` and nothing else, which is the second reason the document exists. It is the only use of that
aggregate in this repository - the golden suite used to carry an e-commerce catalog of its own, and
this directory is what replaced it - and an aggregate no document writes is a generator arm nothing
renders and a plan nothing executes.

The status filter is part of the name, for the reason `recurring_revenue` gives at length: a
terminated subscription is one the month lost, and averaging its last invoice into what an active
subscription is worth answers a different question under a certified name. So `status` is not a
dimension here either.

Carries no anchor, and this is the plain float case rather than an interesting one. A mean is a
division, a division is not exact in binary, and an anchor is compared as rendered text - so the
comparison would pin how a language prints an expansion. The sum and the count it is built from
are both certified beside it, under `recurring_revenue` and `active_subscriptions`, which is where
a drift in either half would surface first.
