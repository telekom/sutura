---
title: Pluggable by declaration, and the mode a data adapter is in
description: Every adapter is configured by a typed declaration rather than discovered - a metadata adapter declares which kinds of metadata it provides, and a data adapter declares whether it reaches a source as a shared service user or as the asking subject - so a capability nobody declared cannot be used, a critical dataset cannot sit behind a shared identity, and adding a capability is a compile error everywhere it has to be decided.
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
a critical dataset read under one identity for everybody.

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

| Mode | What it means | Where it may be used |
| --- | --- | --- |
| `SharedServiceUser` | Every query reaches the source under one identity the deployment holds | Single-user deployments, for anything. Multi-user deployments, for NON-CRITICAL data only |
| `ImpersonationAtSource` | Each query reaches the source as the asking subject, and the source enforces its own policy for that subject | Anywhere, including critical data |

Consequences, and the first is the one that has to be mechanical:

- **A critical dataset behind a `SharedServiceUser` source in a multi-user deployment refuses at
  boot**, naming the dataset and the source. Not at query time, because a misconfigured deployment
  must never serve one question. A plan-time refusal stays behind it for a catalog reloaded while the
  process lives.
- **`SharedServiceUser` is not a defect.** It is correct for a single-user deployment, where
  credentials are static configuration and there is one user on one host, and it is correct in
  multi-user for data nobody needs to see per-subject. Declaring it honestly is the point; the failure
  is a source in that mode being *believed* to impersonate.
- **The mode is not a property of the catalog.** The same catalog may be served by a deployment that
  impersonates and one that does not, so the mode travels in provenance BESIDE the definition digest
  rather than under it, and the same bundle digests identically in both.
- **Mutual TLS does not change the mode.** A channel authenticated by a client certificate is still
  `SharedServiceUser` unless the subject's own identity reaches the source inside it. The two are
  independent and both are declared.

Per source, as far as it is known today: DuckDB is permanently `SharedServiceUser`, because one
process holds one connection under one operating-system identity. PostgreSQL 18, BigQuery and Oracle
can each be `ImpersonationAtSource`, by three different mechanisms, which is what makes the mode a
declaration rather than something derivable from the adapter's name.

## Capabilities beyond the mode

A data adapter also declares what it can do, for the same reason a metadata adapter does: so the
conformance suite skips only what a source said it cannot do, loudly, with the declaration named,
rather than a test being edited or a gap reading green. Which dialect it renders. Whether it can
receive a pushed aggregate of a given kind. Whether it returns Arrow natively. Whether it supports
mutual TLS.

That is the same enum-as-contract discipline, and it earns its keep twice: the matrix uses it to
decide what to run, and the boot check uses the mode for the refusal above.

## What pluggable does NOT mean here

- **No dynamic loading and no scripting.** There is no interpreter in the query path and no dependency
  that would add one. An adapter is a crate that implements a port and is registered at composition
  time.
- **No capability negotiated at run time.** A declaration is read at load and does not change while
  the process lives. A source that gains a capability is a deployment change, which is what makes the
  boot refusal meaningful.
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
