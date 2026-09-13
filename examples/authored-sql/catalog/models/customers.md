---
kind: model
name: customers
source: local
table: customers
columns: [customer_id, region]
---
One row per customer: which region they are in, and nothing else. Joined from `orders` so a
question can group by region without region being a column on the fact table.
