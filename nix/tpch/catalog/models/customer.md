---
kind: model
name: customer
source: crm
table: customer
columns:
  - c_custkey
  - c_nationkey
  - c_mktsegment
---

One row per TPC-H customer, moved into the PostgreSQL source. Reaching it from a line or an order crosses systems.
