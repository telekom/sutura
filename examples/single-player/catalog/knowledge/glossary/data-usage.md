---
kind: glossary
term: data usage
synonyms:
  - data volume
  - Datenvolumen
  - gigabytes per subscription
means: { metric: data_per_subscription }
---
Data volume per subscription, in gigabytes, over the period asked about.

Per subscription that USED the network rather than per subscription that existed: the denominator
counts the subscriptions appearing in the usage rows, and a subscription with no usage on a day has
no row for that day.

It cannot be broken down by segment, region or product family, and that is a fact about the models
rather than an omission - the caveat on the metric says why. A question asking for data usage by
segment is answered by saying it is not available here, never by substituting a breakdown of
something else.
