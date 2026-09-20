---
title: A knowledge-only metadata source speaks through a metric, or it says nothing
description: Whether a metadata source may contribute knowledge and no definitions, decided against ADR 0011's withdrawn claim and the knowledge layer's own shapes - a source MAY, but only through the existing metric-anchored Referent channel, rendering under the metric and never in the preamble, so a knowledge-only deployment with no certified metric contributes nothing and that is correct, not a bug. The reserved number this record was scheduled under (0032) was superseded by six later records; it takes the next free number, 0036.
---

# A knowledge-only metadata source speaks through a metric, or it says nothing

Status: **a decision record** (issue #154, step 1). A later spike decides whether a real BPMN model
names a metric this deployment has already certified - the only way a BPMN source could contribute
anything under this record's answer.

Issue #154 asks, of BPMN specifically, whether a metadata source may contribute **knowledge and no
definitions**. BPMN models a process, not a table: no measure, no grain, no dimension, no table, no
join column - almost nothing the definition side of a bundle can hold, and (plausibly) prose about what
a number means in a process context. That is the knowledge layer or nothing.

ADR 0011 already withdrew the claim that a narrow source could contribute source-level usage prose,
and it withdrew it for precisely the reason that applies here in full. This record makes the surviving
position a decision, at the next free number, and answers the three costs 0011 named so that the next
proposal to reopen it starts from the cost rather than the idea.

## The mechanism, read rather than remembered

A note in this repository is attached to a `Referent`, and `Referent` has three variants -
`Metric { metric }`, `Dimension { metric, dimension }`, `Value { metric, dimension, value }` - each of
which carries a `MetricName`. `Referent::metric()` returns `&MetricName` with no `Option` in its
signature, and `Knowledge::caveats_about` retrieves notes by comparing that metric against the metric a
question named. The retrieval path is keyed on a metric, the prompt prints a caveat under the metric
it is about, and an unscoped note is refused at load as `InconsistentKnowledge::CaveatAboutNothing`.
There is no source variant of `Referent`; there is no `rules` kind; there is no rendering position that
is not under a metric. That is the whole of the channel a metadata source has to a reader.

## The decision

**A metadata source MAY contribute knowledge and no definitions, and it does so only through the
existing metric-anchored `Referent` channel, rendering under the metric, and never in the preamble.**
This is not a new channel: it is 0011's composition model stated as a decision - "what a source may
contribute is a note attached to a metric" - made explicit for a source that has no definition kinds at
all. The three things a knowledge-only source therefore cannot do are not gaps; each is the shape of
the channel:

1. **A knowledge-only source speaks only about metrics something else defined.** A deployment whose
   only metadata source is BPMN has no certified metric for a note to attach to, so it contributes
   nothing, and that is not a bug. `Definitions::assemble` has no minimum-metric refusal, so such a
   bundle still loads, validates and pins - it just carries the knowledge someone else cannot provide.
2. **Nothing reaches the preamble.** A note renders under the metric it is about. The `rules` kind and
   a `Referent::Source` are the shapes that would render in the preamble, and both are refused: a
   `rules`-shaped body of prose scoped to nothing is the unscoped global text channel 0011 declined, and
   the prompt's injection answer depends on that channel not existing. This record does not reopen that.
3. **Declaring a knowledge capability moves every composed deployment's digest.** `KnowledgeCapabilities`
   is hashed with the rest of the bundle, and `Knowledge::assemble` refuses, as
   `InconsistentKnowledge::UndeclaredContent`, a bundle that supplies content for a capability nothing
   declared. So a knowledge-only source declares, at most, the `Capability` kinds it can actually
   produce - which, under cost 1, it can produce only where a composed deployment certified the metric.

## What this means for BPMN

Whether BPMN is a knowledge-only source at all is not answered by a decision about the channel; it is
answered by whether a real BPMN model names a metric this deployment has already certified, in a form
that can be matched **at load** - nothing server-side may match a harvested phrase against a certified
metric at request time, because there is no `PhraseNotDefined` and the agent states its own choice. A
BPMN document element that carries, say, a business-rule prose attached to a column name would need the
deployment's own model-to-metric map to be resolvable when the bundle is assembled. That is issue #154's
step-2 spike (`spike/what-a-bpmn-file-actually-carries`), read field by field in ADR 0016's method.

Until that spike shows a load-matchable metric reference, **no crate is built for BPMN**, and #154 can
close with this record - which is what 0016 did for a claim that did not survive. If the spike does find
a matchable reference, the adapter is a `declaring` source with **no definition kinds and the one
`Capability` it can produce**, through this record's channel, with the issue's provable cells
(`a_knowledge_only_source_contributes_notes_attached_to_metrics_it_did_not_define`,
`an_unscoped_caveat_from_this_source_fails_the_load`), and `nothing_it_supplies_reaches_the_prompt_preamble`
as an invariant.

## Alternatives considered

**Add a `Referent::Source` variant or a `rules` knowledge kind.** ADR 0011 priced this in full: a
fourth variant with no metric rewrites `Referent::metric()` to return `Option` and both of its readers
(`fault_in`, `caveats_about`); a fifth `Capability` rewrites four exhaustive matches and the const
assertion that guards `Knowledge::assemble`'s capability walk; a source-name validation is a new kind of
check (a source name is in the composition, not the definitions); and a preamble rendering position is
exactly the `rules` prohibition. All of it is possible; all of it is a decision about the prompt's trust
boundary, and it belongs in its own record with its own argument. This record declines to reopen it.

**Route what a BPMN model says through first-party prompt text or derived content.** The two concrete
cases 0011's table put there are available to a BPMN source too - prose about what a number means in a
process context is only expressible as a note under a certified metric; and a fact like "this deployment
carries no certified metric layer" is derived from the pinned bundle, not authored by a source.

## Decision

The general answer is **yes**: a knowledge-only metadata source is legal, and it speaks through the
existing metric-anchored `Referent` channel, rendering under the metric, never in the preamble. The
BPMN-specific answer is the spike's: whether a BPMN document names a certified metric at load. The
reserved number this issue was scheduled under (0032) was consumed by six later records; this is 0036.
