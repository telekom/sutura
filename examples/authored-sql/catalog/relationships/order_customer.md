---
kind: relationship
name: order_customer
origin_model: orders
target_model: customers
keys:
  - equal: { origin: customer_id, target: customer_id }
join_type: many_to_one
---
Many orders to one customer. Many-to-one cannot duplicate an order row, so `orders_total` is the
same number whether or not it is grouped by region - which is the property that makes `region` a
dimension a metric on `orders` may reach at all.
