---
kind: relationship
name: order_to_customer
origin:
  model: orders
  column: o_custkey
target:
  model: customer
  column: c_custkey
join_type: many_to_one
---

Many orders to one customer. Crosses from DuckDB to PostgreSQL.
