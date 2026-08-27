---
kind: example
name: business_revenue_in_the_north
asked:
  - what are business customers in the north worth per month
  - Umsatz der Firmenkunden im Norden
question:
  metric: recurring_revenue
  grain: month
  range: { start: 2026-06-01, end: 2026-07-01 }
  filters:
    - { dimension: segment, value: business }
    - { dimension: region, value: north }
---
Two filters and nothing to group by: this asks for one number rather than a breakdown.

Both values are spelled the way the definitions declare them - `business` rather than `B2B`,
`north` rather than `North`. The glossary entries for those phrases exist for exactly this: a filter
is compared against the declared list, and a near miss is declined without the value being repeated
back for comparison.

A filter does not have to appear in the group-by list. Filtering on `region` while grouping by
nothing is a total for one region; filtering on it while grouping by `segment` would be a total per
segment inside that region.
