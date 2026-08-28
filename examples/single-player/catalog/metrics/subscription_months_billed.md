---
kind: metric
name: subscription_months_billed
model: subscriptions
measure:
  simple: { aggregate: count, column: subscription_key }
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
  - name: contract_term
    column: contract_term
    values: [annual, monthly]
    description: >
      Whether the subscription is on a rolling or a committed term. On the snapshot row itself,
      so no relationship is named and no join is planned.
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 62
---
How many subscription-months the period billed.

A count of ROWS and not of subscriptions, which is what the name says and what the aggregate
does. `subscription_base` is the same column under `count_distinct`, and the two are the same
number over this catalog for a reason worth stating rather than relying on: the snapshot holds one
row per subscription per month, this metric declares only the month grain, so inside any one
bucket a subscription key cannot appear twice. They would stop agreeing the moment either half of
that changed - a quarter grain, or a model that split a month into two rows - and only one of them
would still mean what its name says.

So the pair is the same contrast `active_subscriptions` and `subscription_base` draw with a
predicate, drawn here with an aggregate instead: two documents, one differing word, two anchors
over the same month. If a later edit swapped this one to `count_distinct` the two definitions
would silently become the same definition, and there would be nothing but the name to say so.

**It is also the only plain `count` in this repository**, now that the e-commerce catalog the golden
suite used to carry has been retired in favour of this one. Everything else counting
things here counts them distinctly, or counts a condition; a `count` that no document writes is a
generator arm nothing renders. `subscriptions_churned` says the other half of that: a count of a
BOOLEAN column counts the false rows too, so `count` is right over a key and wrong over a flag,
and the two documents are the two sides of the same mistake.

`contract_term` is declared without `via`, which makes this one of the two metrics here that
reaches a group-by key that contributes no join - the plainest key the compiler resolves, and the
one a catalog of exclusively joined dimensions would never once compile. `recurring_revenue`
declares it too, so the case survives either metric being rewritten.

No definitional filter, on purpose. A subscription-month was billed whatever state the
subscription ended the month in, and a terminated one was billed for the month it terminated in -
which is the same reason `subscription_base` carries no filter, and the reason both of them can
serve as a denominator.
