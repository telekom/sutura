---
title: A refusal carries a status
description: Why a refused question now comes back with an explicit HTTP status as well as its code and its sentence, why the retry argument that justified the 200 does not hold, why the refusal keeps its own body rather than moving to the failure body, and the status chosen for each RefusalReason with the reason for it.
---

# A refusal carries a status

Status: **accepted, and amended once.** It changes what the HTTP transport says about a refusal. It
supersedes nothing and changes no domain type: `ToolOutcome::Refusal` is still a *result* and not an
`Err`, which is the invariant [the tool surface](../architecture.md) is built on and which this record
does not touch.

**Amended by [a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md).** One
sentence below - *"the `403`s are not a statement about a credential"* - stopped being true when
`RefusalReason::CredentialUnavailable` arrived with a `403` of its own. The amendment is written where
that sentence is, and the status table below carries the new row. A review is what found this record
unamended while the code that amended it had already merged.

## Context

`POST /v1/query` answered `200` for both outcomes. An answer and a refusal arrived with the same
status, and the `outcome` field was what a caller branched on:

```json
{ "outcome": "refusal", "reason": { "code": "metric_unknown", "detail": "..." } }
```

The argument written into `routes/v1/query.rs` was this: a refusal is a result, an error status
invites a client library to retry, and retrying a governance decision until it succeeds is exactly
the behaviour the refusal exists to prevent.

The second half of that is right and is the reason this design has refusals at all. The first half is
a claim about client libraries, and this repository's rule is to reproduce rather than assert. It was
checked against the current documentation of four:

| Client | What its documentation says about retrying |
| --- | --- |
| `urllib3.util.Retry` (what `requests` mounts via `HTTPAdapter`) | Status-based retries come from `status_forcelist`, "a set of integer HTTP status codes that we should force a retry on", whose default is `None` - "By default, this is disabled with `None`." No status is retried until somebody names one |
| `reqwest` 0.13 | Its `retry` module documents the default policy as "to only retry requests where an error or low-level protocol NACK is encountered that is known to be safe to retry" - a transport condition, not a response status |
| `axios` + `axios-retry` | `axios` retries nothing on its own. The plugin defaults `retryCondition` to `isNetworkOrIdempotentRequestError`: "By default, it retries if it is a network error or a 5xx error on an idempotent request (GET, HEAD, OPTIONS, PUT or DELETE)" |
| Go `net/http` | The `Client`, `Transport` and `RoundTripper` reference documents no status-driven retry at all |

Nothing mainstream retries a `4xx` by default. The two statuses retried by convention are `429` and
`408`, and no refusal maps to either. `422`, where four of the eleven refusal codes now land, is
documented the other way round from the premise: clients receiving a `422` "should expect that
repeating the request without modification will fail with the same error".

**And the `200` cost something the argument never priced.** A governance refusal answered `200` is
indistinguishable from an answer to everything that reads a status and not a body:

- an ingress or proxy access log, and every dashboard built on one;
- an error-rate alert, and therefore an on-call rotation;
- a client's `raise_for_status()`, `response.ok`, or a generated client whose success branch is `2xx`;
- anyone reading a `curl -i` transcript.

A deployment refusing every question read as perfectly healthy. The refusal was legible only to code
written against this specific envelope - which is the one reader that never needed convincing.

The concrete case that forced the decision is `result_too_large`: an answer that exceeded the row cap
was declined rather than truncated, because a partial total under a certified name is wrong in the one
way nothing downstream can detect - and the caller was told about it with a `200`.

## Decision

**A refused question comes back with an explicit HTTP status, the machine-readable `code` it always
carried, and a sentence saying what to change.** All three, so a caller is told the same thing
whether it reads the status, branches on the code, or shows the prose to a person.

`200` now means the question was **answered** and nothing else means that.

### The statuses

One exhaustive match in `sutura_http::wire::refusal`, no wildcard arm, deciding the status, the code
and the sentence together. A `RefusalReason` variant added to the domain fails to compile there until
somebody decides what it is on the wire - the same mechanism that already stopped a new governance
outcome from reaching a caller as an unnamed one, extended to cover the status.

| `RefusalReason` | Status | Why |
| --- | --- | --- |
| `MetricUnknown` | `404` | The name does not resolve in this snapshot. `GET /v1/catalog` is where the ones that do resolve are listed |
| `GrainNotSupported` | `422` | The metric exists and that grain is not rendered for it. Not `404`, which would send the caller looking for the wrong mistake |
| `TimeRangeTooLong` | `422` | The range parsed and both endpoints are real dates. Well formed, out of bounds |
| `TooManyDimensions` | `422` | Same, and the sentence carries the maximum so narrowing needs no second request |
| `DuplicateDimension` | `422` | Same. Refused rather than deduplicated, because a caller who sent it twice believes something we do not |
| `DimensionNotPermitted` | `403` | The metric declares no such dimension |
| `DimensionNotFilterable` | `403` | The dimension can be grouped by and not filtered on |
| `DimensionValueNotAllowed` | `403` | The value is outside the declared allowlist. The value itself is never echoed back |
| `PlanSpansTooManySources` | `409` | Answerable in principle, and this deployment will not read from more data systems than it was arranged to combine. A conflict between what was asked and how the deployment is arranged, which is what no status about the request's own content would say |
| `FederationNotExecutable` | `409` | Exactly two data systems are served, but this build has no adapter that can execute one leg of a two-source question, so it cannot be answered yet. The author of [0009](0009-the-plan-from-one-source-to-many.md) wanted it refused here rather than surfaced as a retryable `503`: it is not a data system being down, and a caller must not retry it |
| `FederationLinkAmbiguous` | `409` | The question's remote dimensions join the metric's own through more than one relationship, and the combiner links its two legs on a single column |
| `MeasureDoesNotFederate` | `409` | A measure a single data system computes (a distinct count) cannot be recombined above two legs, so the splitter refuses it rather than pull its rows up through a combiner that would have to re-count |
| `PlanTablesShareAnIdentifier` | `409` | **Added by [0019](0019-a-table-outside-the-connections-dataset.md) Decision 5.** Two tables the plan would read end their paths in one name, so a column qualified by it names neither. The same grouping as the rows above and for the same reason: answerable in principle, and this deployment cannot express it as one statement. Unlike those rows there usually *is* another question - a dimension needing no join - so the sentence says so, and it names the identifier and neither path: the identifier is what a person can act on, and a path carries the project and dataset a deployment reads |
| `ResultTooLarge` | `413` | The answer did not fit the certifiable cap. Narrowing helps and repeating does not. **Amended after this record was accepted:** the variant now carries a `ResultBound` and covers a second bound - a data system that will not hand the result back in one piece - at the same status and the same code, because *too much data* is one answer and one remedy to a caller. Only the sentence differs, from a nested exhaustive match, and the second bound's carries no number: the bound belongs to the data system and is not reported to us. The widening exists because that case used to arrive as `503`, which this record calls *the one refusal where retrying is reasonable* - and a retry returned the same reply |
| `SourceUnavailable` | `503` | The one refusal where retrying is reasonable |
| `CredentialUnavailable` | `403` | **Added by [0008](0008-a-credential-per-leg-for-the-calling-subject.md), and it is the row that amends the note below.** The asking subject has no credential at the data system the plan reads, and this deployment will not read it under its own identity instead. `403` rather than `401`: re-authenticating to *this* service changes nothing, because the missing grant is at the data system. The sentence says so in as many words and a test asserts it does, precisely so a client cannot read the status as "authenticate harder" and retry forever |

Three groupings are deliberate rather than a shortage of numbers. The status is what a monitor counts
and the `code` is what a client branches on, so variants are grouped by **what the caller should do**:
narrow the question (`422`), ask something the catalog permits (`403`), or stop (`409`). `problem.rs`
already took the same position, where `unavailable` and `at_capacity` share `503` and differ in code.

**The `403`s are not a statement about a credential.** This surface has no per-caller identity - the
token authenticates the deployment - so no token widens a metric's dimension set. `403` is used in its
"understood the request, refuses to fulfil it" sense, and each sentence names the metric and the
dimension so a caller cannot read it as "go and get a better token".

> **Amended by [a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md).** That
> paragraph is now true of every `403` on this surface **except one**. `CredentialUnavailable` *is* a
> statement about a credential - the asking subject's, at the data system, which is not the one they
> presented here - and it exists because a question that cannot run as the subject is refused rather
> than answered under the deployment's own identity. Both halves of the original reasoning survive in
> the narrower form: no token presented to this service widens anything, and the sentence still must
> not read as "go and get a better token", which is why the detail says the missing grant is at the
> data system and that repeating the question returns the same refusal. What is no longer true is the
> unqualified sentence, and the reason is written here rather than left for a reader to notice that
> the table above grew a row the paragraph contradicts.

### Two statuses are shared with something that is not a refusal

`code` separates them, and so does the body: only a refusal carries `outcome`.

- `413` is `too_large` when the **request body** was over the configured limit, and
  `result_too_large` when the **answer** was over the row cap.
- `503` is `unavailable` or `at_capacity` from the failure side, and `source_unavailable` from the
  refusal side.

### No `Retry-After` on any refusal

`Failure::retry_after` already set the rule for this surface: a number that is already known, or no
header, because a guessed number is a promise. The only failure that carries one is `at_capacity`,
where the number is the admission window the caller just spent waiting out. Nothing here knows when a
data system will come back, so `source_unavailable` carries none - and neither does
`Failure::Unavailable`, which is the same situation reached from the other side. A refusal that
invented one would make the two disagree.

### The body keeps its own envelope

**A refusal keeps `crate::wire::OutcomeBody::Refusal` and is NOT moved to `ProblemBody`.** Four
reasons, in the order they decided it.

**1. A refusal is not a `Failure`, and the type system should keep saying so.** `ProblemBody` is
reachable only through `Failure`, which is what an `Err` becomes. Routing a refusal through it needs a
`Failure::Refused` variant, which would put a governance *result* into the error enum - and the whole
point of `ToolOutcome::Refusal` being a variant of the result is that no code path can mistake it for
a transport hiccup. The status changed; the type did not, and this is where that is kept true.

**2. There is no RFC 9457 shape to move to.** It is worth stating plainly, because the obvious
consistency argument assumes one. `ProblemBody` is this repository's own three-field failure body -
`code`, `status`, `detail` - served as `application/json`. It carries no `type`, `title` or `instance`
member and is not served as `application/problem+json`; the string "9457" appears nowhere in the
repository. So the choice was never "our envelope or the standard one". It was "our refusal envelope
or our failure envelope".

**3. The refusal envelope is the existing contract.** Clients read `outcome` and `reason.code`, the
interface description declares them, and the golden corpus and the example session show them. This
change is additive: nothing was removed or renamed.

**4. Discriminability is worth more than uniformity exactly where the statuses collide.** With two
body shapes, `outcome` is a one-field test for whether the governance layer decided or the transport
failed - which is what a caller needs at `413` and at `503`, where the same number arrives from both
sides.

The cost, stated rather than waved away: a caller now parses two failure-ish shapes on one route. It
is bounded, because both carry a `code` and a `detail` at a known place, and the OpenAPI document
declares both per status with the codes that reach it.

One field was added inside `reason`: `status`, the same `u16` the response carries. For the same
reason `ProblemBody` has it - a client that logged only the body still has it - and it makes the two
bodies near-identical in what they carry.

## Consequences

- **Nothing about the domain moves.** `ToolOutcome`, `RefusalReason` and every rule about what may be
  asked are untouched. This decision is a transport mapping and lives entirely in `sutura-http`.
- **The compile is the gate.** A new `RefusalReason` variant does not build until it has a status, a
  code and a sentence. There is no `_ =>` arm and there must not be one: a default would hand a new
  governance outcome to callers under whatever number happened to be chosen first.
- **`413` is arguably wrong by the letter of the spec, and is used anyway.** `413 Content Too Large`
  is defined over the *request* content - "the request entity was larger than limits defined by
  server" - and what is too large in `ResultTooLarge` is the answer. It is recorded here rather than
  hidden. It is used because `413` is the status a person reading a dashboard reads as "too large",
  which is precisely what this refusal has to be unmistakable about, and because the collision with
  the body limit on the same route is resolved by `code` and by the body shape. If a reader disagrees,
  `422` is the alternative and this paragraph is the argument to answer.
- **`DimensionNotPermitted` at `403` was the other close call.** The domain calls it "a name that does
  not resolve", which reads like `422`. It stays at `403` because what the name is checked against is
  the metric's *declared* dimension set - the same catalog statement `DimensionNotFilterable` and
  `DimensionValueNotAllowed` read at finer grain - so a monitor counting attempts to ask outside the
  catalog gets one number rather than two.
- **`SourceUnavailable` is the only refusal a caller should retry, and today's cause is usually not
  transient.** The variant is documented as the one an identity failure will also use, and today it is
  raised by a name comparison: the plan's data system against the adapter this process opened. So a
  `503` here often means a deployment wired wrong rather than one briefly unwell. The status is right
  for the variant's meaning; the sentence is what an operator reads.
- **The example session under `examples/single-player/README.md` showed captured `200 OK`
  transcripts for refusals. It is deleted rather than re-captured** - the residual this bullet
  recorded is closed that way, and the reasoning is worth keeping: a transcript nobody runs goes
  stale in exactly this manner and says nothing while it does. What replaced it is
  `crates/sutura-http/src/wire/refusal.rs`, and it is the strongest of the three: `every_reason()`
  lists all seventeen `RefusalReason` variants with the status and `code` each is given,
  `every_refusal_carries_a_status_a_code_and_a_sentence` drives that list, and the exhaustive match
  with no wildcard arm makes a new variant a COMPILE error rather than an untested one - which is
  the property a transcript could never have. `crates/sutura-serve/tests/served.rs` asserts a
  refusal's status against the same directory over a real port, and
  `crates/sutura-http/src/harness.rs` asserts the envelope per status in process for **ten** of the
  seventeen reasons, the seven it does not reach being `DimensionNotFilterable`,
  `PlanSpansTooManySources`, `FederationLinkAmbiguous`, `MeasureDoesNotFederate`,
  `PlanTablesShareAnIdentifier`, `ResourcesExhausted` and `CredentialUnavailable` - each of which
  needs a fixture the default pair cannot produce. So the exhaustive claim is the unit table's, not
  the transport harness's.
- **The agent prompt still tells an agent a refusal is a successful call.** That is true at the tool
  level, where `sutura_app::prompt` speaks; over HTTP it is no longer true of the status. Whether the
  prompt should mention a transport at all is a separate decision.

## Alternatives considered

**Keep the `200` and document it harder.** The status quo, and it needs no work. Rejected because the
problem is not that callers were under-informed - it is that a whole class of reader never sees the
body at all, and no amount of documentation reaches a proxy access log or an error-rate alert.

**One status for every refusal, `422` for all of them.** Simple, honest about "well formed and
declined", and one number to document. Rejected because it throws away the part a caller can act on
without parsing: `metric_unknown` and `result_too_large` need different responses, and collapsing them
means the status carries no information beyond "not answered" - which is barely better than the `200`.

**Move refusals to `ProblemBody`.** One failure shape for the whole surface, which is a real
consistency win. Rejected for the four reasons above, the first being decisive: it needs a
`Failure::Refused` variant, and putting a governance result into the error enum is the exact confusion
`ToolOutcome::Refusal` exists to prevent.

**A `Retry-After` on `SourceUnavailable`.** Cheap to add and it looks helpful. Rejected because the
number would be invented, which is the one thing `Failure::retry_after` already refuses to do, and
because the same situation reached from the failure side carries no header - two answers to one
question is worse than one incomplete answer.
