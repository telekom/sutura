---
kind: metric
name: subscriptions_churned
model: subscriptions
measure:
  simple: { count_if: churned_in_month }
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

This is the numerator of a rate and not the rate itself, which is a decision about the
business rather than a limit of the vocabulary. A count of terminations is the figure
somebody reconciles against a churn report; the share is `churn_rate`, declared beside it
over the same numerator and `subscription_base`'s denominator. Both are certified, and the
one that answers "how many" is not the one that answers "what fraction".

It used to be a limit of the vocabulary, and the note that said so is worth keeping as a
record of what changed. `count_if` was a measure *shape*, a sibling of `ratio` rather than
a term inside one, so a rate over a conditional count had every ingredient present and
nowhere to write it. The fix was to make the extensible axis the term instead of the shape:
`simple` and `ratio` are the two shapes, `aggregate` and `count_if` are the two terms, and
either term is usable in either half of either shape.
