---
kind: example
name: revenue_by_segment_in_june
asked:
  - how much recurring revenue did each segment bring in June
  - wie viel wiederkehrender Umsatz kam im Juni pro Segment
question:
  metric: recurring_revenue
  grain: month
  range: { start: 2026-06-01, end: 2026-07-01 }
  dimensions: [segment]
---
The plainest shape there is: one metric, one period with both ends given, one dimension to break it
down by.

Note what the period is. June 2026 is `2026-06-01` up to but not including `2026-07-01`, because a
period here is half-open - the end is the first day that is NOT counted. A range of `2026-06-01` to
`2026-06-30` quietly leaves the last day of the month out, and nothing about the answer would say
so.

The result is one row per segment, in minor units, and the caveat on the metric is the reason that
matters before quoting it.
