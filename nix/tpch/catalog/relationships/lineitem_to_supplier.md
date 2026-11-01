---
kind: relationship
name: lineitem_to_supplier
origin:
  model: lineitem
  column: l_suppkey
target:
  model: supplier
  column: s_suppkey
join_type: many_to_one
---

Many lines to one supplier. Crosses from DuckDB to PostgreSQL.
