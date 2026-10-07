---
kind: glossary
term: subscription snapshot
synonyms:
  - monthly snapshot
means: { model: subscriptions }
---
The model most metrics here are measured over: one row per subscription per month, saying what
the subscription was worth in that month and what state it ended the month in.

It is named so that a question about "the snapshot" can be traced to the metrics built on it. It is
not something a request can ask for - a request names a metric, and each metric over this model
says which of its rows it counts.
