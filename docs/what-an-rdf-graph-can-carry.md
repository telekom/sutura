---
title: What an RDF graph can carry, and the one thing all three vocabularies withhold
description: Issue #155 step 2, read in ADR 0016's method against three named RDF vocabularies at named versions - the W3C RDF Data Cube (QB), the W3C CSVW metadata vocabulary, and QB4OLAP 1.3. QB declares measures with no aggregation function and says so itself; QB4OLAP supplies five of this domain's six aggregates and four cardinalities for three JoinType variants; CSVW supplies a table and columns and reproduces a mapping sutura-catalog-okf already ships. All three yield zero of the nine definition kinds, for one structural reason - every kind here is anchored to a column of a named physical table, and an RDF vocabulary names properties and resources. So no catalog-rdf crate is built, and ADR 0038 records the declined surface.
---

# What an RDF graph can carry, and the one thing all three vocabularies withhold

Status: **a finding, and the deliverable** of issue #155's step 2, read in ADR 0016's method
(`docs/adr/0016-what-datahub-can-carry.md`). The issue's own first requirement decides the method:
*"RDF is a data model, not a schema. A finding about 'RDF' in general is not checkable; a finding
about a named vocabulary, at a named version, read field by field, is."* So three named vocabularies
are read below, chosen to be the strongest cases rather than the typical ones - if the vocabulary
closest to a multidimensional semantic model cannot fill these fields, a more general graph cannot.

## What was measured against

Read out of the port rather than out of the prose. `SemanticCatalog::load` returns a
`PinnedDefinitions` whose halves are a `Definitions` and a `Knowledge`, and `DefinitionKind` in
`crates/sutura-domain/src/capabilities.rs` names the nine kinds a declaration is made over. The
shapes that matter here, read at this tree:

| What a kind needs                                     | The type in `sutura-domain`                                                                                      |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| a physical table and the columns it exposes           | `Model` - a `ModelName`, a `SourceName`, a qualified table, a `BTreeSet<ColumnName>`, a `Description`            |
| what a metric measures                                | `Measure::Simple(Term)` or `Measure::Ratio { .. }`, over `Term::Aggregate(AggregatedColumn)` or `CountIf`        |
| an aggregate                                          | `AggregatedColumn` - an `Aggregate` **and** a `ColumnName`; `Aggregate` is `Sum Count CountDistinct Avg Min Max` |
| a join this repository will reach a dimension through | `Relationship` - two `(ModelName, ColumnName)` endpoints and a `JoinType` of `OneToOne ManyToOne OneToMany`      |
| a predicate that is part of what a metric means       | `RequiredFilter` - `Equals NotEquals IsTrue IsNotNull`, every variant carrying a `ColumnName`                    |
| a time resolution                                     | `Grain` - `Day Week Month Quarter Year`, beside a time `ColumnName`                                              |
| what a metric may be broken down by                   | `Dimension` - a `ColumnName`, an optional `via`, an optional `BTreeSet<DimensionValue>` allowlist                |

**Note what every row has in common, because it is this record's whole conclusion**: a `ColumnName`,
of a table a `SourceName` names. `Measure` has no `expression:` field and no `Option<String>` at any
depth, so there is no slot in which an unmappable graph term could be carried as text.

## Vocabulary 1 - the W3C RDF Data Cube (QB)

**Namespace** `http://purl.org/linked-data/cube#`, **W3C Recommendation 16 January 2014.** The closest
published RDF vocabulary to a multidimensional semantic model, and therefore the strongest case. The
real file read is the specification's own **Appendix C, "Complete example Data Cube"** - a normative,
re-checkable artefact that the spec states "passes all the integrity checks", so no local fixture can
be accused of having been chosen to fail.

Its entire measure declaration is three predicates:

```turtle
eg:lifeExpectancy a rdf:Property, qb:MeasureProperty;
    rdfs:label "life expectancy"@en;
    rdfs:subPropertyOf sdmx-measure:obsValue;
    rdfs:range xsd:decimal .
```

A label, a super-property and an XSD range. Against `AggregatedColumn` that fills neither field: no
aggregate, no column. **And the specification declines this itself**, in §8.4, which is the single most
useful sentence in this record:

> Vocabulary terms to represent the aggregation operations employed within a given dataset, and how
> one dataset might be derived from another, are not supported in this version of the Data Cube
> specification.

Its change log records the same thing as a deliberate scoping: *"Added note that aggregation
operations and inter-cube relations are out of scope for this version."* So a `qb:MeasureProperty`
mapped to a `Measure` would be an aggregate **this repository chose**, not one the graph declared -
which is ADR 0016's first worked refusal arriving from the other direction, and a defaulted aggregate
is indistinguishable from a decision.

Read against the nine kinds, with the reason rather than a verdict alone:

| Kind              | What QB offers                                                                       | Verdict                                                                             |
| ----------------- | ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| `Structure`       | `qb:DataSet`, `qb:DataStructureDefinition`, `qb:ComponentSpecification`              | **absent** - a cube of observations; no table is named and no column is named       |
| `Descriptions`    | `rdfs:label`, `rdfs:comment`, `dct:title`, `dct:description`, all language-tagged    | **absent in effect** - see *Why prose cannot travel alone* below                    |
| `Relationships`   | nothing; inter-cube relations are out of scope by the change log above               | **absent**                                                                          |
| `Cardinality`     | nothing                                                                              | **absent**                                                                          |
| `Metrics`         | `qb:MeasureProperty` - no aggregate, no column                                       | **absent**, and refused rather than defaulted                                       |
| `RequiredFilters` | `qb:componentRequired` - whether an attribute must be present on an observation      | **absent** - a presence rule about the encoding, not a predicate on rows            |
| `Grains`          | a `refPeriod` dimension whose `rdfs:range` is an interval resource                   | **absent** - an interval is a value, not one of `Grain`'s five resolutions          |
| `AllowedValues`   | `qb:codeList` to a `skos:ConceptScheme`, `skos:Collection` or hierarchical code list | **absent** - concepts are IRIs; a `DimensionValue` is a value stored in a column    |
| `Anchors`         | `qb:Observation` values                                                              | **out of scope** - that is data, and the 2026-09-05 comment split the data half off |

Two measured details worth keeping, both from Appendix C itself: it declares **no `qb:codeList` at
all**, so the one kind with a partial story is absent even from the specification's own complete
example; and it is written in the **abbreviated** form, which is the subject of the next section.

## Vocabulary 2 - QB4OLAP, which does supply an aggregate and a cardinality

**Namespace** `http://purl.org/qb4olap/cubes#`, `owl:versionInfo "1.3"`, read from the ontology
document itself. A **third-party vocabulary, not a W3C standard**; it `owl:imports` QB and exists to
add what QB's §8.4 left out. It is in this record because omitting it would make the finding above
look stronger than it is.

It genuinely closes two of the gaps:

- **`qb4o:aggregateFunction`**, with five `qb4o:AggregateFunction` instances - `Sum`, `Count`, `Avg`,
  `Min`, `Max`. That is five of this domain's six `Aggregate` variants; **`CountDistinct` has no
  QB4OLAP term**, so a metric needing it is unexpressible on the source side.
- **`qb4o:cardinality`** and `qb4o:pcCardinality`, with four `qb4o:Cardinality` instances - `OneToOne`,
  `OneToMany`, `ManyToOne`, **`ManyToMany`**. Three map onto `JoinType`. The fourth does not exist in
  this domain, and that asymmetry is the interesting half: `ManyToMany` must be a **refusal**, because
  mapping it onto `OneToMany` would manufacture exactly the warrant `Definitions::assemble` refuses a
  dimension through as `JoinWouldDuplicateRows`. A cardinality the source states and this domain
  cannot hold is the issue's third option - widening the vocabulary - arriving from the source side,
  and it is still ADR 0002's decision and never a connector's.

And it withholds the same one thing QB does. `qb4o:LevelProperty`, `qb4o:LevelAttribute` and
`qb4o:LevelMember` are RDF properties and resources, not a column of a named table; there is no
`SourceName`, no table and no `ColumnName` anywhere in the vocabulary. Its own stated purpose is *"to
implement OLAP operators (such as Roll-up, Slice, and Dice) as SPARQL queries directly on this RDF
representation"* - the cube's data **is** the graph. So QB4OLAP hands over the aggregate name and
withholds everything that would make it executable, and the result is still **zero of the nine kinds**.
It also answers the 2026-09-05 scoping comment's first half directly: for QB4OLAP, RDF-for-metadata and
RDF-for-data are not two adapters over one engine, they are the same artefact, and the metadata half
cannot be specified without deciding the data half first.

## Vocabulary 3 - CSVW, which does name a table and its columns

**Namespace** `http://www.w3.org/ns/csvw#`, **W3C Recommendation 17 December 2015** (*Metadata
Vocabulary for Tabular Data*). This is the RDF vocabulary that supplies the thing the other two
withhold: `csvw:Table` with a `csvw:url`, a `csvw:Schema` of `csvw:Column`s each with a `csvw:name`
that the spec makes the canonical column name, and `csvw:foreignKey` between schemas. It carries no
measure and no aggregation function of any kind.

So its mapping is: table and columns **provided**, prose **provided**, foreign keys **structural with
no cardinality** and therefore licensing no dimension, and the whole metric layer **absent**.

**That is, line for line, the mapping `docs/what-okf-can-carry.md` already measured** - because CSVW is
the tabular model of Frictionless Table Schema expressed in RDF, and `sutura-catalog-okf` ships an
adapter over it. A `catalog-rdf` crate reading CSVW would therefore add a parser and an RDF dependency
and **no capability this workspace does not already have**. That is the cheapest possible reason not to
build it, and it is the one that would have surfaced last.

## Why prose cannot travel alone, and it is held by a test rather than by this page

Every vocabulary above is rich in prose, so "at least the descriptions arrive" is the obvious fallback.
It does not, and the reason is mechanical rather than editorial: a `Description` in this domain is a
**field** of a `Model`, a `Metric` or a `Dimension`, never a standalone entity.
`MetadataCapabilities::produced` observes `DefinitionKind::Descriptions` through `describes_anything`,
which reads prose off exactly those three, so a source supplying no `Structure` and no `Metrics`
provably supplies no `Descriptions` either.

That claim is **not** held by this page. It is held by
`descriptions_are_observed_from_prose_rather_than_from_the_field` in
`crates/sutura-domain/src/capabilities/tests.rs`, which asserts prose is observed off the content and
not off the field's presence, beside that file's own note that `Definitions::assemble` "has nothing to
hang a metric on without a model". No cell is added here, because a second test over the same
behaviour would pass against base and is coverage that only looks like proof.

## Only asserted triples, and QB is the reason that is not a switch

The issue asks whether reasoning is in scope. It is not, and the answer needs stating more carefully
than "turn the reasoner off", because two of QB's own normative mechanisms derive triples:

- **The abbreviated form.** *"A well-formed abbreviated RDF Data Cube is an RDF graph which, when
  expanded using the normalization algorithm, yields a well-formed RDF Data Cube"*, and the
  normalization algorithm is *"two sets of SPARQL Update operations"*. Appendix C - the
  specification's own complete example - is in that form. So **reading only asserted triples means
  refusing the abbreviated form**, not normalizing it: normalizing *is* deriving, and an adapter that
  normalized would be inventing observations nobody asserted while its provenance record looked clean.
- **Code list hierarchies.** §8.2 notes that in some publishing tool chains the transitive closure
  `skos:narrowerTransitive` *"will be automatically inferred"*. So a code list may arrive already
  carrying derived members - upstream, before this repository sees it, with nothing in the graph
  distinguishing them.

**The limit beside that claim:** the first of QB's 22 integrity constraints, IC-0, requires the graph
to be *"consistent under RDF D-entailment"*. An assertion-only reader therefore **cannot check IC-0**.
So "only asserted triples" is not a stricter form of validation - it is a different and narrower thing,
and an adapter claiming to validate a cube's well-formedness while reading only assertions would be
overstating its control.

The SHACL answer the 2026-09-05 comment asked for follows from the same reasoning, and it is a
**no with a redirection**. A SHACL closed shape set (`sh:closed true` plus `sh:ignoredProperties`) is
the right graph-level analogue of this repository's `deny_unknown_fields`, and its place is **at load**.
It is not a check run "prior to sending a response", for a structural reason: `load()` takes no
`RequestContext` and a bundle is pinned before a request arrives, so a validation running per response
could yield different content to different callers, which is what pinning exists to prevent. SHACL-Core
validation would be legitimate at load; SHACL-AF `sh:rule` inference would not, for the reason above.
None of this is built, and nothing here should be read as saying it is.

## No phrase reaches a `MetricName`

RDF's densest content is its labels - `rdfs:label`, `skos:prefLabel`, `skos:altLabel` - which are
language-tagged, unordered, and permitted in quantity on one subject. Binding one to a certified
`MetricName` is the harvested-phrase match ADR 0036
(`docs/adr/0036-a-knowledge-only-source-speaks-through-a-metric.md`) and the BPMN spike
(`docs/what-a-bpmn-file-actually-carries.md`) both refused: there is no `PhraseNotDefined`, the
glossary renders into the prompt, and the agent states which metric it chose in its own transcript. Two
deployments could bind one `skos:prefLabel` to two different certified metrics, and nothing would say
so. A phrase may only reach a bundle attached at load to a `MetricName` the adapter actually found, and
none of the three vocabularies above gives it one.

## Conclusion

**No `catalog-rdf` crate and no RDF dependency**, decided in
`docs/adr/0038-an-rdf-source-declines-rather-than-maps-a-part.md`. Three vocabularies, each the strongest
case in its class, yield **zero of the nine definition kinds**, and for one reason rather than three:
every kind in this domain is anchored to a column of a table a `SourceName` names, and an RDF
vocabulary names properties and resources. QB says outright that it does not carry an aggregation
operation; QB4OLAP carries the aggregate name and withholds the column; CSVW carries the column and
withholds the metric layer, reproducing a mapping this workspace already ships. The issue anticipated
this outcome and pre-authorised it - *"no adapter unless a real graph yields enough of the nine kinds;
otherwise the finding is the more valuable half"* - and it is the same terminus the BPMN spike reached
by a different route.

**The decision lives in ADR 0038**
(`docs/adr/0038-an-rdf-source-declines-rather-than-maps-a-part.md`), which records the declined
surface term by term with what would break if each were mapped anyway. This page is the measurement
it decides on; the record is where the decision and its alternatives are, including the one that
costs something - CSVW would work, and is declined on duplication rather than on capability.

## What would change this

Not a better vocabulary - a **map**. An RDF graph becomes readable here the moment something declares
which property of which resource is which column of which named table. That artefact is
deployment-authored and exists in none of the three vocabularies; the published mapping language points
the other way (R2RML, W3C Recommendation 27 September 2012, maps relational data *to* RDF). So the next
step for anyone wanting this is not "an RDF adapter" but a declared graph-to-table map, with its own
issue and its own cost, and this record should be re-read against it rather than trusted from here.

## Limits, stated

Read against published specifications and one fetched ontology document, **not** against a provisioned
triple store, and no RDF is parsed anywhere in this workspace at this tree. Three specific limits:

1. **Three vocabularies is not "RDF".** SKOS-only glossaries, DCAT catalogues, OWL ontologies,
   schema.org graphs and any deployment-local vocabulary are unread here. The finding is that the three
   richest *cube and table* vocabularies yield nothing, not that no RDF vocabulary could.
2. **The QB reading is of Appendix C plus the normative vocabulary reference**, not of a survey of
   published cubes. A real-world cube could carry extension properties this record has not seen -
   though by §8.4 an aggregation term would be an extension, which is the deployment-defined map the
   section above says is the actual missing piece.
3. **The QB4OLAP reading is of the ontology document, not of a corpus.** Whether any published
   QB4OLAP cube also carries a table-and-column mapping alongside it is unmeasured, and it is the one
   observation that would reopen the crate question.
