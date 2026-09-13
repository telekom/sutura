# Authored SQL

The smallest catalog that uses the named escape hatch of
[A named escape hatch for authored SQL](../../docs/adr/0004-a-named-escape-hatch-for-authored-sql.md)
and still composes: two models, one relationship between them, one metric whose computation is
SQL an author wrote, one ordinary closed-vocabulary metric on the same model, the four knowledge
documents every kind needs, and the CSVs the two models describe.

**Why it is not one model and one metric.** `LocalCatalog::capabilities()` declares
`MetadataCapabilities::everything()` - every definition kind and every knowledge capability this
adapter format can carry, unconditionally (`crates/sutura-catalog-local/src/lib.rs`). Composing this
directory into a servable bundle runs `sutura_app::assemble::assemble` before it ever reaches the
authored-SQL check, and that function refuses a contributor whose content does not produce every
kind it declares - a one-model, one-metric catalog fails there as
`the catalog contributions do not compose`, naming the first undeclared kind, before the authored
metric is ever looked at. So the directory carries a relationship (`order_customer`), a dimension
reached through it (`region`, with an allowed-value list), a required filter and an anchor (all on
`orders_total`, the closed-vocabulary metric) and one document under each of `knowledge/glossary`,
`knowledge/caveats`, `knowledge/not-defined` and `knowledge/examples` - the minimum this format's
reference adapter needs to compose at all, kept off the authored metric itself so
`order_value_spread` stays exactly what the escape hatch decision describes.

**What loading it proves, and what it does not.** `order_value_spread`'s document writes
`authored_sql:` and no `measure:`; writing both, or neither, is a load failure. The fragment is
admitted as text - present, bounded, one fragment rather than a script, free of control and
invisible characters - and pinned under the definition digest exactly as written, so editing the SQL
under the certified name moves the digest. **Nothing published compiles it.** No code in a shipped
binary parses the fragment, checks it against the model's columns, or renders it for a dialect; the
local catalog may not even link the crate that could (`cargo xtask check-boundaries` forbids the
edge). So `MAX(nonexistent)` loads here exactly as `MAX(amount_cents)` does.

**What starting on it proves.** Every execution adapter this repository ships leaves
`Warehouse::EXECUTES_AUTHORED_SQL` at its `false` default, and `sutura_app::verify_and_validate` -
the only door to a servable bundle - refuses one that carries an authored metric, by placement
ahead of every anchor check in its body. Composition has to succeed first, which is why this
directory is no longer the one-model catalog an earlier version of this file described: pointed at
a one-model catalog, `sutura query` refuses composition itself, naming an undeclared kind, and never
reaches the authored check at all - that was this file's own defect, held now by a test rather than
by a reading. Pointed at this directory, `sutura query examples/authored-sql/catalog <a question>
examples/authored-sql/data` composes and then refuses with, verbatim:

```text
sutura: the pinned bundle is not fit to serve
  caused by: metric order_value_spread uses authored SQL, and the selected execution adapter cannot execute it
this bundle is not fit to serve
```

exit `1`. The compile, and an answer, belong to the first adapter that declares the constant.

The tests: `the_authored_sql_example_loads_and_its_fragment_is_under_the_digest` in
`crates/sutura-catalog-local` reaches this directory and pins the authored fragment's digest, over
just the model and the metric that carry it - it does not compose the whole bundle, so it says
nothing about the other content here. `the_authored_sql_example_does_not_boot_against_the_in_process_engine`
in `crates/sutura-app` drives this whole directory through `LocalService::start` over the one engine
every shipped binary links, and asserts the exact refusal above - the mechanism the two paragraphs
before it describe, rather than a reading of the source.
