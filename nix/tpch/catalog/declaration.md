---
kind: declaration
definitions:
  - structure
  - descriptions
  - relationships
  - cardinality
  - metrics
  - required_filters
  - grains
  - allowed_values
  - column_types
knowledge: []
---

What this corpus carries. Without this document a local tree declares every kind its format can
carry, and `sutura serve` refuses the bundle at startup because no metric here has an anchor. Every
column is typed, because a model a relationship joins across sources must be; the types are what
`nix/tpch-refusal-rate.py` creates. Widen these lists in the same change that adds a kind.
