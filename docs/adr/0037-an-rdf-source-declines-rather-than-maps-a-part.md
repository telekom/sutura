---
title: An RDF source declines rather than maps the part that fits
description: Issue #155. Decides, against a field-by-field measurement of three named RDF vocabularies at named versions, that this repository builds no RDF metadata adapter - because every definition kind here is anchored to a column of a table a SourceName names, and an RDF vocabulary names properties and resources. Records the declined surface explicitly: a measure with no aggregation function, a cardinality this domain cannot hold, a code list of IRIs rather than stored values, a label that may not reach a MetricName, and the abbreviated cube form that is only well-formed after a derivation. Reasoning is out of scope and SHACL belongs at load, never per response. Takes 0037; the number this issue reserved (0033) was consumed by unrelated work.
---

# An RDF source declines rather than maps the part that fits

Status: **a decision record** (issue #155, step 1, written after step 2 rather than before it). The
measurement it decides on is `docs/what-an-rdf-graph-can-carry.md`, read field by field in ADR 0016's
method over three named vocabularies at named versions. Writing the record second is deliberate: the
issue's first requirement is that a finding about "RDF" in general is not checkable, so there was
nothing to decide until three named vocabularies had been read.

## The tension, which is the whole issue

A graph can express relationships the closed vocabulary cannot. **The measure vocabulary is closed**
is an invariant here, held by a type: `Measure` is two shapes over a `Term` of two terms,
`RequiredFilter` has four operators, and there is no `expression:` field and no `Option<String>`
anywhere on it. So an RDF adapter faces three options on every triple - map it where an exact
equivalent exists, declare it unsupported, or widen the vocabulary - and the third is ADR 0002's
decision with four mechanisms to pass through, never a connector's to make.

**A connector that quietly took the third option by inventing a general expression field would be the
defect this design exists to prevent.** What the measurement found is that the pressure to do so is
real and specific, not hypothetical, which is why the declined surface below is written out rather
than summarised.

## The decision

**This repository builds no RDF metadata adapter, and an RDF source that cannot supply a kind
declines it rather than mapping the part that fits.** The measurement is that all three vocabularies
read - the W3C RDF Data Cube (Recommendation 16 January 2014), QB4OLAP 1.3, and the W3C CSVW metadata
vocabulary (Recommendation 17 December 2015) - yield **zero of the nine `DefinitionKind`s**, and for
one reason rather than three: every kind in this domain is anchored to a `ColumnName` of a table a
`SourceName` names, and an RDF vocabulary names properties and resources.

The second half of the decision is the one that costs something, because one of the three *would* work:
**CSVW is declined even though it maps.** It names a table, its columns and its prose, and its mapping
is line for line the one `docs/what-okf-can-carry.md` already measured - because CSVW is Frictionless
Table Schema expressed in RDF, and `sutura-catalog-okf` ships an adapter over it. A CSVW adapter would
buy a parser and an RDF dependency and no capability this workspace does not have. That is a
duplication argument rather than a capability one, and it is the reason to record it here: the next
proposal should have to answer it rather than rediscover it.

## What is declined, and what would break

Each entry is a term a real vocabulary really carries, so each is a mapping somebody could write.

| Declined                                          | What would break if it were mapped anyway                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `qb:MeasureProperty` as a `Measure`               | It carries a label, a super-property and an `rdfs:range`, and the Data Cube specification states outright that aggregation terms are not supported in that version. The aggregate would be **this repository's choice**, and a defaulted aggregate is indistinguishable from a decision - ADR 0016's first worked refusal, arriving from the source side.                |
| `qb4o:ManyToMany` as a `JoinType`                 | The domain has three cardinalities and QB4OLAP has four. Folding the fourth onto `OneToMany` manufactures the warrant `Definitions::assemble` refuses a dimension through as `JoinWouldDuplicateRows` - a wrong number with a provenance record that looks clean. A cardinality the source states and this domain cannot hold is ADR 0002's decision, not a connector's. |
| `qb:codeList` concepts as `DimensionValue`s       | A `skos:Concept` is an IRI and a `DimensionValue` is a value stored in a column. An allowlist of IRIs would filter nothing and would license a breakdown nobody reviewed.                                                                                                                                                                                                |
| `rdfs:label` / `skos:prefLabel` as a `MetricName` | That is a harvested-phrase match, refused by ADR 0036 and by the BPMN spike: there is no `PhraseNotDefined`, the glossary renders into the prompt, and the agent states its own choice. Two deployments could bind one label to two different certified metrics and nothing would say so.                                                                                |
| `qb:componentRequired` as a `RequiredFilter`      | It says whether an attribute must be present on an observation - a rule about the encoding, not a predicate that is part of what a metric means.                                                                                                                                                                                                                         |
| A level or interval resource as a `Grain`         | `Grain` is five named resolutions beside a time column. Matching a level IRI's name onto one is phrase matching with a numeric consequence.                                                                                                                                                                                                                              |
| `qb:Observation` values as `Anchor`s              | Those are data, not a certified number, and the thread's 2026-09-05 comment split the data half off this issue.                                                                                                                                                                                                                                                          |
| The **abbreviated** cube form                     | See below: normalizing it is deriving it.                                                                                                                                                                                                                                                                                                                                |

**And prose does not escape this either**, which is the answer to the obvious fallback that at least
the descriptions arrive. A `Description` is a field of a `Model`, a `Metric` or a `Dimension`, never a
standalone entity: `MetadataCapabilities::produced` observes `DefinitionKind::Descriptions` through
`describes_anything`, which reads prose off exactly those three. So a source supplying no `Structure`
and no `Metrics` provably supplies no `Descriptions`. That claim is held by
`descriptions_are_observed_from_prose_rather_than_from_the_field` in
`crates/sutura-domain/src/capabilities/tests.rs`, not by this record.

## Reasoning is out of scope, and it is not a switch

Only asserted triples would be read. An inferred relationship licensing a join is the same defect as a
folded cardinality - catalog cardinality is a trusted precondition nothing checks against the data - so
this is not a preference. What the measurement added is that "turn the reasoner off" is not the whole
mechanism, because two of the Data Cube specification's own normative constructs derive triples:

1. **The abbreviated form.** A well-formed abbreviated cube is well-formed only once expanded by the
   specification's normalization algorithm, which is expressed as two sets of SPARQL Update
   operations - and the specification's own complete example is in that form. So reading only asserted
   triples means **refusing the abbreviated form**, not normalizing it. Normalizing is deriving.
2. **Code list hierarchies.** The specification notes that some publishing tool chains infer the
   `skos:narrowerTransitive` closure, so a code list can arrive already carrying derived members, with
   nothing in the graph distinguishing them.

**The limit beside that, because an overstated control is itself a defect:** the first of the Data
Cube's integrity constraints requires the graph to be consistent under RDF D-entailment, so an
assertion-only reader **cannot check it**. Reading only assertions is therefore a narrower thing than
validating a cube, not a stricter one, and an adapter claiming to validate well-formedness while
reading only assertions would be overstating what it does.

## SHACL belongs at load, and never before a response

The thread's 2026-09-05 comment asks whether SHACL checks are needed prior to sending a response.
**No, and the position is wrong rather than merely unnecessary.** A SHACL closed shape set
(`sh:closed true` with `sh:ignoredProperties`) is the right graph-level analogue of this repository's
`deny_unknown_fields`, and its only correct venue is **load**: `SemanticCatalog::load` takes no
`RequestContext` and a bundle is pinned before a request arrives, so a validation running per response
could hand two callers different content, which is exactly what pinning exists to prevent. Under that
placement, SHACL-Core validation would be legitimate and SHACL-AF `sh:rule` inference would not, for
the reason above. **None of this is built**, and nothing here should be read as saying it is.

## Alternatives considered

**Ship the CSVW adapter anyway.** It would work, and that is why it is the alternative worth naming.
Declined on duplication: it reproduces `sutura-catalog-okf`'s mapping over a second serialisation, and
the cost is an RDF parsing dependency with a licence review and an unused-deps entry for no kind this
workspace cannot already read. A reviewer who wants RDF serialisation support for an existing mapping
should reopen this as a *serialisation* question about that crate, which is a much smaller change than
a connector.

**Map `qb:MeasureProperty` with a declared default aggregate.** The default would have to be chosen
per deployment, which makes it a deployment-authored map rather than something the graph declared - so
it is the map the last section describes, and it does not need an adapter to be designed around it.

**Widen the measure vocabulary to hold what a graph says.** ADR 0002's decision, priced there: a
domain variant, a plan variant, a generator arm and a golden. Nothing in the measurement argues for it;
what the graph withholds is the column, not a shape.

**Record nothing and leave the finding as a spike.** This was the first draft's position, on the
argument that every decision was already recorded in 0002, 0011, 0016 and 0036 and that a rule with no
mechanism is a wish. It is wrong for one reason: *not building* is itself a decision with a live
alternative (CSVW), and the reason it was declined is the thing a reader six months from now will not
reconstruct. ADR 0036 is the precedent - it stated 0011's model as a decision for one source.

## What would reopen this

Not a richer vocabulary - a **map**. An RDF graph becomes readable here the moment something declares
which property of which resource is which column of which named table. That artefact is
deployment-authored and appears in none of the three vocabularies; the published mapping language runs
the other way, from relational data to RDF. So the next step for anyone wanting this is a declared
graph-to-table map, with its own issue and its own cost, and this record should be re-read against such
a proposal rather than cited as settling it.

## Decision

**No RDF metadata adapter, no RDF dependency.** An RDF source declines a kind it cannot supply rather
than mapping the part that fits; widening the vocabulary to absorb what a graph says stays ADR 0002's
decision; reasoning stays out of scope, which includes refusing the abbreviated cube form rather than
normalizing it; and SHACL, if it ever arrives, validates at load and never before a response. The
number this issue reserved (0033) was consumed by unrelated work, as was the number #154 reserved;
this is 0037, checked free against `origin/main` and against every open pull request at the time of
writing.
