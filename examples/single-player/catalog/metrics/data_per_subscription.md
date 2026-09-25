---
kind: metric
name: data_per_subscription
model: daily_usage
measure:
  ratio:
    numerator: { aggregate: sum, column: data_gb }
    denominator: { aggregate: count_distinct, column: subscription_key }
    zero_denominator: yields_null
time_column: usage_date
grains: [day, week, month]
dimensions:
  - name: contract_term
    column: contract_term
    via: daily_usage_subscription
    values: [annual, monthly]
    description: >
      Whether the subscription was on a rolling or a committed term that month, reached
      through the compound join to the snapshot - the one hop it takes, and no further.
audience: open
---
Data volume per subscription, in gigabytes.

A ratio with no definitional filter, which is the other half of what `required_filters`
is for: this metric means what it measures over every row in range, and there is no
predicate a reader has to be warned about.

The denominator counts the subscriptions that appear in the range rather than every
subscription that existed during it. A day with no usage is a day with no row here, so
this is volume per subscription that used the network, not volume per subscriber.

`contract_term` is reached through the one relationship this model declares:
`daily_usage_subscription` constrains the snapshot month to the usage month before
joining, so a subscription that changed term mid-year attributes each day's usage to
the term it held that month rather than to whatever it holds today. Every OTHER
attribute the snapshot carries - `product_family`, `region`, `segment` - needs a second
hop this catalog does not yet declare from here, so grouping by one of those is still
not available; `daily_usage` says why the join itself, not the dimension, was the
missing piece.

`zero_denominator: yields_null` because a range holding no usage rows at all is a range
with no figure, not a range whose figure is zero.

No anchor: a sum of decimal gigabytes divided by a count is a float, and pinning one as
text pins how a language prints a binary expansion.
