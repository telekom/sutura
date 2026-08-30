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
  right as far as four mechanisms can tell, and nobody has run one.*
