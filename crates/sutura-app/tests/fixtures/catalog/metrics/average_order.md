---
kind: metric
name: average_order
model: orders
measure:
  aggregate: avg
  column: amount_cents
time_column: order_date
grains: [month]
---
Mean booked order value, in minor units.

Deliberately carries no anchor. An average is not exact in binary, so pinning one
as text would be pinning a formatting decision rather than a number. The two
metrics that do carry anchors are integer sums, where the comparison is exact.
