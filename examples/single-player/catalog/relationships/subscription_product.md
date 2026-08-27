---
kind: relationship
name: subscription_product
origin:
  model: subscriptions
  column: product_key
target:
  model: products
  column: product_key
join_type: many_to_one
---
Many subscription-months to one product.

A snapshot row names the product the subscription was on in that month, so a tariff
change appears as a different `product_key` in a later row rather than as a rewritten
history. Revenue by product family is therefore revenue as it was booked, not revenue
reattributed to whatever the customer is on today.
