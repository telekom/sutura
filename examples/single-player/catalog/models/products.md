---
kind: model
name: products
source: local
table: dim_product
columns:
  - name: product_key
    type: BIGINT
    description: The surrogate key subscriptions join on.
    nullable: false
  - name: product_name
    type: VARCHAR
    description: The tariff's own name, for example "Mobile L Unlimited".
    nullable: false
  - name: product_family
    type: VARCHAR
    description: Which of the four product families this tariff belongs to - mobile, fixed_internet, tv or convergent.
    nullable: false
---
The tariff catalog: one row per sellable product.

There is no price column, and the absence is the point. A list price is not what a
subscription was billed, so a revenue metric that summed it would answer a question
about the price list under the name of a question about revenue. The billed figure sits
on the monthly snapshot, where whatever made it differ from the list price has already
been applied.
