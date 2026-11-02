---
kind: model
name: nation
source: crm
table: nation
columns:
  - name: n_nationkey
    type: BIGINT
  - name: n_name
    type: VARCHAR
  - name: n_regionkey
    type: BIGINT
---

The 25 TPC-H nations, in the PostgreSQL source beside the customers and suppliers that name them.
