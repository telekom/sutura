---
kind: metric
name: active_subscriptions
model: subscriptions
measure:
  simple: { aggregate: count_distinct, column: subscription_key }
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
  value: 58
---
How many subscriptions were active at the end of the month.

Counts the subscription key distinctly rather than counting rows. Over this catalog the
two agree, because both relationships are many-to-one and neither can fan a row out; they
would stop agreeing the moment one could, and the one that keeps meaning "subscriptions"
is the key.

Read it beside `subscription_base`, which is this same count with the status filter taken
away. The two carry different anchors over the same rows in the same month, and the only
thing between them is one definitional predicate. That is what `required_filters` buys,
shown as two numbers rather than asserted in a sentence.
