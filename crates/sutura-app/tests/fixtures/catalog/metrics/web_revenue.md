---
kind: metric
name: web_revenue
model: orders
measure:
  simple: { aggregate: sum, column: amount_cents }
required_filters:
  - equals: { column: channel, value: web }
time_column: order_date
grains: [day, month]
---
Booked order value from the web channel, in minor units.

The filter is part of the name. `web_revenue` does not mean "revenue, which you
may narrow to web if you remember to": it means the web number, and a statement
that left the predicate out would return total revenue under this metric's
certified name. That is the failure this repository exists to prevent, arrived
at by omission rather than by tampering, so the predicate is applied to every
question about the metric and a caller can neither see it nor turn it off.

It is also why `channel` is not declared as a dimension here. A dimension is
something a caller may group by or filter on, and either would be a way to ask
`web_revenue` for the store figure. The metric that answers "how does revenue
split by channel" is `revenue`, which declares the dimension and carries no
required filter.
