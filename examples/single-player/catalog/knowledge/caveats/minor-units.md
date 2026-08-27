---
kind: caveat
name: revenue_is_in_minor_units
about:
  - { metric: recurring_revenue }
  - { metric: mean_subscription_mrr }
  - { metric: revenue_per_customer }
  - { metric: revenue_per_churned_subscription }
---
Every revenue figure in this catalog is in MINOR UNITS - cents, not currency.

A June figure of 202121 for `recurring_revenue` is 2021.21. The underlying column is stored that way
so that a sum is exact integer arithmetic rather than a decimal somebody has to decide where to
round, and no question ever names that column, so this note is the only place a caller learns it.

Divide by a hundred before quoting a figure to a person, and say which unit you are quoting. A
revenue number a hundred times too large is the kind of wrong answer nobody thinks to check.
