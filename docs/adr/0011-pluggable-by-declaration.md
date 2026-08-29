---
title: Pluggable by declaration, and the mode a data adapter is in
description: Every adapter is configured by a typed declaration rather than discovered - a metadata adapter declares which kinds of metadata it provides, and a data adapter declares whether it reaches a source as a shared service user or as the asking subject - so a capability nobody declared cannot be used, an answer records which mode produced it, and adding a capability is a compile error everywhere it has to be decided; plus what composing several metadata sources actually requires, which is a contribution manifest under the definition digest because today that digest covers the assembly and not the composition, and the one claim this record withdrew - a narrow source cannot contribute source-level usage prose to the prompt, because every note the knowledge layer carries is attached to a metric.
---

# Pluggable by declaration, and the mode a data adapter is in

Status: **accepted. The metadata half has a working precedent; the data half is new; the composition
half needs a change to what the definition digest is taken over.**

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

## How long a metadata answer may be cached

A metadata connector that is a live service puts that service on the query path. Datahub or
OpenMetadata being down would then stop sutura answering, and it does not, because of something already
built: **definitions are PINNED.** The load takes no request context, the bundle is read once and
hashed, and a request reads the pinned copy rather than the catalog. A metadata outage is therefore
survivable, which is an unclaimed benefit of pinning rather than a feature anyone added for this.

What is missing is the refresh, and it is **configurable per metadata source**:

- **A TTL per source**, configurable, on which the pinned bundle is refreshed.
- **A refresh swaps a WHOLE validated bundle, atomically, and bumps the definition version.** Never one
  metric's definition in place. Two replicas answering from two halves of a bundle is precisely the
  failure pinning exists to prevent, and a partial swap is how it arrives.
- **An unreachable catalog at refresh time keeps the last validated bundle serving, and says so
  loudly.** This is the same rule as the certificate swap in
  [transport security](0010-transport-security-for-a-source.md): reloading into a broken state is worse
  than not reloading, because it breaks everything rather than leaving something stale.
- **An unreachable catalog at STARTUP, with no bundle, is fail-closed.** There is nothing to serve and
  serving nothing is the correct answer.

**A refresh re-runs the anchors, and it has no caller - so it runs as whoever the source declared.**
This is the question that recurs on a timer, and it is decided rather than open:
[a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) names this exact path -
the anchor check reaches the execution port from the composition root before any caller exists, and a
TTL refresh reaches the same path with no caller either - and gives it its own identity type and its
own port method. What that means here, three consequences and no new mechanism:

- **On a `SharedServiceUser` source the verification identity IS the shared identity**, so a refresh
  validates under exactly the identity every question will run under, and nothing extra is configured.
- **On an `ImpersonationAtSource` source it is a declared, static, least-authority credential in that
  source's own entry**, and if none is declared then a bundle carrying an anchor on a metric that
  reads that source does not boot. A refresh therefore cannot introduce an anchor it has no way to
  validate: the anchor either had an identity at boot or the deployment never started.
- **An anchor that FAILS at refresh keeps the last validated bundle serving and says so loudly**,
  which is the same rule as an unreachable catalog above and for the same reason. What it is not is a
  partial adoption: a bundle whose anchors did not all pass is not `Validated`, so there is no state in
  which some of it swapped.

The limit that record states applies here word for word and is worth repeating rather than
cross-referencing, because a timer makes it recurrent: **a refreshed bundle is proven to compute its
certified numbers for one declared identity**, not for the asking subject, and under row-level security
a per-subject anchor is a function rather than a number.

**Replicas refresh independently, and for up to one interval they serve different digests. Accepted,
with the reason and the limit.** Each replica polls its own TTL, so two replicas can hold two adjacent
bundle versions at the same moment. That is NOT the failure pinning exists to prevent, and the
distinction is the whole of why this is acceptable: the refused failure is one ANSWER assembled from two
halves of a bundle, and it cannot happen because a request reads one pinned bundle from first resolution
to last leg. Two replicas each serving a whole, internally consistent bundle is a different thing, and
the answer says which one produced it - the digest travels with the number, which is exactly what that
field is for.

What it costs, stated rather than implied: **two answers to the same question, a minute apart, may
differ and be both correct**, and the only thing that says so is the pair of digests. Anyone comparing
two numbers across a refresh boundary is comparing two definitions, and a client that ignores the
digest will read a definition change as a data change. That is a real sharp edge and it is the price of
not coordinating.

**Coordinated refresh was the alternative and is declined.** Making every replica adopt a version at
the same instant needs something that hands out the version - a lease, an election, a shared store - and
that is a coordination dependency in front of the thing whose entire purpose was to remove one: the
bundle is pinned so that a metadata outage is survivable. Trading that for a window measured in one TTL
is the wrong trade. A deployment that genuinely needs the window closed shortens the TTL, which shrinks
it, or refreshes by replacing replicas, which eliminates it by never having two versions live in one
replica set.

**The cache is sound because it holds no rows - and only while the metadata view is not per subject.**
In `SharedServiceUser` mode one view exists, so one cached bundle is the whole truth and a TTL is a
freshness question rather than a security one. If a metadata source ever impersonates, meaning a catalog
that shows different metrics to different people, then a shared cached bundle would serve one person's
view to another. The rule is then the same one that governs rows: **keyed on the subject first, or not
cached at all. There is no third option.**

Worth separating from that, because they are easy to conflate: filtering what a caller may SEE out of an
immutable pinned set is per request and is not cached. What is cached is the definitions, which are the
same for everyone by construction - "a metric means one thing" is the reason they are pinned in the
first place. Visibility is a filter over that set and never a source of definitions.

## Data: which mode a source is in, and which capability the adapter has

**Two declarations, by two different declarers, and conflating them is how a mode acquires two
owners.** An earlier version of this heading said "a data adapter declares its mode", which
contradicted this record's own consequence four sections down - *the mode is configuration, not
catalog*. Precisely:

- **The adapter declares a CAPABILITY**, in code, as a required associated item it cannot omit: whether
  it can carry a per-subject credential at all. `sutura-exec-datafusion` over files cannot, and says so.
- **Configuration declares the MODE**, per source: which of the two below this deployment is asking for.
- **The boot check compares them**, and refuses a source configured for an impersonation its adapter has
  no way to perform.

The mode is what the security argument keys on, and it is the configured one - because that is what
decides what a caller actually gets.

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

**The limit of that position, stated where the position is taken.** Declining to carry a
classification means that in MULTI-USER mode sutura has no control over what a shared source exposes.
None. A source in `SharedServiceUser` mode holding data the operator should have put behind
impersonation serves that data to every caller who may call the tool, and there is nothing in the query
path that could notice. The operator's tool is not to configure such a source, and
[a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) part 5c is where that
became an operator obligation rather than a check - along with the one thing that IS mechanised, which
is worth being precise about because it is easy to overstate in either direction: in multi-user mode a
shared source needs a per-source acknowledgement that the settings layer can only construct from a key
an operator wrote against that source's own entry, so the posture cannot be reached by leaving anything
at a default and cannot be inherited from a neighbouring source, and a deployment without it **does not
boot**. That is a control over how the posture is ARRIVED AT. It is not a control over what the source
contains, and it never becomes one.

Consequences:

- **`SharedServiceUser` is honest, not broken.** It is right for a single-user deployment - static
  credentials, one user, one host, and those are development and proof-of-concept shapes - and it is
  right for a source nobody needs to see per subject. The failure is never the mode; it is a source in
  that mode being BELIEVED to impersonate - and *believed* is the operative word, because the boot
  acknowledgement is what makes the belief impossible to hold accidentally, not a check on the data.
- **The mode is recorded per leg, in provenance - and RECORDING IS NOT A CONTROL.** An answer says how
  it was executed, so "everyone sees this account's rows" is a field in the answer rather than something
  inferred from a deployment diagram. Say plainly what that is worth: provenance is read by the agent
  **after the rows were served**, so it cannot prevent a disclosure and does not attempt to. It makes
  one after the fact attributable and it makes a misconfiguration visible to whoever reads an answer.
  The thing that keeps a shared source from being served in multi-user mode unnoticed is the startup
  refusal above, which happens before a listener is bound; provenance is the record, not the gate. An
  earlier version of this bullet said provenance "replaces a classification refusal", which reads as if
  the two were substitutes. They are not: one is a control that was withdrawn, and the other is a field.
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

Every data connector is configured this way, and **the first one the mode lands on is the one that
ships.** `sutura-exec-datafusion` is the engine `sutura-cli` and `sutura-serve` both link, it executes
a plan over local CSV and Parquet, and its mode is `SharedServiceUser` by exactly the argument DuckDB's
is: one process reads a file under one operating-system identity, and there is no place in that path
for a subject to arrive. Saying so explicitly is the point of the declaration - a file engine is the
easiest source in the world to assume nothing about, and "nobody configured a mode for the engine" is
how a deployment ends up believing the whole surface impersonates because its *network* source does.

`sutura-exec-duckdb` is next and is `SharedServiceUser` for the same reason - one process holds one
connection under one operating-system identity - with the difference that it is a dev-dependency today
rather than something a deployment links. PostgreSQL 18, BigQuery and Oracle can each be
`ImpersonationAtSource`, by three different mechanisms, which is what makes the mode a declaration
rather than something derivable from the adapter's name.

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
- **No capability negotiated at run time**, and the two sides of that are not the same sentence, which
  an earlier version of this bullet got wrong by generalising.
    - **The DATA-side mode and capabilities are fixed for the life of the process.** They are
      configuration, read once at startup, and nothing at run time can widen them. That is what lets
      the boot check mean anything: a source that gains a capability is a deployment change, and a
      deployment change is a restart.
    - **The METADATA-side declaration is under the definition digest and travels with the bundle**, so
      the TTL refresh above swaps it along with everything else. It does not change *within* a bundle
      and no request can renegotiate it, but it is not fixed for the life of the process, and a
      deployment whose catalog stopped declaring a knowledge capability finds out at the next refresh
      rather than at the next restart. That is deliberate - it is the same swap, and the digest is what
      makes it visible - but calling it unchanging was simply false.
  Neither side is negotiated with a caller, which is the property this bullet is actually about: there
  is no handshake, no probe and no request field that could ask for a capability, so nothing a caller
  sends changes what a source can do.
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
- **What of this record is scheduled, and what is not.** Only the data half is:
  [the implementation plan](../implementation-plan.md) carries `feat/source-registry` for the per-source
  mode and its boot check, and `feat/conformance-packs` for the declaration that selects packs. The
  metadata half - a second metadata connector, the assembler over several of them, and the contribution
  manifest the digest section decides - is in **no branch in that stack**, and the plan's own thesis
  table says so: the connectors are decided and not built. That matters for two claims in this record
  rather than being a scheduling note. The manifest is a decision with no owner until a deployment
  assembles more than one metadata source, so **until then the true sentence about the digest is the
  smaller one** the digest section names as the fallback - it covers the assembly, not the composition -
  and the availability rules that depend on the manifest are dormant with it. And the deferral of
  `Referent::Source` costs nothing to hold, because the connector that wanted it is unscheduled too.
  **A decision whose record is accepted and whose branch does not exist is a decision, not progress**,
  and saying which is which here is what stops the next reader treating an assembled bundle as
  something this deployment can already serve.

## The connectors this has to carry

The declaration exists to make this list additive rather than structural. Every entry is a target, and
the ones that exist today are marked.

**Metadata**, declaring which kinds it provides:

| Connector | State |
| --- | --- |
| Wren-style markdown and YAML | ships |
| OKF-style markdown and YAML | target |
| Datahub | target, and the one that has been **measured** rather than assumed - [what DataHub can carry](0016-what-datahub-can-carry.md) reads its published model field by field and finds a narrow source, so it is the canonical instance of the composition case below rather than of the rich one |
| OpenMetadata | target |
| A custom data catalog over an RDBMS | target, and the one specified in full below - including the thing it may NOT do, which is contribute source-level usage prose to the prompt |
| BPMN | target |
| RDF | target |

**Data**, declaring a mode and its capabilities:

| Connector | Mode it can declare |
| --- | --- |
| DataFusion over local files | `SharedServiceUser`, and it is the only one of these that SHIPS: both binaries link it, so it is the first adapter the mode is declared for |
| DuckDB | `SharedServiceUser`. A development dependency today |
| PostgreSQL | either, and `ImpersonationAtSource` from 18 through the native OAuth method |
| BigQuery | either, with impersonation through a federated exchange whose principal is the person |
| Oracle | either, with impersonation through proxy authentication, which records the chain natively |

Two things this list is meant to make obvious. The metadata side is where most of the growth is, and
none of it touches the query path: a metadata connector answers what a metric MEANS. And the data side
is four connectors and one mode declaration each, which is the whole security surface of pluggability -
not four adapters each with an opinion about authorization.

## The RDBMS catalog, which is the connector that needs specifying

One entry in that table behaves unlike the rest, and it is worth being concrete because a working
version of it exists elsewhere and the useful half is reproducible here while the other half is not.

**What such a catalog actually has.** Mainly DDL and comments: the tables, the columns, their types,
their constraints, and whatever prose somebody wrote against them. It is not a semantic layer and does
not pretend to be one.

**So its declaration is narrow, and that is why composition exists.** A rich source declares
everything - a full metadata platform provides metrics, descriptions, glossary and lineage on its own,
and a deployment reading only that one needs no composition at all. Composition exists for the NARROW
sources, and this is the canonical narrow one:

| Provides | Declared |
| --- | --- |
| Structure: tables, columns, types | **yes** - this is most of what it has |
| Comments as descriptions | **yes** - the other part |
| Relationships, from foreign keys | **yes**, with evidence rather than assertion - see below |
| Certified metrics, measures, grains, allowed values | **no** - a human declares those elsewhere |

**And here is the part worth having this connector for.** The root-of-trust file records a real gap:
*"catalog cardinality is a trusted precondition: nothing checks the declaration against the data"*. A
source that has DDL can close it. A foreign key names the join and its direction; a primary-key or
unique constraint on the referenced column is EVIDENCE that the side being joined to is unique, which
is exactly the fan-out question a declared cardinality is currently trusted to answer. So this
connector can:

- **Refuse a model whose declared column does not exist**, at load, naming the column and the table -
  instead of the database rejecting it at query time.
- **Refuse a declared cardinality the constraints contradict** - which is worth exactly one direction,
  and the direction has to be stated or the claim is an overstatement.

**A constraint is evidence one way only, and it is not the dangerous way.** A unique or primary-key
constraint on the referenced column PROVES that side is unique. The absence of one proves nothing at
all: a column can be unique in every row of the table and carry no constraint, which is the ordinary
state of a warehouse dimension built by a transformation job. So the only declaration a constraint can
contradict is one claiming the referenced side **may duplicate rows** when it provably cannot - and that
declaration is the harmless one, where the author was merely too cautious and paid for it by having a
dimension refused that would have been safe. **The dangerous case is declared-unique-and-actually-not,
and no constraint catches it**, because the evidence that would catch it is the absence of a constraint,
which is not evidence.

So the honest claim is smaller than "turns a trusted precondition into a checked one", which is how an
earlier version of this record put it. What this connector can do is **confirm the safe direction and
refuse a declaration that is provably over-cautious**. The fan-out risk the cardinality precondition
exists for is untouched: `AGENTS.md` records that nothing checks a declaration against the DATA, and
after this connector nothing still does. Closing that would take a count over the referenced column -
evidence from the data rather than from the metadata - which is a different thing to build and is not
what a catalog connector is.

That is still a stronger contribution than descriptions, and it is still the argument for building this
connector before the richer ones: it narrows where a declaration is trusted, in a direction a reviewer
can check, rather than adding another thing to trust.

### The guidance, and the claim this record had to withdraw

An earlier version of this record said a source's own usage instructions - which search to run before
selecting anything, that a name must be schema-qualified or the database rejects it, that this source
carries no certified metric layer - were *"exactly the shape the knowledge layer already renders"* and
*"one more kind of note, not a new channel"*. **Both sentences were false, and the second one was false
in the direction that matters.** Correcting them costs this connector a feature, and the correction is
worth more than the feature.

**What the mechanism actually is, read rather than remembered.** A note is attached to a `Referent`, and
`Referent` has three variants - a metric, a dimension of one, a declared value of one - each of which
carries a `MetricName`. It is not merely that there is no source variant: `Referent::metric()` is a
`const fn` returning `&MetricName` with no `Option` in the signature, and `Knowledge::caveats_about`
RETRIEVES notes by comparing that metric against the metric a question named. So the retrieval path
itself is keyed on a metric, the prompt prints a caveat under the metric it is about, and an unscoped
note is refused at load as `CaveatAboutNothing`. Source-level usage prose refers to none of the three.
There is nothing legal to attach it to, and nowhere for it to render.

**And the shape it would need is the shape that was deliberately not adopted.** The knowledge module
argues, in its own words, that a `rules` kind was refused because it is *"the only shape among the five
that would be scoped to NOTHING - a body of prose about the deployment at large - which is an unscoped
global text channel from the catalog into the prompt"*, and that *"the injection answer depends on that
channel not existing"*. A `Referent::Source` would be prose about a source at large, rendered in the
document's preamble rather than beside a metric a reader is choosing. **For a single-source deployment
that is the `rules` kind with a scope word in front of it** - and a single-source deployment is exactly
the shape this connector exists for. That is the argument, and it is why the answer is not "add a
variant".

**So the decision is: this connector ships WITHOUT catalog-authored source guidance.** Its guidance
arrives when it has a certified metric to hang a note on, through the channel that already exists: a
caveat about that metric, a glossary phrase for that metric, an absence somebody reviewed. No new kind,
no new referent, no new channel.

**Which leaves the three concrete examples, and two of them were never knowledge to begin with.** That
is the part the withdrawn claim obscured by calling them all one thing:

| What the draft wanted to say | Where it belongs instead |
| --- | --- |
| "Search the catalog before selecting anything", "a name must be schema-qualified or the database rejects it" | **First-party prompt text**, in `sutura-app`'s prompt module, beside the bounds and the refusal section it already renders. These are facts about the TOOL SURFACE, not about a catalog's content - they do not vary per catalog, and a catalog is the wrong owner for them. Owned by this repository, in a diff, under the prompt snapshot |
| "This source carries no certified metric layer" | **Derived from the pinned bundle**, not authored. A bundle with no metrics is a fact the prompt can state from what it was handed, and stating it from content would let a source claim the opposite of what it shipped |
| What a column or a table MEANS | **Descriptions**, which this connector already declares and which render where descriptions render. This half was always available and is not affected |

Two things worth being exact about, because this is a withdrawal and a withdrawal that overstates its
own repair is the same defect in a new place. **The first row is a change to first-party prompt prose
and it is not free**: it is new text in a generated document, it moves the prompt snapshot, and prose
that teaches an agent to attempt what the surface refuses by construction is what that module's
documentation argues against - so it has to be written for the raw tool's surface specifically, which
means it lands with [a raw SQL tool](0013-a-raw-sql-tool-off-by-default.md) or not at all. And **the
first row is not catalog-neutral in the way knowledge is**: it says something true of an RDBMS source
and not of a file engine, so it is conditioned on what the deployment actually has rather than printed
always.

### If somebody proposes `Referent::Source` anyway, this is the price

Recorded so that the next person to want it starts from the cost rather than from the idea, and so that
nobody reads the deferral above as "nobody thought about it". **It is an architecture change, not a
feature**, and it touches every mechanism `AGENTS.md` names for exactly this kind of change:

- **A fourth `Referent` variant with no metric in it**, which changes `Referent::metric()` from
  `&MetricName` to `Option<&MetricName>` and rewrites both readers of it: `fault_in`, whose first line
  resolves the metric, and `caveats_about`, whose filter IS that comparison. Retrieval then needs a
  second path that is not keyed on a metric, because a question names a metric and a source note is not
  about one.
- **A fifth `Capability`**, through `Capability::next`, `Capability::previous`, `Capability::as_str` and
  `prompt::knowledge::claim` - four exhaustive matches, three of them in the domain - plus the `const`
  assertion that holds the seed of the walk `Knowledge::assemble` guards with. That assertion is on the
  DISCRIMINANT rather than on `previous()`, deliberately, because the version on `previous()` caught the
  careful author and missed the mechanical one.
- **A validation rule for a referent that names a source**, which is a new kind of check: the existing
  ones resolve a name against the definitions, and a source name is not in the definitions - it is in
  the composition, which is the manifest decided below. So the two changes are coupled, and the
  manifest would have to exist first for the note to be attached to something the bundle declares.
- **A rendering position in the prompt that is not under a metric**, which is the preamble, which is
  the thing the `rules` prohibition is about. Whatever is written there has to survive the argument
  above rather than reopen it.
- **A moved digest for every deployment**, since `KnowledgeCapabilities` is hashed with the rest.

None of that is impossible. All of it is a decision about the prompt's trust boundary rather than about
a connector's convenience, and it belongs in its own record with its own argument.

**And the other half is a separate, configurable decision.** The working version of this pattern pairs
catalog search with a general select, and an agent composes the two: read what a column means, then run
SQL against it. Whether this surface offers that tool is a per-deployment choice, off by default, and it
is decided in [a raw SQL tool](0013-a-raw-sql-tool-off-by-default.md) rather than here - including the
mechanisms that stop it from ever looking certified.

What does not change either way: a question naming a metric is compiled, and the answer to it is
produced by compilation rather than by a model writing SQL. A deployment that enables the raw tool gets
both paths, clearly distinguished; it does not get a blurrier version of the certified one.

## Metadata sources compose, and that is a decision with teeth

The intention is that metrics are provided by a source that has them, and **other metadata must work
too**: field meanings from one place, a glossary from another, the caveats about a metric from a third.
So a deployment reads SEVERAL metadata sources and gets ONE bundle. Note the third example, which the
withdrawal above constrains: what a source may contribute is a note attached to a metric, not prose
about the source itself.

That is not what exists. `load()` returns a whole `PinnedDefinitions`, and both composition roots wire
exactly one catalog. So this is new, and the rules matter more than the plumbing.

**Exactly one source may provide a given kind for a given entity.** Two sources providing the same kind
for the same entity **refuses the load, naming both and the entity.** Guessing which wins is how a
metric silently means something different after a configuration change, and a precedence rule that
nobody stated is a precedence rule nobody reviewed. A deployment that genuinely wants one can declare it
explicitly, and declaring it shows up in a diff.

**For metrics there is no precedence at all, declared or otherwise.** Two sources defining one metric is
refused, always. Two definitions of one number is the failure this whole system exists to prevent, and
letting configuration pick a winner would put that choice outside review.

**Declared-and-empty stays distinct from not-declared**, which the capability model already handles: a
source that declares descriptions and has none for a particular model is fine and says nothing; a source
that never declared them cannot contribute one.

### The digest has to be made to cover the composition, because today it does not

An earlier version of this record said *"change which sources contribute and the digest moves, which is
correct: it is a different bundle."* **That is false against the code, and the correction is the
decision this section makes.** `PinnedDefinitions::pin` computes the digest from two things and stores
them: the assembled `Definitions` and the `Knowledge`. `DefinitionDigest::of` hashes a canonical form
over exactly that pair - a two-element JSON sequence over the parsed content - and there is no third
element. So the digest covers the **ASSEMBLY**, and two different compositions that assemble to the same
definitions and the same knowledge produce the same digest, indistinguishably. The claim was not merely
imprecise; the whole point of it was to make a composition change visible, and it would not have been.

Two ways out, and this record takes the first. The second is written down because it is the fallback if
the first is judged too expensive, and because stating the smaller true thing is always available:

**Decided: a canonical CONTRIBUTION MANIFEST, hashed as a third element beside the definitions and the
knowledge.** Its shape, in the same style as the two things it joins:

- **What it holds:** one entry per metadata source the deployment configured, keyed by the source's
  declared name. Each entry carries the source's typed capability declaration - which kinds it said it
  provides - whether it was declared required or optional, and whether it was REACHED for this bundle.
  Nothing else: no host, no credential, no URL. Connection material lives apart from governance
  metadata, which is a position [the plan](0009-the-plan-from-one-source-to-many.md) already holds, and
  putting a hostname under the definition digest would make a network move look like a definition
  change.
- **In what order:** a `BTreeMap` keyed on the source name, so collection order is content order rather
  than hash order. That is the same property `Definitions` and `Knowledge` already have and the same
  reason: the canonical form is deterministic because the collections are ordered, not because
  `serde_json` happens to be stable.
- **How it is computed:** by `pin`, from the manifest it stores, in the same step. **No manifest
  parameter, no closure, no trait.** This is the shape the existing invariant already has and it is not
  a coincidence - that constructor has been wrong twice, both times by accepting from a caller something
  that was supposed to describe what it stored, and passing content to untrusted code is not the same as
  that code having used it. A manifest a caller could hand in would be the third instance of the same
  bug.

What it buys, precisely. **An optional source that was configured and could not be reached produces a
different digest from the run that included it**, even when the assembly is byte-identical - which is
what the availability rules below need in order to mean anything. **A source removed from the
configuration is visible in the digest** even if it was contributing nothing anybody used. **And a
deployment that swapped one narrow source for another that happens to carry the same descriptions is a
different bundle**, which is the case the withdrawn sentence was actually about.

What it costs, stated with the claim. **Every committed digest moves once**, when the canonical form
gains its third element - a single, reviewable diff, and the same class of change as adding the
knowledge was. **A single-source deployment carries a one-entry manifest** rather than none, because a
shape that differed between one source and N would put the interesting case on the untested path.
**A source RENAME moves the digest without changing what a metric means**, which is the honest limit:
the manifest keys on the declared name because that is what a reviewer reads in a settings file, and the
cost of that choice is that renaming is a certification event. And **the manifest says what was
configured and reached, not what a source returned**: it is not a content hash per source, so two
different bundles from the same reachable sources are told apart by the assembly, exactly as they are
today.

**Not decided: the smaller true thing.** If the manifest is judged too expensive to land with this
connector, the sentence that replaces it is *"the digest covers the assembly, not the composition, and a
re-composition that assembles identically is indistinguishable"* - and then the availability rules below
lose their teeth and have to say so, because "the digest differs from the one that includes it" is a
claim about the manifest and not about the assembly. **Whichever is true has to be the one written
here.** The version this record shipped previously was neither.

### The assembler is application code, not an adapter over adapters

`AGENTS.md`'s *Ports and Adapters* says *"an adapter never calls another adapter."* Two things have to be
said accurately about that before it decides anything. **It is not an invariant**: it lives under
*Design Principles*, which that file states is advisory and may not be cited as an invariant, and the
only mechanism in the area is `check-boundaries`, which reads dependency DIRECTION and cannot see which
crate calls which. So nothing fails a build here, and an earlier version of this record picked the shape
that breaks the principle without noticing it had.

**A shape still has to be picked, and it is the one that keeps the principle.** The assembler is
**application code in `sutura-app`, over N `SemanticCatalog` ports**, not an assembling adapter that
implements the port over N others. Three reasons, and the third is the one that would have bitten:

1. **The rules being decided here are DOMAIN rules, not one adapter's.** "Exactly one source may provide
   a given kind for a given entity", "two sources defining one metric is refused always" - those are
   statements about what a bundle may be. In an adapter they would be one implementation's opinion, and
   a second assembling adapter could hold a different one.
2. **`load()` stays free of a request context** either way, so the property the port exists for is not
   what chooses between them. Saying so removes the argument the earlier version leaned on.
3. **The composition roots already name the adapters.** `sutura-cli` and `sutura-serve` each wire
   exactly one catalog today; wiring N and handing them to an assembler in `sutura-app` is a change at
   the place that is supposed to change, and it does not add a crate that depends on every adapter -
   which is what an assembling adapter would eventually become.

The resemblance to something refused for DATA sources is worth keeping, along with why it is not the
same thing: a composing adapter with a synthetic source name was refused there because it hid two
identities behind one, and identity is what a data leg carries. A metadata source in shared-service mode
carries no identity, so composing them hides nothing. **If a metadata source ever impersonates, this
section is void and the caching rule above applies instead.**

**Availability, per source, declared.** A source is required or optional:

- **Required and unreachable at startup: fail closed.** There is nothing to serve.
- **Optional and unreachable at startup: start without it, and the bundle records that it did.** The
  digest differs from the one that includes it, which is the point - the answer says which bundle
  produced it rather than quietly serving a lesser one under the same name. **That claim depends on the
  contribution manifest above and on nothing else.** Without it the digest is taken over the assembly,
  so an optional source that was unreachable and would have contributed nothing anybody's question read
  produces the SAME digest as the run that included it - and this bullet would be describing a
  distinction the code cannot make. The manifest is where "the bundle records that it did" is recorded.
- **Unreachable at REFRESH, either way: keep the last validated bundle and say so loudly.** Same rule as
  everywhere else here: reloading into a broken or diminished state is worse than not reloading.

Optionality is therefore a declaration too, not a fallback the code takes on its own. A deployment that
wants to survive its description source being down says so, and accepts that answers then carry a
different digest.

## Exploit what a source actually knows

**Metrics are always the goal, and they are not where a deployment starts.** So the value of a metadata
source is how much of a metric definition it can carry BEFORE a human writes one - because every field a
source fills is a field nobody has to type, and the promotion step from an ungoverned answer to a
certified metric is exactly as smooth as the amount already filled in.

That makes flattening every source to "it provides descriptions" a waste. Some sources know far more,
and the declaration should be able to say so:

| Source | What it can carry beyond descriptions |
| --- | --- |
| DDL and comments | Columns and types; foreign keys as relationships; primary-key and unique constraints as **evidence** for cardinality |
| An ontology in RDF | Labels and alternative labels as glossary phrases; definitions as descriptions; domain and range as relationships; **functional properties and cardinality restrictions as cardinality evidence**; a class hierarchy as dimension structure |
| A process model in BPMN | The stages of a process as the **allowed values** of a status dimension, in their real order, each with what it means |
| A full metadata platform | All of the above, declared as such, so a deployment reading only that one composes nothing |

**One constraint runs through the whole table and is the same one the withdrawal above turns on.** A
glossary entry carries a `Referent` - `GlossaryEntry` holds `means: Referent`, and there is no
constructor without it - so a harvested label becomes a glossary phrase only where it names a metric,
a dimension of one, or a declared value of one. A label for a table or a column has nothing to point
at. So harvesting is bounded by what the bundle already declares, and a rich source does not widen the
set of things a note can be about; it fills in more of what is said about the things already there.

Three consequences worth stating, because each is a decision rather than an observation.

**Cardinality stops being purely trusted wherever a source has evidence, in one direction.** The
root-of-trust file records that a declared cardinality is a trusted precondition nothing checks against
the data. A unique constraint, or a functional property in an ontology, is evidence for exactly that
claim - and the direction is the one the RDBMS section above states in full: it PROVES the referenced
side is unique, its absence proves nothing, so what can be refused is a declaration that was too
cautious rather than one that was too confident. The dangerous case stays trusted. Evidence from
metadata is also still not evidence from the data, and both distinctions stay in the wording rather than
being smoothed into "checked".

**A process model is the best source of an allowlist there is.** A status dimension's legitimate values,
typed by hand, are a list that rots the first time somebody adds a stage. Taken from the process model,
they are the same list the business runs on - and its ORDER is information a hand-typed set does not
carry.

**None of them supplies a measure, and none of them should be asked to.** An ontology does not say that
revenue is a sum of one column with two filters. A human owns that sentence, and inferring it from
structure is precisely the guessing this design exists to avoid: it would produce a certified number
whose definition nobody wrote. So the split is: **structure and meaning can be harvested; the measure is
declared.**

**That claim has since been tested against a source that does carry a metric, and it survived for a
better reason than the one given here.** [What DataHub can carry](0016-what-datahub-can-carry.md)
measures a full metadata platform that has first-class metric and semantic-model entities, and finds
that what it holds a measure as is a raw expression string tagged with a dialect, beside an
independently authored aggregation name that nothing reconciles with it. So the argument is not only
that a structural source cannot say what a measure is - it is that a source which *does* say it says it
in a form that would have to be either translated or half-read, and both of those produce a certified
number nobody here can vouch for. The split above is unchanged; the mechanism under it is stronger.

What that buys, concretely: with a rich source present, a candidate definition can arrive with the model,
the dimensions, their allowed values, the joins and their cardinality already filled, leaving a human to
supply the measure and approve it. That is the difference between promotion being a form to fill and
promotion being a sentence to confirm.
