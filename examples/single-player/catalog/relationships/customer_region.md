---
kind: relationship
name: customer_region
origin_model: customers
target_model: regions
keys:
  - equal: { origin: region, target: region }
join_type: many_to_one
---
Many customers to one region. The second hop of the only chained dimension here.

Its origin is `customers`, not `subscriptions` - which is the whole point of declaring a
chain rather than a relationship per dimension. A chain is a path: hop N starts where hop
N-1 ended, the catalog refuses one that does not join up, and the statement's `ON` clauses
have to be qualified the same way the path is walked.

Many-to-one, for the reason `subscription_customer` gives: a hop that may duplicate rows
would change the measure, and the catalog refuses to reach a dimension through one. Here
that declaration says each region appears once in `dim_region`, which the deployment
counts at startup like every other declared join key.
