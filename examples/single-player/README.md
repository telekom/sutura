# Single player

A complete catalog over a small synthetic telco warehouse, and the shortest path from a
clone to an answered question.

```bash
cargo run -p sutura-cli -- \
  query examples/single-player/catalog \
        examples/single-player/questions/recurring-revenue-by-month.yaml \
        examples/single-player/data
```

```
-- definitions local-working-tree 0be421868ca979d8a7cc4b9d5212c4c021feb7899b5e9601ad1a7143cb2bec73
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
catalog/models/*.md            what tables exist and which columns may be read
catalog/relationships/*.md     which joins are allowed, and at what cardinality
catalog/metrics/*.md           what each certified number means
data/*.csv                     one file per model, named after the table
questions/*.yaml               the corpus, including the ones that are refused
```

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
refused: DimensionValueNotAllowed { metric: MetricName("recurring_revenue"), dimension: DimensionName("region") }
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

The same catalog, the same data and the same questions, served. `sutura-serve` is a second
binary rather than a subcommand of `sutura`: the shipped image holds one executable with no
server in it. `docs/serving.md` is the reference for the configuration and the posture; what
follows is one session against this directory.

This surface has a threat model the command line does not, and four things are worth watching
for as you read. The token is required beyond loopback. It authenticates the **deployment and
not the caller**. A refusal comes back `200` with an outcome rather than a `4xx`. And the
service refuses to start in a posture nobody chose.

### Starting it

It needs a catalog, the data those documents describe, and a name for the snapshot. The token
is *not* required on loopback and is set here anyway - a deployment anything else can reach
does require one, and the gate below is worth seeing.

```bash
E=examples/single-player
export SUTURA_TOKEN="$(head -c 24 /dev/urandom | base64)"

SUTURA__CATALOG__DIR=$E/catalog \
SUTURA__CATALOG__DATA_DIR=$E/data \
SUTURA__CATALOG__VERSION=local-1 \
SUTURA__SECURITY__ACCESS_TOKEN="$SUTURA_TOKEN" \
  cargo run -p sutura-serve
```

`AccessToken::parse` reads an RFC 6750 `b64token` of at least 32 characters - letters, digits,
`-`, `.`, `_`, `~`, `+`, `/`, and `=` only as trailing padding - which is what that generator
produces. A shorter one is a startup refusal naming the length, not a service that boots and
answers `401` to everything.

```
             _
 ___  _   _ | |_  _   _  _ __   __ _
/ __|| | | || __|| | | || '__| / _` |
\__ \| |_| || |_ | |_| || |   | (_| |
|___/ \__,_| \__| \__,_||_|    \__,_|

  identity-aware semantic data runtime for AI agents
  version 0.2.4 - environment development

  INFO sutura_runtime::banner: configuration resolved, environment: development, resolved: Settings { .. }
  INFO sutura_runtime::banner: listening on loopback only - reachable from this host and no other, bind: 127.0.0.1:8080, access_token: "configured"
  WARN sutura_runtime::banner: rate limiting is DISABLED - a caller is bounded only by the data system
  INFO sutura_runtime::banner: per-request bounds, request_timeout_seconds: 30, max_body_bytes: 65536
  INFO sutura_runtime::banner: questions executing at once is bounded; one that cannot get a slot inside the window is answered 503 rather than queued. A question already executing is NOT cancelled by any timeout here, max_concurrent_queries: 8, admission_timeout_seconds: 5
  INFO sutura_runtime::banner: in-process engine width, resolved from the machine - a container with a CPU quota should set runtime.engine_worker_threads instead, engine_worker_threads: 16
  INFO sutura_runtime::banner: the budget for the WHOLE of stopping: the connection drain first, then what is left of it for questions already executing, shutdown_grace_seconds: 15
  INFO sutura_runtime::banner: generated interface description, docs: true
  INFO sutura_runtime::banner: catalog and log, catalog_dir: examples/single-player/catalog, data_dir: examples/single-player/data, definition_version: local-1, log_format: pretty, log_format_explicit: false
  WARN sutura_runtime::banner: NO PER-CALLER IDENTITY: an access token authenticates the DEPLOYMENT, not the caller. There is no request context, no per-request credential and no row-level scoping - every question is answered with whatever access this process already had, whoever asked it
  INFO sutura_serve: catalog loaded and every anchor reproduced its number, definition_version: local-1, metrics: 11
  INFO sutura_http::router: rate limiting disabled - a no-op layer is in its place, environment: development
  INFO sutura_http::router: rate limit buckets are keyed on the peer address - behind a proxy that is ONE bucket for every caller, and rate_limit.client_address is what changes it, client_address: peer
  INFO sutura_http::server: listening, bound: 127.0.0.1:8080, tls: false
```

Two things are shortened above rather than invented: each real line carries an ISO timestamp
before the level and an indented `at crates/...` source location after it, and the resolved
configuration goes out as one very long `Debug` of the whole settings tree. Nothing else is
elided, and in particular the `NO PER-CALLER IDENTITY` line is printed on every boot,
unconditionally, at `WARN` so it survives a filter that drops `info`. An example that hid it
would defeat the point of printing it: the reader who needs it is the one about to deploy this
believing a `401` implies a per-caller identity behind it. Every response block further down
has its `date` header dropped for the same reason and nothing else; the status line, the other
headers and the body are byte for byte what came back.

`metrics: 11` and `every anchor reproduced its number` are the readiness gate, and they are why
this surface has no readiness endpoint to ask. Startup loads the catalog through its port and
re-executes every declared anchor against the data; a bundle whose anchors do not hold starts
nothing, so a process that is listening is a process whose definitions reproduced the numbers
their author certified.

Three of those lines are bounds rather than facts, and they are the ones an operator has to
argue with. `max_concurrent_queries` is how many questions execute at once; a question that
cannot get a slot inside `admission_timeout_seconds` is answered `503` with
`code: at_capacity` and a `Retry-After`, rather than queued behind work that is already
running. `engine_worker_threads` was resolved from the machine here, which the line says
outright, because a container with a CPU quota gets a number the kernel reported and not the
number it is allowed to use.

### A question over the wire

The same question as `questions/recurring-revenue-by-month.yaml`:

```bash
curl -s -i -X POST http://127.0.0.1:8080/v1/query \
  -H "authorization: Bearer $SUTURA_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"metric":"recurring_revenue","grain":"month","range":{"start":"2026-01-01","end":"2026-07-01"}}'
```

```
HTTP/1.1 200 OK
content-type: application/json
content-length: 347
```

```json
{
  "outcome": "answer",
  "provenance": {
    "definition_version": "local-1",
    "definition_digest": "0be421868ca979d8a7cc4b9d5212c4c021feb7899b5e9601ad1a7143cb2bec73"
  },
  "columns": ["period", "recurring_revenue"],
  "rows": [
    ["2026-01-01", "237320"],
    ["2026-02-01", "232822"],
    ["2026-03-01", "216700"],
    ["2026-04-01", "206160"],
    ["2026-05-01", "202994"],
    ["2026-06-01", "202121"]
  ]
}
```

The service answers one line of JSON weighing the 347 bytes above; the block is that line
reformatted, with each row kept on a line of its own.

**The `provenance` block is why this is an answer rather than a number.** The version names the
snapshot, the digest is over the canonical form of the definitions that produced these rows,
and `ToolOutcome::Answer` carries it as its own typed field beside the rows - there is no
constructor that merges the two or that omits it. So the same question against an edited
catalog is visibly a different answer rather than quietly the same one. That digest is the one
`crates/sutura-cli/tests/snapshots/example_digest.snap` pins, so the figure in this README and
the figure the test suite certifies cannot drift apart in silence.

The cells are strings and not JSON numbers. `mrr_cents` is an integer in minor units, an anchor
is compared as rendered text, and one rendering everywhere is what makes the number in an
answer the number in the anchor that certified it. It is the same table the `query` command
prints at the top of this file.

### A refusal over the wire

The same question as `questions/refused-value-not-allowed.yaml`. `region` is filterable, and
`offshore` is not one of the five values the metric declares for it:

```bash
curl -s -i -X POST http://127.0.0.1:8080/v1/query \
  -H "authorization: Bearer $SUTURA_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"metric":"recurring_revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"filters":[{"dimension":"region","value":"offshore"}]}'
```

```
HTTP/1.1 200 OK
content-type: application/json
content-length: 144

{"outcome":"refusal","reason":{"code":"dimension_value_not_allowed","detail":"that value is not one `recurring_revenue` declares for `region`"}}
```

**`200`, and that is the contract rather than an oversight.** A refusal is a *result*: the
caller asked something they may not have, and the answer is no. A `4xx` invites a client
library to retry - most retry a `429`, many retry a `503` - and retrying a governance decision
until it succeeds is precisely the behaviour the refusal exists to prevent. `outcome` is the
field a caller branches on; `code` is the stable name of the reason, and the sentence beside it
is for a person.

Note what that sentence does *not* contain: the word `offshore`. A rejected value reflected
back into a response reaches a log, a UI and an agent's context, so the domain's refusal reason
carries the dimension and not the value, and the wire shape does not put it back. The other
four `refused-*` questions come back the same way, with `grain_not_supported`,
`metric_unknown`, `dimension_not_filterable` and `dimension_not_permitted`. None of them
reaches a data system.

A *failure* is a different thing from a refusal and does carry a status. A body holding a key
the question shape does not declare is one:

```
HTTP/1.1 400 Bad Request
content-type: application/json
content-length: 222

{"code":"not_a_question","status":400,"detail":"Failed to deserialize the JSON body into the target type: sql: unknown field `sql`, expected one of `metric`, `grain`, `range`, `dimensions`, `filters` at line 1 column 101"}
```

Without `deny_unknown_fields` on the question shape that body would deserialize cleanly with
the `sql` key dropped on the floor, and a caller who believed they had sent SQL would be
answered as though they had asked the modelled question instead.

### The token gate

No token, or the wrong one, on any path that resolves to a handler:

```bash
curl -s -i http://127.0.0.1:8080/v1/catalog
```

```
HTTP/1.1 401 Unauthorized
content-type: application/json
content-length: 84

{"code":"unauthorized","status":401,"detail":"this service requires a bearer token"}
```

That is the `ProblemBody` shape, and it is the shape of *every* failure on this surface: a
stable `code`, the status repeated so a client that logged only the body still has it, and a
sentence for a person. Three fields and no more, in every one of them - the only thing that
varies is a `Retry-After` header, present on the failures where there is an honest number to
put in it. A wrong token is byte for byte the same response as no token, on purpose - telling a
caller which of absent, malformed and wrong they got tells them whether the secret they tried
was close.

The `401` is worth exactly what the startup log said it was worth. It proves the caller holds a
secret an operator wrote down. It cannot be scoped to part of the catalog, it cannot be revoked
for one party without revoking it for all of them, it does not reach the data system, and there
is no request context behind it: every question is answered with whatever access this process
already had, whoever asked. `examples/multi-player/README.md` is where the gap that would close
it is written down.

### The liveness probe

```bash
curl -s -i http://127.0.0.1:8080/health
```

```
HTTP/1.1 200 OK
content-type: application/json
content-length: 15

{"status":"ok"}
```

No token, and outside the version prefix, because a probe has no credential to present and an
orchestrator must not have to be reconfigured across a version bump.

**The body is deliberately that and nothing more.** No version, no build identifier, no digest,
no dependency list, no configuration, no catalog content. This is the one path an
unauthenticated caller can always reach, so every field it might carry is a field handed to
anybody who can route a packet: a version string is a lookup into a list of known
vulnerabilities, and a dependency list is that list pre-assembled. A test in
`crates/sutura-http/src/routes/health.rs` asserts those fifteen bytes exactly, and it is there
to fail when somebody adds "just the version".

### The interface description

Generated from the handlers, so a route and its documented path cannot disagree.

```bash
curl -s http://127.0.0.1:8080/openapi.json -H "authorization: Bearer $SUTURA_TOKEN"
```

Summarised, because the document is eleven kilobytes:

```
openapi 3.1.0   info.version 0.2.4
paths: /health  /v1/catalog  /v1/query
securitySchemes: undefined
info.description contains "NO PER-CALLER IDENTITY": true
```

`/docs` is the browser interface over that document - a `303` to `/docs/`, which serves HTML.
Both are behind the token when one is configured, because a description of this surface is a
description of what this deployment measures, and because a document behind a secret is a
secret worth guessing at. Without the token, `/openapi.json` is a `401`.

No security scheme is declared, and that absence is deliberate rather than missing. `utoipa`
would happily describe a bearer scheme and put an `Authorize` button in the browser interface,
and a scheme in a document reads as an authentication model. There is none. The `401` on each
operation says what happens, and the document's own description says what it means: it carries
the same `NO PER-CALLER IDENTITY` paragraph the startup log prints, so an integrator meets it
without reading this page.

**It is off in production by default.** The same process with `SUTURA_ENVIRONMENT=production`
answers:

```
HTTP/1.1 404 Not Found
x-ratelimit-limit: 20
x-ratelimit-remaining: 19
content-length: 0
```

A map of the surface is something a deployment turns on rather than something it has to
remember to turn off. It is a `404` and not a `401` because a path matching no route never
reaches a token gate, and what that discloses is only which paths exist - which the published
document says anyway. The two rate-limit headers on it were a surprise worth writing down: the
general tier's limiter is the outermost layer of the versioned subtree, so this request costs a
cell and then matches nothing.

Production is also where the limiter is on, which development is not:

```
GET /health x8, and the probe tier is 2 per second with a burst of 5:
200 200 200 200 200 429 429 429
```

```
{"code":"rate_limited","status":429,"detail":"too many requests; slow down and retry"}
```

Rate limiting is not authentication either. It bounds how fast something can be done, not who
may do it, and the bucket it counts against is a network address rather than a principal.

### A refusal to start

The refusals are real, and the cheapest one to provoke is a bind other hosts can reach, with no
token and nothing said about where TLS is terminated:

```bash
SUTURA__SERVER__HOST=0.0.0.0 \
SUTURA__CATALOG__DIR=$E/catalog \
SUTURA__CATALOG__DATA_DIR=$E/data \
  cargo run -p sutura-serve
```

```
sutura-serve: this configuration is not fit to serve:
  - server.host is 0.0.0.0:8080, which is reachable from other hosts, and security.tls_termination is `none`. Say where TLS is terminated - one of: sidecar, ingress, in-process - or bind 127.0.0.1. The declaration does not encrypt anything: it records which cleartext hop this bearer token crosses, which is a fact only this deployment knows
  - this service is bound where other hosts can reach it, so security.access_token must be set. It authenticates the DEPLOYMENT and not the caller: sutura has no per-caller identity, so every query still runs with whatever access this process already had
```

The process exits `1` and never binds, and both refusals are reported at once rather than one
per restart - the difference between one fix and a fix-and-restart loop. Production with no
token is the same shape with one entry:

```
sutura-serve: this configuration is not fit to serve:
  - this is a production deployment, so security.access_token must be set. It authenticates the DEPLOYMENT and not the caller: sutura has no per-caller identity, so every query still runs with whatever access this process already had
```

A misspelled key is a refusal too, because a key that is silently ignored is a default the
operator believes they overrode:

```
sutura-serve: the configuration sources could not be read
  caused by: unknown field `portt`, expected one of `host`, `port`, `request_timeout_seconds`, `max_body_bytes`, `tls_certificate`, `tls_key` for key `server`
```

Each of these is a refusal rather than a warning on purpose. A warning is read by whoever
happens to be looking at the log, in the format the collector was configured for. A process
that does not start is read by everybody.

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

The same directory is an integration test, and there is no second copy of it:

```bash
cargo test -p sutura-cli --test example
```

It loads the catalog, pins the digest, re-runs every anchor, runs the whole corpus and
snapshots the generated SQL and whatever came back - rows, a refusal reason, or the error chain
of the one question that fails. That is what stops the commands above from rotting: an edit that
changes what this example does shows up as a snapshot diff to review rather than as a README that
used to be true.

Two of its assertions are not snapshots and are the reason a case cannot quietly leave. The
`refused-` prefix is read as a convention in both directions, so a refusal fixture that started
answering and a plain question that started being refused are each a failure rather than a passing
corpus. And the measure vocabulary is asserted as four exact sets - shapes, terms, the terms a
ratio holds, and the aggregates - so this section's table cannot claim coverage the catalog has
stopped carrying.

That covers the command-line half. The serving session above is pinned in two pieces rather
than by a third test. The numbers, the digest and the refusal reasons are the values this test
snapshots already, because the HTTP surface asks the same question of the same bundle - and the
envelope around them is asserted against the real router, in-process and with no socket, by the
harness in `crates/sutura-http/src/harness.rs`: a refusal is a `200` carrying
`outcome: refusal`, a missing token is a `401` carrying `code: unauthorized`, a body holding
`sql` is a `400` naming the field, `/health` is those fifteen bytes exactly, and the interface
description is served in development and not in production.

What neither pins is the JSON *formatting* of the blocks above, or the `detail` sentences beside
the codes. Those were captured from a running process and reformatted, not asserted.
