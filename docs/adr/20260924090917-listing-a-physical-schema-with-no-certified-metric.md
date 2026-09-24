---
title: Listing a physical schema with no certified metric
description: Issue #971. Decides that `sutura_app::prompt`'s onboarding ramp for a bundle with physical structure and zero certified metrics may additionally list every model's and column's NAME - never a description or a type - and only where an operator declares `prompt.physical_schema: listed`. Default is `omitted`, matching the setting every deployment written before this key existed is already in. The trigger stays the ramp's own (zero metrics, whole bundle), which is what makes the listing safe to add with no per-caller audience scoping of its own: a bundle in this shape has no metric an audience grant could have narrowed.
---

# Listing a physical schema with no certified metric

Status: **accepted**.

## The tension

`sutura_app::prompt::physical_schema_guidance` renders "## Physical structure is not a certified
metric" for a bundle that has models and zero metrics - the onboarding shape a fresh `rdbms` or
`okf` catalog produces before anyone has authored a semantic layer over it. Before this record, the
section named that structure exists and named none of it - not the model, not the table, not a
single column - on the argument (still true, and unchanged by this record) that a column name in an
agent's context is a name it will eventually try to use, and this surface has nowhere for one to go.

That argument is right for a *certified* bundle. It is not obviously right for THIS one: a
zero-metric bundle has no `Query::metric` an agent could type a physical name into, because there is
no certified operation over it yet - the whole point of the ramp is "author a metric, then ask
`query`". Naming the physical schema in this specific, narrow shape does not teach an agent to
attempt something the surface refuses; it tells whoever is about to author the metric what they have
to work with, which is the audience this section is already written for ("a person must author
semantic metadata").

## The decision

**A deployment may opt in, by an explicit key, to listing every model's and column's NAME in the
zero-metric ramp - never a description, never a type, and never anything else.** `prompt.
physical_schema: listed | omitted`, default `omitted`.

- **Names only.** A model's own `description` stays untrusted, unquoted prose exactly as the
  section already argues about every other description in this document; nothing here changes that.
  A column's TYPE and its own description are `github.com/telekom/sutura#966`'s deliverable to
  `Model`, not this record's - if that issue lands a type, whether to list it is a decision for
  that record to make, not an extension of this one made in advance of the field existing.
- **`omitted` is the default**, for the reason `CatalogProse::Omitted` is never one:
  the dangerous default here runs the OTHER way from that setting's. There, silence is the safe
  default because an agent handed no prose infers a metric's meaning from its name; here, naming a
  schema an operator has not reviewed for an agent's context is the disclosure to default away from.
  `omitted` is also the state every deployment written before this key existed is already in, so
  turning this record into a mechanism costs no deployment a behaviour it had.
- **The trigger is unchanged: zero metrics, whole bundle.** This is what makes the listing safe
  with no audience-scoping mechanism of its own. `docs/adr/0028`'s per-caller grants narrow which
  METRIC a caller may see; a bundle with none has nothing for a grant to have narrowed, so this key
  can never show one caller more of a metric than their own audiences would. If a future record
  ever wants to list physical structure ALONGSIDE certified metrics - a mixed bundle, some tables
  documented for onboarding while others already have a metric - that is a new decision needing its
  own audience story, not an extension of this one.

## What was measured, and what was not

This is a decision record over a rendering rule, not a spike over an external system - unlike the
`what-<source>-can-carry` findings, there is no field-by-field measurement to report here;
`physical_schema_guidance`'s existing trigger is the code this record read.

## Acceptance, and the test this record decided

`crates/sutura-app/src/prompt/tests/physical_schema.rs`'s
`physical_structure_without_a_metric_gets_the_semantic_promotion_ramp` is **kept**, unchanged: it
does not set `prompt.physical_schema`, so it stays at the default (`Omitted`) and every name stays
hidden, exactly the property it asserted before this key existed.
`physical_structure_without_a_metric_lists_names_when_the_operator_turns_the_key_on` is the
**inverted** counterpart this record adds: over the identical fixture, with `Listed`, the model's
and the column's names both reach the text and the model's own description still does not.

## Alternatives considered

**List column names but not model names, or the reverse.** Considered and declined: a name with no
table to hold it (or a table with no idea what is in it) is a half-answer to "what do I have to work
with", and there is no narrower disclosure story that makes one safer than the other - both are
schema an operator either has or has not reviewed.

**Key it per-catalog rather than per-deployment.** A `catalogs[].list_physical_schema` field would
let one declaring source opt in while another stays silent. Declined for now: the ramp's own trigger
is a property of the WHOLE bundle (zero metrics across every source), so a per-catalog key would be
answering a question the section does not currently ask on a per-source basis either. Revisit if a
mixed bundle (one zero-metric source beside one with metrics) becomes a real deployment shape - the
alternative above about audience scoping applies to it too.

**Ship it as always-on.** Declined per the explicitness argument above: it would be a physical
schema an operator never reviewed reaching an agent's context by default, which is the disclosure
this record exists to gate.

## What would reopen this

`#966` landing a column type/description on `Model`. This record's own limit already states it: what
to do with those two fields is that record's decision, and this key's `listed` value should not be
read as having pre-decided it.
