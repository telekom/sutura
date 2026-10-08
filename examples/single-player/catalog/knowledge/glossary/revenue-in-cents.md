---
kind: glossary
term: revenue in cents
synonyms:
  - MRR in cents
means: { model: subscriptions, column: mrr_cents }
---
The stored figure every revenue metric here is computed from: one subscription's recurring revenue
for one month, in minor units.

It is named so that a question quoting the stored figure can be traced to `recurring_revenue` and
the averages over it, which are what a request asks for. A request cannot name a column, and a
number read through any of those metrics is in cents - divide by a hundred before quoting it.
