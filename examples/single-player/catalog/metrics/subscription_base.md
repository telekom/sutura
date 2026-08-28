---
kind: metric
name: subscription_base
model: subscriptions
measure:
  simple: { aggregate: count_distinct, column: subscription_key }
time_column: month
grains: [month]
dimensions:
  - name: segment
    column: segment
    via: subscription_customer
    values: [business, consumer, wholesale]
    description: The commercial segment of the customer.
  - name: region
    column: region
    via: subscription_customer
    values: [central, east, north, south, west]
    description: Where the customer is.
  - name: product_family
    column: product_family
    via: subscription_product
    values: [convergent, fixed_internet, mobile, tv]
    description: The kind of product rather than the individual tariff.
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 62
---
How many subscriptions the month held, whatever state they ended it in.

Unfiltered on purpose, and that is the definition rather than an oversight: this is the
denominator a churn share is taken against, and a base that counted only the survivors
would leave the terminated subscriptions in the numerator and out of the denominator at
the same time.

It is `active_subscriptions` with one predicate removed. The two carry separate anchors
for exactly that reason. If a later edit dropped the predicate from the other one, the
two definitions would silently become the same definition, and the anchor is what says
so.
