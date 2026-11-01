---
kind: relationship
name: customer_to_nation
origin:
  model: customer
  column: c_nationkey
target:
  model: nation
  column: n_nationkey
join_type: many_to_one
---

Many customers to one nation, inside the PostgreSQL source.
