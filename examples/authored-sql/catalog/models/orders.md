---
kind: model
name: orders
source: local
table: orders
columns: [ordered_at, order_id, customer_id, amount_cents]
---
One row per order, with its value in minor units.

`customer_id` is what `order_customer` joins on. It carries no attribute of its own; `region`
lives on `customers`, reached through that relationship, which is what gives this catalog a
dimension that is not on the fact table itself.
