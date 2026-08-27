---
kind: model
name: customers
source: local
table: customers
columns: [id, region_code, segment]
---
One row per customer. Attributes only: nothing here is additive, so joining to it
cannot change a measure.
