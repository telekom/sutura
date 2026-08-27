---
kind: glossary
term: subscriber count
synonyms:
  - subscribers
  - active lines
  - Anschluss
  - Vertrag
means: { metric: active_subscriptions }
---
How many subscriptions were active at the end of the month.

A subscriber count is a SUBSCRIPTION count and never a customer count, which is the most expensive
confusion there is in a telco data set: one customer may hold a mobile contract, a fixed line and a
television package, and this metric counts three of them. The caveat on the metric says the same
thing where somebody is about to ask for it.

"Anschluss" and "Vertrag" both arrive in German questions and both mean a subscription here. For
the base that also counts the subscriptions the month lost, ask `subscription_base` instead.
