---
kind: model
name: part
source: crm
table: part
columns:
  - name: p_partkey
    type: BIGINT
  - name: p_mfgr
    type: VARCHAR
  - name: p_brand
    type: VARCHAR
  - name: p_type
    type: VARCHAR
  - name: p_container
    type: VARCHAR
---

One row per TPC-H part, in the PostgreSQL source.
