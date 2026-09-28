---
kind: model
name: customers
source: local
table: dim_customer
columns:
  - name: customer_key
    type: BIGINT
    description: The surrogate key every fact table joins on.
    nullable: false
  - name: customer_id
    type: VARCHAR
    description: The business key a person would quote, for example C0001.
    nullable: false
  - name: segment
    type: VARCHAR
    description: The commercial segment of the customer - business, consumer or wholesale.
    nullable: false
  - name: region
    type: VARCHAR
    description: Which of the five regions this customer is in.
    nullable: false
---
One row per customer, holding the attributes an answer may be grouped by and nothing
else. Nothing here is additive, so a join to it cannot change a measure.

`customer_key` is what every fact carries and `customer_id` is the number a person
would quote. No metric groups by the business key: a result with one row per customer
is a list of customers rather than a measure of anything, and `segment` and `region`
are the two attributes that put a number in context instead of replacing it.
