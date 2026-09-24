---
title: A cross-posture federated answer is disclosed per leg
description: Owner ruling. A federated answer whose legs decide identity differently is answered with one entry per leg naming that leg's posture, instead of refused as LegsDecideIdentityDifferently. Records why the refusal's own reasoning stays true, which control is load-bearing instead of it, what the disclosure does NOT buy, and the four layers re-measured before the refusal was deleted.
---

# A cross-posture federated answer is disclosed per leg

Status: **accepted by owner ruling**. The refusal, its type and its `409` are deleted; the per-leg
record that already shipped beside them is what answers the question now.

## The ruling

> if one of the systems is configured as shared-service user then that one can be used as well but
> in general yes full individualized impersonation.

Per-subject identity for Postgres, Oracle and ClickHouse is **deferred, not abandoned** - upstream
pull requests are open and ClickHouse is wanted. What makes this record necessary rather than an
enhancement is the arithmetic in between: `sutura-exec-bigquery` is the only adapter declaring
`ImpersonationCapability::PerSubjectCredential`, so **every heterogeneous federation is cross-posture
by construction.** Refusing the mix did not refuse an edge case; it refused BigQuery federating with
anything but another BigQuery.

**Who decides, restated by the owner on 2026-09-23 in answer to review 5291270117** - which asked that
the mix be treated as a security exception rather than inferred from the per-leg labels: it is
**temporary and configuration-dependent, and the decision is the operator's.** A deployment that
configures a source as `shared-service-user` - Oracle today, whose per-subject path waits on
upstream - has chosen that its rows are read under that shared identity, and the boot-time
acknowledgement on that source's own entry is where it said so. Mixing those rows with an
impersonated leg is therefore that operator's decision and acceptable from sutura's side; sutura's
part is to refuse an unacknowledged shared source at boot and to disclose, per leg, which identity
read what. Full per-subject impersonation on every source remains the goal; this ruling covers the
interval until upstream lets each adapter deliver it.

## What changes, and it is a net deletion

`ExecutedAs::uniform`, `UniformlyExecuted`, `LegsDecideIdentityDifferently` and
`RefusalReason::LegsDecideIdentityDifferently` are gone, with the `409` arm on both transports, the
agent-facing guide and the prompt bullet. `ExecutedAs` - a `BTreeMap<SourceName, SourcePosture>`,
non-empty by construction, refusing a second leg for a source already recorded - is threaded where
the wrapper was: `PinnedDefinitions::provenance` takes it, `Provenance::executed_as` returns it,
`CallRecord::executed_as` borrows it. A second newtype over a set of posture labels would have been a
second representation of `legs().map(|(_, posture)| posture.as_str())`, so none was built.

**Nothing was added to disclose.** The per-leg record already shipped on every surface - the HTTP
body's `executed_as`, the MCP structured content, the MCP **text** half (`read from <source> as:
<posture>`) and the audit event - with one `{source, posture}` per source read off the adapter that
executed, never off a settings tree.

**Not a breaking response shape.** The field was already a list; a mixed answer fills it with two
*different* posture values instead of two equal ones. No schema edit, and no golden re-record: every
committed golden is mono-source or uniform and keeps its exact bytes. The one behavioural break is
that a request which used to get `409 legs_decide_identity_differently` gets `200`.

## The sentence this record refuses to soften

The deleted refusal's reasoning **stays true**: rows a shared identity was permitted to see, added to
rows the asking subject was permitted to see, make a total no identity is entitled to, under a
certified metric name and with valid provenance attached. Deleting the refusal does not make that
arithmetic go away and this record does not claim it does.

**And the disclosure is not the control.** `executed_as` and `rows` are serialized into one body on
both transports, with no streaming and no second message, so a caller who reads which leg came from
which authorization domain already holds the rows. The only outcome that reaches a caller *before*
the rows is a refusal, and this record removes one.

What answers for the arithmetic is written whole, not as an opt-in:

> A mixed answer does span two authorization domains, an operator declared each one in writing on
> its own entry before this process started, and the answer names which leg came from which.

**Not** *the operator opted in, so it is fine*. The difference is that the first names the mechanism
and the moment - a written declaration, read at boot - and the second names a feeling. What moves
here is `NotFitToServe::SharedSourceNotAcknowledged`, from advisory to **load-bearing**: it used to be
the earlier of two checks over the same fact, and it is now the only one.

## The sentences this record retires, quoted so they cannot come back

Four pages stated the deleted rule in four different words, which is the shape a correction that
lands in one document and is not carried to its siblings takes. Each is quoted here once, in order to
be wrong here and nowhere else, and each is registered in `xtask`'s `CONTRADICTED` table against the
signature that refutes it:

- *a mixed-posture **answer is unconstructible*** - the invariants skill's own row, held by the type.
- *It is refused rather than labelled* - the HTTP refusal's sentence to a caller, and *Refused rather
  than disclosed* on the domain variant that produced it.
- *every leg decides identity the SAME way, or the question is refused* - the query-surface skill's
  change table.
- *one deployment still cannot get two genuinely different POSTURES onto one federated answer* - the
  same skill's four limits.

The first three are false as of this record. The fourth is false for a `--features bigquery` build
and remains true of every published artefact, for a different reason: no release links the
impersonating adapter.

## The four layers re-measured before anything was deleted

An unacknowledged shared source cannot reach this path, and the refusal that was deleted was the
fourth check of a fact three earlier ones already hold:

| # | Layer                                                      | Where                                                                                                                                                                                 | What it refuses                                                                                                                          |
| - | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| 1 | Boot                                                       | `sutura_config::Settings::load` returns `Err(NotFitToServe)` when `refusals()` is non-empty, pushed per source on `source.identity().is_none()` in multi-user mode                    | The whole deployment. **No listener is bound.**                                                                                          |
| 2 | The parse, independently of any refusal                    | `sutura_config::sources`: the witness comes from the entry's own acknowledgement or from the single-user mode declaration, and `"shared-service-user"` maps through `witness.map(..)` | There is no shared `SourcePosture` **value to construct** without a witness, so the posture is `None` and the source carries no identity |
| 3 | Adapter construction                                       | Six sites in `sutura-cli` take `configured.identity()` and fail with *declares no identity a query could run under*                                                                   | No adapter, which arrives on the federated path as `source_unavailable` **before minting**                                               |
| 4 | The bare `sutura` command, the one path that skips layer 1 | `sutura_cli::sources::files` mints its own single-user acknowledgement for the built-in `files` source                                                                                | Stronger than *one source*: that command refuses a catalog spanning two data systems outright, so it has no federated answer to mix      |

Only **two** non-test construction sites of `SharedIdentityDeclared` exist in the tree - the settings
parse and that built-in declaration.

**A second refusal becomes more important and needed no edit.** `Presented::agrees_with` is asked
**per leg**, so cross-posture makes it the check that each leg actually ran as what the record claims:
a subject credential at a shared source, or a shared witness at an impersonating one, is a
`ServiceError::Posture` rather than an answer. Its own limit is unchanged and stated where it lives -
the witness is prose and equality of it is the only comparison, so a byte-identical forgery is
indistinguishable.

## What this does not buy

- **Which shared identity** a leg was read as is still not named anywhere.
  `Presented::SharedServiceUser` carries the acknowledgement witness and no identity, so two
  `shared-service-user` legs may be two deployment-held identities and nothing here tells them apart.
  *Same posture* is decidable; *same asker* is not.
- **Two authorization domains, one budget.** A mixed answer's spend charge sums both legs' estimates
  against **one** subject key (`0030-where-a-budget-lives.md`, all-or-nothing) regardless of which leg
  ran as whom. Already the shape before this record; worth stating because a mixed answer is the first
  case where the two halves are not the same principal.
- **Nothing about a real cross-posture run.** No published artefact links the impersonating adapter,
  and no served binary has executed as a caller - `where-identity-is-proven.md` is the register for
  which venue may be cited for which claim, and this record adds no row to it. What is held is that
  the orchestrator answers the mixed shape, the two transports publish both labels on every half, and
  `agrees_with` still refuses a leg presenting the wrong shape.
- **No narrowing on the deferred adapters.** `SourcePosture::deliverable_by` flips on its own when an
  adapter's `IMPERSONATION` changes, and that mechanism is untouched: the day Postgres, Oracle or
  ClickHouse declares `PerSubjectCredential`, a deployment mixing it with a shared source is answered
  by this record with no code change.

## The order in `federated::answer_federated` is unchanged

Load-bearing, so it is written down: registry lookup, capability gate per leg, `ExecutedAs` off both
adapters, **one** `broker.mint` over the `SourceSet`, `agreeing_with`, both `dry_run_leg`,
`charge_subject`, `run_leg` sequentially, `Legs::of`, `combine`, the working-set ceiling, `top`, the
row cap, the response bound. `top`, the row cap, the response bound and the combiner's byte pool read
no posture and needed no change.
