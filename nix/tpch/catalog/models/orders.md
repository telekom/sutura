---
kind: model
name: orders
source: lake
table: orders
columns:
  - o_orderkey
  - o_custkey
  - o_orderstatus
  - o_totalprice
  - o_orderdate
  - o_orderpriority
---

One row per TPC-H order, in the same DuckDB source as its lines, so a join between the two stays in one statement.
