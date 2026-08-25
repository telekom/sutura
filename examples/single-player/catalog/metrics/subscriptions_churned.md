---
kind: metric
name: subscriptions_churned
model: subscriptions
measure:
  count_if: { column: churned_in_month }
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
  value: 3
---
How many subscriptions terminated inside the month.

`count_if` rather than a count of the column, and the difference is a wrong number that
raises no error: `count(churned_in_month)` counts the rows where the column is not null,
which is every row, so it would report the whole base as churn. How the condition is
spelled differs between data systems, which is the problem of the generator and exactly
the kind of thing that should not be in a catalog document.

No status filter, because churn is an event inside the month rather than a state at the
end of it. Narrowing on the surviving state would remove the rows being counted.

There is no churn RATE here, and its absence is a limit of the vocabulary rather than a
decision about the business. A rate is a conditional count divided by a distinct count,
and each half of a ratio is one aggregate over one column: `count_if` is a shape a
measure can have, not a term a ratio can hold. So both halves ship as metrics, this one
and `subscription_base`, and the division belongs to whoever asked for it. Two certified
numbers and one visible division beats one number nobody can reproduce.
