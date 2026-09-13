---
kind: example
name: orders_total_by_day
asked:
  - what was the total order value on September 1st
  - wie hoch war der Bestellwert am 1. September
question:
  metric: orders_total
  grain: day
  range: { start: 2026-09-01, end: 2026-09-02 }
---
One day, the only day the committed CSV holds. `orders_total` is the closed-vocabulary metric this
catalog also declares; `order_value_spread` is the authored one and is what every adapter this
repository ships refuses to start on, so no worked question here asks for it.
