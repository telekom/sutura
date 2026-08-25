---
kind: metric
name: voice_minutes
model: daily_usage
measure:
  simple: { aggregate: sum, column: voice_min }
time_column: usage_date
grains: [day, month]
---
Outgoing voice minutes.

Here so the plainest shape in the vocabulary appears in the example unadorned: one
aggregate over one column, no definitional filter, no join, no ratio. Most certified
metrics look like this, and a catalog whose every entry needed a paragraph of
justification would be a catalog nobody trusted.

Same grains as `data_per_subscription`, and the same reason for carrying no anchor: a sum
of decimals is a float, so an anchor written as text would pin a formatting decision
rather than a number.
