---
kind: metric
name: average_order_value
model: orders
measure:
  ratio:
    numerator: { aggregate: sum, column: amount_cents }
    denominator: { aggregate: count_distinct, column: order_id }
    zero_safe: true
time_column: order_date
grains: [day, month]
---
Booked value per order, in minor units.

The same figure as `average_order` over this catalog, and not the same
definition. `avg(amount_cents)` is the mean of a column: it averages whatever
rows the statement produced. This one averages orders, by dividing a sum by a
distinct count of the order key, and stays per-order regardless of what the rows
did on the way. They agree here because `orders` has one row per order and the
only declared relationship is many-to-one, which cannot fan a row out. That
agreement is a property of this catalog rather than of the two definitions, and
only one of them keeps saying "per order" if the shape of the statement changes.

`zero_safe` is written out because a period with no orders has to mean
something, and both answers are defensible. Here it means null: a day with no
orders is a day with no average, which is a different statement from a day whose
average is zero.

Carries no anchor, for the reason `average_order` does not: a division is not
exact in binary, so pinning one as text would pin a formatting decision rather
than a number.
