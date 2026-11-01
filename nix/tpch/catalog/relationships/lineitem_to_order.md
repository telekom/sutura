---
kind: relationship
name: lineitem_to_order
origin:
  model: lineitem
  column: l_orderkey
target:
  model: orders
  column: o_orderkey
join_type: many_to_one
---

Many lines to one order. Both sides are in the DuckDB source, so this hop is pushed down.
