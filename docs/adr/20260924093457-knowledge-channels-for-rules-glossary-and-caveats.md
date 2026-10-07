---
title: Knowledge channels for rules, glossary referents on models and columns, and caveats on relationships
description: The three answers issue #969 asked for, decided A/A/B (issue #969) - global rules route to the operator's instructions_file and gain no new kind (Q1=A); glossary entries whose referent is a model or a column become two new Referent variants, with metric() returning Option (Q2=A); caveats about a relationship are expanded at load into one caveat per metric that uses it, with no new Referent variant and CaveatAboutNothing unchanged (Q3=B). Amends ADR 0036, which declined the rules kind and the source-scoped Referent::Source; this record reopens neither, and prices each option it did not take.
---

# Knowledge channels for rules, glossary referents on models and columns, and caveats on relationships

Status: **a decision record** (issue #969). The maintainer's decision, "Confirm A/A/B" on
2026-09-27, is recorded at `github.com/telekom/sutura/issues/969#issuecomment-5905960128`. It is A/A/B: Q1 rules = A, Q2 glossary = A,
Q3 relationship caveats = B, as laid out in the option tables below. This record
amends ADR 0036, which made the surviving position a decision - a knowledge-only source speaks
through a metric, or it says nothing - and reopens neither the `rules` kind nor a
`Referent::Source`, pricing each option it did not take in full.

Issue #969 found the gap by scanning sutura `origin/main` against the full WrenAI type set (wrenai
0.13.1 / engine 1.28.1). The golden reference carries three kinds this repository does not:

1. **Global rules** - a body of prose about the deployment at large, scoped to nothing.
2. **Glossary entries whose referent is a MODEL or a COLUMN** - not only a metric, a dimension of
   one, or a declared value of one.
3. **Caveats about a RELATIONSHIP** - a join, rather than a metric.

The constraint from `crates/sutura-domain/src/knowledge.rs` is stated first because it is the one
that can be lost: **no note may select, widen or parameterise what executes.** Knowledge is
descriptive only while the prompt is its only consumer, and that is a property of where it is read
rather than of what it contains. `CaveatAboutNothing` stays: every note is attached to something
the bundle declares, and an unscoped note is refused at load. This record prices each option
against that invariant.

## What exists, read rather than remembered

A note is attached to a `Referent`, and `Referent` has three variants - `Metric { metric }`,
`Dimension { metric, dimension }`, `Value { metric, dimension, value }` - each of which carries a
`MetricName`. `Referent::metric()` returns `&MetricName` with no `Option` in its signature.
`Knowledge::caveats_about` retrieves notes by comparing that metric against the metric a question
named. The retrieval path is keyed on a metric, the prompt prints a caveat under the metric it is
about, and an unscoped note is refused at load as `InconsistentKnowledge::CaveatAboutNothing`. A
fifth `Capability` is a compile error in `Capability::next`, `Capability::previous`,
`Capability::as_str`, and `sutura_app::prompt::knowledge::{claim, recorded}` - five exhaustive
matches, three in the domain (one test-only: `previous` is `#[cfg(test)]`, so only two compile-fail
in shipped domain code) and two in the app - plus the `const` assertion on the discriminant that
holds the seed of `Knowledge::assemble`'s undeclared-content guard.

The operator has one channel for authored prose about the deployment at large:
`PromptSettings::instructions_file` is a configured path, layered on top of the derived text and
appended as its last section. It is operator-owned, not catalog-owned: the file is read at the
composition root, and a configured path that is missing fails loudly rather than omitting the
section. `CatalogProse` governs whether the catalog's own descriptions are quoted into the prompt,
separately from the operator file.

`Query` has no field for a model, a table or a column. A caller asks about a metric, and the answer
is produced by compilation. A name in an agent's context is a name it will eventually try to use,
and `Referent` has no variant for one.

## 1. Global rules

The reference project carries a `rules` kind: a body of prose about the deployment at large,
scoped to nothing - search before selecting, qualify names, this deployment has no certified
metric layer. ADR 0011 withdrew the claim that such prose was "one more kind of note, not a new
channel", and ADR 0036 made the surviving position a decision: the `rules` kind is the only shape
among the five that would be scoped to nothing, and the injection answer depends on that channel
not existing.

### Options

**Option A: map a catalog's rules onto the operator instructions file.** A catalog that carries
rules prose does not get a new kind; instead, the operator writes the same prose into
`instructions_file`, or a tool flattens the catalog's rules section into the operator's file at
deploy time. The catalog carries no `rules` kind, `Capability` gains no variant, and `Referent`
gains no variant. The prose reaches the prompt through the channel that already exists, owned by
the operator rather than the catalog.

- **Security argument.** The `instructions_file` is operator-owned, read from a path the operator
  configured, and appended as the last section of the derived prompt. A catalog does not supply
  it; the operator does, or a tool the operator runs does. The injection surface is the one
  `instructions_file` already has: authored prose, bounded by review of the file the operator
  pointed at, not by the digest. The catalog's own digest does not cover this text, which is the
  right property - the operator's rules are not the catalog's claim. **The limit:** the operator's
  rules are not under the definition digest, so a change to them is not visible in a bundle's
  provenance. That is the same position `instructions_file` already holds, and it is honest about
  it: the file is read at the composition root, not pinned.
- **Code cost.** None in the domain or the prompt module. The operator writes the file, or a
  deploy-time tool does. A catalog adapter that harvests rules prose would need a place to put
  it; if that place is the operator's file, the adapter writes outside the bundle, which is a new
  kind of side effect no adapter has today. The cheaper path is a human or a script outside sutura.
- **Limit.** Five of the reference project's seven rules are already enforced by types in this
  crate or are unrepresentable here - certified metrics only, no DML, a bounded time range, a join
  that cannot duplicate rows, one data system. Restating them in the prompt is what
  `sutura_app::prompt`'s module documentation argues against: text teaching an agent to attempt
  what the surface refuses by construction. The remaining two are a glossary entry and a caveat,
  which the knowledge layer has. So the rules that survive the filter are few, and the ones that
  are already structural should not be restated.

**Option B: a reviewed catalog `rules` kind under the digest, rendered in the untrusted block.**
A fifth `Capability` variant (`Rules`), a `Rules` collection in `Knowledge`, and a rendering
position in the prompt's untrusted block - the section marked as catalog-authored content, beside
the glossary and the caveats. The prose is under the definition digest, so a change to it moves the
digest, and the prompt snapshot records it.

- **Security argument.** The prose is reviewed catalog content, bounded by `MAX_NOTE_BODY_BYTES`
  and `MAX_KNOWLEDGE_BYTES`, rendered in the untrusted block where the prompt already places
  catalog-authored text. It does not select, widen or parameterise what executes: it is prose an
  agent reads, not a field on `Query`. The injection surface is the one the glossary and caveats
  already have: authored text under the digest, quoted into the untrusted block. **The limit:**
  the untrusted block is the right place, but the `rules` kind is the only shape scoped to
  nothing - a body of prose about the deployment at large - and `CaveatAboutNothing` exists to
  refuse exactly that shape for every other kind. Option B holds `CaveatAboutNothing` by giving the
  `rules` kind its own scoping rule: a rule is about the deployment, not about a metric, so the
  check does not apply. That is a new rule about what scoping means, and it is the thing ADR 0036
  declined to reopen.
- **Code cost.** A fifth `Capability` (five exhaustive matches - three in the domain, one test-only, and two in the app - the const assertion, the
  undeclared-content guard), a `Rules` collection and `KnowledgeInput` field, a `Rules` note
  record, a rendering position in `sutura_app::prompt` that is not under a metric (the
  preamble or a top-level untrusted section), and a scoping rule that says "a rule is about the
  deployment" rather than "about something the bundle declares". The rendering position is the
  thing the `rules` prohibition is about: whatever is written there has to survive ADR 0036's
  argument rather than reopen it. Every committed digest moves once when the canonical form gains
  its `Rules` element.

**Option C: refuse by name.** A catalog that carries a `rules` section fails the load with a
named error: `InconsistentKnowledge::RulesKindRefused` or equivalent. The operator's channel is
the only one, and a catalog that tries to use a `rules` kind is told to use `instructions_file`
instead.

- **Security argument.** Same as today: the `rules` kind does not exist, and the named refusal
  makes that a load-time error rather than a silent drop. The injection surface does not grow.
- **Code cost.** A parse-time check in the markdown adapter (or wherever a catalog is read) that
  rejects a `kind: rules` document with a message naming the operator channel. No domain change,
  no new `Capability`, no new `Referent`. The check is in the adapter, not the domain, which is
  the split `Knowledge::assemble`'s own documentation argues against - a check inside the markdown
  adapter is a check a metadata-service adapter would not have. The honest version is a domain
  enum that carries `Rules` as a variant the declaration can name but the content cannot supply,
  which is a fifth `Capability` that licenses nothing, and that is half of Option B's cost.

### Decision: Option A

**Option A, decided by the owner.** Global rules route to the operator's
`instructions_file`; no new kind, no domain code. The operator instructions file is the channel
that already exists, it is operator-owned, and the rules that survive the structural filter are
few. Option B was not taken: it reopens the preamble question ADR 0036 closed, and the cost is a
fifth `Capability` plus a rendering position that is not under a metric. Option C was not taken:
it adds a check for a kind the domain does not carry, a refusal for something nobody can send
through the type system today. The owner who wants catalog-authored rules under the digest takes
Option B in a later record; this one takes Option A at no code cost.

## 2. Glossary entries whose referent is a MODEL or a COLUMN

The reference project's glossary maps a phrase to a model, a column, or a filter - not only to a
metric, a dimension of one, or a declared value of one. `Referent` has three variants, all
carrying a `MetricName`, and `GlossaryEntry::means` is a `Referent`. A glossary entry for a model
or a column has nothing to point at.

`Query` has no field for a model, a table or a column. A caller asks about a metric, and the
answer is produced by compilation. A glossary phrase that means a model or a column is prose an
agent reads, and the name in it is a name it will eventually try to use. The structured rendering
of a glossary line - the phrase, the `Referent`, the body - is built from a `Referent`, so today
it cannot name a model or a column.

### Dependency on PR #995

Column referents depend on PR #995 (`feat(catalog): column type, description and key evidence`),
which has merged. PR #995 gives a `Column` a type, a description and a nullability claim, extending
`Model` from a set of column names to a record that carries per-column metadata. With it landed,
a column is a name with a type, a description and a nullability claim, so a glossary entry that
means a column points at a real thing rather than a bare name. Model referents do not depend on
#995: a `Model` exists today, with a name, columns and a description. Both referents are
unblocked.

### Options

**Option A: `Referent::Model { model: ModelName }` and `Referent::Column { model: ModelName,
column: ColumnName }`.** Two new variants, each carrying the names the bundle already declares.
`Referent::metric()` returns `Option<&MetricName>` - the signature changes, and both readers
(`fault_in`, `caveats_about`) rewrite. The glossary line renders the model or column name
beside the phrase, in the untrusted block, under a section for model-level glossary entries
rather than under a metric block.

- **Security argument.** A glossary entry that means a model or a column is prose an agent reads;
  it does not select, widen or parameterise what executes, because `Query` has no field for a
  model or a column. The name reaches the agent's context, and the agent may try to use it - but
  the name already reaches the context through the model's own description, which
  `CatalogProse::Quoted` renders today. A glossary referent for a column is a new way to name a
  column in the prompt, not a new way to execute one. **The limit:** `Phrase` and `NoteBody` are
  free text, and an author can already write a column name into either; a load-time scan that
  refused a body naming a column would close that, and it is not here, for the reason
  `knowledge.rs` states - it would make an authored note refuse for naming a column in a sentence
  about why the column is not the thing being asked for. So the mechanical constraint is the
  `Referent` type, not a scan, and the claim this option holds is the same one `knowledge.rs`
  already makes: the structured renderings cannot name a model or a column, because the type has
  no variant for one. Option A adds the variant, so the claim narrows to "the structured
  renderings name only what `Referent` names", which is a weaker claim.
- **Code cost.** Two `Referent` variants: `Referent::metric()` from `&MetricName` to
  `Option<&MetricName>`, rewriting `fault_in` (whose first line resolves the metric) and
  `caveats_about` (whose filter is that comparison). `ReferentRepr` gains a `model` field and a
  `column` field, with the `deny_unknown_fields` shape it already has. `Capability` does not
  grow: the glossary is one capability, and a model- or column-referent glossary entry is a
  glossary entry. The rendering in `sutura_app::prompt::knowledge` gains a branch for model and
  column referents, rendered in a section that is not under a metric. `Knowledge::assemble`'s
  glossary check (`fault_in`) resolves a model or column name against the definitions, which is
  a new resolution path: the existing one resolves a `MetricName` against the bundle's metrics,
  and a model or column name is resolved against the bundle's models and columns. A model or
  column that does not exist fails the load, the same way a metric that does not exist fails
  today. Every committed digest moves once when the canonical form gains the two variants.

**Option B: render model and column referents as descriptions, not glossary entries.** A
catalog that has a phrase for a model or a column puts the phrase in the model's or column's
description (which PR #995 gives a column), not in the glossary. The glossary stays
metric-anchored. The phrase reaches the prompt through the description channel, which
`CatalogProse` governs, not through the glossary's structured rendering.

- **Security argument.** The description channel already exists, is under the digest, and is
  governed by `CatalogProse`. A phrase in a description is prose, not a `Referent`, so the
  structured rendering cannot name a model or a column - the claim `knowledge.rs` makes stays at
  full width. The injection surface is the one descriptions already have. **The limit:** a
  description is not a glossary entry. The glossary indexes phrases and renders them as "X means
  Y"; a description is a block of prose under a model or column heading. An agent searching for a
  phrase finds it in the glossary, not in a description block, and the prompt's glossary section
  is the thing an agent reads to resolve a synonym. So Option B is a weaker answer to the
  question "what does this phrase mean" - it says the phrase is somewhere in the description, not
  that it maps to one thing.
- **Code cost.** None in the domain or the glossary. PR #995 supplies the column description; the
  model description exists today. An adapter that harvests a phrase for a model or column writes
  it into the description rather than the glossary. No new `Referent` variant, no new
  `Capability`, no digest move.

**Option C: refuse model and column referents by name.** A glossary entry whose `means` names a
model or a column fails the load with a named error, because `Referent` has no variant for one.
The message points the author at the description channel.

- **Security argument.** Same as today. The type has no variant, so the structured rendering
  cannot name one. The claim stays at full width.
- **Code cost.** None, if the refusal is the parse error `ReferentRepr` already produces for an
  unknown shape. A model or column key in a glossary entry's `means` is a `deny_unknown_fields`
  error today, because `ReferentRepr` has `metric`, `dimension` and `value` only. So the refusal
  exists; naming it as a deliberate choice rather than a schema accident is a message change in
  the adapter, not a domain change.

### Decision: Option A

**Option A, decided by the owner, for both model and column referents.** PR #995 has merged, so
both are unblocked. The glossary is the structured index an agent reads to resolve a phrase, and
a phrase that means a model or a column is exactly what it is for. The `Referent` type is the
mechanism that holds the claim "the structured renderings cannot name a model or a column", and
adding the variant is the honest way to narrow that claim rather than routing around it. The cost
is `Referent::metric()` returning `Option<&MetricName>`, which is the cost ADR 0011 priced in
full. Option B was not taken: it is cheaper but answers a weaker question - "where is the phrase"
rather than "what does it mean". Option C was not taken: it is today's state, and it is a schema
accident rather than a decision.

## 3. Caveats about a RELATIONSHIP

The reference project carries caveats about a join: a grain trap on a relationship, a fan-out
risk, a base that is not what it sounds like. `Caveat` holds `about: Vec<Referent>`, and every
`Referent` carries a `MetricName`. A caveat about a relationship has no metric to attach to, and
`CaveatAboutNothing` refuses a caveat scoped to nothing.

`Query` names a metric, not a relationship. A caveat about a relationship is prose an agent reads
before asking a question whose answer depends on that join, and the prompt renders it... where?
Under a metric, the way today's caveats do? Under a relationship listing, if one exists? In the
preamble, which is the channel ADR 0036 closed?

### Options

**Option A: `Referent::Relationship { model: ModelName, relation: RelationshipName }`.** A new
variant that names a relationship the bundle declares. `Referent::metric()` returns
`Option<&MetricName>`, and `caveats_about` gains a second retrieval path: a question names a
metric, and a caveat about a relationship that the metric's definition uses is retrieved by
walking the metric's joins. The caveat renders under the metric block, the same way today's
caveats do, when the metric's definition references that relationship.

- **Security argument.** A caveat about a relationship is prose; it does not select, widen or
  parameterise what executes. The retrieval path is the new question: `caveats_about` today
  filters on `Referent::metric() == question.metric()`, and a relationship referent has no
  metric. The second path - "does this metric's definition use this relationship?" - is a
  load-time walk over the metric's declared joins, not a request-time phrase match. It is
  deterministic and auditable, the same way `fault_in`'s resolution is. **The limit:** a metric
  whose definition uses a relationship will show the caveat, and a metric whose definition does
  not will not. That is correct: a caveat about a fan-out trap on a join is relevant to a question
  that uses the join, not to one that does not. The injection surface is the one caveats already
  have. `CaveatAboutNothing` stays: a relationship referent names something the bundle declares,
  and a relationship that does not exist fails the load.
- **Code cost.** A `Referent` variant (the `metric()` signature change, `fault_in` and
  `caveats_about` rewrites, `ReferentRepr` extension). A new resolution path in `fault_in` for a
  relationship name against the bundle's declared relationships. A new retrieval path in
  `caveats_about` that walks the question's metric definition to find whether it uses the
  relationship. `Capability` does not grow: caveats are one capability. The rendering stays under
  the metric block. Every committed digest moves once. The relationship must be declared by the
  bundle today; if `Relationship` is not yet a first-class declared entity in the definitions,
  the cost includes making it one or refusing a caveat about an undeclared relationship.

**Option B: a caveat about a relationship rendered as a caveat about each metric that uses it.**
No new `Referent` variant. A catalog author writes a caveat about a relationship, and the
adapter - or `Knowledge::assemble` - expands it into one caveat per metric whose definition
references that relationship, each carrying a `Referent::Metric`. The caveat renders under each
metric, the way today's caveats do.

- **Security argument.** The caveat is prose under a metric, retrieved by the existing
  `caveats_about` path, with no new retrieval logic. The expansion is at load time, not request
  time, and the digest covers the expanded form. `CaveatAboutNothing` applies: if no metric uses
  the relationship, the caveat is about nothing and fails the load. **The limit:** the caveat
  body is the same for every metric it is expanded into, which may be wrong - a fan-out trap on a
  join is relevant to a metric that counts rows and not to one that sums a column, and the same
  body serves both. An author who wants different prose per metric writes separate caveats, which
  is what they do today.
- **Code cost.** No `Referent` change. `Knowledge::assemble` gains an expansion step: a caveat
  whose `about` names a relationship is expanded into N caveats, one per metric that uses it. The
  expansion needs a walk over the definitions to find which metrics reference the relationship,
  which is the same walk Option A's retrieval path needs. The difference is when it runs: Option
  B runs it at load and stores the expanded caveats; Option A runs it at request time in
  `caveats_about`. No new `Capability`, no rendering change. Every committed digest moves once if
  the canonical form changes; if the expansion is before `assemble`, the digest covers the
  expanded form and may not move.

**Option C: refuse relationship caveats by name.** A caveat whose `about` names a relationship
fails the load, because `Referent` has no variant for one. The message points the author at
per-metric caveats.

- **Security argument.** Same as today. No new variant, no new retrieval path. The claim stays at
  full width.
- **Code cost.** None in the domain. A caveat about a relationship today is a `Referent` with a
  `metric` that does not resolve, which fails as `CaveatUnknownMetric`. Naming it as a
  relationship refusal is a message change, not a structural one.

### Decision: Option B

**Option B, decided by the owner.** A caveat about a relationship is expanded at load into one
caveat per metric whose definition uses it, each carrying a `Referent::Metric`. This keeps the
retrieval path, the rendering position, and the `CaveatAboutNothing` check unchanged. The cost is
a load-time walk over the definitions to find which metrics reference the relationship, not a
`Referent` variant or a new retrieval path at request time. Option A was not taken: it is the
stronger answer if the owner wants a relationship to be a first-class referent that renders once
rather than N times, but it reopens `caveats_about` and adds a request-time walk. Option C was not
taken: it is today's state, and it is a misnamed metric refusal rather than a decision.

## What does not change, whichever option is chosen

- **`Query` gains no field.** A caller asks about a metric. No note selects, widens or
  parameterises what executes, because no note is a field on `Query`.
- **`CaveatAboutNothing` stays.** Every note is attached to something the bundle declares, and an
  unscoped note is refused at load. A `rules` kind is the one shape that would need its own
  scoping rule, and that is the thing ADR 0036 closed.
- **The prompt-injection argument is restated for each new channel.** Knowledge is descriptive
  only while the prompt is its only consumer. A note that reached a field on `Query` - a phrase
  resolved server-side, a relationship that changed the plan - would break that, and none of the
  options above adds one. The limit is the one `knowledge.rs` states: prose is bounded, authored,
  reviewed content whose digest moves when a word of it changes; it is not mechanically
  constrained, and claiming otherwise would be claiming the wrong mechanism.

## Alternatives considered

**Add a `Referent::Source` variant.** ADR 0011 priced this in full, and ADR 0036 declined to
reopen it. A source-level referent is prose about a source at large, rendered in the preamble, and
for a single-source deployment it is the `rules` kind with a scope word in front of it. This record
does not reopen it. If the owner wants source-level prose, the path is Option A for rules (the
operator file) or a new ADR with its own argument.

**Route everything through first-party prompt text.** ADR 0011's table already put two of the
three concrete rules examples there, and the argument applies to model and column referents too:
a model's description, a column's description (after #995), and an operator's `instructions_file`
are channels that exist. The question is whether they answer the same question the glossary does -
"what does this phrase mean" - and the answer is that they answer a related one: "what is this
thing". The glossary is the structured index; the descriptions are prose blocks. Both are
legitimate, and the decision is to use both rather than one.

## Decision

**A/A/B (issue #969).**

1. **Q1 global rules = A.** Route to the operator's `instructions_file`; no new kind, no domain
   code.
2. **Q2 glossary referent = A.** `Referent::Model` and `Referent::Column`, with
   `Referent::metric()` returning `Option<&MetricName>` and `fault_in` / `caveats_about` rewritten.
   PR #995 has merged, so both model and column referents are unblocked.
3. **Q3 relationship caveats = B.** Expand at load into one caveat per metric that uses the
   relationship; no new `Referent` variant, no request-time walk, `CaveatAboutNothing` unchanged.

The issue's acceptance criteria - a merged ADR, a knowledge golden for each decided kind, and a
named load refusal for each rejected kind - are met when the code lands in the follow-up split:
Q1 (independent) → Q2 model → Q2 column → Q3 → goldens plus a named load refusal per rejected
kind. Q2 and Q3-A would have shared the `metric()` signature change; with Q3=B, Q3 is
independent of it and lands after Q2.

## Amendment, 2026-09-26: `CatalogProse` renders metric descriptions only

Option A's security argument says a model's or a column's name "already reaches the context through
the model's own description, which `CatalogProse::Quoted` renders today", and Option B routes a
phrase through "the description channel, which `CatalogProse` governs". By default the prompt quotes
a metric's description and no other; model and column prose reach it only through the opt-in
physical-schema listing (`prompt.list_physical_schema`), which quotes them under
`prompt.catalog_prose` - `sutura_domain::catalog::Column`'s doc says so. So under Option A a glossary
referent is, by default, the first way a model or a column is named in the prompt, and Option B has
to extend the description channel to models and columns before a phrase can travel through it. The options and
decisions above stand as decided; this corrects the premise both security arguments share.

## Second amendment, 2026-10-07: what the build of Q2 and Q3 decided where this record was silent

Four points the build had to decide. The A/A/B decision stands; this records how it was built.

- **A caveat about a model or a column is refused at load**, as `CaveatAboutAModel`. Q2 gives the
  two new referents to the glossary; this record did not say what a caveat naming one means. A
  caveat is printed under the metric it is about, and a model has no block to print it in, so such
  a caveat would load and be read by nobody. This is a build-time gap filled conservatively, and
  the refusal names the model.
- **A glossary line about a model or a column is rendered in the glossary section, marked
  descriptive only** - a request names a metric, never a model or a column - so the line cannot
  read as a name an agent may send.
- **Q3=B is built as N caveats, each about ONE metric.** A caveat may name `relationships:` in
  place of `about:`. At load, `Knowledge::assemble` expands it into one caveat per metric that
  uses one of those relationships, where **uses means a dimension of the metric names the
  relationship anywhere in its `via` chain**. Each derived caveat is about exactly the dimensions
  that metric reaches through it (`Referent::Dimension`, not the `Referent::Metric` Option B
  sketched): a trap in a join is a trap in the questions that walk it, and a total grouped by
  nothing does not walk it. Each carries the authored body under the name `<caveat>__<metric>`.
  No `Referent` variant was added and no request-time walk exists; the bundle holds only the
  expanded caveats, and the digest covers them. Five load refusals name what goes wrong: a
  relationship the definitions do not declare (`CaveatUnknownRelationship`); one no dimension is
  reached through (`CaveatRelationshipReachesNoMetric` - a named refusal, not
  `CaveatAboutNothing` as Option B said); a caveat naming both `about` and `relationships`
  (`CaveatAboutAndThroughRelationships`); a derived name past the 63 characters a name may have
  (`DerivedCaveatNotAName` - refused rather than shortened, because two metrics sharing a prefix
  would shorten to one name); and a derived name that is already a caveat, authored or derived
  (`DerivedCaveatNameTaken`). The aggregate byte cap counts what the expansion produces, so a
  bundle composed from several sources, which re-assembles the derived caveats as authored ones,
  is held to the same total. **The limit:** a cross-model ratio's hop to its shared calendar is
  matched by model in `sutura_semantic`'s resolver and named in no `via`, so it is not walked - a
  caveat about that relationship is refused as reaching no metric rather than attached to the
  ratio.
- **A derived caveat is shown to a caller only if that caller sees the metric it was derived
  from.** Its name carries that metric's name, so showing it to anyone else would name a metric
  the caller cannot see. Two metrics that reach one dimension through one relationship get two
  caveats, and a caller who sees the dimension under one of them sees that one's caveat and
  nothing that names the other. It is the rule every caveat is filtered by - a
  `Referent::Dimension` is visible only where its metric is - and
  `a_caller_who_sees_one_of_two_metrics_sharing_a_dimension_gets_exactly_that_metrics_caveat`
  holds it: with the dimension arm of that rule no longer reading the metric, the cell fails.
