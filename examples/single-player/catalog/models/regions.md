---
kind: model
name: regions
source: local
table: dim_region
columns: [region, sales_area]
---
One row per region, holding the one attribute a region rolls up into. Nothing here is
additive, so a join to it cannot change a measure.

This model exists for the shape rather than for the attribute. It is the only model in
this catalog that a fact table does not carry a key for: `fct_subscription_monthly` has
no region column at all, so `sales_area` is reachable only by joining `dim_customer`
first and `dim_region` after it. That is a TWO-hop chain, and it is the case a catalog
of single-hop dimensions never renders - hop 2's `ON` clause has to name `dim_customer`,
the table hop 1 arrived at, and a planner that qualified every hop by the fact table
instead produced `fct_subscription_monthly.region`, a column that does not exist. It
failed as a binder error here and would have been a silently different grouping on a
fact table that happened to carry a column of that name.

`region` is the join key and is the same text `dim_customer.region` holds. Five regions,
each in exactly one sales area, so the rollup is a partition: a total by `sales_area`
reconciles with the same total by `region` and with the ungrouped one.
