---
kind: relationship
name: daily_usage_subscription
origin_model: daily_usage
target_model: subscriptions
keys:
  - equal: { origin: subscription_key, target: subscription_key }
  - truncated_equal: { origin: usage_date, grain: month, target: month }
join_type: many_to_one
---
Many daily usage rows to one subscription-month snapshot.

The snapshot has one row per subscription per MONTH, and a daily usage row names a day,
so a key on `subscription_key` alone would match every month that subscription existed -
`subscription_key = subscription_key AND month = month_of(usage_date)` is the join that
does not. Two typed terms, not a condition string: an ordinary equality on the
subscription key, and the usage date truncated to the month before the second equality,
using the same per-dialect truncation a metric's own time bucket renders.

Many-to-one, and what that promises is now about the PAIR: `(subscription_key, month)`
identifies at most one row of `subscriptions`, not `subscription_key` alone and not
`month` alone - a customer can hold several subscriptions in one month, and one
subscription spans many months. The deployment counts the pair against its distinct
combinations at startup, the same way it counts a single-column key, and a table that
contradicts this declaration is a bundle that does not start.
