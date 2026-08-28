---
kind: glossary
term: business customers
synonyms:
  - B2B
  - Firmenkunden
  - enterprise customers
means: { metric: recurring_revenue, dimension: segment, value: business }
---
The commercial segment a customer sits in, as this catalog spells it: the value is `business`, in
the singular and in lower case.

The spelling is the whole reason the entry exists. A filter is compared against the list of values
the definitions declare, and a question filtering on `B2B` or `Business` is declined with
`DimensionValueNotAllowed` - which names the dimension and, deliberately, does not repeat the value
back. So resolve the phrase here rather than guessing at the wire form.

The `segment` dimension is declared on most metrics in this catalog with the same three values, and
this entry is written against `recurring_revenue` because that is the metric people ask about it.
