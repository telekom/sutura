---
kind: model
name: customer
source: crm
table: customer
columns:
  - name: c_custkey
    type: BIGINT
  - name: c_nationkey
    type: BIGINT
  - name: c_mktsegment
    type: VARCHAR
---

One row per TPC-H customer, moved into the PostgreSQL source. Reaching it from a line or an order crosses systems.
