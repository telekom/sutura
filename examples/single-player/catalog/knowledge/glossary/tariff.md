---
kind: glossary
term: tariff
synonyms:
  - Tarif
  - plan
  - product name
means: { metric: recurring_revenue, dimension: product_name }
---
The individual product somebody is subscribed to, by its own name rather than by its kind.

It can be grouped by and NOT filtered on: the definitions declare no list of values for it, because
the list of tariffs moves faster than a reviewed catalog does. A question that filters on a tariff
is declined with `DimensionNotFilterable`, and the remedy the refusal gives is the right one - group
by it, and read the row you wanted out of the result.

For the kind of product rather than the individual tariff, group by `product_family`, which does
declare its values and can be filtered.
