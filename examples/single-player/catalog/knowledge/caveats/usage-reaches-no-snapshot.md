---
kind: caveat
name: usage_reaches_no_snapshot
about:
  - { metric: voice_minutes }
---
This one cannot be broken down at all - not by segment, not by region, not by product.

The usage rows are daily and the subscription snapshot is monthly, so a join between them has to
constrain the snapshot month to the usage month or every day of usage is multiplied by the number
of months that subscription existed. `daily_usage_subscription` is that join now - a second key
term truncates the usage date to its month before comparing it to the snapshot's own - and
`data_per_subscription` reaches `contract_term` through it, one hop from `subscriptions`.
`product_family`, `region` and `segment` all sit a further hop away and are still not reachable
from here. This metric declares no dimension on purpose: it is the plainest shape in the
vocabulary, one aggregate over one column with no join at all, and adding one here would be the
second metric losing that unadorned case rather than gaining a real one `data_per_subscription`
does not already cover.

The consequence for a question is concrete: voice minutes by segment is not available. Say so.
Answering with usage overall, or with a segment breakdown of some other metric, is a wrong answer
in the shape of a right one.
