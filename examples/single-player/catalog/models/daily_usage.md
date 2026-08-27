---
kind: model
name: daily_usage
source: local
table: fct_usage_daily
columns: [usage_date, subscription_key, data_gb, voice_min]
---
One row per subscription per day on which it used anything.

It carries `subscription_key` and no relationship reaches from here to the subscription
snapshot, which looks like an omission and is not. The snapshot has one row per
subscription per MONTH, so a join on `subscription_key` alone matches every month that
subscription existed and multiplies each day of usage by that count. The sum over those
rows is a wrong number that raises no error anywhere.

The join that would be correct also constrains the snapshot month to the usage month,
and a relationship here declares one column on each side. So the relationship is absent
rather than declared wrongly, and every metric on this model groups by time and by
nothing else. Usage per product family is a definition this catalog cannot yet express,
and saying so is better than shipping a number that is quietly several times too large.
