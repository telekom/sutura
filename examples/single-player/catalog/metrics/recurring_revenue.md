---
kind: metric
name: recurring_revenue
model: subscriptions
measure:
  simple: { aggregate: sum, column: mrr_cents }
required_filters:
  - equals: { column: status, value: active }
time_column: month
grains: [month]
dimensions:
  - name: segment
    column: segment
    via: subscription_customer
    values: [business, consumer, wholesale]
    description: The commercial segment of the customer, reached through one declared relationship.
  - name: region
    column: region
    via: subscription_customer
    values: [central, east, north, south, west]
    description: Where the customer is.
  - name: product_family
    column: product_family
    via: subscription_product
    values: [convergent, fixed_internet, mobile, tv]
    description: The kind of product rather than the individual tariff.
  - name: product_name
    column: product_name
    via: subscription_product
    description: >
      Declared without a value list, so it can be grouped by and not filtered on.
  - name: contract_term
    column: contract_term
    values: [annual, monthly]
    description: >
      Whether the subscription is on a rolling or a committed term. On the snapshot row
      itself, so no relationship is named and no join is planned.
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 202121
---
Recurring revenue recognised in the month, in minor units, from active subscriptions
only.

The filter is part of the name. This metric does not mean "revenue, which you may narrow
to active if you remember to": it means the active figure, and a statement that left the
predicate out would return revenue including terminated subscriptions under a certified
name. That is the failure arrived at by omission rather than by tampering, so the
predicate is compiled into every question about this metric and a caller can neither see
it nor turn it off.

`status` is therefore not a dimension here. A dimension is something a caller may group
by or filter on, and either would be a way to ask this metric for a number it does not
mean.

`contract_term` is the one dimension here that names no relationship, and it is declared
for what that costs the compiler rather than for the split it reveals. Every other
dimension on this metric is reached `via` a many-to-one join, so a catalog holding only
those would never render the case where a group-by key is read straight off the fact
table and contributes no join of its own. It is declared on
`subscription_months_billed` too, so the case survives either metric being rewritten.

The anchor is what this metric produced for June 2026 when it was certified. It is
re-executed on every run and at startup, so a definition that has stopped meaning what it
claimed fails readiness rather than answering.
