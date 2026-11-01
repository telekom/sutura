---
kind: model
name: lineitem
source: lake
table: lineitem
columns:
  - l_orderkey
  - l_partkey
  - l_suppkey
  - l_quantity
  - l_extendedprice
  - l_discount
  - l_returnflag
  - l_linestatus
  - l_shipdate
  - l_shipinstruct
  - l_shipmode
---

One row per TPC-H order line. The fact table, kept in the DuckDB source.
