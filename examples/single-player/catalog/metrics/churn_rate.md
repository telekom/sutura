---
kind: metric
name: churn_rate
model: subscriptions
measure:
  ratio:
    numerator: { count_if: churned_in_month }
    denominator: { aggregate: count_distinct, column: subscription_key }
    zero_denominator: yields_null
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
  value: "0.04918032786885246"
---
What share of the month's subscriptions terminated inside it.

The metric this vocabulary could not express, and the reason it was changed. A rate is a
conditional count over a distinct count, and while `count_if` was a measure *shape* it
could not be a *half* of a ratio: every ingredient was present and there was nowhere to
write it. Read `subscriptions_churned.md` for what that cost, and this frontmatter for what
it looks like now - the numerator is a term like any other.

The two halves are the two metrics that used to ship in its place. The numerator is
`subscriptions_churned`, a conditional count and not `count(churned_in_month)`, which counts
non-null rows and would report the whole base as churn. The denominator is
`subscription_base`, unfiltered, because a base that counted only the survivors would leave
the terminated subscriptions in the numerator and out of the denominator at once.

No status filter, for the reason `subscriptions_churned` gives: churn is an event inside the
month rather than a state at the end of it, and narrowing on the surviving state would remove
the rows being counted.

`zero_denominator: yields_null` because a month with no subscriptions at all has no churn
share. Zero would say every subscription survived, which is a claim about a month that had
none.

## Why this one carries an anchor when the other ratios do not

An anchor is compared as rendered TEXT, at the metric's coarsest declared grain. That is what
makes a float-valued metric hard to anchor, and it is why the other two ratios here carry
none: a decimal that had to be converted to binary, or summed in an order nobody specified,
pins how a language prints an expansion rather than pinning a number.

Neither of those reaches this metric, and the reason is arithmetic rather than nerve. Both
halves are counts, so both are exact integers in a double whatever order they were summed in,
and the whole measure is therefore one division of two exactly-represented values - a single
rounding, with nothing before it that an evaluation order could perturb. `data_per_subscription`
is the case that is genuinely different: a sum of decimal gigabytes over a count, where two
engines may legitimately differ in the last place.

Both halves are also separately certified, which is what makes the anchor readable rather than
opaque: 3 under `subscriptions_churned` and 61 under `subscription_base`, over this same month.
A reader checks `0.04918032786885246` against those two, and a run that produced different
digits would have done different arithmetic rather than different formatting.

And it is the only anchor in this repository that re-executes a conditional count inside a
ratio, which is the shape the vocabulary was changed for. A shape nothing certifies is a shape
that merely compiles.
