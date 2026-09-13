---
title: Getting started
description: Download the published binary, verify it, take the corpus, and ask one certified question - no clone and no Rust toolchain.
---

# Getting started

Download the binary, verify it, take the corpus, ask a question. **No Rust toolchain and no
clone**, and nothing to provision on the data side either: the engine is compiled into the binary
and reads CSV and Parquet files where they lie.

**What you need** is a Linux host on x86_64 or aarch64, and an authenticated `gh`. Every command
below goes through it: it is how the release assets and the corpus tarball are fetched, and
`gh attestation verify` is one of the two checks the install step runs.

!!! note "The artefacts need read access to the repository, for now"

    This page is published and the repository is not yet, so the release assets, the corpus tarball
    and the `ghcr.io` tags all refuse a reader who cannot read `telekom/sutura` - the image route
    below for macOS and Windows included. `gh auth login` under an account that can is the way in
    today, and **this note is what to delete when the repository becomes public**, because the
    commands themselves do not change.

## What a release publishes, and for which platforms

**Four triples, and all four are Linux.** There is no macOS binary and no Windows binary - that is
what a release builds rather than a note about what has not been tried yet, and `nix/shipped.nix`
is where the set is declared:

| Asset                                      | libc                                                             |
| ------------------------------------------ | ---------------------------------------------------------------- |
| `sutura-x86_64-unknown-linux-gnu.tar.gz`   | glibc, so a host no older than the builder's                     |
| `sutura-aarch64-unknown-linux-gnu.tar.gz`  | glibc, same floor                                                |
| `sutura-x86_64-unknown-linux-musl.tar.gz`  | none - statically linked, no dynamic loader and no version floor |
| `sutura-aarch64-unknown-linux-musl.tar.gz` | none, same                                                       |

Take a musl one unless you have a reason not to: it has nothing to resolve at load time, so the
only question it can fail on is the architecture. Each tarball holds one file, the executable, at
its root.

**On macOS or Windows there is no binary to download**, and the two ways in are the image and a
source build. The unsuffixed image tags at `ghcr.io/telekom/sutura` are this command-line tool -
[verifying a release](verifying-a-release.md) is the tag table, and [serving over HTTP](serving.md) is the shape for the server, whose tags are the `-serve` ones.
[Building from source](#building-from-source) is at the bottom of this page.

**Both published binaries carry cargo's default features**, which for this one means it links no
data-system adapter at all - `sutura doctor`'s `data systems` line reads
`none - this build reads files, and pushes down to nothing`. That is not a hole in this tutorial.
Reading files is the whole of what this build does and it is enough for every command below, up to
and including a certified answer with its anchors re-executed. `just shipped` is what asserts it,
out of the shipped binary's own embedded dependency list rather than out of a manifest.

## Install it

```bash
gh release download --repo telekom/sutura \
  --pattern 'sutura-x86_64-unknown-linux-musl.tar.gz' \
  --pattern 'sutura-x86_64-unknown-linux-musl.tar.gz.sha256'
sha256sum -c sutura-x86_64-unknown-linux-musl.tar.gz.sha256
gh attestation verify sutura-x86_64-unknown-linux-musl.tar.gz --repo telekom/sutura
tar -xzf sutura-x86_64-unknown-linux-musl.tar.gz
mkdir -p ~/.local/bin
install -m755 sutura ~/.local/bin/sutura
```

No tag on the first command takes the latest release; `gh release download <tag>` pins one.
Anywhere on your `PATH` will do instead of `~/.local/bin`, and `sutura` with no arguments lists the
commands.

**The two checks answer different questions and neither answers the other's.** `sha256sum -c` says
the bytes match the digest published beside them, which catches a truncated or corrupted download
and nothing else. `gh attestation verify` says these bytes are what this repository's release
workflow emitted at a tag - so a substituted file fails it, and it exits non-zero on a single
changed byte. What neither of them says is that the release is fit for what you want to do with
it; [verifying a release](verifying-a-release.md) is that page, and its last section is the list of
things a green verification does **not** establish.

```bash
sutura doctor
```

That prints what the binary in your hand was built with - profile, allocator, engine, the
`data systems` line above, and a redaction probe that proves a secret still renders as one in the
shipped build rather than only under test.

## The corpus

There is a catalogue, the data behind it and a directory of questions in this repository already:
`examples/single-player`, the one corpus both test suites run on. Take it as a tarball.

```bash
TAG=$(gh release view --repo telekom/sutura --json tagName --jq .tagName)
mkdir -p sutura-corpus
gh api "/repos/telekom/sutura/tarball/$TAG" | tar -xz -C sutura-corpus --strip-components=1
cd sutura-corpus
```

**`gh release view` takes no tag, so `TAG` is the LATEST release** - the same one `gh release
download` with no tag gave you. If you pinned a tag above, set `TAG` to that tag instead. A corpus
and a binary from two different releases is not an error and nothing refuses it: the definitions
digest under [`query`](#asking) is where the difference surfaces, as a different number rather than
as a complaint.

**The corpus is deliberately not a release asset.** A published asset is signed, attested,
inventoried by an SBOM and named in the release notes, once per binary per triple; `nix/shipped.nix`
is where that set is declared and it declares executables. Sample data does not earn that
machinery, and adding it would make every release carry a second thing to sign in order to ship
360 KB of sample definitions and CSV. So the corpus stays in the repository, and the way to it is a
**download rather than a clone** - no git, no toolchain, one request. Our own release smoke test
takes it the same way: `.github/serve-smoke.sh` mounts a checkout's `examples/single-player` into
the image it tests.

**The limit, next to the claim.** That tarball is not signed, carries no provenance and has no
published checksum - the signed artefact is the binary. Treat the corpus as what it is, sample data
you are about to read, and note that nothing on this page writes to it.

Every path below is relative to `sutura-corpus/`, which is the repository's own layout - which is
why the same commands run unchanged in a clone, and why `just documented` can run every one of them
in CI.

## What a catalogue says

```bash
sutura catalog examples/single-player/catalog
```

Eleven metrics; five of them below, and the elision is this page's rather than the command's:

```text
version local-working-tree
digest  8042ba92eaddce5e96e055cc43a635a64161d54ec21bd4f3367e7a1f58f5b4c5

active_subscriptions
  measure    count_distinct(subscription_key)
  filters    status = "active"
  grains     month
  dimensions product_family, region, segment
  anchor     59 over [2026-06-01, 2026-07-01)
churn_rate
  measure    count_if(churned_in_month) / count_distinct(subscription_key), zero denominator yields_null
  filters    none
  grains     month
  dimensions product_family, region, segment
  anchor     0.04838709677419355 over [2026-06-01, 2026-07-01)
recurring_revenue
  measure    sum(mrr_cents)
  filters    status = "active"
  grains     month
  dimensions contract_term, product_family, product_name, region, segment
  anchor     202121 over [2026-06-01, 2026-07-01)
revenue_per_churned_subscription
  measure    sum(mrr_cents) / count_if(churned_in_month), zero denominator fails
  filters    none
  grains     month
  dimensions none
  anchor     none
subscription_base
  measure    count_distinct(subscription_key)
  filters    none
  grains     month
  dimensions product_family, region, segment
  anchor     62 over [2026-06-01, 2026-07-01)
```

The digest is over the canonical form of the parsed definitions, so reformatting a document does not
move it and changing what a metric means does. It travels with every answer. `local-working-tree` is
the version a directory of markdown gets when nobody said otherwise - a deployment passes a commit
id, and the digest is what says what the content actually is.

Two of those five are worth reading together. `active_subscriptions` and `subscription_base` are the
same count over the same rows in the same month, with one definitional predicate between them, and
their anchors are 59 and 62 - which is what a required filter buys, shown as two numbers rather than
asserted in a sentence.

`describe` prints one metric in full, including the prose from the body of its document - which is
what the markdown half of the format is for.

```bash
sutura describe examples/single-player/catalog recurring_revenue
```

## Asking

A question is a small file. There is no field in it for SQL, a table, a predicate or a list of row
ids, so an uncertified question is not something you can write down:

```yaml
metric: recurring_revenue
grain: month
range:
  start: 2026-06-01
  end: 2026-07-01
dimensions: [region]
```

`compile` turns it into a statement and stops. No data system is involved, which makes it the
command to reach for when the question is what we would have run:

```bash
sutura compile examples/single-player/catalog \
               examples/single-player/questions/recurring-revenue-by-region.yaml
```

```sql
-- dialect duckdb
SELECT "dim_customer"."region" AS "region",
       CAST(DATE_TRUNC('month', "fct_subscription_monthly"."month") AS DATE) AS "period",
       SUM("fct_subscription_monthly"."mrr_cents") AS "recurring_revenue"
FROM "fct_subscription_monthly"
     LEFT JOIN "dim_customer"
       ON "fct_subscription_monthly"."customer_key" = "dim_customer"."customer_key"
WHERE "fct_subscription_monthly"."month" >= ?
  AND "fct_subscription_monthly"."month" < ?
  AND "fct_subscription_monthly"."status" = ?
GROUP BY "dim_customer"."region", CAST(DATE_TRUNC('month', "fct_subscription_monthly"."month") AS DATE)
ORDER BY "dim_customer"."region", CAST(DATE_TRUNC('month', "fct_subscription_monthly"."month") AS DATE)
LIMIT 10001

-- $1 = 2026-06-01
-- $2 = 2026-07-01
-- $3 = "active"
```

(printed on one line; wrapped here to read). The plan behind the statement is printed after it. Pass
a dialect as a third argument - `duckdb`, `postgres` or `clickhouse` - to see the same plan rendered
for another data system: `ClickHouse` gets `dateTrunc` and `sum`, Postgres gets `$1` and `$2`
instead of `?`.

Three details are deliberate and easy to misread. **Every value is a bind parameter, including
`$3`** - that one is not something the caller sent, it is `recurring_revenue`'s own
`status = "active"`, and it is bound rather than written into the statement so that there is one
path for values and not two. The join is a **`LEFT JOIN`**, because an inner one drops fact rows
that have no matching dimension row and so silently changes the measure it was only asked to break
down. And the limit is **`10001`, not `10000`**: the cap is ten thousand rows, and asking for one
more is how a result that *reached* the cap is told apart from one the cap *cut off*. If that extra
row comes back, the question is refused as too wide to certify rather than answered with a total
that is quietly missing its tail.

`query` answers it. It checks every declared anchor first, and will not serve a bundle whose anchors
did not all match:

```bash
sutura query examples/single-player/catalog \
             examples/single-player/questions/recurring-revenue-by-region.yaml \
             examples/single-player/data
```

```text
-- definitions local-working-tree 8042ba92eaddce5e96e055cc43a635a64161d54ec21bd4f3367e7a1f58f5b4c5
region	period	recurring_revenue
central	2026-06-01	51739
east	2026-06-01	32598
north	2026-06-01	42157
south	2026-06-01	21203
west	2026-06-01	49425
null	2026-06-01	4999
```

That last row is the `LEFT JOIN` above, visible. One subscription in the data names a customer the
customer table does not hold, so its revenue groups under a null region instead of vanishing: the six
rows sum to 202121, which is what `recurring_revenue` answers for June ungrouped and what its anchor
declares. An inner join would have answered 197122 here and 202121 there, under one metric name, with
nothing raising an error.

## Being refused

A refusal is a result, not an error, and the exit status says so. Ask for a region the catalogue does
not declare a value for:

```bash
sutura query examples/single-player/catalog \
             examples/single-player/questions/refused-value-not-allowed.yaml \
             examples/single-player/data
```

```text
refused: DimensionValueNotAllowed
  the dimension is filterable and the value is not one the definitions declare
  metric: recurring_revenue
  dimension: region
  remedy: Use a value from that dimension's list below. The refusal does not repeat your value back to you, on purpose, so compare against the list rather than expecting a correction.
```

Note what the refusal does not say: the value you asked for. The remedy says outright that it will
not repeat it back, because a rejected value echoed into a message reaches a log, a terminal and an
agent's context - which is how a rejected value becomes somebody else's input.

The variant is the machine-readable half and the two sentences are the human half, and they come
from one table: `sutura_app::prompt::guidance` is what `sutura prompt` renders for an agent and what
this terminal prints, so a person and an agent are told the same thing about the same refusal.

The other question files under `examples/single-player/questions/` named `refused-*` reach the rest
of the refusals, one per reason.

## Your own catalogue

A catalogue is a directory of markdown documents. Each one declares what it is, so a file in the
wrong place is an error rather than a definition that was quietly never loaded.

A model names a table and its columns:

```markdown
---
kind: model
name: subscriptions
source: local
table: fct_subscription_monthly
columns: [month, subscription_key, customer_key, status, mrr_cents, churned_in_month]
---

One row per subscription per month. Money in minor units, so a total is exact.
```

`source:` is the data system this model's table lives in, and `local` above is not a keyword - it is
the name of the one data system the `sutura` command declares for itself: a directory of CSV or
Parquet files, the one you pass as the last argument, read as whoever ran the command. **Any other
name works, and it works by being declared.** Put a configuration directory beside your catalogue:

```yaml
# conf/base.yaml, reached with SUTURA_CONFIG_DIR=conf
security:
  identity: single-user
  single_user_because: "one analyst, one laptop, the files they already have"
sources:
  warehouse:
    kind: files
    data_dir: "/absolute/path/to/data"
    posture: shared-service-user
```

Then `source: warehouse` is answered, and the data directory comes off that entry rather than off the
command line - so `sutura query catalog/ question.yaml` takes no third argument. Passing one anyway is
fine when it names the same directory the entry does, and refused when it names a different one: two
answers that agree are one answer, and a disagreement is not something to resolve silently. It is the
same `sources:` tree [the service](serving.md) reads, which is the point: one declaration, whichever
surface asks.

**What a declaration does NOT say**, and it is worth knowing before pointing one at a directory: it
says where a data system is, never that the files there hold what your catalogue certifies. The only
thing that checks the data is an **anchor** - a metric that declares one has its number re-executed
before anything is served, and a metric that declares none is answered out of whatever is there,
under the catalogue's real digest. That is true of the service too. Anchor the metrics you care
about; [Being refused](#being-refused) is what a mismatch looks like when one is declared.

**A configuration that will not SERVE will not answer here either**, and that is one door into the
settings rather than two - `sutura prompt` goes through the same one, so the two cannot refuse
differently. The consequence catches people out on a first run, because it reaches variables that
have nothing to do with the question:

- `SUTURA_ENVIRONMENT=production` with no access token configured stops this command, and the
  refusal names the two variables that get you back to answering from the directory on the command
  line.
- **`SUTURA__SERVER__HOST` set off-loopback in your shell stops it too**, on TLS termination, from a
  command that binds no listener at all. Every settings key can be set as one `SUTURA__`-prefixed
  variable, and this command reads the server's refusals; that one is the variable whose refusal
  does not name it. Unset it. It is not the environment or the configuration directory, so the
  remedy printed underneath will not point at it.

`kind: bigquery` and `kind: postgres` are the two network kinds, and each needs a build carrying its
default-off feature - the published binaries deliberately link neither, which
`checks.shipped-features` reads out of each released binary rather than out of a manifest. (That is
an artifact decision, not a build-time saving.) [Building from source](#building-from-source) is how
to get one. The BigQuery block below is the cloud variant; the Postgres declaration, verified and
mutual TLS modes, and least-authority role grants are in [Serving](serving.md#a-postgres-source-least-authority-and-its-channel).

**The `security:` block above is still required** - the snippet below replaces the `sources:` block
and nothing else. Any non-empty `sources:` with no `security.identity` is refused at startup, naming
that key; it is the same refusal a deployment gets, which is the point of there being one settings
tree.

```yaml
# conf/base.yaml - the `security:` block from above, plus:
sources:
  warehouse:
    kind: bigquery
    billing_project: "your-project"
    dataset: "marts"
    credential_file: "/absolute/path/to/key.json"
    max_bytes_billed: 1073741824
    posture: shared-service-user
```

`posture: shared-service-user` is the honest declaration for a service-account key: one identity for
everybody who asks. `impersonation-at-source` parses and is **refused** - the adapter can carry a
subject's credential, and no binary attaches a broker that exchanges one, so serving it would read
every row as the process while the declaration promised otherwise. `sutura doctor` says which build
you have on its `data systems` line.

**Two limits worth knowing before you rely on this.** No automated test in this repository has ever
run a query from this command against a real dataset - the furthest any of them reaches is reading
the credential file, because the transport's host is a compile-time constant with no loopback to
point at. What HAS been accepted by a real dataset is the corpus, through `just bigquery-acceptance`,
on the adapter's own suite. And the job's deadline comes off `server.request_timeout_seconds`: the
default 30 leaves a job **10 seconds** and the maximum 300 leaves it **145**, because an answer makes
two calls and each pays a connect margin. A slow question is cancelled by that bound with nothing
waiting on any request.

`table:` may also name where the table lives, when that is more than the connection's own default:

```markdown
table: fct_subscription_monthly # the connection's default dataset
table: sales.fct_subscription_monthly # a named dataset, or schema
table: analytics-prod.sales.fct_subscription # a named project above a named dataset
```

Each part is parsed on its own and quoted on its own in the statement, so a path is never a string
somebody assembled. **How deep a path is usable depends on the data system**: BigQuery resolves all
three forms and a join across two of its projects is one native statement; Postgres and ClickHouse
resolve a schema or a database; the in-process engine resolves neither, because it registers one file
per model and has nothing above it - a qualified model there fails at startup rather than at query
time. `docs/adr/0019` is the decision and states each dialect's arm.

A metric names a model, what it measures, the grains it answers at and the dimensions it may be
broken down by:

```markdown
---
kind: metric
name: recurring_revenue
model: subscriptions
measure:
  simple: { aggregate: sum, column: mrr_cents }
required_filters:
  - equals: { column: status, value: active }
time_column: month
grains: [month]
dimensions:
  - name: contract_term
    column: contract_term
    values: [annual, monthly]
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 202121
---

Recurring revenue recognised in the month, in minor units, from active subscriptions only.
```

`measure` has two levels. A **term** is what one number is computed from, and there are two of
them: `aggregate` with a `column`, as above, or `count_if` over a boolean column. A **shape** says
how terms combine, and there are two of those: `simple` is one term, `ratio` is one divided by
another. Either term goes in either position:

```yaml
# How many rows have a boolean column true. Its own term because COUNT(col) counts
# the false ones too, which is a wrong number that raises no error.
measure:
  simple: { count_if: churned_in_month }
```

```yaml
# A ratio: one term divided by another, over possibly different columns.
measure:
  ratio:
    numerator: { aggregate: sum, column: mrr_cents }
    denominator: { aggregate: count_distinct, column: customer_key }
    zero_denominator: yields_null
```

```yaml
# A conditional count as the numerator of a rate - the combination the vocabulary
# exists at two levels for. `examples/single-player` ships it as `churn_rate`.
measure:
  ratio:
    numerator: { count_if: churned_in_month }
    denominator: { aggregate: count_distinct, column: subscription_key }
    zero_denominator: yields_null
```

A metric may also carry `required_filters`, which are part of what it *means* rather than something
a caller chooses:

```yaml
required_filters:
  - equals: { column: status, value: active }
```

Four things matter before you write one:

- **A measure is a shape and terms from a closed vocabulary, not an expression.** There is no field
  for `sum(price * quantity)`, and [the closed vocabulary for measures](adr/0002-a-closed-vocabulary-for-measures.md) argues why: a string field is an escape
  hatch, and an escape hatch on the query path is the thing being defended against. What the
  vocabulary cannot say can be declared under the explicitly named `authored_sql:` sibling, and
  today that declaration loads and pins and nothing more: no published adapter compiles or executes
  the fragment, so a bundle carrying one is refused at startup. `zero_denominator` is required
  rather than defaulted, because "a rate over an empty period is null" and "is an error" are both
  defensible and a definition should say which - which is also why it is a word (`yields_null` or
  `fails`) rather than a boolean that records only that somebody thought about it.
- **A `required_filter` is definitional, and a caller can neither see it nor turn it off.**
  `recurring_revenue` *means* the active figure; a statement that left the predicate out would
  return revenue including terminated subscriptions under a certified name. That is a wrong answer
  arrived at by omission rather than by tampering, which is the more likely failure and the harder
  one to notice. The modelling consequence: a metric with a required filter on `status` should not
  also declare `status` as a dimension, or grouping by it would be a way to ask the metric for the
  figure it excludes.
- **`values` is what makes a dimension filterable.** Without it the dimension can be grouped by and
  not filtered on, because a filter needs an allowlist - the alternative is comparing against
  whatever the caller sent. `recurring_revenue` declares `product_name` without one for exactly
  that reason.
- **An `anchor` is a number this metric produced when somebody certified it.** It is re-executed
  before the bundle is served, so a definition that has stopped meaning what it claimed fails
  readiness instead of answering. Declare one for any metric whose value you would act on.

For the data, `query` expects one file per model, named after the model's table, in the directory you
pass it - `<table>.parquet` if it is there, `<table>.csv` otherwise. Nothing is written and there is
no database: the engine registers each file in process and reads it where it lies on every run, so
the answer cannot drift from the files.

## Building from source

**This is the contributor's route, and it is the second half of this page rather than the first for
a reason.** Everything above runs on a published artefact, which is what a reader who wants an
answer should be holding. You want a source build for one of two things: a change to this
repository, or a feature no published binary carries.

A clone and the toolchain the repository pins are what this needs -
[Contributing](contributing.md) is the setup, and `just` with no argument lists every task. Every
command above then has a second spelling, which is the one this repository's own suites run:

```bash
cargo run -p sutura-cli -- \
  query examples/single-player/catalog \
        examples/single-player/questions/recurring-revenue-by-region.yaml \
        examples/single-player/data
```

No feature flag and nothing to install for that: the engine is compiled into the binary and reads
the CSVs itself, so `query` works in a plain `cargo run`. A data system's driver is a development
dependency here - present to prove the SQL we render actually runs, not to answer your questions.

A build that can open a BigQuery dataset is the one thing above that a published binary cannot do,
and it is a feature away:

```bash
cargo build --release -p sutura-cli --features bigquery
```

`sutura doctor` on the result says `bigquery, over the wire` where a published artefact says `none`,
which is how somebody holding a binary finds out which of the two they have.
