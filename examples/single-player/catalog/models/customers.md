---
kind: model
name: customers
source: local
table: dim_customer
columns: [customer_key, customer_id, segment, region]
---
One row per customer, holding the attributes an answer may be grouped by and nothing
else. Nothing here is additive, so a join to it cannot change a measure.

`customer_key` is what every fact carries and `customer_id` is the number a person
would quote. No metric groups by the business key: a result with one row per customer
is a list of customers rather than a measure of anything, and `segment` and `region`
are the two attributes that put a number in context instead of replacing it.
