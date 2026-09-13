---
kind: metric
name: orders_total
model: orders
measure:
  simple: { aggregate: sum, column: amount_cents }
required_filters:
  - is_not_null: { column: customer_id }
time_column: ordered_at
grains: [day, month]
dimensions:
  - name: region
    column: region
    via: order_customer
    values: [north, south]
    description: Where the customer is, reached through the one declared relationship.
anchor:
  range: { start: 2026-09-01, end: 2026-09-02 }
  value: 4300
---
Total order value in minor units, over orders whose customer is known.

An ordinary closed-vocabulary metric on the same model as `order_value_spread`, declared so this
catalog carries every kind [`MetadataCapabilities::everything`] commits `sutura-catalog-local` to
supplying: a relationship, a dimension reached through one, a required filter and an anchor, none
of which the authored metric needs on its own. The two metrics share a model and a fact table and
mean different things - this is a sum, `order_value_spread` is a range - which is the point of
naming them separately rather than folding one into the other.

The anchor is what this metric produces for 2026-09-01 against the committed two-row CSV: both
orders clear the filter, and `1200 + 3100 = 4300`.
