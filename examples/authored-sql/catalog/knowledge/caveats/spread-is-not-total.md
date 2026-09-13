---
kind: caveat
name: spread_is_not_total
about:
  - { metric: order_value_spread }
  - { metric: orders_total }
---
Both metrics read the same two columns off the same model and mean different things.
`order_value_spread` is `MAX(amount_cents) - MIN(amount_cents)`, the range of what was ordered.
`orders_total` is `SUM(amount_cents)`, the total of what was ordered. Neither approximates the
other, and a question asking for "the spread" should not be answered with the total.
