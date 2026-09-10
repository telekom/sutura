---
kind: relationship
name: subscription_customer
origin:
  model: subscriptions
  column: customer_key
target:
  model: customers
  column: customer_key
join_type: many_to_one
---
Many subscription-months to one customer.

The cardinality is declared rather than inferred because it decides whether a join may
change a measure. Many-to-one cannot duplicate a snapshot row, so a revenue total is
the same number whether or not it was grouped by region. The reverse direction can
duplicate, and the catalog refuses to reach a dimension through one that may.

That first sentence is a claim about the DATA, and a claim about data has to be checked
somewhere: two `customers` rows sharing one `customer_key` would add a revenue twice on
one data system and be collapsed away on two, so the same question answered two numbers
and neither was refused. The deployment now counts `customer_key` against its distinct
values once, at startup, and a table that contradicts this line is a bundle that does not
start. It is checked on the data systems whose adapter can count and not on the others,
and it is checked at startup rather than continuously - a row inserted afterwards violates
this declaration with nothing noticing until the next boot.
