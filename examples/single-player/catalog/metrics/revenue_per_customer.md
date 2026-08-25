---
kind: metric
name: revenue_per_customer
model: subscriptions
measure:
  ratio:
    numerator: { aggregate: sum, column: mrr_cents }
    denominator: { aggregate: count_distinct, column: customer_key }
    zero_safe: true
required_filters:
  - equals: { column: status, value: active }
time_column: month
grains: [month]
dimensions:
  - name: segment
    column: segment
    via: subscription_customer
    values: [business, consumer, wholesale]
    description: The commercial segment of the customer.
---
Recurring revenue per customer, in minor units, over active subscriptions.

Per CUSTOMER and not per subscription, which is the whole reason this is a ratio rather
than an average. `avg(mrr_cents)` is the mean of a column: it averages whatever rows the
statement produced, and those rows are subscription-months. A customer holding three
subscriptions is one customer and three rows, so the two answers differ by however much
the base fans out, and only one of them is the number people mean by revenue per
customer.

`zero_safe` is written out because a month with no active subscriptions has to mean
something and both answers are defensible. Here it means null: a month with no customers
is a month with no revenue per customer, which is a different statement from a month
whose figure is zero.

Carries no anchor. A division is not exact in binary, so pinning one as text would pin a
formatting decision rather than a number, and the sum it is built from is already
anchored under `recurring_revenue`, which is where a drift would show up first.
