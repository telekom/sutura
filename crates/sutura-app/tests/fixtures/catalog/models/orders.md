---
kind: model
name: orders
source: local
table: orders
columns: [order_id, order_date, customer_id, channel, amount_cents]
---
One row per order, as booked.

Money is held in minor units so that a total is exact. A metric that summed a
decimal column would depend on how two languages happen to print the same bits,
and an anchor comparison over that is a test that fails for the wrong reason.
