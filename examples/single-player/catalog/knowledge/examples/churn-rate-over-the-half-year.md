---
kind: example
name: churn_rate_over_the_half_year
asked:
  - how has the churn rate developed this year
  - wie hat sich die Abwanderungsrate entwickelt
question:
  metric: churn_rate
  grain: month
  range: { start: 2026-01-01, end: 2026-07-01 }
---
A question about a trend is one question covering the whole trend, not six questions covering a
month each.

The grain decides the shape of the answer: `month` over six months returns six rows, one per month.
`churn_rate` declares no other grain, so there is no finer version of this question - a weekly churn
rate is not a narrower cut of the same number, it is a number nobody certified, and asking for one
is declined with `GrainNotSupported`.

Nothing to group by here. Adding `segment` would answer a different and equally reasonable question,
which is worth confirming before choosing between them.
