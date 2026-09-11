---
title: What a BigQuery test runs against
description: The blocking decision the BigQuery adapter step was gated on - three options priced against what each one can vouch for, why an emulator is refused outright, why the acceptance leg runs on a developer's own project and not in CI, and the consequence that the corpus for this dialect claims rendering and parse-checking and never acceptance, with the one gap that measurement exposed in the parse check itself.
---

# What a BigQuery test runs against

Status: **accepted, and amended repeatedly - the amendments below carry the current state.** The
rendering half is built and green. The acceptance leg is no longer opt-in-only and no longer absent
from CI - see *Amendment, 2026-08-30* - and the adapter is no longer unwired: a composition root
links it, behind a default-off feature, which is the *Second amendment, 2026-08-30* below. **Read
the amendments before citing anything above them**; the reasoning stands, several of its conclusions
do not.

**The ordinals were renumbered on 2026-09-06, and a citation older than that may name a different
section than it did.** Three branches appended here in two days and the sequence drifted into two
*Fifth* amendments - the second after the *Sixth* - plus an unnumbered one mid-list. The headings now
run consecutively in file order, and `cargo xtask check-guidance` holds that rather than a reviewer:
`xtask/src/guidance/pages.rs` is the rule and states what it does not reach, which is chronology and
every cross-reference outside this page.

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

**Corrected: two of the three are masked from their SECOND occurrence, not their first, and that is a
property of the runner rather than of this job.** The runner prints a step's resolved `env:` block into
that step's own `Run` group before it executes the body, so the dataset and the table - which reach the
masking step through its `env:` - are in the log once, in cleartext, before the `::add-mask::` lines
run. The project id is not, and the difference is the lesson: it never becomes an expression, because
only the key's *path* goes through `env:` and `python3` reads the value out of the file. This cannot be
repaired by masking earlier: the printed dictionary is assembled from the `env` context, which carries
job-level and workflow-level entries too, so hoisting the values prints them in every step's group; and
an expression read inside a `run:` body is echoed with the expression already resolved. **The one thing
that closes it is making those two environment secrets rather than variables** - the runner adds every
secret to its masker before the first step runs. That is a forge change, no file in this repository can
make it, and no gate in this repository can see whether it has been made.

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

**This record said the change adding the wire would be the change that could first run it against a
project. It was not** - the machine it was written on had no `gcloud`, no application-default
credential and no project, so the diff that wrote the wire executed nothing. **A LATER diff did, and
the two sentences that followed this one here have been deleted rather than hedged:** they read
*"nothing has been executed against a real dataset, the acceptance leg has never run"*, and
*Amendment, 2026-08-30* above records the run that made both false. What survives unchanged is the
narrowness: the `data_systems:` axis still gains no entry, because one live statement is not a
registered data system. The summary sentence therefore survives with one word
changed - *five* mechanisms rather than four, the fifth being the wire's own suite over response
documents that are not the service's - and 0018's *What is still not claimed* section is where that
is spelled out.

## Second amendment, 2026-08-30: the adapter is registered, so the last *not wired* is spent

**Status of the amendment: accepted.** Nothing above is contradicted by this section - it closes the
one thing every version of this page, and `AGENTS.md`'s *Built And Not Wired*, kept naming as absent:
*no composition root links the crate, and `sutura-serve` refuses `kind: bigquery` by name.* Both
sentences are now false, and this records what replaced them so a reader does not have to infer it
from a diff.

### What is wired

`sutura-serve` opens a `kind: bigquery` source. `open_engine` holds the exhaustive match over
`sutura_config::SourceKind` - it MOVED there from `build_engine`, because a kind whose answer is a
different registry can only be dispatched by the function that chooses one - and `build_bigquery`
composes the three layers the acceptance leg composes, in the same order and through the same
constructors: a `WireAgent::pinned` carrying `JobBounds`, a `Credential` read off a declared file, a
`BigQueryWire` over both, and a `BigQueryWarehouse` over that.

Two values the source entry did not carry before, both required for that kind and both refused on a
`files` one:

- **`credential_file`**, absolute. Required rather than falling back to `CredentialFile::well_known`,
  which is what the acceptance leg uses: a test may resolve a credential from whichever of three
  Google variables happened to be exported, and a *service* may not - that is an identity nobody
  declared. It is read at BOOT, for the reason the inbound key set is read before the listener opens.
- **`max_bytes_billed`**, with no default. It is the only bound on bytes SCANNED anywhere in this
  repository and the only number in the settings tree that spends money, so both safe defaults are
  wrong in opposite directions: a small one refuses ordinary questions on a large table and a large one
  is indistinguishable from no bound. Its RANGE stays the adapter's - `BytesBilledCeiling::parse` - so
  there is one parse of it and a value outside the range is a startup refusal naming the key.

The **query deadline** is not a new key: it is filled from `server.request_timeout_seconds`, which is
what `wire::QueryDeadline`'s own documentation asks a composition root for by name. A job that outlives
the request it is answering is billed for a result nobody is waiting for.

### Which binaries link it, and the one that deliberately does not

**`sutura-serve`, behind a default-off `bigquery` feature. `sutura-cli`, not at all.**
*(The second half is SUPERSEDED by the seventh amendment below: `sutura-cli` has a default-off
`bigquery` feature of its own since telekom/sutura#121. The reason given below is why it took a
feature rather than a plain dependency, and that half still holds.)* The reason is
the four cross builds and it is measured rather than assumed:

- the release derivations pass `--package sutura-cli`, so they never compile `sutura-serve` itself;
- but `crane.buildDepsOnly` is deliberately **unscoped** - the flake says so, because the checks share
  that derivation - so a non-optional dependency in `sutura-serve` would compile `ureq`, rustls and
  `ring` for all four cross dependency derivations, two of which are musl, for a binary that links none
  of it.

So the feature is the honest shape, and the refusal that used to name the KIND now names the FEATURE:
`open_bigquery` has two definitions of one signature, and the `cfg(not(feature = "bigquery"))` one
tells an operator to build with `--features bigquery` rather than to change the `kind:`. Every gate
here passes `--all-features`, so the registration is compiled, linted and tested on every run; a bare
`cargo build` and the four cross builds are unchanged.

**What that means for a shipped artifact, stated plainly because a feature is easy to read as
availability:** no published binary opens a `BigQuery` dataset today. The image holds `sutura-cli`,
which links the engine only; `sutura-serve` is not a `nix` package at all and is built from source.

### Two limits this registration introduces, both refusals rather than silences

1. **A catalog reading two KINDS of source does not start.** `sutura_app::Warehouses<W>` is generic in
   one adapter type, so this process holds two file sources or two datasets and cannot hold one of
   each; the closed enum over adapter types is an architecture decision that module already defers.
   `one_kind` makes the limit a startup refusal naming both entries and both kinds, because the
   alternative is a source nothing opened answering `SourceUnavailable` - a refusal that reads as
   *nobody configured that* about a source the operator configured.
2. **`refuse_unattached` is files-only now.** It compares the served bundle's tables against what the
   engine holds, and the engine holds them because `attach` put them there. A `BigQuery` source has no
   attach step, so a bundle naming a table the dataset does not hold STARTS, where a `files` deployment
   in the same state does not. An anchor closes it for a metric that matters, because an anchor
   re-executes at boot; closing it for the rest is a per-model pre-flight, which is a network call per
   model rather than a check on a set.

### What is still not claimed

Everything the amendment above narrows stays narrow. The `data_systems:` axis of the golden matrix
gains no entry: the registration is a composition, and that registry's rule is that a cell which
cannot execute in the suite reads as coverage. `IMPERSONATION` still reads `NoPlaceForASubject`, so a
`BigQuery` source serves every caller as one service account and an `impersonation-at-source`
declaration against it is a boot refusal - which is the posture cross-check working, not per-subject
execution arriving. And the corpus-wide leg this page specifies - #78's importer shape
pointed at a dataset - was still not built when this amendment was written. **It is now**, and the
third amendment below is its record; this sentence is left as the pointer rather than deleted,
because a reader arriving at an amendment wants to know which of its limits a later one spent.

## Third amendment, 2026-08-31: the corpus leg is built, and two of the four bullets are answered

**Status of the amendment: accepted.** The first amendment above ended with a sentence that has stopped
being true, and it named itself as the thing to watch:

> **The leg this page specifies is #78's importer shape pointed at a dataset - load the fixtures, run
> the 21 questions, compare rows with the engine - and it is not built.**

It is built. `crates/sutura-exec-bigquery/tests/corpus.rs`, three `#[ignore]`d tests behind the same
`just bigquery-acceptance` the smoke leg uses. **And it has RUN, green, in CI on 2026-08-31** - the
`bigquery-acceptance` job, 8 tests passed, five of them the smoke leg's and three this one's, against
the `bq-test` environment's real dataset. So the sentence above is superseded by a measurement rather
than by an intention.

### Which of the four bullets, exactly

| Bullet | Where it stands |
| --- | --- |
| the corpus's statements are **accepted** and return rows | **Answered**, and in two halves for a cost reason stated below: every question that compiles to a plan is put to the endpoint as a **dry run**, which is free, and separately **executed** by the row comparison. Measured: **22 accepted, 9 refused by the compiler before a statement existed** |
| the rows **agree with the engine's** for the same plan | **Answered** for CONTENT - *exactly* in the sense the **eleventh amendment** narrows, which is rendered content and not cell type - with one stated exclusion below. For ORDER, answered with **one measured divergence** the first run found - see *the finding* below. Measured on this amendment's run: **16 agreed exactly, 5 agreed on content and differed on null placement**. The divergence is **closed** by the fourth amendment, and the leg now compares order exactly |
| the **bucket** is right | **Answered for `MONTH`, `DAY` and `ISOWEEK`**, which is every grain the corpus asks. `QUARTER` and `YEAR` are still rendered and never executed anywhere |
| the result is the endpoint's **complete** answer | Already answered by the smoke leg, and answered again here on every question: the seam's `Incomplete` refusal not firing is the evidence |
| *(not one of the four)* the **anchors** hold | Measured: **6 anchors reproduced by the endpoint**, the same verdict the engine reaches |

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

### The finding, which is what having the leg was for

**`ORDER BY x` does not say where a null goes, and the two sides disagree.** `DataFusion` orders nulls
LAST; `GoogleSQL` orders them FIRST. The example corpus reaches it because
`fct_subscription_monthly` holds a `customer_key` with no row in `dim_customer`, so every question
grouping by a dimension behind that `LEFT JOIN` comes back with one null-dimension row - and the two
data systems put that row at opposite ends.

**Measured, on the leg's first two CI runs.** The first went red on
`recurring-revenue-by-region-and-family`: nineteen rows, identical contents, one of them moved from
last to first. With the divergence pinned, the second run gives the full tally and it is **five
questions rather than one** - `recurring-revenue-by-region-and-family`, `recurring-revenue-by-region`,
`recurring-revenue-by-segment`, `revenue-per-customer-by-segment` and
`subscription-months-by-region-and-term`, the last at 61 rows:

```text
bigquery-corpus: 16 answers agreed exactly, 5 agreed on content and differed on NULL
                 placement, 9 refusals agreed, 1 excluded, 31 in the corpus
```

That is a fifth of the corpus, which is worth knowing before deciding the fix is cosmetic.

**What it is, precisely.** No number is wrong: the row CONTENT is identical on both sides. What differs
is the order of rows in a certified answer, which the plan does claim, because it emits `ORDER BY`. So
it is the class this record already names as the thing a parse check cannot see - *a rendered statement
that is valid SQL with different semantics* - and it is invisible to every local check for a sharper
reason than `ISOWEEK` was: a golden pins the statement TEXT, and **the text is identical on both
sides**. There is no arm to get wrong. Only two data systems executing it can disagree.

**Why it was pinned rather than fixed in the same diff.** The fix belongs to `sutura-sql`: state the
placement, which matches the engine's own default. That rewrites the `BigQuery` SQL goldens and is a
change with its own review; the branch that found it owns the acceptance leg. So the divergence was
pinned in a way that could not rot: `the_corpus_rows_agree_with_the_engine` compared content as a SET
and, where the two orders differed, **required the orders to be identical once the null-bearing rows
were dropped** and required at least one such row to exist. A divergence for any other reason failed; a
content difference failed; and this one failed on the day the generator stated the placement - at which
point this section and the `NULL_PLACEMENT` constant were to be re-scoped rather than deleted.

**That day has come, and the fourth amendment below is the re-scoping.** This section stays as the
record of the finding and of the instrument that caught it; what it says about the tolerance is now
history rather than a description of the code. One claim in it is also **corrected** there: *"all four
dialects spell `NULLS LAST`"* is false about the rendered text, and the correction matters because it
is the difference between a claim about behaviour and a claim about bytes.

**Whether `ClickHouse` and `Postgres` agree with the engine here is not answered by anything**, which
is the same shape as this page's existing note about `ClickHouse`'s week. Both are rendered and
parse-checked and neither has ever executed.

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
- **A composition root now links the crate** - the second amendment above - so the sentence that used to
  stand here, *no composition root links the crate and `sutura-serve` refuses `kind: bigquery` by name*,
  is false. What this leg still does not reach is a SHIPPED one: the registration is behind a default-off
  `bigquery` feature on a binary `nix` does not package, and the image and all four cross binaries are
  `sutura-cli`, which links the engine only. So no published artifact opens a dataset, and a green here
  says nothing about one that does.
- **This leg WRITES, and two runs against one dataset will race** - *and, since #119, no longer.*
  Four tables named after the example models - `dim_customer`, `dim_product`,
  `fct_subscription_monthly` and `fct_usage_daily` - used to be replaced on every run under names
  that were fixed because the generator rendered them unqualified and the job's `defaultDataset`
  resolved them. The ninth amendment below is the closure: every table is now suffixed with the
  run's own token, and a per-run table name stays safe to print because only the dataset and the
  project are resources. The committed fixture names themselves need no `::add-mask::` in a public
  log.

## Fourth amendment, 2026-08-31: the finding is fixed, and the leg's tolerance is deleted rather than relaxed

**Status of the amendment: accepted.** The third amendment's *finding* section ends by naming the
condition under which it would be re-scoped - *the day the generator states the placement* - and this is
that re-scoping. `sutura_sql::generate`'s `ordered_nulls_last` wraps every `ORDER BY` expression in the
dialect layer's own `Ordered` node with `nulls_first: Some(false)`, from `generate` and `generate_leg`
alike, so a plan no longer leaves null placement to whichever data system happens to run it.

### The claim, stated as behaviour rather than as bytes

The third amendment wrote that *all four dialects spell `NULLS LAST`*. **That is false about the text,
and the difference is the whole reason this paragraph exists.** The layer renders the keyword only where
it is not already the target's default:

| Dialect | Its own default | What renders |
| --- | --- | --- |
| `BigQuery` | nulls **first** | `ORDER BY x NULLS LAST` - the keyword, because it is the one that would otherwise disagree |
| `DuckDb`, `Postgres`, `ClickHouse` | nulls **last** | a bare `ORDER BY x` - the layer collapses a keyword that changes nothing |

So what converges is the **behaviour** and not the statement, and only one dialect's goldens move.
`every_order_by_states_nulls_last` in `crates/sutura-sql/src/generate.rs` is the measurement that keeps
that honest: it asserts the keyword for `BigQuery` and asserts its **absence** for the other three, so a
future layer version that started spelling it everywhere - or stopped spelling it for `BigQuery` - fails
by name rather than by golden diff. A test asserting "all four render `NULLS LAST`" would have been red
the day it was written.

### What moved, and what deliberately did not

- **30 `BigQuery` SQL goldens** gain the keyword: 22 in the main corpus, five leg shapes, and three
  qualified-path statements. Reviewed as a diff, per this repository's rule that a golden is regenerated
  and never typed.
- **No other dialect's golden changes**, which is the table above holding rather than an omission.
- **No plan golden changes.** Null placement is a rendering decision and the serialized `QueryPlan` and
  `LegPlan` do not carry it, so the definition digest is unmoved and no anchor re-certifies.

### The leg's tolerance is deleted, not relaxed

`the_corpus_rows_agree_with_the_engine` now compares CONTENT and ORDER **exactly**, with no tolerance
for either. Gone with it: the `Agreement` enum's second variant, the `diverged_on_null_placement`
counter, the `without_nulls` instrument, the `Row::holds_a_null` flag and the `NULL_PLACEMENT` constant.
The assertion that used to hold the measurement open - *at least one question diverges on null
placement* - is **removed rather than inverted**, because a floor of zero is not a measurement of
anything; what replaces it is the exact order comparison, which fails if the placement ever stops being
stated.

The set comparison stays, ahead of the order comparison, and that is deliberate: *different rows* is a
wrong number and *same rows, different order* is a generator that stopped saying how to sort them. One
assertion for both would report the first as the second.

### Measured, and the prediction is spent

This section was written as a prediction and is now a measurement, which is the only reason it is worth
reading: **the `bigquery-acceptance` job is green on 2026-08-31 with the exact-order comparison in
place**, and the tally is the one predicted to the number:

```text
bigquery-corpus: 21 answers agreed exactly on content AND order, 9 refusals agreed, 1 excluded,
                 31 in the corpus
```

**Read `exactly` as the eleventh amendment narrows it.** The comparison behind this tally was
render-based, so it establishes acceptance, rendered row content and ORDER - and says nothing about
cell type. No number is restated for the typed policy that replaced it.

All five questions the third amendment named now agree **exactly** - `recurring-revenue-by-region-and-family`
at 19 rows, `recurring-revenue-by-region` at 6, `recurring-revenue-by-segment` at 4,
`revenue-per-customer-by-segment` at 4 and `subscription-months-by-region-and-term` at 61 - so the 16
that agreed exactly plus those 5 is the 21 above. 22 statements accepted, 9 refused before rendering and
6 anchors reproduced are unchanged from the third amendment, which is the other half of the claim: this
change moved row ORDER and nothing else.

**What it still does not claim, and it is unchanged by the measurement.** `just validate`'s sandbox has
no network, so the corpus leg compiles and is skipped there - a developer's terminal is not where this
is decided, and the `data_systems:` axis of `crates/sutura-app/tests/adapters/mod.rs` still gains no
entry for the reason the third amendment gives. `ClickHouse` and `Postgres` null placement is still
answered by nothing: both are rendered and parse-checked and neither has ever executed, so their arm of
the table above is read off documented behaviour rather than measured. And identity is untouched.

## Fifth amendment, 2026-08-31: where each identity claim is proven

**Status of the amendment: accepted.** What every earlier amendment here establishes - and it is worth
saying once rather than scattered - is *accepted, and correct for one identity*: a service-account key
is `SharedServiceUser`, so every acceptance run to date proves the product for a single identity and
nothing about who asked. This amendment settles **where each identity claim is proven**, and it is the
decision issue #81 records; it exists only in that conversation until now.

It **amends the first amendment's conclusion** in the same way that amendment amended the original
decision: CI is no longer the place the *two-subject* identity claim is refused from. The first
amendment made room for a service-account key in CI; this one decides that a *second* key, and a genuine
row-grant mechanism at the source, belong there too. What stays refused from CI is the thing no CI
runner can supply - a **workforce** identity, which is a representation of a person and needs single
sign-on. The venue for that is a developer's own machine, one-off, and recorded.

### The decision: four venues, four claims

| Venue | What runs there | What it is allowed to claim |
| --- | --- | --- |
| **CI, every run, every contributor** | **Fakes** at the port (the house rule - ports get fakes, not mocked HTTP) | Every outcome the port can produce, including each refusal |
| **CI, in-repo runs only** | The acceptance leg, on a **service-account key** in an environment secret | `BigQuery` accepts what we generate, and the rows agree with the engine |
| **CI, in-repo runs only** | Two distinct principals against a row access policy | *Two principals, two answers* - the mechanism, not the identity class |
| **A developer's machine, one-off** | **Workforce** impersonation with a real person | That a human subject's own identity reaches the source |

### What each cell required, and which are now true

**The fakes cell is the oldest and still holds.** The wire is exercised against a fake
[`transport::JobTransport`] and the broker against a fake [`StsExchange`] - a fake implements the port,
never a documented HTTP layer, so a test can never assert our own request bytes back to us. Nothing in
this amendment changes it; it is here because a decision about venues starts with the one venue already
settled.

**The acceptance cell is built.** The first amendment's job, the third and fourth amendments' corpus,
all green on 2026-08-31 in the `bq-test` environment. It is the second row of the table.

**The two-principal cell is decided and not built.** What it must show is not that two askers get
*different* answers but that each reads **what it is entitled to read** - a row access policy grants the
two principals differently, and each answer is checked against its principal's own entitlement. It
needs two things neither this repository nor an agent can provision, because both are live-account work
with a human owner:

- a **second service-account key** in the `bq-test` environment, kept apart from the first the way the
  first is kept apart from the tree; and
- a **row-level grant** at the source - a `BigQuery` row access policy - whose per-principal rows are
  known and written down, so "entitled to" is a check against a documented shape and not a comparison
  of two numbers.

Until those exist the cell has no subject: an `#[ignore]`d leg whose credentials no environment holds
would be exactly the scaffolding this record exists not to build, and a row-grant mapping guessed at is
a granted-nothing check. When the two key generations and the policy are in place, the cell lands as a
second identity in the existing `bigquery-acceptance` leg and needs no new job - the venue is already
CI in-repo only.

**The workforce cell is local because it can only be local, and it has never run.** A workforce
identity provider requires a client id and single-sign-on configuration, takes `subject_token_type:
id_token`, and exists to represent a person; no CI runner can supply one at any price, so this is the
one venue that is deliberately not in any gate. **What still blocks it is a question this record must
carry rather than assume:** whether the identity provider will mint an ID token whose audience is a
third party's provider at all - a claim no code and no repository secret can test, and the deciding
unknown for per-subject `BigQuery`. Until a run answers it, the whole workforce path is unproven.

### The service-account key is accepted, and its expiry is written down

Every in-repo acceptance - the corpus and, later, the two-principal cell - runs on a **long-lived
service-account bearer** held as a repository secret. That is accepted as the *for now*, with the cost
named: rotation somebody has to remember, and a credential that leaks if the environment secret does.

**The condition that ends it, stated so it is not left to rot:** when rotation becomes a chore, or when
a second project needs a credential, move the acceptance leg to Workload Identity Federation - GitHub
mints an OIDC JWT under `permissions: id-token: write`, Google STS exchanges it for a short-lived token,
and **there is no key at all**. A workload pool accepts `subject_token_type: jwt` with an attribute
condition pinning the repository. Until then the key is the cost of the evidence, and this paragraph is
its expiry.

**One sentence of that paragraph was wrong when it was merged, and it is corrected here rather than
quietly**, because it was the half a reader would have used as the signal to watch. It said *neither
`id-token` nor a Google auth action appears in any workflow*, and therefore that the moment either did
was the diff to look for. Measured 2026-09-03: `git log -S` dates `id-token: write` in
`.github/workflows/release.yml` to telekom/sutura#97, merged **two hours before this amendment
landed** - the release path mints an OIDC token for Fulcio to sign a certificate against, which is a
keyless-signing exchange with a different audience and no Google in it at all. So the permission is not
the signal and never was; a workflow can hold it for years with the key untouched. **What is greenfield
is the Google half**: no `google-github-actions/auth`, no workload pool, and no STS exchange anywhere in
this repository. The signal is therefore a Google STS exchange in the acceptance job with no key placed
beside it, **and that one is mechanical**: `cargo xtask check-venues` reads the job's own credential state
and holds it to exactly one mechanism - a key and a federated token together is a half-finished migration,
neither is a leg authenticating with nothing, and the keyless state fails asking for this paragraph to be
amended. A gate whose failure is good news is the only shape that reaches a record.

**The transferable part, since this record keeps finding the same shape:** a limit whose evidence is
*nothing in the tree does X* is only as good as a search somebody ran. This one was written from
recollection two hours after the tree stopped agreeing with it, and no gate could have known - which is
why the sentence now names the command and the date, per this repository's own rule about a number in
prose. **And the correction is registered rather than merely made:** both halves of the wrong sentence -
the claim about the tree, and the *watch for the permission arriving* instruction it justified - are
wordings of one entry in `check-guidance`'s contradicted-claims table, resting on
`.github/workflows/release.yml` still holding `id-token: write`, so the rule retires itself the day that
stops being true instead of forbidding a sentence that has become correct again. **Two limits of that,
stated rather than left to be discovered.** It matches a literal, so a paraphrase escapes - the same
limit the gate records for every row in that table. And this page is `except`ed from the rule, because
the quotation above would otherwise fail it: that exemption is a blind spot, and what keeps it from
outliving its reason is `a_page_a_rule_exempts_holds_a_wording_that_rule_forbids`, which fails if this
page stops quoting the sentence it corrects.

**The gate named above had the same defect, one round later, and it is fixed here rather than shipped.**
`check-venues` read this job for `id-token: write` - the very proxy this amendment exists to retire,
narrowed from *any workflow* to *this job*. That retires the one false positive and keeps the class: the
permission granted here for some other keyless exchange would report a half-finished **Google**
migration that does not exist. What it reads now is what the paragraph above actually names - a Google
auth action, a workload pool, or an STS endpoint in this job - and
`an_id_token_grant_is_not_the_google_signal_this_record_corrected` is the test that keeps the proxy out.

### The limit each venue must state

- **A fork's pull request gets no secret**, so the acceptance job is absent on exactly the
  contributions least likely to have been run locally. That is the first amendment's original objection
  surviving in reduced form - the environment is the mechanism that makes the fork skip rather than
  leak, and it cannot do more.
- **A key proves `SharedServiceUser` only.** Every green here answers *does the service accept this and
  do the rows agree* for one identity, and nothing about who asked. The `data_systems:` golden axis
  gaining an entry, and even a green two-principal cell, do not promote a key to a subject.
- **A local-only verification has no gate on any path**, so the workforce run must leave a record - the
  date, the versions, and what was run. This is not hypothetical: a claim that some tier made a cell
  "newly answerable" was wrong precisely because the only measurement was one terminal run and the
  record did not say so. The workforce cell's record must say enough that a second person can decide
  whether the claim is repeatable, and it must state the audience answer above as a finding, not as an
  assumption.

## Sixth amendment, 2026-08-31: the two-principal cell's two prerequisites are now provisioned

**Status of the amendment: accepted.** The fifth amendment said the two-principal cell was "decided
and not built" because it needed two things "neither this repository nor an agent can provision,
because both are live-account work with a human owner": a second (and third) service-account key, and
a row-level grant at the source whose per-principal rows are written down. **The test-infrastructure
PR ([#106](https://github.com/telekom/sutura/pull/106)) now provisions exactly those two, through the
pulumi stack, so that paragraph's blocker is spent.**

### What is provisioned, and the one sentence above it that stopped being true

- **A second and third service-account key.** The stack creates two principals (`sa_a`, `sa_b`) and
  exports their keys; `just infra-set` places them in the `bq-test` environment as
  `SVC_SUTURUA_BQ_PRINCIPAL_A` and `SVC_SUTURUA_BQ_PRINCIPAL_B`, kept apart from the CI key the same
  way every key here is kept apart from the tree.
- **A row access policy whose per-principal rows are written down.** Two first-class
  `RowAccessPolicy` resources on the stack's `fact_events` table grant `sa_a` the rows where the
  grouping column equals principal A's value and `sa_b` the rows where it equals B's - disjoint by
  construction, with the predicate spelling the entitlement so "entitled to" is a check against the
  policy text and not a comparison of two numbers. The exact row values stay config (they are never a
  committed identifier), which is the same rule the fifth amendment applied to the values.

What the fifth amendment said the cell **needed to exist** therefore now exists, and the cell is no
longer blocked on provisioning. What is still required is the cell itself: an `#[ignore]`d leg in the
existing `bigquery-acceptance` suite that executes the same query as principal A and as principal B
and asserts each reads exactly the rows its policy grants - the second row of the fifth amendment's
table, made real. That leg is the subject of the follow-up PR on top of this one; it needs no new
job, because the venue is already CI in-repo only.

### What the workforce cell needs that this does not provide

None of the above touches the workforce row of the table. Its blocker is not a missing credential but
the **audience question** the fifth amendment carried: whether the identity provider will mint an ID
token whose audience is a third party's provider at all. A service-account key and a workload pool
cannot answer that - the subject is a person, not a key - so it stays the one venue with no gate and
no run, owned by the enterprise-IdP half ([#105](https://github.com/telekom/sutura/issues/105)). This
amendment changes nothing about it; it is recorded here so the two venues are not elided into one
"identity is provisioned now".

## Seventh amendment: the command-line tool opens a dataset too, behind its own default-off feature

**This record decided `sutura-cli`, not at all, and telekom/sutura#121 reverses that half.** The
reason it was refused is worth reading before the reversal, because the reason has not gone away: the
release derivations pass `--package sutura-cli`, so a non-optional dependency there would put `ureq`,
rustls and `ring` - which compile C and assembly - into four cross builds, two of them musl. What
changed is not the cost but the shape of the answer: a **default-off feature** pays that cost only
when somebody asks for it, which is the general rule `.agents/skills/sutura/crate-map` states for an
adapter with a native or an outbound-TLS dependency, and it is the same shape this record's SECOND amendment already took
for `sutura-serve`.

**What #121 was actually about**, and why the reversal is not a change of mind about cost: the only
binary a release publishes could answer questions from local files and nothing else, so every claim
this record's acceptance legs establish - four dialects, pushdown, a real dataset accepting our SQL -
was reachable only from a source checkout. That is a documentation and tutorial problem before it is
a capability one.

**What is built.** `crates/sutura-cli/src/sources/bigquery.rs`'s `open` composes the same three
layers `sutura-serve`'s `build_bigquery` composes, through the same public constructors:
`BigQueryWarehouse::new` over `BigQueryWire::new` over `Credential::read`, with one `WireAgent`
cloned so the token exchange and the job share a pool. It is a **copy and not a shared function**,
because the two composition roots are separate binaries and neither may depend on the other; what is
genuinely shared is this adapter's own constructors, so a fix to the credential path lands once. The
posture cross-check is called against that adapter's own `IMPERSONATION`, and an
`impersonation-at-source` entry is refused **by name, before the credential file is read**, for the
same reason the server refuses one: no broker that exchanges a subject's credential is attached, so
answering would read every row as the process while the declaration promised otherwise.

**The job's deadline comes off `server.request_timeout_seconds`**, through
`QueryDeadline::within_request_timeout`, even though this binary has no listener a job could outlive.
The alternative was a number invented in the composition root, which is the drifting duplicate the
settings tree exists to prevent.

**The limit that buys, stated as a number rather than a caveat, because review measured it:**
`CALLS_PER_ANSWER` is 2 and the connect margin is 5 seconds, so the shipped default of 30 gives a job
**10 seconds** and `RequestTimeout::MAX_SECONDS` of 300 caps it at **145** - against a
`QueryDeadline::MAX_SECONDS` of six hours. On the command-line tool that key therefore bounds nothing
that exists and imposes a ceiling designed to protect an HTTP connection the command does not have: a
twelve-second question is cancelled by `jobTimeoutMs` with nobody waiting on any request. The
adapter's own `QueryDeadline::parse` exists for "a deployment stating a budget outright" and is
deliberately not used, because a second key on one binary is the duplicate the first half of this
paragraph refuses. What would lift it is a settings key meaning *how long a QUESTION may take* rather
than how long a REQUEST may - one number both roots read - and that is a settings decision rather
than this record's.

**Three more things no test observes on the command-line root**, listed because a composition that is
tested reads as a path that is exercised. `OpenedWith::attached` is `None` for a dataset and no test
sees that value, because every one of them stops at the credential read - what it feeds is `mcp`'s
`if let Some(attached)`, and the files arm is what exercises that branch. The `mcp` command's
`Opened::BigQuery` arm is instantiated by no test either: all three go through `open_engine` directly,
and reaching it by hand gives the same `credential_file` refusal. And the credential refusal prints
the operator's own absolute path out of their own settings - inherited unchanged from
`sutura-exec-bigquery` and identical on the served path, noted only because *Design Principles* says
an error is not a place for a path.

**And what watches the feature-OFF lane, said at the sharper end than "a behaviour that differs by
build":** `just test`, `just mcp-e2e` and `checks.nextest` all pass `--all-features`, so the
`cfg(not(feature = "bigquery"))` refusal and the test that provokes it are run by none of them. What
does compile that lane is `cargo xtask check-default-features`, in `just gates`: it derives its
package list from `nix/shipped.nix`'s `binaries`, so this crate joined it the moment the feature
existed, and it runs `cargo clippy` as well as `cargo check` because a check cannot see a lint - which
is how a `doc_markdown` failure lived on such a line in both roots until somebody ran clippy at the
default set. **Its limit is CI's:** `ci.yml` reaches every gate as a nix check or a nix app and that
gate is neither, so what CI has for this lane is the four `cross` link builds for the compile half and
nothing at all for the lint half.

> **Amended, and the limit is gone rather than narrowed.** That gate is now a nix app -
> `ci.yml` runs `nix run .#default-features` on every pull request that touches Rust - so the lint
> half of this lane is gated in CI and not only in `just gates`. An app and not a check for the
> reason the paragraph above gives for it not being a hygiene gate: it shells out to cargo, which
> wants a resolvable registry and a writable target directory the sandbox has not got. **What did
> not change:** both passes stop at metadata, so the four `cross` builds are still the only thing
> that LINKS the default set, and they are still `needs: [ci]`.

**The measurement this record's own rule asks for, taken 2026-09-02.** `cargo check -p sutura-cli
--all-targets` on the default feature set touches neither `ring` nor `ureq`; the same command with
`--features bigquery` compiles `ring` from C and assembly. That is the whole argument for default-off
stated as a build rather than as a prediction, and it is also why the four `cross` jobs are
**unchanged** by this branch: they pass no `--features`, so they compile exactly what they compiled
before. What would price the alternative - the feature ON for four triples - is a build nothing asks
for, so it has not been run.

**And what still has not happened, because a feature reads like availability.** No published artefact
links the adapter. `nix/shipped.nix` builds both binaries with cargo's DEFAULT features and its
features paragraph now says so for both, which is telekom/sutura#121's third step decided as its own
recommendation had it: the CLI ships without the feature, and a deployment that needs a dataset runs
a build that carries it. `checks.shipped-features` is what keeps that from drifting - it reads
`ureq`'s and `ring`'s absence out of each shipped binary's own embedded dependency list rather than
out of a manifest, so a `bigquery` that stopped being optional on either crate fails a gate.

**Two lines elsewhere in this record are dated by that and are corrected here rather than in place**,
because each was right when it was written. *"The release derivations pass `--package sutura-cli`, so
they never compile `sutura-serve` itself"* and *"`sutura-serve` is not a `nix` package at all"* were
both spent by telekom/sutura#111, which publishes both binaries. And *"the image holds `sutura-cli`,
which links the engine only"* needs the qualifier *as published*: the crate can now link a second
adapter, and the artefact still does not.

## Eighth amendment, 2026-09-03: the build nothing asked for has been run, and it went the other way

**The sentence this amendment exists to correct** is in *The measurement this record's own rule asks
for, taken 2026-09-02*, above: *"What would price the alternative - the feature ON for four triples -
is a build nothing asks for, so it has not been run."* It is now false, and it was falsified by
telekom/sutura#121's own step 2 rather than by anything external. `nix/shipped.nix` grew a
`probeFeatures` field; a feature named there gets a `<bin>-<feature>-<triple>-ci` package per
release triple, and the four `cross` jobs build them beside the shipped set on every pull request.
The record's build-cost reasoning above it is corrected here rather than in place, per this page's
own convention.

**First, that the A/B is an A/B, and it was re-taken on the triple that is the risk.** The probe
derivation and the shipped `-ci` derivation for `aarch64-unknown-linux-musl` were dumped and
diffed. Their input sets are IDENTICAL - 23 input derivations and 4 input sources on both sides,
including the same `sutura-deps-aarch64-unknown-linux-musl`, so the dependency derivation is shared
rather than rebuilt - and exactly three environment keys differ: the cargo build command, which
gains a trailing `--features bigquery`; the disabled check command, which gains the same; and the
output path. The job logs confirm the shared half from the other end by never building a second
`sutura-deps-<triple>`. Nothing else varies between the two columns below.

**The cost, as COMPILED UNITS.** Seconds drift with whatever else a runner is doing; a unit count
does not. Counted off the `Compiling` lines of the CRATE derivation's own log and not the job's -
`sutura-deps-<triple>` is the shared dependency build and is excluded, which is the whole reason
units beat seconds here - for all four published triples of CI runs 33808343712, 33838360913 and
33843494162, feature off and then on, the same in all three:

| triple | OFF | ON |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | 100 | 112 |
| `aarch64-unknown-linux-gnu` | 100 | 112 |
| `x86_64-unknown-linux-musl` | 100 | 112 |
| `aarch64-unknown-linux-musl` | 100 | 112 |

Re-taken on 33843494162 with the same answer in all four cells, which is the point of counting
units: the seconds moved by 20% across those three runs and this table did not move at all.

**+12 units on every one of the four, none dropped, and the same twelve crates every time:**
`ring`, `untrusted`, `rustls`, `rustls-pki-types`, `rustls-webpki`, `webpki-roots`, `ureq`,
`ureq-proto`, `httparse`, `getrandom 0.2`, `utf8-zero`, `sutura-exec-bigquery`. That is the adapter
plus the outbound TLS closure and nothing besides. The unanimity is the useful part: a per-triple
figure would leave open which triple the closure is dearer on, and none of them is.

**The seconds, for whoever wants them, and they are two different numbers.** The FEATURE's delta:
runs 33781193001 and 33792642655 gave `+0.5 / -0.1 / +1.2 / +1.3s` and `+0.4 / -0.3 / +1.3 / +0.9s`
over crate derivations of 69-84s - so **-0.4% to +1.7% across two runs**, which is noise. Cite that
range, never a cell, and prefer the unit counts. The STEP's cost is the other number and it is not
small: the probe is a second crate derivation, and three runs of it read **62-85s** per job:
33808343712 gave 69/74/75/85s, 33838360913 gave 72/72/73/80s, 33843494162 gave 62/71/73/73s.
**That third run is why the range is cited and not a cell** - it put a value BELOW the low end this
record had stated twice, which is what a wall clock on a shared runner does. Feature-off and
feature-on both LINKED on all four triples in every run, with `file` reporting the right
architecture for each - `statically linked` on aarch64-musl,
`static-pie linked` on x86_64-musl, `dynamically linked` on the gnu pair, exactly as the shipped
builds report on the same triple.

**Was default-off necessary? For build cost, no. And the first version of this amendment got the
REASON wrong, which is worth recording because the wrong reason was the plausible one.** It said the
closure was already in the deps derivation with the feature off, so the probe could not have cost
much. Half of that is right and half of it is not:

- **Right:** `craneLib.buildDepsOnly` is called on the unscoped argument set, deliberately, so the
  checks share one dependency derivation; `cargoExtraArgs` is set on the final attrset instead. That
  derivation therefore resolves the WHOLE workspace at cargo's default features - so `ring`,
  `rustls` and `ureq` are compiled inside `sutura-deps-<triple>` on all four triples **with the
  feature off**, visible in all four job logs. The musl C-and-assembly cost this record priced on
  2026-09-02 is paid on every pull request either way.
- **Wrong about WHY, and this is the correction of the correction.** The sentence above used to end
  *"and `sutura-exec-bigquery` is a workspace member taking `ureq` non-optionally"*. It does not:
  `crates/sutura-exec-bigquery/Cargo.toml` declares `ureq = { workspace = true, optional = true }`
  behind its `wire` feature, so at the default set that crate contributes nothing. The observation
  was right and the mechanism was invented, which is the harder half of this defect class to
  notice - a green measurement makes a plausible explanation feel checked. Measured 2026-09-04:

  ```
  $ cargo tree --workspace --target aarch64-unknown-linux-musl -i ureq -e normal,dev
  ureq v3.4.0
  [dev-dependencies]
  `-- sutura-catalog-datahub          <- non-optional, for its provisioned-instance probe

  $ cargo tree --workspace -i ureq -e normal,build
  ureq v3.4.0
  [build-dependencies]
  `-- libduckdb-sys -> duckdb -> sutura-exec-duckdb    <- host side only
  ```

  So for a musl target the ONE edge into `ureq` at the default set is a **dev-dependency** of the
  DataHub catalog crate, and `buildDepsOnly` builds dev-dependencies - which is what puts the
  closure in the shared derivation. `libduckdb-sys` accounts for the host-side copy, and is why the
  gnu host job's log shows `ring` compiled twice. **What that changes for the decision:** nothing,
  and it is worth knowing anyway, because the cost is paid by a dependency that has nothing to do
  with this adapter. Drop that dev-dependency and the musl build-cost argument comes back.
- **Wrong:** that those units are then REUSED. They are not. The deps build compiles at the
  workspace feature union while the probe asks for one package, so the v2 resolver hands it a
  narrower feature set, a different `-C metadata`, and a recompile - `sutura> Compiling ring
  v0.17.14`, in the probe's own derivation, is that recompile. This is the same structural effect
  telekom/sutura#223 measured from the other side for the warm-start gate.
- **So why is it nearly free?** Slack. The twelve units start in the first seconds and finish long
  before `datafusion`, which sits on the critical path ahead of `sutura-exec-datafusion` and the
  crate itself. **That is a property of this dependency graph, not of the feature** - a shorter
  critical path would expose the same twelve units as wall clock.

**What default-off IS still necessary for, unchanged and now the whole of the argument:** no
published artefact links an outbound TLS stack, which `checks.shipped-features` asserts out of each
binary's own embedded dependency list rather than out of a manifest. The decision of step 3 stands;
what does not stand is *the four cross builds* as its reason. **Keep the decision, drop the
build-cost justification for it** - an argument that a measurement contradicts is worth less than no
argument, because it invites the next reader to trust the rest of the paragraph.

**Verifying the lane found a dead gate IN the lane, and that is the part with the longest shelf
life.** The first version of the workflow step RECONSTRUCTED which probes to build: it filtered the
flake's attribute names for `-<triple>-ci`, subtracted the shipped binaries' names, and split what
remained to recover the executable and the feature. So `nix/shipped.nix`'s package-naming rule was a
coupling nothing could check. Measured 2026-09-03 by reordering that name from
`<bin>-<feature>-<triple>` to `<bin>-<triple>-<feature>`, `probeFeatures` untouched: the
reconstruction went **empty**, the step printed *"no binary declares a probeFeatures entry, so no
feature-on build was measured"* - which was false, one was declared - and **exited zero**. This
amendment's whole claim would have reverted to *assumed* behind a green run and a reassuring
sentence, which is the failure mode this repository cares about most.

The step reads a manifest now: `nix build .#feature-probes-<triple>` yields a file whose rows carry
the package, the executable and the feature, so nothing in the shell derives anything. Three
directions were then reproduced by hand rather than argued:

| Break | Before | Now |
| --- | --- | --- |
| Probe package renamed, `probeFeatures` intact | empty set, false notice, exit 0 | probe still found and built, printed sentence still correct |
| `probeFeatures = [ ]` | notice, exit 0 | `::error::` naming this record, **exit 1** |
| Manifest attribute gone from the flake | not expressible | `nix build` fails, **exit 1** |

`file` cannot carry any of this and is a readout rather than an assertion: it exits **zero** on a
path that does not exist, measured. What asserts the link is `nix build` succeeding.

**And the same verification caught the step's own printed SENTENCE overstating, which is the
smaller half of the same defect.** It printed its elapsed seconds as *"That is the price of the
feature ON"* - and did so in CI, four times a pull request: `85s`, `69s`, `75s`, `74s` in run
33808343712. Those seconds are the whole probe. The feature is the +12 units above, worth under 2%,
and a wall clock in that step cannot separate the two, so the number was off by roughly the width
of the thing it claimed to measure. Corrected to say which number it is and where the other lives.

**And it is corrected in prose rather than held, which is the part worth writing down.**
`check-guidance` DOES read `.github/**` - its file scope is `md`, `nix`, `yml`, `yaml`, `toml` and
`sh` - so this sentence was reachable by the one gate in the repository that fails a false claim,
and nothing had registered it. Registering it now is about 15 lines more than
`xtask/src/guidance/claims.rs` has: `wc -l` says 985 on 2026-09-04 against `cargo xtask max-lines`'
unexemptable 1000, and the split that makes room is in flight on another branch. So the correction
is held by REVIEW until that lands, and the
general lesson is the one the remedy scan already taught from the other side - ask what reads a
mechanism's own prose, and expect the answer to be nothing until somebody has registered it.

**And the lane fails closed on the defect it exists to catch, reproduced rather than assumed, and
re-reproduced on the merged tree on 2026-09-04.** A type error was planted inside the feature-ON
`open` in `crates/sutura-cli/src/sources/bigquery.rs`, and both host derivations were built from
that one tree:

| derivation | result | crate-derivation units |
| --- | --- | --- |
| `sutura-aarch64-apple-darwin-ci` (shipped) | **exit 0** - blind to it | 105 |
| `sutura-bigquery-aarch64-apple-darwin-ci` (probe) | **exit 1**, `error[E0308]: mismatched types` at `bigquery.rs:70:25` | 117 |

The 105 and the 117 are the same +12 the four CI triples show, taken from the same pair of builds
that produced the asymmetry - so one reproduction answers both questions. That asymmetry is the
whole argument for the step existing: before it, every one of those four jobs was green on a tree
where the documented source build did not compile.

**The limits of this measurement, next to it.**

- **The PROFILE is not the documented one, and this is the limit that narrows the claim most.**
  `docs/getting-started.md` says `cargo build --release`; the probe builds `ci`. `[profile.release]`
  is `lto = "thin"`, `codegen-units = 16`, `panic = "abort"`; `[profile.ci]` inherits `dev` -
  `lto = "off"`, `codegen-units = 256`, unwind. Thin LTO and an abort personality across `ring`'s C
  and assembly are a **different link**. So what is proven is *the feature's closure cross-compiles
  and links for each published triple*, not *the reader's exact command links*. Adding a
  release-profile probe would double the step's cost for a codegen difference nobody has priced; the
  concession is in the step's printed notice instead, where a reader of a green run will see it.
- **The probe links and never RUNS**, so nothing here says the feature works - only that it builds.
  `just bigquery-acceptance` is the leg that answers the other question.
- **Binary size is unmeasured**: no step prints it, so the artefact-closure argument above is still
  qualitative.
- **`sutura-serve`'s `tls` and `bigquery` are deliberately unprobed**, so none of this is evidence
  about the server.
- **The planted-error RED was reproduced on `aarch64-apple-darwin` only**, the one triple that host
  builds natively. The musl link has been exercised green on every pull request and never red, and
  CI is the only venue that can redden it, because a musl dependency closure is not cached on a
  developer's machine.
- **The cost of HAVING the probe is real even though the cost of the feature is not:** 62-85s on
  each of four `cross` jobs, per pull request, across runs 33808343712, 33838360913 and
  33843494162. That is the price of the answer rather than the price of the feature - and the range
  widened downward on the third run, so quote the range.
- **The `file` readout asserts the CPU and not the libc.** `nix/assert-linked.sh` matches `file`'s
  answer against the triple's CPU and refuses a path that is not executable - which is what the
  bare `file` here did not do, since it exits zero on a missing path. What it does not assert is
  `statically linked` versus `dynamically linked`, because those strings vary with linker, target
  and `file` version in ways nobody here has measured. A musl target linked dynamically would pass.

**Two couplings this lane rests on. One is held now, and it was not when the lane landed.**

**WHICH FEATURES ARE PROBED - held, by `cargo xtask check-shipped-binaries`.** The refusal in
`.github/workflows/cross-link.yml` is *the manifest has at least one row*, and that was read as *a
probe cannot silently disappear*. It is not the same claim: zero rows means *the CLI probe is gone* only because the other binary
declares none, which is a property of today's data rather than of the construction. Declare a probe
for `sutura-serve` and delete `"bigquery"` from `sutura-cli`'s, and the manifest is still non-empty
- the four link legs go green and this record's claim reverts to *assumed* with no signal at all.
The coupling that closes it is the one nothing checked: a page tells a reader to run
`cargo build --release -p sutura-cli --features bigquery`, and `probeFeatures` had to contain that
feature. The gate now reconciles the two, in both directions - a documented feature no probe covers
fails, and so does a tree where no page documents such a build at all, because a reconciliation
against nothing passes everything.

**WHICH TRIPLES ARE PROBED - still not held.** The link matrix in
`.github/workflows/cross-link.yml` spells the four triples as literals, as `release.yml`'s does, and
`check-shipped-binaries` reconciles the shipped BINARIES between those files and `nix/shipped.nix` -
not the TARGETS. So a triple added to `crossTargets` and to `release.yml` but not to
`cross-link.yml` would ship having been linked by nothing, with the feature or without it, and the
only thing saying otherwise is a comment above that matrix. **The matrix moved out of `ci.yml` and
this pointer moved with it** - which is the second failure mode of a coupling held by prose: not
only can the comment go, the sentence naming where to read it can be left behind. `cross:` in
`ci.yml` is now a three-key caller with no `strategy.matrix` in it at all, so a reader sent to that
file finds nothing to add a triple to. The reverse direction does fail closed, freely: a matrix
target that is not a release target has no
`feature-probes-<triple>` attribute, so `nix build` fails. The gate's own header argues a matrix
cannot be derived because `strategy.matrix` takes literals, which is exactly the argument for
reconciling this pair too. It is the same shape as the rule above and is a separate change.

## Ninth amendment, 2026-09-04: the acceptance leg is re-entrant, and two runs no longer race

**Status of the amendment: accepted.** The third amendment's "NEW" bullet - *this leg WRITES, and two
runs against one dataset will race* - stopped being true in #119, and this is its record. The bullet
itself now points here; a merged record that quietly stops being true is worse than one wrong from
the start, and the race was the one way this leg could fail for a reason that looked like a product
bug.

### The mechanism: a per-run table suffix, applied before the plan is compiled

Every corpus table is now named with a token unique to the run (`GITHUB_RUN_ID` in CI, a clock+pid
value locally) and a per-test leg (`accept`, `rows`, `anchors`) - `dim_customer_<token>_rows`, and so
on. The suffixed names live in the DEFINITIONS: the leg rebuilds a `PinnedDefinitions` whose models'
tables carry the suffix, and both the engine and `BigQuery` compile against that bundle, so the plan
and the dataset agree (only the models' tables change; relationships and metrics are cloned verbatim,
and `pin` recomputes the digest). Two runs, or this leg's own three tests under nextest's default
parallelism, each create, read and drop only their own tables.

### The two mechanisms that keep it clean, and why both are there

- **Each `CREATE` sets a 24-hour table expiration.** This is the GUARANTEE half, and it exists for a
  reason specific to this crate: the shipped profiles run `panic = "abort"`, so a cancelled runner
  never unwinds - a `Drop` guard that would clean up is skipped before it runs. Table expiration is
  the mechanism that survives that: the table self-deletes after the interval whether or not anybody
  drops it.
- **Each run DROPs its own tables when it finishes.** This is the TIDY half - a complete run leaves
  nothing behind even for the 24-hour interval. It is not the guarantee, because an abort skips it;
  the expiration is.

### The per-run table name is safe to print

The suffixed names are committed-fixture-names-plus-tokens, not resources: only the dataset and the
project are resources. A run prints its table names as they load, which is what lets a log say which
run wrote them; the dataset and project are still masked in CI, and a table name never carries them.

### The belt: a CI concurrency group shared across refs

`ci.yml`'s `bigquery-acceptance` job now carries a job-level `concurrency` group keyed on the
dataset (`vars.SUTURA_BQ_DATASET`), not on the ref, with `cancel-in-progress: false`. Two in-repo
runs against the one `bq-test` dataset therefore QUEUE rather than interleave. It is the belt, not
the braces: the per-run suffix is what makes them safe, and the developer's own local run is a
concurrent writer the group cannot serialise - its per-run table names announce it on the shared
dataset, and the `just bigquery-acceptance` comment says that in one sentence.

### What becomes provable, and how

`just bigquery-acceptance` twice concurrently, locally, both green - the table names differ per run,
and each run drops its own. The CI job is green on a branch, its printed table names carrying the
run suffix.

### Not in scope

Cross-dataset fixtures (#118) and the conformance packs (#116) are still future work; this change
makes the first safe to add rather than doing it.

## Tenth amendment, 2026-09-04: the two-principal cell exists, and what it is *not*

**Status of the amendment: accepted; the cell is written and has not been run.** The sixth amendment
said the cell's two prerequisites were provisioned and that "what is still required is the cell
itself". This is that cell - issue #123 - and this amendment records three decisions taken while
writing it, one measurement that closed a question the wrong way, and the two things a human still
has to do.

### What the cell is, and the one sentence that keeps it honest

`crates/sutura-exec-bigquery/tests/two_principals.rs`, reached by `just bigquery-two-principals` and
by `nix run .#bigquery-two-principals`. One `QueryPlan` value, borrowed twice, so **the statement is
the same statement and not two that resemble each other** - and each leg presents a bearer minted
from that principal's own service-account key through this crate's own `Credential`, which is issue
#123's second bullet answered by reusing the credential path a deployment runs rather than by
hand-rolling a token exchange.

**It is leg 2's SOURCE half and not leg 2.** The two principals are service accounts whose private
keys the leg holds, so nothing was exchanged and nobody asked. What a green run would establish is
that a real data system applies the row grant of the principal whose bearer this adapter *presented*
rather than the identity the transport holds; what it says about a caller is nothing.
`docs/where-identity-is-proven.md` carries that as a venue of its own with five exclusions, and it
deliberately does **not** move the *two subjects read two different row sets* row up to it: two
principals is not two subjects, and eliding those is the overstatement that page exists to prevent.

### Three decisions, and the reason each went the way it did

**It has its own task and its own nix app rather than being a third leg inside
`just bigquery-acceptance`.** It needs five values and two key documents the other two legs do not,
and one task demanding all of them would make the legs a developer holding one credential *can* run
unreachable. Both apps filter on the test BINARY and not on a test list, so the property the
acceptance app's comment states survives: a test added to either target is reached without a count
anywhere being edited.

**It does not seed the table, though issue #123 asked it to.** `BigQueryWarehouse::load_fixture`
renders `CREATE OR REPLACE TABLE`, and replacing a table drops its row access policies - so the one
loader this crate has would disarm the grant the cell asserts on, and the run after it would compare
two identical row sets. There is no arbitrary-SQL entry point to reach for instead, deliberately.
So the rows belong to whoever owns the policies: the predicate and the rows it selects are two halves
of one grant, and splitting them across the stack and a test file is how they drift. The stack seeds
one row per principal beside the two `RowAccessPolicy` resources, and the cell's non-emptiness
assertion is what refuses to read an unseeded table as a pass.

**The control leg accepts two outcomes and prints which it got.** What the endpoint answers a
principal no policy grants decides how strong that control is, and it is unmeasured: documented
behaviour is no rows, the observable alternative is a refusal, and both are *not reading either
principal's rows*. The first green run narrows it to one sentence; until then the test says so at the
assertion rather than guessing.

**Two outcomes is what the sentence said and fourteen is what the code accepted**, which review
found. `let Ok(rows) = ... else { return; }` took every `BigQueryError` variant as evidence of a
refusal - and the sharpest of them are not refusals at all: `UnmappedType`, `NotAnInteger` and their
siblings mean the endpoint answered and rows came back, so a deployment that had just read the
policied table printed *the deployment's own identity was refused* and passed the control without
looking at what it read. `Render` and `PresentedDisagreesWithPosture` are defects in the test file
and were green; one `Endpoint` timeout left the cell with no control while both subject legs passed.
It is an exhaustive `match` now, in `wire::tables::was_refused`'s shape and for its reason - a new
variant is a compile error at that line rather than a new way to pass - accepting only a `403` that
is not `rateLimitExceeded`/`quotaExceeded`. **`401` is deliberately refused too:** a credential that
cannot authenticate leaves the control unable to distinguish anything while reading as though it
had. The limit that survives is on the page beside the claim: a `403` does not say WHICH grant was
missing, so a missing row grant and a missing `bigquery.jobUser` are indistinguishable here.

### The measurement that closed a question the wrong way

The three behaviours the cell rests on were probed from a developer machine against the acceptance
project, with the credential that machine holds. Creating a table, `INSERT`ing into it and reading it
back all work. **Creating a row access policy does not:** the acceptance credential is refused
`bigquery.rowAccessPolicies.create` - `Access Denied ... Permission bigquery.rowAccessPolicies.create
denied on table` - so a policy cannot be created, replaced or inspected from a developer machine at
all. Two consequences, and both are limits rather than defects:

- **whether `INSERT` works on a table that HAS row access policies was not measured**, because no
  policy could be created to try it against. The seeding decision above makes it the stack's problem
  rather than the test leg's - but **it does not close the question, and an earlier wording of this
  bullet read as though it had.** Review found the three resources unordered: the seed job and the
  two policies each depended on the table alone and the policies were bound to no name, so pulumi
  created all three concurrently and which path `up` took was the scheduler's choice. The ordering
  is declared now - the job first, both policies `depends_on` it, and the principals' read grant
  waits for both policies so a fresh `up` never leaves a principal able to read an unpolicied table.
  **What remains, stated rather than moved:** the policies persist, so from the second apply onwards
  a changed grouping value re-digests the job against a table that already carries them, and the
  `INSERT` runs on a policied table deterministically. `just infra-up` after a config change is the
  first place anyone can observe it. **Whoever gets there: record the answer here.** If it is
  refused, `up` fails on the resource the whole fixture depends on; if it is accepted but filtered,
  the cell reports *principal A read no rows at all* and points two steps from the cause;
- **the policied dataset is not listable from a developer machine either**, so the cell's own
  configuration cannot be discovered locally and no local run of it is possible. That is why the
  venue page says the claim is not answered yet rather than that the run failed.

### What is left for a human, and neither is code

1. **The `bq-test` environment has to carry five more values** - the policied dataset and table, the
   grouping column, and the grouping value each policy grants. `sync-bq-test-env.sh` exports them
   from the stack now, so `just infra-set` is the mechanism; it needs the pulumi state, which lives
   with whoever ran `just infra-up`.
2. **The CI job has to run the leg, and that change is WRITTEN and must not merge yet.** It is a
   separate pull request stacked on this one rather than a future one - the sentence this item first
   carried said it had not happened, and it had - and it stays in draft for the reason it was kept
   separate: wiring a job to five variables that do not exist yet turns the `bigquery-acceptance` job
   red on every push until somebody sets them, and a job that is red for a configuration reason is a
   job people learn to ignore. The order is the environment first, the job second.

   **The cap decision this item used to carry has evaporated, and the sequence is worth recording
   because it is the ordinary case rather than an accident.** An earlier wording said
   `.github/workflows/ci.yml` was *over* the 1000-line cap
   [#285](https://github.com/telekom/sutura/issues/285) is about, in the present tense, and paired
   it with `987 → 1042`. Neither was true of any tree this record shipped in: measured with
   `wc -l`, the file was **999** on `main` and at this record's own head, and this change touches no
   workflow at all. Then [#289](https://github.com/telekom/sutura/pull/289) landed and took it to
   **856** - `wc -l .github/workflows/ci.yml` on `main` at `fd5959ea`, and on this branch after
   carrying that merge in. So there is **143 lines of headroom** and no cap decision for the job
   change to make: `devco/max-lines-ignore` needs no `[warn]` entry for this workflow, and one added
   before #289 landed should come out. That file is for generated and vendored output, where length
   is a function of what is described; a hand-written workflow in it is a promise to split, and a
   promise nobody needs is worse than none.

   **The transferable half:** a line count written in the present tense is a measurement with no
   date, and nothing in this repository derives it. Write the command and the commit beside the
   number, or the number is a claim that rots on somebody else's merge.

### What review of the cell found, recorded because each was a claim rather than a bug

Two rounds, and no count in this heading on purpose - it was *three* and then it was not. The second
round's other findings are recorded where they belong rather than listed again here: the control leg
accepting fourteen error variants is under *Three decisions* above, the seed job and the two policies
being unordered is under *The measurement that closed a question the wrong way*, and four citations
pointing at the ninth amendment when this one is the tenth were simply wrong and are corrected.

**A verdict was carrying two states.** The venue page's `can` meant *the venue is capable and the
standing test lives somewhere else*, and this cell needed *the standing test lives here and nothing
has run it*. Those are not the same and only the first is evidence, so `cargo xtask check-venues`
grew a sixth verdict - `unrun` - which it refuses from a venue nothing reaches and which the venue's
own section has to use in that word.

**And the first version of those rules held a spelling rather than the transition, which review of
this cell found.** All three read the page: nothing read whether a run had happened, so the sentence
*the change that carries the first green run moves that cell* was still a sentence nothing read -
which is what it had just been criticised for being. The closure is a fourth rule with a mechanism:
the venue's `Reached by` task is resolved against every `just <task>` and `nix run .#<app>` the
workflows, the local composite actions and the shared `nix/` shell invoke - one walk, shared with
`check-workflows`, because a step moving out of a workflow is the recorded way a reference leaves a
gate's sight - and an `unrun` cell whose venue is among them is refused. Measured by wiring
`nix run .#bigquery-two-principals` into `ci.yml` and re-running the gate: it fails naming this
venue, and passes again with the line removed.

**The rule is one-sided, and the side it does not hold is stated on the page beside the claim.** An
invocation is not a green run: a wired job that always skips reddens the cell, and a hand-run is
invisible to it. The authority for *did this pass* is the GitHub API, unreachable from
`checks.hygiene` for the same reason `nix eval` is unreachable from `check-workflows`. So moving the
cell to `yes` stays review's, with the run named beside it; what stopped being possible is leaving
it at `unrun` while a job runs it.

**One assertion in the cell could not fail.** With each answer asserted to be exactly its own
principal's grouping value, and the fixture control already refusing a pair whose values are equal,
disjointness followed. It is deleted rather than kept: an assertion that cannot go red reads as an
independent check and is not one, and it made the page and this record advertise three where there
are two.

**The step this amendment asks for nearly broke the venue that HAS a green run.** The two policied
resource names were added to the masking step's emptiness guard, which exits non-zero and runs
before the shared-key leg - so an unset value belonging to a venue with no run would have stopped
the only BigQuery venue with evidence. Masked there, guarded in the cell's own step. `check-venues`
was satisfied either way, which is why review and not the gate found it, and it is the sharpest
example of this record's own warning about a job red for a configuration reason.

### And two limits this cell does not close, named so they are not read as closed

**The single-credential limit is superseded by the twelfth amendment.**
`acceptance/properties.rs` now derives recognised `$RUNNER_TEMP` destinations from lines naming
configured secrets and requires each file in an `rm` argument list. Indirect copies, `cd`-relative
writes and whether a conditional cleanup actually executes remain outside that scan.

**Nothing checks that the two nextest filters are complements.** `binary(two_principals)` and
`not binary(two_principals)` appear in two `just` recipes and two flake apps, and no gate reads a
nextest filter expression - so renaming the target would silently un-filter the acceptance app.
Two nextest profiles carrying the pair once, next to the tests, is the shape that would close it.

## Eleventh amendment, 2026-09-05: what *exactly* meant on the measured run, and what it did not

**Status of the amendment: accepted; it corrects a published claim and carries no live run.** The
fourth amendment above records `21 answers agreed exactly on content AND order`, and the third
amendment's status cell read *Answered for CONTENT, exactly*. Both are narrowed here, because the
comparison that produced them was not exact in the sense either sentence invites.

**The comparator of that day compared cells through `Value::render`, which is a display form.**
`Value::Null` and `Value::Text("null")` both render `null`; `Value::Integer(1)` and `Value::Text("1")`
both render `1`. A cell the endpoint answered as text where the engine answered a number - or as the
word `null` where the engine answered nothing at all - was therefore counted as agreement on that
run. The 21 are evidence about **acceptance**, about **rendered row content** and about **ORDER**,
and about nothing else. The status cell's *exactly* has to be read that way, which is why it now says
so at the cell rather than here alone.

**Both legs share one typed policy now.** `sutura_domain::warehouse::agreement`, behind a default-off
feature and `cfg(test)`: the variant is part of the comparison key, and the one approximation is
named and scoped - `RealTolerance::DIFFERENTIAL`, thirteen significant digits, reaching `Value::Real`
and no other variant.

**No number is restated for it, deliberately.** Nothing has run the corpus leg against the real
dataset since the policy changed, so the tally above stays as the transcript of the run that made it
and this amendment corrects the reading rather than replacing the measurement. What is measured for
the typed policy is that module's own suite plus two cells per leg asserting that a null and the word
`null`, and an integer and its own text, are refused **by the content comparison** - the diagnosis
named in the expected panic, not merely that some panic fired. The next `bigquery-acceptance` run is
what would restate the number with *exactly* meaning what it says.

## Twelfth amendment, 2026-09-07: the first of the tenth amendment's two limits is closed

**Status of the amendment: accepted; it closes a limit this record named, and carries no new run.**

The tenth amendment states that `check-venues` reads ONE credential path, so *written under
`$RUNNER_TEMP` and removed* and *this file is a second copy of the secret* are held for the CI key
and **by review** for the two principal keys placed beside it. That sentence was true when it was
written and is not now: telekom/sutura#389 derives the credential set from the job, which is the fix
this record named. `properties::credential_placement` reads recognised redirects into `$RUNNER_TEMP`
from a line that spells a secret, and each such file has to be deleted by name; `shape::removes` reads an
`rm`'s ARGUMENT LIST, because one `rm` over three paths contains the whole-string form of none of
them but the first - so the form it replaces would also have failed a job that removes three
correctly.

**Measured on this job, tree restored after each:** the two principals' writes added under
`$RUNNER_TEMP` and left out of the cleanup, `cargo xtask check-venues` names both files and
`just hygiene` exits 1; with one `rm` naming all three, the leg's own path LAST, it exits 0. The
pre-change pair that both exited 0 was the additional writes with and without separate ordinary
cleanup commands, while retaining the original primary cleanup. The combined cleanup with the
primary path last instead exited 1 before this change: it was a false refusal of a correct job.

**Review narrowed the shell spelling claim.** Plain and braced `RUNNER_TEMP` expansions, with the
whole word or directory double-quoted, now resolve to the same path in both the primary-write and
additional-copy checks. A single-quoted variable is literal and earns no placement or cleanup.
Cleanup arguments end at the command boundary: a trailing comment or later `echo` removes nothing,
while an `rm` behind a compound-command keyword is still read. This recognises commands; it does
not establish that a condition lets them execute.

**What is still review's, narrower than the sentence it replaces:** a copy made by a command that
does not spell the secret's name - a `cp` of the key file, a `base64 -d` of it - a `cd`-relative write
or other shell-built path, and a removal whose condition never fires. The second limit that
amendment names, the two nextest filters, is
untouched and is telekom/sutura#430.

## Thirteenth amendment, 2026-09-10: the cross matrix is reduced, and this record's probe claim narrows with it

**Status of the amendment: accepted; it withdraws a claim this record makes about a venue, and
carries no new run.**

The eighth amendment says that a feature named in `probeFeatures` gets a package per release triple
*"and the four `cross` jobs build them beside the shipped set on every pull request"*, and prices
having the probe at *"62-85s on each of four `cross` jobs, per pull request"*. The eighth
amendment's cell list also states that *"the musl link has been exercised green on every pull
request and never red"*. None of the three survives unqualified, and the first two had already
stopped being true before this change: the pull-request leg was reduced to the two aarch64 triples
earlier, so *four, on every pull request* was describing a venue that had stopped running.

**What runs now.** `cross-link.yml` builds ONE reduced set on every event it is called for -
`aarch64-unknown-linux-gnu` for the architecture axis and `x86_64-unknown-linux-musl` for the
static-allocator C axis, one cell each so a red leg says which axis broke - and the full four
survive only in `release.yml`'s `build`, whose `publish` job asserts that exactly four artefacts
arrived. `cargo xtask check-workflows` holds all three literals, the release four included; that
last one is what makes the reduction a later signal rather than an absent one.

**What it costs this record's claim, stated rather than smoothed over.** The
`feature-probes-<triple>` step rides that matrix and NOTHING else runs it - the release path runs
neither it nor the embedded-dependency-list assertion - so the `--features bigquery` link is proven
on two triples and in no venue at all for `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-musl`. `nix/shipped.nix`'s `probeFeatures` declarations are untouched and
`cargo xtask check-shipped-binaries` still reconciles them against the documented builds, so the
DECLARATION is intact; what shrank is how many triples exercise it.

**And the half that improves, because it is the reason the pair is this pair.**
`x86_64-unknown-linux-musl` is back on the pull-request leg, where it had not run since that leg
became the aarch64 pair - so the musl-link sentence above is true again for the host architecture on
every pull request, and false for `aarch64-unknown-linux-musl` in ordinary CI. Priced against what
it replaces: the reduction runs two cells on a pull request where four ran before this record's
period and two ran after it, and two on `main` where four ran.
