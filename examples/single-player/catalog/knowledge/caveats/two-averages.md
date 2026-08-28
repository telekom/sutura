---
kind: caveat
name: two_averages_of_the_same_revenue
about:
  - { metric: mean_subscription_mrr }
  - { metric: revenue_per_customer }
---
These two both sound like "average revenue" and they average different things.

`mean_subscription_mrr` is the mean of the revenue column over subscription-months, so a
subscription that existed for six of the months asked about is weighted six times.
`revenue_per_customer` is total revenue over the count of distinct customers, so a customer holding
three subscriptions counts once.

Neither is wrong and they are not interchangeable. A question that says "average revenue" without
saying average over what is one to clarify before answering, and whichever figure is reported should
be named along with it.
