---
kind: relationship
name: lineitem_to_part
origin:
  model: lineitem
  column: l_partkey
target:
  model: part
  column: p_partkey
join_type: many_to_one
---

Many lines to one part. Crosses from DuckDB to PostgreSQL.
