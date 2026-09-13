---
kind: metric
name: order_value_spread
model: orders
authored_sql:
  portable: MAX(amount_cents) - MIN(amount_cents)
time_column: ordered_at
grains: [day, month]
---
The distance between the largest and smallest order value in the period.

This is catalog-authored SQL because it combines two aggregates into one number, outside the
closed measure vocabulary. The fragment is stored as written and pinned under the definition digest;
nothing published compiles or executes it, so every adapter this repository ships refuses to start
on this catalog, naming this metric.
