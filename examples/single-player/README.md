# Single player

A complete catalog over a small synthetic telco warehouse, and the shortest path from a
clone to an answered question.

**This page is the corpus's reference: what is in the directory, why each document is drawn the way
it is, and what a test holds.** The guided path - install it, ask one question, read the provenance,
be refused, write your own metric - is [getting started](../../docs/getting-started.md).

`crates/sutura-cli/tests/documented.rs` runs every `sutura catalog`, `describe`, `compile` and
`query` invocation on either page, and holds every refusal block and `-- definitions ` line they
print as output against the output of the command in the fence above it. **The one command it does
not run is `sutura mcp` below**, which speaks a protocol on its own pipes; `just mcp-e2e` drives
that one with a client that answers. That is the whole of the exception, and it is not an honour
rule: a page printing any other subcommand of this binary fails that suite by name rather than
being skipped, and the subcommand list it classifies against is read out of the binary itself.
`just documented` runs it. What it does **not** hold is any output line that is neither a refusal
block nor a provenance stamp - a listing's rows and a compiled statement are pinned by
`crates/sutura-cli/tests/example.rs`'s snapshots instead.

```bash
cargo run -p sutura-cli -- \
  query examples/single-player/catalog \
        examples/single-player/questions/recurring-revenue-by-month.yaml \
        examples/single-player/data
```

```
-- definitions local-working-tree 8042ba92eaddce5e96e055cc43a635a64161d54ec21bd4f3367e7a1f58f5b4c5
period  recurring_revenue
2026-01-01      237320
2026-02-01      232822
2026-03-01      216700
2026-04-01      206160
2026-05-01      202994
2026-06-01      202121
```

Four things happened before that table appeared. The catalog was read and hashed, so the
digest on the first line names exactly the definitions the number came from. Every metric
that declares a certified number re-executed and reproduced it. The question was resolved
against those definitions and compiled to one statement, with every value bound as a
parameter. Only then did anything run. If the second step had failed, there would be no
table: a bundle whose anchors do not hold is not fit to serve, and saying so is the whole
point.

No feature flag and no database. The engine reads these CSVs directly, so `query` works in a plain
`cargo run` - and `compile` renders the statement for any dialect without reading data at all.

## What is here

```
catalog/models/*.md                 what tables exist and which columns may be read
catalog/relationships/*.md          which joins are allowed, and at what cardinality
catalog/metrics/*.md                what each certified number means
catalog/knowledge/glossary/*.md     the words a question may arrive in, and the one thing each means
catalog/knowledge/caveats/*.md      what to know before trusting a number, per metric
catalog/knowledge/not-defined/*.md  terms deliberately left undefined, and what to say instead
catalog/knowledge/examples/*.md     worked questions: how somebody asked, and what to send
data/*.csv                          one file per model, named after the table
questions/*.yaml                    the corpus, including the ones that are refused
```

The first three decide what executes. The four under `knowledge/` decide what a person - or an
agent - understands about it, and they are read by `sutura prompt` and by nothing on the query
path: a glossary that could select what runs would not be descriptive content. Both halves are
under the same digest, because a glossary decides which metric a question is about.

**The directory names are for whoever is reading the tree.** Every document declares its own
`kind:` in its frontmatter, and the loader walks one tree and dispatches on that - so a file in
the "wrong" directory loads exactly the same, and the layout is not part of the format. That is
also the part of this catalog a metadata service could reproduce: a document kind is a concept, not
a path.

A catalog document is YAML frontmatter and a prose body, and the prose is part of the
format rather than a comment. It travels with the definition and comes back out of
`sutura describe`, so it is where a definition says what it means and why it is drawn that
way. Read `catalog/metrics/churn_rate.md` first, and `subscriptions_churned.md` beside it:
between them they are the record of a metric this vocabulary could not express, and of the
change that made it sayable.

Three more commands, in the order a reader usually wants them:

```bash
E=examples/single-player
cargo run -p sutura-cli -- catalog  $E/catalog
cargo run -p sutura-cli -- describe $E/catalog recurring_revenue
cargo run -p sutura-cli -- compile  $E/catalog $E/questions/recurring-revenue-by-region-and-family.yaml
```

`compile` needs no data at all. It prints the statement, the parameters and the plan, which
is the useful thing to look at when the question is what sutura decided rather than what
the answer was.

## The measure vocabulary, in one catalog

The vocabulary has two levels, and the second one is the extensible one. A **term** is what
one number is computed from - an `aggregate` over a column, or a `count_if` over a boolean
column. A **shape** says how terms combine: `simple` is one term, `ratio` is one divided by
another. Either term is usable in either half of either shape. Beside that, a metric may carry
filters that are part of its definition. There is no field anywhere that takes a SQL
expression, and every combination below is here:

| Metric | Measure | Why it is written that way |
| --- | --- | --- |
| `voice_minutes` | `simple` + `aggregate` | One aggregate over one column. Most metrics look like this |
| `recurring_revenue` | `simple` + `aggregate` + `required_filters` | The active-only predicate is part of the name, and a caller can neither see it nor remove it |
| `mean_subscription_mrr` | `simple` + `avg` + `required_filters` | The mean of a COLUMN. The only `avg` here, and the thing `revenue_per_customer` is not |
| `subscription_months_billed` | `simple` + plain `count` | The only plain `count` here. Everything else counting things counts them distinctly, or counts a condition |
| `subscriptions_churned` | `simple` + `count_if` | A count of a boolean column would count the `false` rows too |
| `revenue_per_customer` | `ratio` of two aggregates | A sum over a distinct count of customers. Not the mean of a column, and computing it as one is a different number |
| `churn_rate` | `ratio` with a `count_if` numerator | A conditional count over a distinct count. This is the one that needed the two levels |
| `revenue_per_churned_subscription` | `ratio` with a `count_if` DENOMINATOR, and `zero_denominator: fails` | The mirror of `churn_rate`, and the only document here that says an empty denominator is a fault rather than a figure |

Between them those eight write every shape, every term, both meanings of a zero denominator and
four of the six aggregates, and `crates/sutura-cli/tests/example.rs` asserts each of those sets as
an equality rather than as a lower bound - so dropping a case is a failing test and adding one is
a line in a diff. `min` and `max` are the two that are absent, because a metric using either would
have to mean something first.

`churn_rate` is worth opening for the second reason as well: the catalog could not express it
until the vocabulary stopped treating a conditional count as a whole measure, and both files
say so - one as the metric, the other as the record of what it cost.

The pair worth reading together is `active_subscriptions` and `subscription_base`. They are
the same aggregate over the same rows in the same month; one carries `status = active` as a
definitional filter and the other does not. For June 2026 their certified numbers are 59
and 62, and `subscriptions_churned` reports 3 for the same month. One predicate, three
numbers that add up, and no caller can reach any of them. `churn_rate` is the fourth: 3 over
62, certified as `0.04838709677419355`.

`subscription_months_billed` is the pair drawn a second way, with an aggregate instead of a
predicate: it is `subscription_base`'s column under a plain `count`, and 62 for the same month
because this snapshot holds one row per subscription per month and the metric declares only the
month grain. The two would part company the moment either of those changed, which is why they are
two documents and two anchors rather than one.

## The dimension that needs no join

Every dimension in this catalog is reached `via` a declared relationship except one.
`contract_term` - `monthly` or `annual` - sits on the snapshot row itself, so it names no
relationship and contributes no join: the compiler reads the group-by key straight off the fact
table. It is declared on `recurring_revenue` and on `subscription_months_billed`, so the case
survives either of them being rewritten, and
`questions/recurring-revenue-by-term-and-family.yaml` is the mixed plan: one local key and one
reached through `subscription_product`, in the same statement. Every join in that statement is
there because of the *other* key, which is the readable form of the claim.

One customer key in `data/fct_subscription_monthly.csv` matches no row in `data/dim_customer.csv`,
also on purpose. The join is a LEFT join because a fact must not vanish for want of a dimension
row, so that subscription appears under a `null` group rather than dropping out of the total -
which is what makes `questions/recurring-revenue-june.yaml` and
`questions/recurring-revenue-by-region.yaml` reconcile. Group the June figure by region and the six
groups still sum to 202121.

## Anchors

Six metrics declare an `anchor`: a range and the number the metric produced over it when
it was certified.

```yaml
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 202121
```

Each one is re-executed before the catalog can answer anything. Change a number in
`data/`, or widen a required filter, and the run stops with a mismatch naming the metric
instead of quietly returning a different figure under the certified name.

An anchor is compared as rendered TEXT, at the metric's coarsest declared grain, which is
what makes a float-valued metric awkward to anchor: a sum of decimals is not exact in
binary and two engines may legitimately differ in the last place, so the comparison would
pin how a language prints an expansion rather than pinning a number. Money is held in minor
units throughout for the same reason: `mrr_cents` is an integer, so a total is exact and
the comparison is too.

Four of the five metrics without anchors are the ones that argument reaches. `churn_rate` is the
one float that carries one, because both of its halves are counts: exact integers in a
double however they were summed, so the whole measure is a single rounding of two
exactly-represented values, and both of them are separately certified beside it. Its own
file makes that case in full.

`revenue_per_churned_subscription` is the fifth, and it is unanchored for a different reason worth
keeping straight: the float argument would not apply to it either - both halves are counts again -
but it declares `zero_denominator: fails`, so anchoring it would mean picking a range where the
denominator happens not to be zero and making the readiness of the whole bundle depend on that
staying true. The period that metric exists to demonstrate is the one where it has no figure, and
an anchor cannot be that period.

## Refusals

Nine questions in the corpus are named `refused-*` because that is what they are for. A
refusal is a result rather than an error, decided before anything runs, and it names what
was wrong:

```bash
cargo run -p sutura-cli -- \
  query examples/single-player/catalog \
        examples/single-player/questions/refused-value-not-allowed.yaml \
        examples/single-player/data
```

```
refused: DimensionValueNotAllowed
  the dimension is filterable and the value is not one the definitions declare
  metric: recurring_revenue
  dimension: region
  remedy: Use a value from that dimension's list below. The refusal does not repeat your value back to you, on purpose, so compare against the list rather than expecting a correction.
```

The other eight ask for a grain the metric does not declare, a metric nobody has defined, a
filter on a dimension that is group-by only, a dimension on a metric that has none, the same
dimension twice, five group-by keys where four is the limit, ten thousand years of history, and a
dimension on a metric whose definitional filter is the reason it declares only one. None of them
reach the data system.

A refusal is not the only thing that is not an answer, and the corpus carries the other one too.
`questions/revenue-per-churned-subscription-january.yaml` asks a question the definition permits,
over a month that has seventy rows and no terminations, of a metric declaring
`zero_denominator: fails`:

```
sutura: the data system did not answer
  caused by: column revenue_per_churned_subscription came back as a value that is not a finite number
  caused by: inf is not a finite number
```

That is a *failure* and not a refusal, and the distinction is the governance one. A refusal is
something a caller asked for and may not have, decided before anything runs. Here the caller asked
something the metric permits, the data system answered, and what refuses to carry the answer is
`Value::Real`: the division was emitted unguarded because the definition says an empty denominator
is a fault, and IEEE float division by zero returns `inf` rather than raising. Without that check
the string `inf` would come back under a certified metric name, which is exactly what it used to
do. The June question beside it answers a real figure, so this is not a metric that simply never
works.

## Over HTTP

The same catalog, the same data and the same questions, served. `sutura-serve` is a second binary
rather than a subcommand of `sutura`: the shipped image holds one executable with no server in it.

This surface has a threat model the command line does not, and four things about it are worth
knowing before you point anything at it. The token is required beyond loopback. It authenticates
the **deployment and not the caller**. A refusal comes back under a status of its own rather than a
`200`, keeping the `outcome` body a caller branches on. And the service refuses to start in a
posture nobody chose.

Two places carry that, and neither of them is here. `docs/serving.md` is the reference - the
configuration, the postures, every endpoint, what each refusal's status and `code` are, and what
the process will not start with. `crates/sutura-serve/tests/served.rs` is the tested half, and it
runs **against this directory**: it starts the binary on a kernel-chosen port, asks questions out
of `questions/`, and asserts the missing-token refusal, a certified answer, a refusal arriving as
its documented status, a caller's own token against every forgery, and a key set this deployment
cannot use stopping the process. `just serve-e2e` runs it, and `docs/serving.md` says which of
those claims nothing asserts.

**A captured session used to sit here instead, and deleting it is the point rather than a tidy-up.**
This section was 392 lines and is 27. What went was terminal output - a startup, two `curl` calls,
the token gate, the liveness probe, the generated interface description and a refusal to start -
restating a reference page that owns all of it, with nothing holding a byte of it true. It had already rotted: the refusal block
higher up this page printed a `Debug` dump the binary stopped emitting when `render_refusal` landed,
and `crates/sutura-cli/tests/documented.rs` exists because of it. A session nothing runs is a
promise about a program, and this repository's rule for those is that a test makes them or they are
not made.

## Over MCP

A chat client points at the `sutura` binary's `mcp` command. It launches the process and
speaks the Model Context Protocol on its pipes - the same two tools the HTTP surface serves
(`describe_catalog` and `ask_metric`, from the same declaration), for a locally run, single
player session over this same directory.

```bash
sutura mcp examples/single-player/catalog examples/single-player/data
```

Point the client at `sutura mcp <catalog-dir> [data-dir]` - the directory is optional, because a
deployment that declares its data system has already said where the data is - and it lists the
catalog and answers certified questions exactly as `query` would. The process prints, on standard error, that it
grants every capability to whoever can reach it: a pipe has no header a token could arrive in,
so the limit is stated beside the mode rather than left as a default.

## The data

It is synthetic, all of it. A seeded pseudo-random generator produced it, the customer
numbers look like `C0001` because a generator wrote them, and no row corresponds to a real
person, contract or account. It is shaped like telco data so the metrics are recognisable,
and none of the figures mean anything outside this directory.

It is also deliberately small. Every file is a few hundred lines and under the repository
limit of 1000, which is a property worth keeping rather than an accident: a quickstart is
worth more when a reader can open the CSV and check the arithmetic by hand. The daily usage
extract is therefore a two-week window, 2026-06-01 to 2026-06-14, rather than the full
quarter, and the two metrics over it are anchored to nothing for the float reason above.
The monthly snapshot covers all six months of the first half of 2026, because a revenue
series with one point is not a series.

Three properties of it are load-bearing and look like defects, so they are written down here
rather than left to be tidied up by somebody reading the CSV. `contract_term` is constant per
subscription across the months, because a contract's term is not a monthly attribute. One
subscription in June names a customer key that `dim_customer.csv` does not have, which is what
puts a `null` group in every answer grouped by a customer attribute. And January has rows and no
terminations at all, which is the month `revenue_per_churned_subscription` has no figure for.

## As a test

**The same directory is the corpus of every test that reads a catalog, and there is no second copy
of it anywhere.** That is a recent thing worth stating plainly: the golden suite under
`crates/sutura-app/tests` used to carry a second, e-commerce catalog of its own, so the documented
example and the certified one were two directories that agreed only as long as somebody kept them
agreeing. There is now one, and this is it.

```bash
just documented   # the pages: every command they print, and every line of output
just test         # the whole workspace, the other two suites below included
```

`crates/sutura-cli/tests/example.rs` loads the catalog, pins the digest, re-runs every anchor, runs
the whole corpus and snapshots the generated SQL and whatever came back - rows, a refusal reason, or
the error chain of the one question that fails. It runs on the one pair the shipped binary composes:
the local catalog adapter and the engine.

The golden suite under `crates/sutura-app/tests` expands this same corpus over a matrix - every
registered catalog adapter, every dialect the compiler renders for, every registered data system -
and compares the parsed definitions against a statement of them written out by hand in Rust, so two
readers of these documents cannot agree by sharing a bug. Between them: an edit that changes what
this example does shows up as a snapshot diff to review rather than as a README that used to be
true.

Two of `example.rs`'s assertions are not snapshots and are the reason a case cannot quietly
leave. The `refused-` prefix is read as a convention in both directions, so a refusal question that
started answering and a plain question that started being refused are each a failure rather than a
passing corpus. And the measure vocabulary is asserted as five exact sets - shapes, terms, the terms a
ratio holds, the aggregates, and both meanings of a zero denominator - so this section's table cannot
claim coverage the catalog has stopped carrying.

`crates/sutura-cli/tests/documented.rs` reads this page and `docs/getting-started.md`. It runs every
`sutura catalog`, `describe`, `compile` and `query` invocation out of both from a clone's working
directory, and requires every refusal block and every `-- definitions ` stamp either page prints as
output to be what the command in the fence above it printed - the whole block, from the `refused:`
line to the end of its fence, because that is what one refusal is. A definitions digest is held
wherever it appears in prose, this page and `docs/serving.md` included. It exists because the
refusal block in the *Refusals* section above had rotted into a `Debug` dump the binary stopped
emitting, on the first page a reader is sent to.

**Three things it does not hold, next to the claim.** Not a whole captured block: both pages elide
and wrap on purpose, and the rows are what `example.rs`'s snapshots are for - so an output line that
is neither a refusal nor a provenance stamp is unheld. Not `sutura mcp`, which is why the exception
is named at the top of this page. And not an invocation that sets an environment variable in front
of the command, which it refuses rather than runs, because it strips every `SUTURA*` variable and so
cannot be the deployment such a line describes.

The served half of this catalog is pinned by `crates/sutura-serve/tests/served.rs` and by the
in-process harness in `crates/sutura-http/src/harness.rs`: a refusal carries the status its reason
maps to, with one test per status checking the `code` and that `reason.status` agrees with the
status line, a missing token is a `401` carrying `code: unauthorized`, a body holding `sql` is a
`400` naming the field, `/health` is those fifteen bytes exactly, and the interface description is
served in development and not in production. That harness reaches ten reasons, not all seventeen -
the exhaustive table is `crates/sutura-http/src/wire/refusal.rs`, where every variant's status and
`code` are listed and the match assigning them has no wildcard arm, so a new refusal fails to
compile until somebody decides. What nothing pins is the JSON *formatting* of a response or the
`detail` sentences beside the codes.
