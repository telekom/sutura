---
kind: caveat
name: churn_is_subscription_level
about:
  - { metric: churn_rate }
  - { metric: subscriptions_churned }
---
Churn here is an event on a SUBSCRIPTION inside a month, not a state of a customer.

A customer who cancels one of three contracts appears in both of these figures and has not churned
as a customer. A customer who cancels all three appears three times. Neither number can be turned
into a customer-level churn rate by dividing it by anything this catalog defines, which is why
customer churn is recorded as undefined rather than left for somebody to work out.

The other half of it: neither metric carries a status filter, and that is the definition rather than
an oversight. Churn is the event inside the month, so narrowing to the state a subscription ended
the month in would remove exactly the rows being counted.
