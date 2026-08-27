---
kind: model
name: products
source: local
table: dim_product
columns: [product_key, product_name, product_family]
---
The tariff catalog: one row per sellable product.

There is no price column, and the absence is the point. A list price is not what a
subscription was billed, so a revenue metric that summed it would answer a question
about the price list under the name of a question about revenue. The billed figure sits
on the monthly snapshot, where whatever made it differ from the list price has already
been applied.
