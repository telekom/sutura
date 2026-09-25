---
kind: model
name: daily_usage
source: local
table: fct_usage_daily
columns: [usage_date, subscription_key, data_gb, voice_min]
---
One row per subscription per day on which it used anything.

It carries `subscription_key` and, since the compound join landed, a relationship
`usage_subscription` reaches from here to the subscription snapshot. The snapshot has one
row per subscription per MONTH, so a join on `subscription_key` alone would match every
month that subscription existed and multiply each day of usage by that count - a wrong
number that raises no error anywhere.

`usage_subscription` is the fix: it pairs `subscription_key` with a key that truncates
each usage day to its month and compares it against the snapshot's `month`, so a day
joins only the snapshot row of the month it falls in. A compound key is the shape that
says this - a relationship with one column on each side cannot, and this is the case
that gap-in-the-catalog documents once existed to describe. `voice_minutes` reaches the
product family through it, and the golden suite pins that grouping does not multiply the
measure.
