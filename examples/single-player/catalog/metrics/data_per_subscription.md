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
grains: [day, month]
---
Data volume per subscription, in gigabytes.

A ratio with no definitional filter, which is the other half of what `required_filters`
is for: this metric means what it measures over every row in range, and there is no
predicate a reader has to be warned about.

The denominator counts the subscriptions that appear in the range rather than every
subscription that existed during it. A day with no usage is a day with no row here, so
this is volume per subscription that used the network, not volume per subscriber. The
second number needs the monthly snapshot, and `daily_usage` says why this catalog cannot
yet join to it.

`zero_denominator: yields_null` because a range holding no usage rows at all is a range
with no figure, not a range whose figure is zero.

No anchor: a sum of decimal gigabytes divided by a count is a float, and pinning one as
text pins how a language prints a binary expansion.
