---
kind: model
name: orders
source: lake
table: orders
columns:
  - name: o_orderkey
    type: BIGINT
  - name: o_custkey
    type: BIGINT
  - name: o_orderstatus
    type: VARCHAR
  - name: o_totalprice
    type: DECIMAL(15,2)
  - name: o_orderdate
    type: DATE
  - name: o_orderpriority
    type: VARCHAR
---

One row per TPC-H order, in the same DuckDB source as its lines, so a join between the two stays in one statement.
