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
  - name: product_family
    column: product_family
    via: [daily_usage_subscription, subscription_product]
    values: [convergent, fixed_internet, mobile, tv]
    description: >
      The kind of product the subscription was on that month, reached through the
      compound join to the snapshot and then to the product dimension.
audience: open
---
Data volume per subscription, in gigabytes.

A ratio with no definitional filter, which is the other half of what `required_filters`
is for: this metric means what it measures over every row in range, and there is no
predicate a reader has to be warned about.

The denominator counts the subscriptions that appear in the range rather than every
subscription that existed during it. A day with no usage is a day with no row here, so
this is volume per subscription that used the network, not volume per subscriber.

`product_family` is reached through a chain of two relationships: `daily_usage_subscription`
constrains the snapshot month to the usage month before joining, so a subscription that
changed product mid-year attributes each day's usage to the product it was on that
month rather than to whatever it is on today. Grouping by anything else the snapshot
carries - `region`, `segment` - is the same shape, one dimension declared at a time;
`daily_usage` says why the join itself was the missing piece rather than the dimension.

`zero_denominator: yields_null` because a range holding no usage rows at all is a range
with no figure, not a range whose figure is zero.

No anchor: a sum of decimal gigabytes divided by a count is a float, and pinning one as
text pins how a language prints a binary expansion.
