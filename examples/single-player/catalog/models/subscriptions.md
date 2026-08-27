---
kind: model
name: subscriptions
source: local
table: fct_subscription_monthly
columns: [month, subscription_key, customer_key, product_key, status, mrr_cents, churned_in_month]
---
One row per subscription per month: what that subscription was worth in the month, and
what state it was in at the end of it.

A snapshot, not a log. `month` is the first day of the month being described, so every
row of a month carries the same date and a day grain would bucket them all onto the
first. That is why no metric on this model declares one: the answer would be arithmetic
that looks like a daily series and is not.

Money is held in minor units so that a total is exact. A metric that summed a decimal
column would depend on how two languages happen to print the same bits, and an anchor
comparison over that is a test that fails for the wrong reason.

`status` and `churned_in_month` describe the same event from two sides and both are
here on purpose. `status` is a state at the end of the month, which is what a revenue
definition narrows on. `churned_in_month` is something that happened inside the month,
which is what a churn count counts. In the general case neither follows from the other:
a subscription that terminates and is restored ends the month in a state that says
nothing about the termination.
