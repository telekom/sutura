---
kind: metric
name: revenue
model: orders
measure:
  aggregate: sum
  column: amount_cents
time_column: order_date
grains: [day, month]
dimensions:
  - name: channel
    column: channel
    values: [web, store]
    description: Where the order was placed. On the order itself, so no join.
  - name: region
    column: region_code
    via: orders_customer
    values: [north, south, west]
    description: The customer region, reached through one declared relationship.
  - name: segment
    column: segment
    via: orders_customer
    description: >
      Declared without a value list, so it can be grouped by and not filtered on.
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 470023
---
Total booked order value, in minor units.

The anchor is the number this metric produced for June 2026 when it was
certified. It is re-executed on every run and at startup: a definition that has
stopped meaning what it claimed fails readiness rather than answering.
