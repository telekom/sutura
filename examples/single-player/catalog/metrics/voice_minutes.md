---
kind: metric
name: voice_minutes
model: daily_usage
measure:
  simple: { aggregate: sum, column: voice_min }
time_column: usage_date
grains: [day, month]
audience: open
dimensions:
  - name: product_family
    column: product_family
    via: [usage_subscription, subscription_product]
    values: [convergent, fixed_internet, mobile, tv]
    description: >
      The kind of product the subscription that used the minutes belongs to. Reached through
      `usage_subscription` - the compound join from a usage day to the monthly snapshot -
      and then on to the product. Grouping by it does not multiply the minutes, because the
      compound key stops every usage day from joining every month that subscription existed.
---
Outgoing voice minutes.

Here so the plainest shape in the vocabulary appears in the example unadorned: one
aggregate over one column, no definitional filter, no join, no ratio. Most certified
metrics look like this, and a catalog whose every entry needed a paragraph of
justification would be a catalog nobody trusted.

The product family dimension is the one exception added when `usage_subscription`
landed: it is the dimensionless metric reaching a dimension through the compound join,
which is the case `daily_usage`'s prose calls the gap a one-column relationship cannot
close. Grouping by it must not change the measure - that is what the compound key is
for, and the golden suite pins it.

Same grains as `data_per_subscription`, and the same reason for carrying no anchor: a sum
of decimals is a float, so an anchor written as text would pin a formatting decision
rather than a number.
