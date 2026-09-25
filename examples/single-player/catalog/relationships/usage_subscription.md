---
kind: relationship
name: usage_subscription
origin:
  model: daily_usage
  column: subscription_key
target:
  model: subscriptions
  column: subscription_key
keys:
  - origin: subscription_key
    target: subscription_key
  - origin: usage_date
    grain: month
    target: month
join_type: many_to_one
---
Usage joined to the monthly snapshot, correctly.

On `subscription_key` alone this join multiplies each usage row by every month that
subscription existed, so a sum over it is a wrong number that raises no error
anywhere. The second key fixes that: it truncates each usage day to its month and
compares it against the snapshot's `month` column, so a day joins only the snapshot row
of the month it falls in. A compound key is the shape that expresses this - `daily_usage`
declares why a one-column join would be the absent-one here, and this relationship is
that gap closed.

The snapshot is `many_to_one` from a day: one usage day's subscription key and month
address at most one snapshot row, because the snapshot holds one row per subscription
per month.
