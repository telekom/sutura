---
kind: metric
name: average_order
model: orders
measure:
  simple: { aggregate: avg, column: amount_cents }
time_column: order_date
grains: [month]
---
Mean booked order value, in minor units.

Deliberately carries no anchor. An average is not exact in binary, so pinning one
as text would be pinning a formatting decision rather than a number. The two
metrics that do carry anchors are integer sums, where the comparison is exact.

Declared beside `average_order_value`, which is the same question asked as a
ratio: a sum over a distinct count of the order key. Both are kept, because the
mean of a column and the mean per order are different definitions that happen to
agree over this catalog.
