---
kind: relationship
name: orders_customer
origin:
  model: orders
  column: customer_id
target:
  model: customers
  column: id
join_type: many_to_one
---
Many orders to one customer.

The cardinality is declared rather than inferred because it decides whether a
join may change a measure. Many-to-one cannot duplicate an order row; the
reverse direction can, and the catalog refuses to reach a dimension through it.
