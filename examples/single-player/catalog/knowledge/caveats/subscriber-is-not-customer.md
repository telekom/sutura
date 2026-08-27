---
kind: caveat
name: subscriber_is_not_customer
about:
  - { metric: active_subscriptions }
  - { metric: subscription_base }
  - { metric: subscription_months_billed }
  - { metric: subscriptions_churned }
---
Every one of these counts SUBSCRIPTIONS. None of them counts customers.

One customer may hold several subscriptions - a mobile contract, a fixed line, a television package
- so a report presenting any of these figures as a customer count overstates the customer base by
however much the average customer holds. There is no metric here that counts customers at all: the
only place a customer count appears is inside the denominator of `revenue_per_customer`, where it is
never reported on its own.

So if somebody asks how many customers there are, the answer is that this deployment does not answer
it - not a subscription count handed over under the other name.
