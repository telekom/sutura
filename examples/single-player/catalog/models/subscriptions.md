---
kind: model
name: subscriptions
source: local
table: fct_subscription_monthly
columns:
  - name: month
    type: DATE
    description: The first day of the month this snapshot row describes.
    nullable: false
  - name: subscription_key
    type: BIGINT
    description: The surrogate key daily_usage joins on, through usage_subscription.
    nullable: false
  - name: customer_key
    type: BIGINT
    description: The customer this subscription belongs to, reached through subscription_customer.
    nullable: false
  - name: product_key
    type: BIGINT
    description: The product this subscription was on this month, reached through subscription_product.
    nullable: false
  - name: status
    type: VARCHAR
    description: The subscription's state at the end of the month - active or terminated.
    nullable: false
  - name: mrr_cents
    type: NUMERIC
    description: Recurring revenue for the month, in minor units.
    nullable: false
  - name: churned_in_month
    type: BOOLEAN
    description: Whether this subscription churned during the month, independent of its end-of-month status.
    nullable: false
  - name: contract_term
    type: VARCHAR
    description: The subscription's own contract length - annual or monthly.
    nullable: false
primary_key: [subscription_key, month]
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

`contract_term` is the one attribute here that belongs to the subscription rather than to the
customer or to the product, so it is the one dimension a metric on this model reaches without a
join. Every other dimension in this catalog is declared `via` a relationship, and a catalog
where that was true of all of them would never once compile the plainest case there is: a
group-by key read straight off the fact table, contributing no join of its own. It is
declared on two metrics rather than one so the case survives either of them being rewritten.
