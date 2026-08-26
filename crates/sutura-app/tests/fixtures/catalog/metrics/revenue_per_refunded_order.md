---
kind: metric
name: revenue_per_refunded_order
model: orders
measure:
  ratio:
    numerator: { aggregate: sum, column: amount_cents }
    denominator: { count_if: refunded }
    zero_denominator: fails
time_column: order_date
grains: [day, month]
---
Booked value per refunded order, in minor units.

The one metric in this catalog that chooses `fails`, and it is here to be
executed rather than to be read. `average_order_value` chooses `yields_null`,
so before this document existed nothing in the suite reached the other word: no
golden, no row snapshot and no differential comparison touched it, and the
variant looked covered because the enum had a test for its spelling.

`fails` says an empty denominator is a fault and not a figure, and it is chosen
here because it is true of this definition: a period with no refunds does not
have a smaller revenue-per-refund, it has none, and answering a number for it
would be answering a different question under this name. The generator emits the
division unguarded for that reason, the data system answers a non-finite double,
and `Value::Real` will not hold one - so the question fails, naming the column,
instead of reporting the string `inf` under a certified metric name.

June 2026 has three refunded orders, so it answers a real figure and its rows
are pinned like any other. July 2026 has none while still having orders, which
is the shape that reaches the word: rows exist, so the period is not empty and
there is a group to answer for, and the denominator is nevertheless zero.

Carries no anchor, for the reason `average_order_value` does not: a division is
not exact in binary, so pinning one as text would pin a formatting decision
rather than a number.
