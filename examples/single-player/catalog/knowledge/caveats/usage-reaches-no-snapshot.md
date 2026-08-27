---
kind: caveat
name: usage_reaches_no_snapshot
about:
  - { metric: data_per_subscription }
  - { metric: voice_minutes }
---
Neither of these can be broken down at all - not by segment, not by region, not by product.

The usage rows are daily and the subscription snapshot is monthly. A join between them has to
constrain the snapshot month to the usage month, or every day of usage is multiplied by the number
of months that subscription existed. A relationship in this catalog declares one column on each
side, so that join cannot be written here at all, and it is therefore absent rather than declared
wrongly and quietly multiplying rows.

The consequence for a question is concrete: usage by segment is not available. Say so. Answering
with usage overall, or with a segment breakdown of some other metric, is a wrong answer in the shape
of a right one.
