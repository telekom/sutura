# Authored SQL

The smallest catalog that uses the named escape hatch of
[A named escape hatch for authored SQL](../../docs/adr/0004-a-named-escape-hatch-for-authored-sql.md):
one model, one metric whose computation is SQL an author wrote, and the two-row CSV the model
describes.

**What loading it proves, and what it does not.** The metric document writes `authored_sql:` and no
`measure:`; writing both, or neither, is a load failure. The fragment is admitted as text - present,
bounded, one fragment rather than a script, free of control and invisible characters - and pinned
under the definition digest exactly as written, so editing the SQL under the certified name moves the
digest. **Nothing published compiles it.** No code in a shipped binary parses the fragment, checks it
against the model's columns, or renders it for a dialect; the local catalog may not even link the
crate that could (`cargo xtask check-boundaries` forbids the edge). So `MAX(nonexistent)` loads here
exactly as `MAX(amount_cents)` does.

**What starting on it proves.** Every execution adapter this repository ships leaves
`Warehouse::EXECUTES_AUTHORED_SQL` at its `false` default, and the only door to a servable bundle
refuses one that carries an authored metric before any anchor runs: the failure names
`order_value_spread` and reads `uses authored SQL, and the selected execution adapter cannot execute
it`. Pointing the binary at this directory shows that refusal and nothing else. The compile, and an
answer, belong to the first adapter that declares the constant.

The tests: `the_authored_sql_example_loads_and_its_fragment_is_under_the_digest` in
`crates/sutura-catalog-local` reaches this directory;
`an_authored_metric_does_not_boot_against_the_in_process_engine` in `crates/sutura-app` holds the
startup refusal against the one engine every shipped binary links.
