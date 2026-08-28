---
title: Pluggable by declaration, and the mode a data adapter is in
description: Every adapter is configured by a typed declaration rather than discovered - a metadata adapter declares which kinds of metadata it provides, and a data adapter declares whether it reaches a source as a shared service user or as the asking subject - so a capability nobody declared cannot be used, an answer records which mode produced it, and adding a capability is a compile error everywhere it has to be decided.
---

# Pluggable by declaration, and the mode a data adapter is in

Status: **accepted. The metadata half has a working precedent; the data half is new.**

Pluggable metadata and pluggable data are requirements. So is a tight security focus, and the two pull
in opposite directions unless pluggability is built one specific way: **a closed set of typed
declarations, checked exhaustively, rather than a plugin system that discovers what an adapter can
do.** This record is that construction.

## The decision

**Every adapter is configured on its declared capability, and a capability nobody declared cannot be
used.** Two axes, because the two ports answer different questions:

- A **metadata** adapter declares WHICH KINDS of metadata it provides.
- A **data** adapter declares WHICH MODE it is in, and the modes are
  `SharedServiceUser` and `ImpersonationAtSource`.

Neither declaration has a permissive default. An adapter that does not say does not get the benefit of
the doubt, because the failure of a default here is silent: content served that nobody vouched for, or
a source read under one identity for everybody while a deployment believes otherwise.

## Metadata: what the adapter provides

The precedent exists and works, and this decision generalises it rather than inventing anything.
Knowledge capabilities are already declared per provider, `Capability::every()` walks them through
exhaustive matches, and content for a kind the provider did not declare **fails the load** as
`UndeclaredContent` naming what happened and where.

Three properties of that construction are the reason it is the pattern to extend:

1. **Emptiness cannot carry the distinction.** *Declared and empty* means the prompt may say nothing
   is known to be undefined. *Not declared* means the prompt must not imply the absence list is
   complete. A map with no entries cannot tell those apart, which is why the declaration exists at
   all.
2. **The walk cannot be walked past.** The successor and predecessor functions are exhaustive matches,
   a `const` assertion holds the seed of the walk, and a third exhaustive match decides what each
   capability licenses a document to say. A new kind is a compile error in every place that has to
   decide about it.
3. **The declaration is under the digest.** A deployment that quietly stopped declaring a kind has
   changed what its prompt claims, and provenance that did not move would certify the old claim.

Extending it to the rest of a metadata adapter's surface - metrics, dimensions, relationships,
lineage, whatever a Datahub or OpenMetadata adapter can and cannot answer - keeps all three. The
vocabulary stays closed and the enum stays the contract.

## Data: which mode the adapter is in

**A data adapter declares its mode, and the mode is what the security argument keys on.**

| Mode | What it means | What decides what a subject sees |
| --- | --- | --- |
| `SharedServiceUser` | Every query reaches the source under one identity the deployment holds | That identity's grants. Every caller sees the same rows |
| `ImpersonationAtSource` | Each query reaches the source as the asking subject | The SOURCE: its IAM, its row and column policies, its own catalog |

**Sensitivity is not declared here, and that is deliberate.** What a person may see lives in the data
catalog and in that person's own permissions at the source. Sutura does not carry a per-dataset
classification, does not derive one upward through joins, and does not refuse a question because a
dataset was labelled. **That is the whole reason impersonation exists:** the source is the thing that
knows, and reaching it as the subject is what lets it decide. A sensitivity flag here would be a second
opinion about someone else's authorization, which is the failure this design is arranged against.

Consequences:

- **`SharedServiceUser` is honest, not broken.** It is right for a single-user deployment - static
  credentials, one user, one host, and those are development and proof-of-concept shapes - and it is
  right for a source nobody needs to see per subject. The failure is never the mode; it is a source in
  that mode being BELIEVED to impersonate.
- **The mode is recorded per leg, in provenance.** An answer therefore says how it was executed, so
  "everyone sees this account's rows" is visible in the answer rather than inferred from a deployment
  diagram. That is what replaces a classification refusal.
- **The mode is configuration, not catalog.** The same bundle may be served by a deployment that
  impersonates and one that does not, so the mode travels BESIDE the definition digest rather than
  under it, and the same catalog digests identically in both.
- **A mode the deployment cannot DELIVER refuses at boot.** A source declared `ImpersonationAtSource`
  while nothing can mint a credential for the asking subject is a configuration that would have to
  fall back, and there is no fallback: the execution port takes a credential. Refused at startup,
  naming the source, because a misconfigured deployment must not serve one question.
- **Mutual TLS does not change the mode.** A channel authenticated by a client certificate is still
  `SharedServiceUser` unless the subject's own identity reaches the source inside it. Both are
  declared, and they are independent.

Every data connector is configured this way, DuckDB included. DuckDB's mode is `SharedServiceUser`,
because one process holds one connection under one operating-system identity, and saying so explicitly
is worth more than treating it as a special case. PostgreSQL 18, BigQuery and Oracle can each be
`ImpersonationAtSource`, by three different mechanisms, which is what makes the mode a
declaration rather than something derivable from the adapter's name.

## Capabilities beyond the mode

A data adapter also declares what it can do, for the same reason a metadata adapter does: so the
conformance suite skips only what a source said it cannot do, loudly, with the declaration named,
rather than a test being edited or a gap reading green. Which dialect it renders. Whether it can
receive a pushed aggregate of a given kind. Whether it returns Arrow natively. Whether it supports
mutual TLS.

That is the same enum-as-contract discipline, and it earns its keep twice: the matrix uses it to
decide what to run, and the startup check uses the mode to refuse a deployment that declares an
impersonation it has no way to perform.

## What pluggable does NOT mean here

- **No dynamic loading and no scripting.** There is no interpreter in the query path and no dependency
  that would add one. An adapter is a crate that implements a port and is registered at composition
  time.
- **No capability negotiated at run time.** A declaration is read at load and does not change while
  the process lives. A source that gains a capability is a deployment change, which is what lets a
  startup check mean anything.
- **No adapter-specific vocabulary leaking inward.** The real test of pluggability is not the trait,
  it is whether the pinned bundle stays catalog-neutral. The moment a provider's own identifier,
  aspect name or property shape appears in the domain's public types, the port is decorative and the
  second adapter is the only thing that proves otherwise. The local markdown adapter is that second
  adapter, which is why it is not optional even where it looks small.

## Consequences

- Two declarations become part of every adapter's registration, and both are typed. The conformance
  matrix reads them, the boot check reads the mode, and provenance records the mode per leg.
- A new capability of either kind is a compile error in every exhaustive match that must decide about
  it. That is the intended cost and it is the mechanism, not a side effect.
- The mode makes one thing impossible to state accidentally: that a deployment impersonates when it
  does not. Everything else in this record follows from wanting that one property to be unrepresentable
  rather than reviewed.

## The connectors this has to carry

The declaration exists to make this list additive rather than structural. Every entry is a target, and
the ones that exist today are marked.

**Metadata**, declaring which kinds it provides:

| Connector | State |
| --- | --- |
| Wren-style markdown and YAML | ships |
| OKF-style markdown and YAML | target |
| Datahub | target |
| OpenMetadata | target |
| A custom data catalog over an RDBMS | target, and the one with an extra obligation: how an agent is meant to use it is prompt content, so it declares that and the prompt renders it |
| BPMN | target |
| RDF | target |

**Data**, declaring a mode and its capabilities:

| Connector | Mode it can declare |
| --- | --- |
| DuckDB | `SharedServiceUser`. Ships as a development dependency today |
| PostgreSQL | either, and `ImpersonationAtSource` from 18 through the native OAuth method |
| BigQuery | either, with impersonation through a federated exchange whose principal is the person |
| Oracle | either, with impersonation through proxy authentication, which records the chain natively |

Two things this list is meant to make obvious. The metadata side is where most of the growth is, and
none of it touches the query path: a metadata connector answers what a metric MEANS. And the data side
is four connectors and one mode declaration each, which is the whole security surface of pluggability -
not four adapters each with an opinion about authorization.
