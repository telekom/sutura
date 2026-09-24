---
kind: model
name: daily_usage
source: local
table: fct_usage_daily
columns: [usage_date, subscription_key, data_gb, voice_min]
---
One row per subscription per day on which it used anything.

It carries `subscription_key`, and a join on that column alone matches every month the
subscription existed and multiplies each day of usage by that count against the
subscription snapshot, which has one row per subscription per MONTH. `data_per_subscription`
reaches `product_family` through `daily_usage_subscription`, whose second key term
constrains the snapshot month to the usage month rather than joining on the subscription
key alone - two typed terms, not a wider relationship. Every OTHER dimension here still
comes from grouping by time and by nothing else: the compound join reaches one dimension
one metric declares, not every column of the snapshot.
