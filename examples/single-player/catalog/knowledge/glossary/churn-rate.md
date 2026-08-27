---
kind: glossary
term: churn rate
synonyms:
  - Abwanderungsrate
  - cancellation rate
  - attrition rate
means: { metric: churn_rate }
---
The share of a month's subscriptions that terminated inside it.

Subscription-level, always. The denominator is every subscription the month held and the numerator
is the ones that ended, so a customer who cancels one of three contracts moves this number by one
subscription and not by one customer. Customer-level churn is a different figure and this catalog
does not define it - the note recording that is what to say when somebody asks.

If what somebody wants is the COUNT of terminations rather than the share, that is
`subscriptions_churned`, which is this metric's own numerator.
