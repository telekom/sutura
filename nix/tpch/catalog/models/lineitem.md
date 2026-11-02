---
kind: model
name: lineitem
source: lake
table: lineitem
columns:
  - name: l_orderkey
    type: BIGINT
  - name: l_partkey
    type: BIGINT
  - name: l_suppkey
    type: BIGINT
  - name: l_quantity
    type: DECIMAL(15,2)
  - name: l_extendedprice
    type: DECIMAL(15,2)
  - name: l_discount
    type: DECIMAL(15,2)
  - name: l_returnflag
    type: VARCHAR
  - name: l_linestatus
    type: VARCHAR
  - name: l_shipdate
    type: DATE
  - name: l_shipinstruct
    type: VARCHAR
  - name: l_shipmode
    type: VARCHAR
---

One row per TPC-H order line. The fact table, kept in the DuckDB source.
