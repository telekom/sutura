---
kind: relationship
name: subscription_customer
origin:
  model: subscriptions
  column: customer_key
target:
  model: customers
  column: customer_key
join_type: many_to_one
---
Many subscription-months to one customer.

The cardinality is declared rather than inferred because it decides whether a join may
change a measure. Many-to-one cannot duplicate a snapshot row, so a revenue total is
the same number whether or not it was grouped by region. The reverse direction can
duplicate, and the catalog refuses to reach a dimension through one that may.
