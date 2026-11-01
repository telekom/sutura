---
kind: relationship
name: nation_to_region
origin:
  model: nation
  column: n_regionkey
target:
  model: region
  column: r_regionkey
join_type: many_to_one
---

Many nations to one region, inside the PostgreSQL source.
