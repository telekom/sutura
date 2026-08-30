---
title: What a BigQuery test runs against
description: The blocking decision the BigQuery adapter step was gated on - three options priced against what each one can vouch for, why an emulator is refused outright, why the acceptance leg runs on a developer's own project and not in CI, and the consequence that the corpus for this dialect claims rendering and parse-checking and never acceptance, with the one gap that measurement exposed in the parse check itself.
---

# What a BigQuery test runs against

Status: **accepted.** The rendering half is built and green; the acceptance leg is opt-in and is not
in CI, because there is nothing in CI for it to run against.

`docs/implementation-plan-bigquery.md` gates its first BigQuery step on one decision, to be made
before the adapter is written: **what does a test run against?** There is no BigQuery in a container.
The choice decides whether the step can claim *acceptance* at all or only *rendering*, and
[0007](0007-federating-across-different-data-systems.md) already says plainly that `parse-checked` is
narrower than accepted.

This record makes the decision and records what it costs, so it is not implied by whatever the first
test happened to do.

## The three options, priced

### An emulator that speaks the API - REFUSED, and not on cost

Something that answers the job API without being GoogleSQL would give a green corpus over statements
the real service may reject, which is **the exact failure `differential.rs` exists to prevent**: that
test runs one plan through the engine and through a real DuckDB and compares rows, because agreement
between two things that are both wrong is not evidence.

This is the option to refuse first and hardest, because it is the one that produces the most
confident-looking green. It is refused on principle rather than on effort: an acceptance test whose
subject is not the system whose acceptance is in question is not an acceptance test.

### A real project in CI, with a service-account secret - REFUSED for this step

It would genuinely vouch for acceptance. What it costs:

- **This repository is public.** A workflow secret is not available to a pull request from a fork, so
  the check would be absent on exactly the contributions least likely to have been run locally - the
  shape of gate that is worse than none, because its green means "nobody could run it".
- **Every CI run acquires a dependency on a cloud project**, its billing, its IAM and its
  availability. A gate that fails for a reason outside the diff gets disabled, and this file's own
  guidance says so.
- **It needs a project.** Which project, and who pays, is a decision with an owner outside this
  repository.

Not rejected forever: if an organisation-level credential and an owner appear, this becomes the best
option and the acceptance leg below is the thing that gets pointed at it. What is rejected is
*blocking this step* on that.

### The Storage Read API against a fixture table - REFUSED, wrong instrument

It reads a table without running our SQL. The thing in doubt is whether **the statement we generate
is accepted and means what we think**, and a path that never submits the statement cannot answer it.
It would test the transport and call it acceptance.

## The decision

**A real project, reachable from a developer's own machine, with the acceptance leg marked
not-in-CI.**

What makes it reachable today is that `just gcloud-login` already lands: it performs both logins that
matter, writes nothing into this repository, and its own documentation states that it serves
`SharedServiceUser` and cannot become impersonation. That is exactly the posture the adapter step
declares, so the step can be built and demonstrated with nothing more.

**Which project, dataset and location a developer works against is not written down here and will not
be.** A developer names it in their own environment, for the reason the plan page gives: this
repository is public, so the value belongs on the machine and only the hook belongs here.

And the *fields* for two of those are not in this repository for the same reason the value is not:
`SourcePlacement::BigQuery` declares a billing project and a dataset and nothing else. A dataset's own
project is unrepresentable - an omitted `defaultDataset.projectId` resolves in the request's (billing)
project, so a dataset owned by a different project than the payer cannot be declared - and neither is
`location`, which the result-paging call needs for a dataset outside the two multi-regions. Stated
rather than added because no transport consumes them; the change that adds the wire is the one that
decides the fields.

## What the corpus therefore claims, exactly

**Rendering and parse-checking. Not acceptance.** In CI, for this dialect:

- the 21 questions render, and their statements and parameters are pinned as goldens reviewed as a
  diff;
- every statement is parsed with the BigQuery target;
- no value from a question reaches a statement as text, and every identifier is quoted - both
  asserted with the dialect's own quote character, which is a backtick;
- `LIMIT 10001` is present, which is the row cap's SQL leg.

**DuckDB remains the only data system that vouches for acceptance in CI**, through the anchors and
through `differential.rs`. Postgres, ClickHouse and now BigQuery are rendered and parse-checked and
nothing more. The *Invariants* row in AGENTS.md says this, and now says how much narrower.

## The gap the measurement exposed, which is the part worth carrying

Writing this dialect turned "parse-checked is narrower than accepted" from a hedge into a measured
number, and the finding is sharper than expected: **within one target, the parse check cannot see a
function's argument order.**

BigQuery spells the time bucket `DATE_TRUNC(<date>, MONTH)` - the date first, the granularity a bare
keyword - where the other three take `DATE_TRUNC('month', <date>)`. Both shapes, rendered for
BigQuery, **parse cleanly as BigQuery.** So a generator arm that got this wrong would render, parse,
snapshot green, and fail at the service on the first real question.

Three consequences:

1. `sutura_sql::dialect::DateTruncShape` is an exhaustive **declaration** rather than a check, so a
   fifth dialect cannot be added without stating its spelling.
2. `the_parse_check_cannot_tell_the_two_bucket_shapes_apart` keeps the measurement as a test, so the
   claim cannot quietly stop being true - and if the parser ever grows a signature check, that test
   fails and the right response is to re-scope the claim rather than weaken the assertion.
3. The severity is worth stating precisely, because the two failure modes here are not equal.
   Getting the bucket wrong is a **rejection** at the service: the first argument is the value to
   truncate, a string coerces there only if it is a canonical date, and `'month'` is not one. Getting
   the **quote character** wrong is the wrong-number risk, because a double quote opens a string in
   GoogleSQL - `SELECT "amount"` selects the text `amount`. That one happens to be caught in CI, on a
   qualified column, and it is caught by luck about the shape we generate rather than by a guarantee.

## The second thing measurement found, and it is a wrong number rather than a rejection

The bucket's *shape* is above. Its *semantics* hid a second defect, on a grain **no golden covers**,
and it is worth its own section because the corpus could not have found it.

`BigQuery`'s `WEEK` **begins on Sunday** - its own reference says `WEEK` is equivalent to
`WEEK(SUNDAY)`. A real DuckDB 1.5.5, asked directly, answers
`DATE_TRUNC('week', DATE '2026-08-30')` - a Sunday - with **2026-08-24, a Monday**, putting that
Sunday in the previous week. So the obvious keyword mapping, `Week => "WEEK"`, would bucket a
Sunday's rows under a different period on BigQuery than on the data system that vouches for
acceptance. **No error anywhere; a different number under a certified name.**

`BigQuery`'s Monday-based part is `ISOWEEK`, and that is what the generator asks for. The other four
grains need no such care: `DAY`, `MONTH`, `QUARTER` and `YEAR` mean the same thing in every dialect
this repository renders for.

**Why the corpus could not catch it, and what holds it instead.** The example questions ask only `day`
and `month`, so no snapshot renders a week bucket at all - which is precisely the kind of gap a golden
suite cannot report, because a missing case looks exactly like a passing one. What holds it is a test
pinning the keyword by value, plus the reasoning written at the match arm.

**The limit, because this claim is narrower than it sounds:** what was compared is BigQuery's
documented behaviour against a *measured* DuckDB. Whether `ClickHouse`'s week agrees with DuckDB's is a
**pre-existing** question that this arm does not touch and that no test in this repository answers.
Flagging it rather than fixing it, because the three-dialect renderings are unchanged here and a
silent change to one of them belongs in its own diff.

## The acceptance leg, when somebody runs it

It is **not built in this change**, and the reason is the same decision applied to itself: the adapter
in this repository has no transport that speaks to the endpoint, because nothing here can verify one.
`sutura_exec_bigquery::transport::JobTransport` is the seam it arrives at, and the change that
implements it is the change that can first run it against a project - which is where the dependency
decision (an HTTP stack and a credential library, in a workspace that cross-builds to musl and holds
an exact licence allowlist) belongs.

When it lands, what it has to assert is what would actually be new information:

- the corpus's statements are **accepted** and return rows, which is the claim CI cannot make;
- the rows agree with the engine's for the same plan, which is `differential.rs`'s shape pointed at a
  second data source;
- the bucket is right, which is the one thing above that no local check reaches;
- the result is the endpoint's **complete** answer and not the first page - a job `jobComplete` with
  `totalRows` equalling the delivered count, which is what the seam's `JobRows::of` now forces a
  transport to vouch for rather than leaving completeness to the shape of a page.

And it stays out of `just validate`, because a gate that needs a cloud project is a gate that fails
for an environment reason on somebody else's machine.

## Consequences

- The `data_systems:` axis of `crates/sutura-app/tests/adapters/mod.rs` gains **no** BigQuery entry.
  That registry's own rule is that an entry is something somebody could deploy, and a cell that cannot
  execute reads as coverage. The **dialect** axis gains one, unconditionally.
- A developer who wants to try it against their own project needs the transport, which does not exist
  yet. Until then the honest summary of BigQuery support in this repository is: *the statement is
  right as far as four mechanisms can tell, and nobody has run one.* **That sentence is
  superseded - see the amendment above: somebody has now run one.**

## Amendment, 2026-08-30: acceptance IS in CI now, and the reason this record gave is what changed

**Status of the amendment: accepted.** The decision above stands as reasoning and its *conclusion* is
superseded in one specific way. This section says which, because a merged record that quietly stops
being true is worse than one that was wrong from the start.

### What this record refused, and on what grounds

*A real project in CI, with a service-account secret* was refused, and the FIRST of its three stated
costs was the deciding one:

> **This repository is public.** A workflow secret is not available to a pull request from a fork, so
> the check would be absent on exactly the contributions least likely to have been run locally - the
> shape of gate that is worse than none, because its green means "nobody could run it".

**That cost is answered by a mechanism this record did not consider: a GitHub *environment*.** An
environment's secrets are not exposed to a `pull_request` run from a fork at all - the job cannot
start with them - so the failure mode is not "a fork sees the secret" but "a fork's run skips". Which
is the honest outcome for a runner that had no choice, and is this repository's own rule about skipping
versus failing.

The record's own words left this open: *"Not rejected forever: if an organisation-level credential and
an owner appear, this becomes the best option."* A credential and an owner appeared.

### What is now in CI

`.github/workflows/ci.yml`'s `bigquery-acceptance` job. Its own job, so a failure is attributable and
the rest of CI does not wait on a cloud call. It names environment `bq-test`, writes the key from that
environment's secret to a path under `$RUNNER_TEMP` - **outside the checkout** - points
`GOOGLE_APPLICATION_CREDENTIALS` at it, runs `nix run .#bigquery-acceptance`, and removes it in an
`if: always()` step.

**Three things about the credential's path, because two of them cost real failures:**

- It is never echoed, never interpolated into a command line, and never passed as an argument.
  `printenv` writes it and `umask 077` precedes the write. A multi-line JSON key is exactly the shape
  that defeats naive log masking, so nothing relies on masking.
- **Outside the checkout is not tidiness.** The secret sweep scans the WORKING TREE and does not honour
  `.gitignore`, so a key inside the repository fails `just secrets` - measured, three leaks - and that
  blocks the push stage for everybody, not only the person who put it there.
- The billing project is read from the key's own `project_id`, so no project variable is configured.
  The dataset and table come from environment `vars`; all three fail loudly when unset rather than
  skipping.

**And the three RESOURCE names are masked, which is a second disclosure channel that the credential
handling above does not cover and that this record nearly missed.** Workflow logs on a public
repository are public. The wire carries the endpoint's own `message` on a refusal - bounded,
deliberately, because a `400 invalidQuery` with only a reason code proved undiagnosable - and that
message quotes what it refused, as `project:dataset.table`. So a **failing** acceptance run would have
printed a project id, a dataset and a table name into a log anybody can read, which is precisely the
class *This Repository Is Public* says not to disclose. The job therefore emits `::add-mask::` for all
three before the leg runs: the project id read out of the key, and the dataset and table from the
environment's variables. `::add-mask::` is the right mechanism because it does not care where a value
appears - it redacts every later log line in the job, a panicking test's own message included.

**The cost is stated rather than hidden:** in CI the diagnostic that made carrying the message worth
while is redacted away. A maintainer who needs the unredacted text runs the leg locally, where the log
is theirs. That is the correct direction for a public repository and it is a real loss.

**Where the fixture's location lives, and where it does not.** The dataset and the table are GitHub
**environment variables** on `bq-test` - scoped to the one environment, not repository-wide - and the
billing project is read from the key. None of the three is written into this repository, which is the
same split `.envrc` already uses for a developer's machine: the hook is in the repository and the value
is not. **A missing variable fails the job with a message naming it**, rather than skipping, because
reaching this job at all means somebody configured the environment.

**It is an app and not a `checks.*` output, and that is forced rather than chosen:** nix checks run in
a sandbox with no network, so acceptance could not be a check even with a credential. `just validate`
therefore still does not cover this, and the *Consequences* below still hold on that point.

### What the leg now claims, exactly - and the word that still does not belong

**A statement this repository generated was accepted by `BigQuery` on 2026-08-30, answered as one
complete page, and its numbers were the fixture's.** That is the first time anything here has had a
statement accepted by that service, and it happened from a developer's machine before the CI job
existed. The three tests are: a dry run accepted, a real run whose two bucketed sums are 42 and 99
against a four-row fixture that discriminates, and a negative control - a table the dataset does not
hold, refused rather than panicking.

**The word "corpus" still does not belong near it.** This is ONE hand-built `SUM` over a two-column
table: no join, no `COUNT(DISTINCT`, no `CASE WHEN`, no `NULLIF` ratio, no `CAST(... AS FLOAT64)` and
no `ISOWEEK` - and `ISOWEEK` plus `DATE_TRUNC`'s argument order are precisely the two things the
*measurement* sections below identified as invisible to a parse check. So the four acceptance bullets
this record wrote are answered as: the statement is accepted (for one statement), the answer is proved
complete, and **the rows are NOT compared against the engine's** and the bucket is checked only for
`MONTH`. The corpus-wide leg is #78's importer shape pointed at a dataset, and it is not built.

**And identity is untouched.** A service-account key is `SharedServiceUser` - one identity for
everybody who asks - so this establishes *accepted, and correct for that identity*, and says nothing
whatever about per-subject execution. `BigQueryWarehouse::IMPERSONATION` still reads
`NoPlaceForASubject`.

### One finding the live run produced that no local check could have

The first submission came back `400 invalidQuery`: *"Cannot access field day on a value with type
INT64"*. The cause was in the FIXTURE rather than the generator - the plan's metric label was the same
word as the table name, and `GoogleSQL` resolved the qualifier to the select-list alias instead of the
table. Worth recording twice over:

1. **`Definitions::assemble` refuses a dimension named after its metric, and nothing refuses a metric
   label equal to the TABLE name.** On this dialect that produces a statement the service rejects.
   Flagged rather than fixed here, because it is a domain change and this record is not the place.
2. **It is why the endpoint's `message` is now carried on the refusal, bounded.** It had been dropped
   deliberately - *what is not read cannot be logged by accident* - and a status plus a reason code
   that together say *your SQL is wrong* turned out to be undiagnosable. The bound answers the original
   concern; dropping the field answered it by removing the diagnostic too.

## What has happened since, and the one sentence above that stopped being true

**The transport exists.** [0018](0018-what-the-bigquery-wire-is-built-from.md) is the dependency
decision this record deferred, and `sutura_exec_bigquery::wire` is what came of it: `jobs.query` over
a blocking HTTP client already resolved in `Cargo.lock`, behind a default-off feature, with the
acceptance leg written as `crates/sutura-exec-bigquery/tests/acceptance.rs` and reached by
`just bigquery-acceptance`.

So the last bullet above is corrected rather than left standing: a developer who wants to try it now
has the transport and needs only `just gcloud-login` and three values in their own environment.

**But what they would be running is NOT the acceptance leg this page specifies, and that is worth
being blunt about.** The four bullets under *The acceptance leg, when somebody runs it* ask for the
corpus's statements accepted, the rows agreeing with the engine's, the bucket checked, and the answer
proved complete. `tests/acceptance.rs` delivers the fourth and a single instance of the first: one
hand-built `SUM` over a two-column table a developer supplies. It contains no join, no
`COUNT(DISTINCT`, no `CASE WHEN`, no `NULLIF` ratio, no `CAST(... AS FLOAT64)` and no `ISOWEEK` - and
`ISOWEEK` and `DATE_TRUNC`'s argument order are exactly the two things this page MEASURED the parse
check to be blind about, so they are what a live run is worth most for. **The leg this page specifies
is #78's importer shape pointed at a dataset - load the fixtures, run the 21 questions, compare rows
with the engine - and it is not built.** `docs/adr/0018` records that gap, and the smoke leg's own
header opens with it.

**Everything else on this page still holds, including the part that matters most.** This record said
the change adding the wire would be the change that could first run it against a project. **It was
not.** The machine it was written on has no `gcloud`, no application-default credential and no
project, so nothing has been executed against a real dataset, the acceptance leg has never run, and
the `data_systems:` axis still gains no entry. The summary sentence therefore survives with one word
changed - *five* mechanisms rather than four, the fifth being the wire's own suite over response
documents that are not the service's - and 0018's *What is still not claimed* section is where that
is spelled out.

## Second amendment, 2026-08-30: the corpus leg is built, and two of the four bullets are answered

**Status of the amendment: accepted.** The first amendment above ended with a sentence that has stopped
being true, and it named itself as the thing to watch:

> **The leg this page specifies is #78's importer shape pointed at a dataset - load the fixtures, run
> the 21 questions, compare rows with the engine - and it is not built.**

It is built. `crates/sutura-exec-bigquery/tests/corpus.rs`, three `#[ignore]`d tests behind the same
`just bigquery-acceptance` the smoke leg uses.

### Which of the four bullets, exactly

| Bullet | Where it stands |
| --- | --- |
| the corpus's statements are **accepted** and return rows | **Answered**, and in two halves for a cost reason stated below: every question that compiles to a plan is put to the endpoint as a **dry run**, which is free, and separately **executed** by the row comparison |
| the rows **agree with the engine's** for the same plan | **Answered**, row for row, with ONE stated exclusion below |
| the **bucket** is right | **Answered for `MONTH`, `DAY` and `ISOWEEK`**, which is every grain the corpus asks. `QUARTER` and `YEAR` are still rendered and never executed anywhere |
| the result is the endpoint's **complete** answer | Already answered by the smoke leg, and answered again here on every question: the seam's `Incomplete` refusal not firing is the evidence |

**And one claim that is not on the list and is the strongest of the four.** The corpus leg also asks
the endpoint to reproduce every ANCHOR the engine reproduces. An anchor is a number somebody
*certified*, written in the catalog and compared as rendered text at the metric's coarsest grain, so
`verify_anchors` reaching the same verdict on both sides is a claim about the definition still meaning
what its author said - which is a different and larger thing from two adapters agreeing.

### What that reaches that no local check could

The corpus renders, for this dialect: 12 `LEFT JOIN`s, 6 `COUNT(DISTINCT`, 4 `CASE WHEN`, 4 `NULLIF`
ratios, one `avg`, 90 `CAST`s and **3 `ISOWEEK` buckets**. `ISOWEEK` and `DATE_TRUNC`'s argument order
are precisely the two constructs the *measurement* sections above identified as invisible to a parse
check - so this leg is where those two stop resting on a keyword pinned by value plus reasoning at a
match arm.

**The section above that said the corpus asks only `day` and `month` was true when it was written and
is not now**: `data-per-subscription-by-week` is in the corpus and renders `ISOWEEK` three times. That
section stays as the record of why the keyword is pinned by value, and this paragraph is the correction
to its factual claim.

### The one question excluded from the row comparison, and why it is not a hole

`revenue-per-churned-subscription-january`, excluded from the *comparison* and not from the run: it
executes, and **both sides are required to fail, each against its own expected reason.** The metric
declares `zero_denominator: fails`, so the generator emits a bare `/` with no `NULLIF`, and January's
denominator is zero. `DataFusion` and `DuckDB` return `inf`, which the port refuses as a non-finite
value; `GoogleSQL` RAISES on a zero divisor and the endpoint answers `400`. AGENTS.md already stated
that the `zero_denominator: fails` credit belongs to `DuckDB` rather than to this arm - this is where
that stops being a claim about documentation.

A cell that cannot execute reads as coverage, which is this repository's rule, and the reason this is
an exclusion with a named literal in the source rather than a `continue` on a condition: `DIVIDES_BY_ZERO`
is one constant a reviewer can grep, and the test asserts that exactly one question reaches it.

### The importer, and the two decisions in it worth a record

#78's `PostgresWarehouse::load_csv` infers a column type per column from the committed bytes and
renders `CREATE TABLE` plus `COPY ... FROM STDIN`. The first half transfers; the second does not.

1. **The rows travel inside the statement.** There is no `COPY` here, and handing the endpoint a CSV
   body beside a statement needs either a load job or `tabledata.insertAll` - each a second wire
   surface nobody has run, which is this record's own reasoning about verification applied again. So
   the load is ONE `CREATE OR REPLACE TABLE ... AS SELECT * FROM UNNEST([STRUCT ...])` per table:
   atomic in the way `CREATE OR REPLACE` is, idempotent against the last run, and with no state where
   the table exists empty.

2. **Because the rows are in the statement, the loader REFUSES rather than escapes.** A fixture CSV is
   a document read off disk, which this repository treats as untrusted input. Every cell is
   re-rendered from a value the importer parsed - an `i64`, a finite `f64`, one of two boolean
   keywords, a shape-checked ISO date - and the one arm with no parse to hide behind refuses: text
   outside `[A-Za-z0-9 _.-]` is a typed refusal naming a position, never a quoted literal. So
   `O'Brien` and `'); DROP TABLE x --` are *unrepresentable* in a rendered statement rather than
   escaped into one. That is *prefer unrepresentable to checked* at the position where this loader
   would be the one to break the no-injection property.

**`transport::JobTransport` gains a third method, `apply`, behind the new `fixtures` feature - so the
shipped port stays at the two questions this record's own reasoning gave it.** It cannot be `run`:
that method's contract is a COMPLETE result set, and the wire refuses an answer whose `totalRows` does
not equal the delivered count. A `CREATE OR REPLACE TABLE` job has no result set for that check to be
about, so routing DDL through `run` would rest on a response shape nobody here has measured - in the
one place where a wrong guess is a silently half-loaded fixture rather than a visible failure. `apply`
requires `jobComplete` and requires nothing else.

**`fixtures` is default-off and, unlike `wire`, not for a dependency - it adds none.** It is that this
feature is the only thing in the crate that can issue `CREATE OR REPLACE TABLE`, and a build that
serves questions has no business holding one.

### What one run costs

**Under a cent, and the number is set by the count of BILLED JOBS rather than by the size of the
fixtures.** On-demand billing has a 10 MiB minimum per table referenced per query and the whole
fixture set is 40 KB, so what a run spends is decided by how many jobs really read data. A full run
submits 12 loads, 22 dry runs, ~22 executions and 12 anchor re-runs; **the loads and the dry runs cost
nothing** - a `CREATE TABLE AS SELECT` over a literal array scans nothing, and the endpoint charges
neither slots nor bytes for a dry run - so ~34 jobs are billed, referencing one to three tables each.
That is on the order of half a gibibyte of billed bytes, about a third of a cent at the current
on-demand rate.

**The acceptance half being free is why it is a test of its own** rather than folded into the row
comparison. The first shape of it went through `sutura_app::answer`, which dry-runs *and* executes, so
it doubled the bill for a claim a dry run already makes; asking the compiler for the plan and calling
`dry_run` directly costs nothing and localises the failure - a red there says *not accepted* with no
row comparison in the way.

Every job is still capped by `maximumBytesBilled` at a gibibyte, which is what protects a developer who
points this at a dataset already holding something large under one of the four fixture names.

### The grant this needs and the smoke leg does not

**It creates tables, so reading the dataset is no longer enough.** The credential needs
`bigquery.tables.create`, `bigquery.tables.updateData` and `bigquery.tables.delete` in the target
dataset - `roles/bigquery.dataEditor` on the dataset is the usual way to say that - on top of the
`roles/bigquery.jobUser` needed to submit a job at all. [0019](0019-a-table-outside-the-connections-dataset.md)
records that the acceptance credential's IAM refuses `datasets.create`; creating a TABLE inside a
dataset it already reads is a different grant. A run whose credential lacks it fails at the first load
with the endpoint's own `accessDenied` on the chain, which is a diagnosable failure rather than a
mysterious one - and it is a **configuration** change on the `bq-test` environment's service account
rather than anything this repository can carry.

### What this leg still does not claim, and one new limit it introduces

- **Identity is untouched.** A service-account key and an application-default login are each one
  identity for everybody who asks. `BigQueryWarehouse::IMPERSONATION` still reads
  `NoPlaceForASubject`, so a green here is *accepted, and correct for that identity*.
- **The `data_systems:` axis of `crates/sutura-app/tests/adapters/mod.rs` still gains no entry**, and
  the *Consequences* below still hold on that point: a cell in that registry runs inside `just test`,
  and this cannot, because the nix sandbox has no network. The corpus leg is a second `#[ignore]`d
  integration target in the adapter's own crate, reached by the same app.
- **No composition root links the crate**, and `sutura-serve` refuses `kind: bigquery` by name.
- **NEW: this leg WRITES, and two runs against one dataset will race.** Four tables named after the
  example models - `dim_customer`, `dim_product`, `fct_subscription_monthly` and `fct_usage_daily` -
  are replaced on every run. The names are fixed rather than suffixed because the generator renders a
  model's table unqualified and the job's `defaultDataset` resolves it, so a per-run name would need
  the catalog to change. The dataset this is pointed at should therefore hold nothing else under those
  names, and a second concurrent job in the same dataset is a defect nothing here prevents. The names
  themselves are committed fixtures rather than resources, so unlike the project and the dataset they
  need no `::add-mask::` in a public log.
