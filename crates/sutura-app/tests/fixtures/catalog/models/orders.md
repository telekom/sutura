---
kind: model
name: orders
source: local
table: orders
columns: [order_id, order_date, customer_id, channel, amount_cents, refunded]
---
One row per order, as booked.

Money is held in minor units so that a total is exact. A metric that summed a
decimal column would depend on how two languages happen to print the same bits,
and an anchor comparison over that is a test that fails for the wrong reason.

`refunded` is a boolean, so it is counted with `count_if` rather than with
`count`: a count of the column counts the `false` rows too, which is a wrong
number that raises no error. It is false for every July order on purpose, which
is what gives `revenue_per_refunded_order` a period where its denominator is
zero.
