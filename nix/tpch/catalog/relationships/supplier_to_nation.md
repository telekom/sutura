---
kind: relationship
name: supplier_to_nation
origin:
  model: supplier
  column: s_nationkey
target:
  model: nation
  column: n_nationkey
join_type: many_to_one
---

Many suppliers to one nation, inside the PostgreSQL source.
