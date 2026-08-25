---
kind: metric
name: orders_placed
model: orders
measure:
  aggregate: count
  column: order_id
time_column: order_date
grains: [day, month]
dimensions:
  - name: region
    column: region_code
    via: orders_customer
    values: [north, south, west]
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 8
---
How many orders were booked.

Counts the order key rather than rows. Over a joined result those are different
numbers, and the one that means "orders" is the key.
