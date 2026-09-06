---
title: What DataHub can carry, and the measure it cannot
description: A research spike, measured against the SemanticCatalog port rather than reasoned about - DataHub 1.7.0 does have first-class metric and semanticModel entities, and what it holds a measure as is a raw expression string tagged with a dialect, which this repository reads and reports and will not execute. The physical model, the descriptions and the join columns are what a deployment carries unconditionally; the Amendment, 2026-09-02 below narrows the rest, because a deployment-defined structured property carries a certified metric whole - so the metric layer and everything riding with it are conditional provides the adapter DECLARES rather than absences, and that declaration, not this sentence, is the authority for which kinds a bundle may carry. The two golden adapters stay the reference and keep the strict oracle test, and DataHub is a DECLARING adapter.
---

# What DataHub can carry, and the measure it cannot

Status: **accepted as a finding, and now acted on.** The finding was the deliverable; the adapter it
scheduled has since landed as `sutura-catalog-datahub` - a `declaring` `SemanticCatalog` that reads
recorded entity aspects and declares it provides structure, descriptions and the join columns and no
metric layer. Everything it decides is tested against a fake reader over recorded aspects; the HTTP
client over the versioned OpenAPI entity surface is still owed, and the read-path cost this record
left open has since been measured against a provisioned instance. **Issue #202 changed the *no metric
layer* half of that sentence, and the *Amendment, 2026-09-02* at the foot of this page is where the
change is recorded - read it before citing `provides no measure` above, read the *Addendum* under it
before citing decision 7 or the *Required filters* argument, both of which it narrows, and read
*Revision, 2026-09-04* below that before citing anything as unmeasured against a live instance.**

**Reframed after review, and the reframe is recorded rather than smoothed over.** The measurements
below are unchanged and were independently verified. What changed is the conclusion drawn from one of
them: the first version read the catalog conformance matrix as every adapter's contract and concluded
that DataHub needs a *composition* step before it can be useful. It does not. The oracle test is the
**golden adapters'** contract, and everything else conforms or declares - which is what
[pluggable by declaration](0011-pluggable-by-declaration.md) already decided and what
`sutura-catalog-local`'s own documentation already says about itself. The withdrawn reading is written
out in *Alternatives considered* rather than deleted, because the reasoning that produced it is the
reasoning somebody will produce again.

`AGENTS.md`'s crate table lists a `-datahub` crate as planned. Four accepted records name DataHub as a
metadata connector -
[pluggable by declaration](0011-pluggable-by-declaration.md),
[conformance packs](0012-conformance-packs-for-inputs-and-adapters.md),
[federating across different data systems](0007-federating-across-different-data-systems.md) and
[the plan](0009-the-plan-from-one-source-to-many.md) - and until this record
`docs/implementation-plan.md`'s stack table had **no row for it at all.** This
record closes that gap, and it puts the question before the row because the answer decides the row's
dependencies.

## The question

**Can DataHub express a complete semantic model as this repository defines one, or only part of one?**

Three answers were possible and each implies different work:

| If DataHub supplies | It is | What it costs |
| --- | --- | --- |
| complete `Definitions` | a second adapter held to the golden adapters' own contract | low - a registration, as the invariant promises |
| descriptions and a glossary only | a `Knowledge` source, which this repository confines to descriptive content with the prompt as its only consumer | a different feature, arguably a different port |
| **part** - the physical model and the prose, the measure still authored | a **declaring** adapter: it says which kinds it provides and which it does not, and is tested against that | a required capability declaration on `SemanticCatalog`, which does not exist yet |

**It is the third.** The reason is not the one that was expected, and the difference matters enough to
be the substance of this record: the expectation was that DataHub has no metric at all. As of
**1.7.0** it has one, it is `category: core`, and what it holds a measure as is **a raw expression
string tagged with a dialect.**

**The third row's cost is written as it is now understood, and it is smaller than the first version of
this record said.** That version priced it as a merge story - an assembler over several sources plus a
contribution manifest that moves every committed digest. The declaring reading prices it as a required
associated item on one port. Both are real work; only one of them is in the way.

## What was measured against

Read out of the code rather than out of the prose, because the prose is a summary and a port is a
signature. `SemanticCatalog::load` returns a `PinnedDefinitions`; its two halves are a `Definitions`
and a `Knowledge`; and `Definitions::assemble` is the only constructor of the first. So *can a source
express our model* is really *can it fill these fields, and survive these checks.*

| What has to be filled | The type in `sutura-domain` |
| --- | --- |
| a physical table and the column set it exposes | `Model` - `ModelName`, `SourceName`, `TableName`, `BTreeSet<ColumnName>`, `Description` |
| a join | `Relationship` - a name, two `(ModelName, ColumnName)` endpoints, and a `JoinType` of `OneToOne`, `ManyToOne` or `OneToMany` |
| what a metric measures | `Measure` - `Simple(Term)` or `Ratio { numerator, denominator, zero_denominator }`, over a `Term` of `Aggregate(AggregatedColumn)` or `CountIf { column }`, where `Aggregate` is the closed set `sum`, `count`, `count_distinct`, `avg`, `min`, `max` |
| a predicate that is part of what the metric MEANS | `Vec<RequiredFilter>` - `Equals`, `NotEquals`, `IsTrue`, `IsNotNull`, values typed as `DimensionValue` |
| when, and at what resolution | `ColumnName` plus a `BTreeSet<Grain>` |
| what it may be broken down by | `Dimension` - a column, optionally `via` one relationship, optionally an allowlist of at most `MAX_VALUES_PER_DIMENSION` values |
| the number it produced when it was certified | `Option<Anchor>` - a `TimeRange` and a value as text |
| what a reader has to know | `Knowledge` - phrases, caveats, reviewed absences and worked examples, each carrying a `Referent` that names a metric, a dimension of one, or a declared value of one |

Two of those are worth restating because they are the checks rather than the fields.
`Definitions::assemble` **refuses** a dimension reached through a relationship whose declared
cardinality may duplicate rows, as `JoinWouldDuplicateRows`. And every field above is a type with no
free-text SQL anywhere in it: `Measure` has no `expression:` key and no `Option<String>` at any depth,
which is the load-bearing half of
[first-party semantic models](0001-first-party-semantic-models.md).

## What DataHub 1.7.0 actually has

Everything below was fetched on **2026-08-29** from DataHub's public documentation and from the
public `datahub-project/datahub` repository. Where a schema is quoted it is the `.pdl` file rather
than the rendered page, because the page paraphrases and the schema does not. **Nothing here comes
from any deployment.**

**Metrics and semantic models are new, in Beta, and off by default.** The feature guide says so in its
own words: *"Metrics & Semantic Models is currently in **Beta**. The underlying entity model is stable,
but the UI experience, lineage visualization, and ingestion coverage are actively evolving."* Core
deployments enable it by setting `METRICS_ENABLED=true` on the GMS service; a managed deployment needs
version 2.1.0 or later and a per-tenant flag. Both halves of that are published
([feature guide](https://raw.githubusercontent.com/datahub-project/datahub/master/docs/features/feature-guides/metrics-and-semantic-models.md),
read 2026-08-29). **The release is `v1.7.0`, published 2026-08-04**, and the entity index at
[docs.datahub.com](https://docs.datahub.com/docs/generated/metamodel/entities/) reports 1.7.0 as the
current version. `metric` and `semanticmodel` both appear in that index, both are `category: core` in
`metadata-models/src/main/resources/entity-registry.yml` at the `v1.7.0` tag, and **the whole
`com/linkedin/metric` package is absent at the `v1.6.0.1` tag** - checked by asking the contents API
for that directory at both refs and getting a listing at one and a 404 at the other. So this is one
release old, which is the single most important fact about it.

**And DataHub says plainly what it is.** From the same guide: *"DataHub Metrics is a **catalog for
metric definitions**"*, and its FAQ answers *"Can DataHub query my metric values?"* with *"No. DataHub
is a catalog for metric definitions - the calculation, dimensional context, lineage, and governance.
Value computation and visualization stay in your BI tool or semantic layer."* That is not a limitation
being conceded; it is the product boundary, and it is the same boundary this repository sits on the
other side of.

### The shapes, verbatim

| Schema | Fields, as read |
| --- | --- |
| `metric/MetricInfo.pdl` | `name: string`, `description: optional string`, `created`, `lastModified`, `semanticModel: optional Urn`, `expression: optional MetricExpression`. **That is the entire calculation surface.** |
| `metric/MetricExpression.pdl` | one field: `dialects: array[DialectExpression]` |
| `metric/DialectExpression.pdl` | `dialect: Dialect`, `expression: string` - doc comment *"The raw expression string."* |
| `metric/Dialect.pdl` | `ANSI_SQL`, `SNOWFLAKE`, `MDX`, `TABLEAU`, `DATABRICKS`, `MAQL`, `OTHER` |
| `semanticmodel/SemanticFieldAnnotation.pdl` | `type: SemanticFieldType`, `expression: MetricExpression`, `aggregationFunction: optional string`, `dimension: optional Dimension` |
| `semanticmodel/SemanticFieldType.pdl` | `DIMENSION`, `MEASURE`, `FILTER`, `OTHER` |
| `semanticmodel/Dimension.pdl` | one field: `isTime: boolean = false`, and the record documents itself as intentionally minimal |
| `semanticmodel/SemanticModelRelationship.pdl` | `name`, `from`, `fromColumns`, `to`, `toColumns`, `aiContext`, `cardinality: optional ERModelRelationshipCardinality` |
| `ermodelrelation/ERModelRelationshipCardinality.pdl` | `ONE_ONE`, `ONE_N`, `N_ONE`, `N_N` |
| `schema/SchemaMetadata.pdl` | `fields: array[SchemaField]`, `primaryKeys: optional array[SchemaFieldPath]`, `foreignKeys: optional array[ForeignKeyConstraint]`, plus a deprecated `foreignKeysSpecs` |
| `schema/ForeignKeyConstraint.pdl` | `name`, `sourceFields: array[Urn]`, `foreignFields: array[Urn]`, `foreignDataset: Urn`. **No cardinality field.** |
| `glossary/GlossaryTermInfo.pdl` | `id`, `name`, `definition: string`, `parentNode`, `termSource`, `sourceRef`, `sourceUrl`, a deprecated `rawSchema`, plus custom properties. **No synonyms field.** |

Each of those was read at
`https://raw.githubusercontent.com/datahub-project/datahub/master/metadata-models/src/main/pegasus/com/linkedin/<path>`
on 2026-08-29, and the rendered entity pages for `metric`, `semanticmodel`, `dataset`, `glossaryterm`,
`businessattribute` and `structuredproperty` were read the same day under version 1.7.0.

**The ref each fact came from, because it is not one ref and pretending otherwise is how this record
would go stale invisibly.** The field lists above are the **development branch**. What was checked at
the `v1.7.0` **tag** is narrower and deliberate: that the `com/linkedin/metric` package exists there,
that it does not exist at `v1.6.0.1`, and that both entities are registered `category: core` there. The
field lists were not re-read per tag, so read them as *the model as it is being developed* rather than
as *the model 1.7.0 shipped* - and the *Beta* caveat below is the reason that distinction is not
pedantic: one aspect's schema version already differs between the tag and the branch.

## Field by field

| What we need | What DataHub has | Verdict |
| --- | --- | --- |
| `Model.table`, `.columns` | a `dataset` entity with `schemaMetadata.fields` | **faithful** |
| `Model.source` | the `dataPlatform` URN, and `dataPlatformInstance` | **faithful**, with a name mapping a deployment decides |
| `Model.description` | `datasetProperties`, `editableDatasetProperties`, `documentation`, `institutionalMemory` | **faithful, and richer than ours** - separate ingested and human-edited prose, and multiple documentation sources |
| `Relationship` endpoints | `SemanticModelRelationship.from`/`fromColumns`/`to`/`toColumns`, or `schemaMetadata.foreignKeys` | **faithful, and WIDER**: both are arrays, so a multi-column join is expressible where ours is one column each |
| `Relationship.join_type` | `SemanticModelRelationship.cardinality`, and `ERModelRelationshipProperties.cardinality` | **present, and declared unsupported anyway** - see below |
| `Measure` | `MetricInfo.expression`, or a `MEASURE`-annotated field's `aggregationFunction` | **NOT faithful** - see below |
| `Metric.required_filters` | nothing first-class | **absent** - see below |
| `Metric.time_column` | `Dimension.isTime` marks a dimension as temporal | **partly**: which dimension is time, not which column a metric measures time on |
| `Metric.grains` | nothing | **absent.** No grain, no resolution, no time-bucket vocabulary anywhere in the two packages |
| `Dimension.allowed_values` | nothing on a field. `glossaryRelatedTerms.values` is a term-to-term `HasValue` edge; `structuredProperty` has an `allowedValues` array bounding its OWN value | **absent** for a column allowlist. Both near-misses are enumerations over something else |
| `Anchor` | nothing, and by design - see the FAQ quoted above | **absent** |
| `Knowledge` glossary | `glossaryTerm` with a required `definition`, `glossaryNode` for hierarchy, `glossaryRelatedTerms` for `IsA`/`HasA`/`HasValue`/`IsRelatedTo` | **partly** - rich, and no synonym field; a second spelling has to be a second term related to the first |
| lineage | `upstreamLineage`, `fineGrainedLineages`, `metricUpstreams` with dataset and field edges | **we have no shape at all** - see below |

## The four sticking points

### The measure, which is the answer

**DataHub's measure is a string, and the closed vocabulary has no counterpart in it.**

`MetricInfo.expression` resolves, in two hops, to `{ dialect, expression: string }` per dialect. There
is no numerator, no denominator, no aggregate, no column, and no operator. The official example in
DataHub's own SDK library - `metadata-ingestion/examples/library/semantic_model_create.py`, read
2026-08-29 - writes a whole metric as `expression="SUM(ORDERS.amount)"` with
`dialect=DialectClass.SNOWFLAKE` and nothing else about the calculation. That is precisely the
shape [a closed vocabulary for measures](0002-a-closed-vocabulary-for-measures.md) exists not to have,
and [first-party semantic models](0001-first-party-semantic-models.md) states why in one line: *"A
string field is an escape hatch, and an escape hatch on the query path is the thing being defended
against."*

**There is one structured signal, and taking it would be worse than refusing it.**
`SemanticFieldAnnotation` carries `aggregationFunction: optional string` beside `expression`, so a
`MEASURE`-annotated field can say `SUM` next to the column it annotates - which looks like exactly
`AggregatedColumn::new(Aggregate::Sum, column)`. Three reasons not to harvest it, and the third is the
one that decides:

1. **It is an open string, not an enum.** Parsing it against our `Aggregate` set is cheap and is the
   pattern this repository already uses, so this alone is not an obstacle - it is a `parse` that
   refuses what it does not recognise.
2. **`optional`, and a search of the public ingestion sources suggests it is mostly absent.** Of the
   connectors that emit these entities at all, the one present in the 1.7.0 release populates no
   aggregation function; the one that does populate it is on the development branch and not in the
   release. **That is a code search rather than a measurement of any deployment**, so read it as
   *nothing found* rather than *nothing exists* - a deployment authoring annotations through the SDK
   would populate the field by hand, exactly as the published example does.
3. **The two fields are independently authored and nothing reconciles them.** The published example
   writes `aggregation_function="SUM"` and `expression="SUM(amount)"` on adjacent lines of one field
   definition, as two values a human typed twice. Harvesting the first and discarding the second means **certifying a number from
   half of a definition whose other half we did not read** - and where the halves disagree, the answer
   is wrong under a certified name, arrived at by omission. That is the exact failure mode this
   repository is arranged against, and `recurring_revenue` in the example catalog is the worked
   instance of it: the whole definition is `sum(mrr_cents)` **and** a filter on `status`, and either
   one alone is a different number.

**And the dialect sets do not intersect, which closes the other route.** The obvious alternative is to
take `expression` as authored SQL through the hatch
[a named escape hatch for authored SQL](0004-a-named-escape-hatch-for-authored-sql.md) decides. It does
not reach: `sutura_sql::dialect::ALL` is `DuckDb`, `Postgres` and `ClickHouse`, and DataHub's `Dialect`
is `ANSI_SQL`, `SNOWFLAKE`, `MDX`, `TABLEAU`, `DATABRICKS`, `MAQL`, `OTHER`. **Not one value is in
both sets**, and three of DataHub's are not SQL at all - MDX, Tableau's calculation language and MAQL
would not parse in a SQL parser and are not meant to. Getting from `ANSI_SQL` to our three would be a
translation, and *we never translate SQL we did not generate* is an invariant with a mechanism behind
it. Even setting that aside, the hatch is in `AGENTS.md`'s *Built And Not Wired* section: no shipped
binary can execute an authored expression, because the engine generates no SQL and the adapter that
renders is a dev-dependency. So reading `metricInfo.expression` as a computation would move a refusal
from load time to query time rather than answer anything.

### Cardinality: declared, optional, and one value we cannot represent

**DataHub does declare cardinality, which was the open question, and the news is better than
expected and smaller than it looks.** `SemanticModelRelationship.cardinality` is an
`ERModelRelationshipCardinality` of `ONE_ONE`, `ONE_N`, `N_ONE`, `N_N`. Three of those map onto
`JoinType` directly. `N_N` has **no `JoinType` variant**, so a relationship declaring it has to be
refused by the adapter - and there is no `InconsistentDefinitions` variant for *a cardinality we
cannot represent*, because until now no source could state one. That refusal would be the adapter's
own typed error, which is where a source's vocabulary is supposed to be mapped.

Four limits, each stated next to the claim, and the second one is the reason this section reads as a
warning rather than as good news:

- **It is `optional`.** A relationship with no cardinality declared cannot be used to reach a
  dimension at all, because `Definitions::assemble` needs a `JoinType` and the safe default does not
  exist: assuming `ManyToOne` is assuming the fan-out away, and assuming `OneToMany` refuses every
  dimension. So absent means refused, and absent is the ordinary state of ingested metadata.
- **On the PHYSICAL relationship it is not optional - it defaults to `N_N`.** The other consumer of
  that enum is `ERModelRelationshipProperties`, and the field there reads
  `cardinality: ERModelRelationshipCardinality = "N_N"` - verified at the `v1.7.0` tag. **A default of
  many-to-many is a default of the most permissive value**, which means a relationship nobody thought
  about carries the same declaration as one somebody decided was many-to-many, and the two are
  indistinguishable. For us that is the harmless direction only by accident: `N_N` maps to no
  `JoinType`, so both get refused. **The direction to be careful about is a future temptation to
  "read the cardinality and use it"** - that would be reading a default as a decision, which is the
  precise failure mode 0011's evidence argument is written to avoid. `AGENTS.md`'s trusted-precondition
  caveat gets *weaker* on this input, not stronger, and this record refuses to describe it as an
  improvement.
- **It is a declaration, exactly as trusted as ours.** `AGENTS.md` records that *catalog cardinality
  is a trusted precondition: nothing checks the declaration against the data.* Reading it from DataHub
  does not close that - it moves the person who is trusted from whoever wrote a markdown file to
  whoever authored the semantic view, and DataHub validates nothing about the rows. That is a
  governance improvement in some deployments and a governance question in others; it is not a check.
- **The join columns are unpaired arrays.** `fromColumns` and `toColumns` are two `array[string]`
  fields with nothing binding index *i* of one to index *i* of the other. Ours is a single column each
  precisely so that no such pairing can be got wrong; a source that carries the wider shape carries the
  ambiguity with it, and an adapter reading a two-column join is guessing an order.
- **The evidence path 0011 describes does not come through this aspect.**
  [Pluggable by declaration](0011-pluggable-by-declaration.md) argues that a primary-key or unique
  constraint is evidence, in one direction, for the side being joined to being unique.
  `schemaMetadata.primaryKeys` exists and gives that. **`ForeignKeyConstraint` carries no cardinality
  field at all** - verified by reading it - so on an ordinary warehouse dataset the join and the
  uniqueness evidence come from two different places and the cardinality declaration comes from
  neither. The one-direction argument in 0011 survives intact; what it does not get is a free ride on
  the new relationship shape.

### Required filters: absent, and absent in the way that matters

`required_filters` is the field that makes a metric mean what its name says, and **DataHub has no
counterpart.**

The `metric` entity's registered aspect list, read out of `entity-registry.yml` at the `v1.7.0` tag, is
`metricInfo`, `metricRelationships`, `metricUpstreams`, `ownership`, `domains`, `globalTags`,
`glossaryTerms`, `institutionalMemory`, `structuredProperties`, `status`, `deprecation`,
`dataPlatformInstance`, `subTypes`, `documentation`, `browsePathsV2`, `applications`, `aiContext`.
**No aspect on that list carries a predicate, and none carries a grain either** - which is the same
listing that settles two of the absent rows above.

- `SemanticFieldType` has a `FILTER` symbol, documented as *"A named boolean predicate used to filter
  results, distinct from a dimension"*. Its body would be a `MetricExpression`, so it is a SQL string
  again, and the research found no code path in the public repository that emits it.
- There is a record called `DatasetFilter`, and it is the nearest thing in the model. It carries **no
  `@Aspect` annotation**, so nothing can hold it as metadata about a dataset; its `type` is an enum
  with exactly one symbol, `SQL`, documented as *"The partition is represented as a an opaque, raw SQL
  clause"*; and its own comment says *"In the future, we'll likely add support for more structured
  predicates."* Its one consumer is a volume assertion's monitoring scope. So the nearest thing to a
  predicate in DataHub is a monitoring detail, is opaque by its own description, and is not attachable
  to a dataset at all.
- The metric entity's own documentation puts `filters` - along with `additivity` and `metricKind` -
  into `structuredProperties`, explicitly as platform-specific metadata that does not yet warrant a
  first-class field. A structured property is genuinely typed - five value types, an optional
  `allowedValues` list, `SINGLE` or `MULTIPLE` cardinality, an immutability flag, and a server-side
  validator that enforces all of it on write. **And it is scalar-only.**
  `PrimitivePropertyValue` is `typeref PrimitivePropertyValue = union [ string, double ]`, verified at
  the `v1.7.0` tag, so a structured property value is a string or a number and there is no nested or
  record-typed form. A four-operator predicate over a named column put there is a JSON string DataHub
  will validate for length and never interpret. What a structured property cannot carry is therefore
  not only the *obligation* - it cannot carry the *shape* either. Which is the point:

**Nothing in DataHub says a filter is REQUIRED.** `RequiredFilter` is not a predicate a caller may
apply - it is one applied to every question about the metric, which a caller cannot see, choose or
remove. That is a statement about how a definition must be *used*, and a catalog that records a
predicate as a property records a fact, not a duty. So even a deployment that populated a structured
property called `filters` would be handing us a value we would have to decide to enforce, and
deciding that on a source's behalf is exactly the substitution of our reading for the author's that
0001 refuses.

### What DataHub has that we have no shape for

**Lineage, and it is out of scope rather than useful.** `upstreamLineage`, `fineGrainedLineages` and
`metricUpstreams` model dataset-level and column-level derivation. There is **no lineage type anywhere
in `crates/`** - `grep -rni lineage crates/` returns nothing - and none is wanted on the query path: a
plan resolves to exactly one source, a measure reads columns a model declares, and knowing where a
column came from upstream changes none of it. It is genuinely valuable metadata and it is somebody
else's product. Where it could matter later is provenance for a human reviewing a promotion candidate,
which is a different feature from anything in this repository today.

**`AiContext` is the one that is not out of scope**, and it is the closest thing in DataHub to work
this branch has already done. `aiContext` is a registered aspect on `metric` and on `semanticModel`
themselves - read out of `entity-registry.yml` at the `v1.7.0` tag - as well as attaching to a schema
field and to a semantic-model relationship, and it carries `synonyms`, `instructions`, `examples` and
`customInstructions` - which is very nearly the shape of `Knowledge`: a glossary phrase, a caveat, a
worked example, attached to the metric it is about. Two things follow. It is the field
a DataHub `Knowledge` contribution would come from, rather than the glossary alone, because
`GlossaryTermInfo` has no synonyms field and `Phrase` is what our glossary is made of. And it arrives
as unbounded free text over a network - four fields, no length cap, no normalisation, no uniqueness
rule - which makes `Phrase::parse`, `NoteBody::parse` and `MAX_KNOWLEDGE_BYTES` load-time gates on a
real input rather than bounds on a file somebody in this repository wrote. That is the first time those
bounds would be doing the job they were designed for. **It is also on `metric` and `semanticModel` and
not on `glossaryTerm`**, which is worth knowing before assuming the glossary is where a DataHub
`Knowledge` contribution comes from.

**And here is the one place the two designs point in opposite directions, named rather than elided.**
DataHub's published roadmap for this feature commits to natural-language questions being resolved
against catalogued metric definitions **using those synonyms** - which is server-side phrase
resolution. This repository refuses that by construction: `RefusalReason` has no `PhraseNotDefined`,
the glossary renders into the agent-facing prompt, and **the agent states which metric it chose in its
own transcript**, which is what makes the choice auditable. Both designs put a synonym list next to a
metric; one resolves it inside the service and one hands it to the caller to resolve out loud. **That
is a real fork and not a gap on either side.** For an adapter it means one specific thing: harvesting
`synonyms` into `Phrase` is legitimate, and letting anything server-side *match* on them is the
architecture decision `AGENTS.md` says a second consumer of knowledge would be.

**Also worth recording, because it is the interchange story nobody asked for:** DataHub's `Dialect`
enum documents itself as aligned one-to-one with the Open Semantic Interchange specification. If a
neutral wire format for a metric definition ever matters here, that is where two vendors have
converged, and it is a better place to look than at any one tool's model.

## The answer

**Case three. DataHub supplies part of a semantic model as this repository defines one.**

Precisely: it supplies **the physical model, the descriptions, the joins with a declared cardinality,
and a glossary that needs a metric to point at.** It does not supply **a measure this repository will
execute, a definitional filter in any form, a grain, a value allowlist, or an anchor.**

There is a sharp consequence in the test suite, and it is worth stating before the decision because
the first version of this record drew the wrong conclusion from it.

`crates/sutura-app/tests/golden/catalogs.rs` expands seven behaviours over every registered catalog,
and its central one is `agrees_with_the_oracle`: every registered `SemanticCatalog` must produce
**the same `Definitions`** as the hand-written oracle over `examples/single-player`. That corpus
contains `measure: { simple: { aggregate: sum, column: mrr_cents } }` and
`required_filters: [{ equals: { column: status, value: active } }]` and `grains: [month]` and value
allowlists and an anchor. **A source that cannot express five of those cannot pass that test**, and no
amount of adapter code changes it - the missing thing is in the source. That finding stands, and it has
been verified independently rather than taken on this record's word.

**What it MEANS is the part this record had to correct.** The first version concluded that DataHub
therefore needs a *composition* step in front of it - something to supply the measures so that a
combined bundle could satisfy the oracle. That is one reading. It is not the one this repository had
already decided, and the difference is not cosmetic: composition-first makes a narrow source unusable
until unbuilt work lands, and the reading below makes it usable now.

**The reading that matches [pluggable by declaration](0011-pluggable-by-declaration.md) is that the
oracle test is the GOLDEN adapters' contract and not every adapter's.** There are two golden adapters.
`sutura-catalog-local` is the metadata reference - wren-style, where the model and the structures are
defined here, so it can be held to producing all of them - and `sutura-exec-duckdb` is the data-system
reference. Everything else **conforms, or declares what it does not provide.** A declaring adapter is
then tested against its own declaration rather than against the reference bundle: what it says it
provides must be exactly what it provides, and what it says it does not provide must be **visibly
absent rather than silently missing.**

That is not this record's idea to invent. It is in the tree already, in `sutura-catalog-local`'s own
words, and it predicted this case before anybody measured it:

> **This adapter declares every knowledge capability there is, and that is a statement about the
> ADAPTER rather than about the directory it read.** [...] A metadata-service adapter is the other
> case: it has glossary terms with synonyms and no way at all to record an absence, so it will declare
> the two it can represent and never the other two. [...] `KnowledgeCapabilities::all` rather than a
> list of the four, deliberately [...] which is what makes this **the reference adapter** [...] An
> adapter mapping a fixed external schema gets the opposite treatment - `of([..])`, so a new kind
> leaves its declaration alone.

**That prediction is now measured, and it is nearly right and wrong in one checkable place.** DataHub
does have glossary terms and does have no way to record an absence, exactly as written. What it does
not have is synonyms *on the glossary term*: `GlossaryTermInfo` has no such field, and the synonym list
lives on `AiContext` instead. The shape of the conclusion survives; the field it names moved.

So the invariant *"Adding a metadata provider or a data system is a registration, not a test edit"* is
**true as written, and what this record adds is WHICH test.** For a golden adapter it is the oracle. For
a declaring adapter it is fidelity to its declaration - still a registration plus a declaration rather
than a test edit, because the declaration is what selects the assertions.

**And a narrow source is usable on its own, which is the claim the composition-first reading quietly
denied.** Checked rather than assumed: `Definitions::assemble` has **no minimum-metric refusal** -
there is no `NoMetrics` variant and no such check anywhere - so a bundle of models, relationships and
zero metrics assembles, pins and validates, because zero metrics is zero anchors. 0011 already says the
prompt can state *this deployment carries no certified metric layer* as a fact derived from what it was
handed rather than from something a source claimed. A DataHub-only deployment therefore gets a bundle,
a prompt that tells the truth about it, and a path to add metrics. It does not get a refusal at load
for being narrow.

**The honest limit on that, stated next to the claim, and it is not about DataHub.** What a zero-metric
bundle cannot do on its own is open a data system, and the two composition roots differ - which is
worth getting right because the mechanism moved recently - **twice, and the second move dates this
paragraph's own original wording, which said the command-line tool reads no source registry.** It does,
since telekom/sutura#121. Both roots read the `sources:` tree: every source a catalog names is looked
up there, and a missing entry is *"this catalog reads from `<source>`, and no `sources.<source>` entry
declares where that ..."*. They differ in what an ABSENT entry means. `sutura-serve` refuses, full
stop. `sutura-cli` falls back to its own built-in declaration - a `files` source called `local` over
the directory on the command line - so an undeclared source must still be named `local` there, because
nothing declared it and an engine wearing another name over the caller's own files would answer that
catalog's certified numbers out of them. Both refuse a catalog declaring **no models** - *"this catalog
declares no models, so there is nothing to open"* - which a DataHub bundle passes. So a
DataHub bundle whose models carry platform URNs is servable exactly when a deployment has declared a
source per platform it wants to read. **That is configuration, not a refusal of DataHub**, and it is
the concrete thing the guidance below has to mention. It is written here so nobody reads *usable alone*
as *answers questions alone*.

## The decision

**1. There are two golden adapters, and they are the reference everything else is measured against.**

`sutura-catalog-local` on the metadata side and `sutura-exec-duckdb` on the data side. The reason a
golden adapter can be held to the whole model is that the model is defined here: a wren-style directory
of markdown with YAML frontmatter has a document shape for every field `Definitions::assemble` needs,
so *"produce the same `Definitions` as the hand-written oracle"* is a contract it can meet. **That
contract stays exactly as it is and is not weakened by anything in this record.**

**2. Every other adapter conforms, or DECLARES what it does not provide - and the negatives are the
point.**

An absence must be **declared**, never inferred from silence. 0011 decided this and already carries the
three-state distinction that makes it work: *declared*, *not declared*, and *declared-and-empty*, with
the note that *not declared* means the prompt must not imply the absence list is complete. A map with
no entries cannot tell those apart, which is why the declaration exists at all.

**The mechanical shape to reuse is already in the tree, and it should be copied rather than reinvented.**
`Warehouse::IMPERSONATION` is an associated constant **with no default**, so an adapter that omits it
does not compile - pinned by a `compile_fail` doctest whose struct is literally named `Undeclared`,
beside a compiling twin named `Declared` differing in exactly the one line. Its own documentation gives
the argument in full: *"A defaulted capability would mean an adapter that said nothing got the benefit
of the doubt in whichever direction the default pointed - and both directions are wrong."* And the
negative has a **name** rather than being an absence: `ImpersonationCapability::NoPlaceForASubject`,
whose doc says saying so explicitly is the point.

The contrast next door is the other half of the vocabulary. `dry_run` **is** defaulted, and its doc
explains why: *"an adapter for which it is not cheaper has no way to say so if the port demands an
implementation, and the honest thing for it to do is nothing."* So the rule this record adopts is that
one: **a capability whose absence changes what a caller may believe is required with no default; a
capability whose absence is merely a missed optimisation may be defaulted, and the default says why.**

The metadata-side analogue does not exist yet. `SemanticCatalog` today is an associated `Error` and
`load`, and nothing on it declares anything. **That is the gap this record schedules**, and it is the
one thing here that is a change to a port rather than a new adapter.

**3. What DataHub declares, concretely - and this is where the research lands.**

Not a list of blockers. The content of a declaration, with the negatives named because naming them is
what makes the source safe to use. **Every *does NOT provide* below that a deployment-defined structured
property can supply is narrowed by the *Amendment, 2026-09-02*** to a declared-and-empty may-provide, and
the adapter's own capability declaration - not this table - is the authority for which those are:

| Kind | DataHub declares | On the evidence of |
| --- | --- | --- |
| Structure - tables, columns, types | **provides** | `dataset` + `schemaMetadata.fields` |
| Descriptions | **provides**, and richer than ours | four aspects, ingested and human-edited kept apart |
| Relationships - the join columns | **provides**, with a caveat below | `SemanticModelRelationship`, or `schemaMetadata.foreignKeys` |
| Relationship CARDINALITY | **does NOT provide** | optional on the semantic join; on the physical relationship it *defaults to `N_N`*, so a default is indistinguishable from a decision |
| Metrics and measures | **does NOT provide** | `MetricInfo.expression` is a raw string in a dialect set that does not intersect ours; `aggregationFunction` contradicts it with nothing reconciling the two |
| Definitional filters | **does NOT provide** | no aspect carries a predicate; the nearest record has no `@Aspect`; structured properties are scalars |
| Grains | **does NOT provide** | nothing in either package; `Dimension` is one boolean |
| Value allowlists | **does NOT provide** | no field-level enumeration; the two near-misses enumerate something else |
| Anchors | **does NOT provide** | DataHub's own FAQ: value computation stays in the BI tool |
| Glossary phrases | **provides, conditionally** | `AiContext.synonyms` plus `glossaryTermInfo.definition` - and only where the bundle already declares a metric for a `Referent` to name |
| Caveats | **provides, conditionally** | `institutionalMemory`, `documentation`, `deprecation.note` - same `Referent` condition |
| Reviewed absences | **does NOT provide** | there is no *deliberately undefined* concept; `deprecation` and `status.removed` are different claims |
| Worked examples | **does NOT provide** | `AiContext.examples` is free text, and an `Example` must carry a `Query` that validates against the metric's grains, dimensions and allowlists |

**Two of those are "does not provide" where the field EXISTS, and that is the interesting kind.**
Cardinality and the measure are both present in DataHub and both declared unsupported here - not
because reading them is hard, but because reading them would be reading something the source does not
guarantee. The `N_N` default means an unconsidered relationship and a considered many-to-many are the
same value. The `aggregationFunction`/`expression` pair means taking either one alone certifies half a
definition. **Declaring those two unsupported is a better outcome than harvesting them**, and it is a
better outcome than the composition step the first version of this record proposed: it is one line of
declaration each, it is visible in a diff, and it fails nothing.

**4. `metricInfo.expression` is READ and REPORTED, never executed and never converted.**

A DataHub metric is a **promotion candidate**: a name, a description, an owner, a semantic model, a set
of dimensional fields, and a string somebody wrote in a dialect this repository does not render. What
an adapter may do with the string is show it to a person deciding whether to author a `Measure`. What
it may not do is compile it, translate it, or infer a `Measure` from the `aggregationFunction` beside
it. **Unbuilt** - a decision about what the adapter will be permitted to do, not a description of
anything that exists.

**5. A relationship reaches a dimension only where cardinality is declared and representable.**
Absent or `N_N` is refused by the adapter, naming the relationship, rather than defaulted in either
direction: `ManyToOne` as a default assumes the fan-out away, and `OneToMany` refuses every dimension,
so one is unsafe and the other is a silent feature removal. **Under decision 3 this is not a
degradation** - the adapter declares that it does not provide cardinality, so a deployment reads that
in the declaration rather than discovering it when a dimension is missing. The relationship still
arrives; what it cannot do on its own is license a join.

**6. `agrees_with_the_oracle` is NOT weakened, and a declaring adapter needs a different assertion.**

**Status: implemented.** What this decision specified was the *split in the conformance matrix* - the
part that "deliberately not written" left open - and the catalog-matrix diff writes it: a required
`SemanticCatalog::KIND` (`CatalogKind`, golden or declaring) with no default, a `GoldenCatalog` marker
that routes the golden-only cells, and a registration whose `declaring`/`golden` tag is a compile-time
assertion against that constant - so a registration that disagrees with its own declaration does not
build, and `agrees_with_the_oracle` (and the three example-corpus cells) cannot be expanded for a
declaring adapter at all. The AGENTS.md registration row states the same split. The oracle test itself
is untouched, held by the golden adapters only, exactly as this decision says it must be. The golden
adapters keep the oracle test.
A declaring adapter gets **declaration fidelity**, which is two assertions rather than one:

- *everything it declared, it produced* - for each declared kind, the bundle carries content of that
  kind, so a declaration is not aspirational;
- *everything it did not declare is absent* - nothing of an undeclared kind appears in the bundle,
  which is the direction `Knowledge::assemble`'s `UndeclaredContent` guard already covers for knowledge
  and which nothing covers for definitions.

Plus the negative-capability shape 0012 already decided: **where a declared absence has something to
try, the pack tries it and the absence must hold**, and where there is nothing to perform, no test is
written - because *"a green test named `..._is_declared_unsupported_...` over nothing is
coverage-shaped and measures nothing"*. A metadata adapter that declares it provides no grains has
nothing to perform, so what holds there is the fidelity assertion above and not an invented action.

**7. Guidance, not requirement - and it is the easiest thing here to overshoot.**

A deployment that already runs DataHub gets value from the model it already has: that is decision 3,
and nothing in the docs may turn it into a precondition. So the guidance that lands with the connector
**states what a user MAY populate and what each thing buys**, once, with the benefit next to it - and
never that DataHub must be configured a particular way for this to work. Concretely, the shape it may
take: a semantic model with `cardinality` set on its relationships lets those relationships license a
join; `AiContext.synonyms` on a metric a deployment has also certified lets a glossary phrase render;
and a `sources.<alias>` entry per platform a DataHub model names is what lets that model's data system
be opened at all. **Each of those is an option with a payoff, and the absence of all three is a
supported configuration** - the bundle still loads, and the prompt still tells the truth about it. The two sentences that may not be written are *"configure DataHub like this"* and
*"DataHub is not usable without X"*, because the second one is false and the first one is not ours to
say.

**8. Lineage is out of scope, and `docs/architecture.md` overstated it.** That page said metrics,
dimensions, the glossary *and lineage* arrive through `SemanticCatalog`. There is no lineage type in the
workspace and none is planned, so the sentence claimed an input that does not exist. Corrected on this
branch, in the same diff as this record, because an overstated claim is itself the defect.

## Consequences

- **The stack table gains a capability-declaration row and a connector row.** The declaration row can
  start now, because it is a port change plus a conformance shape and needs no live service. The
  connector depends on it. **Neither depends on composition**, which is the substantive change from the
  first version of this record.
- **`SemanticCatalog` gains a required declaration, and that is the only port change here.** Today the
  trait is an associated `Error` and `load`. What it needs is the `Warehouse::IMPERSONATION` shape: an
  associated item with **no default**, so an adapter cannot be silent about what it cannot supply, with
  a `compile_fail` doctest and its compiling twin to pin that the omission does not build. `dry_run` is
  the counter-example to imitate deliberately, not accidentally - it is defaulted, and its doc says why.
- **`docs/implementation-plan-identity-and-services.md` said the remaining metadata connectors are
  all "`feat/source-registry`-shaped once the packs exist: a registration, a declaration, and
  fixtures."** That is closer to right than the first version of this record allowed, and it is still
  not right: a registration and a declaration is exactly the cost, **once the declaration exists to
  make** - and it does not exist yet. So the correction is narrower than the one this branch first
  wrote. It is corrected on this branch to say that.
- **0011's metadata half stops being wholly dormant, but only the part this needs.** That record's
  metadata work is three things: the capability declaration, the assembler over N sources, and the
  contribution manifest. **The declaration is scheduled here. The assembler and the manifest are not**,
  and this record no longer claims they block anything - they become valuable when a deployment wants
  DataHub's structure *and* certified metrics in one bundle, which is a real want and a separate step.
  0011's own sentence still applies to those two: *"a decision whose record is accepted and whose branch
  does not exist is a decision, not progress."*
- **No committed digest moves from the narrow declaration, and a correction to the claim it made.**
  The first version of this record priced a manifest that would have moved every pin. Under the
  narrow declaration nothing the reference adapters hash is hashed differently, because the physical
  content is unchanged - and the declaration ITSELF now travels in each contributor's
  `ContributionManifest` under the digest (0011's later amendment), which means an adapter that
  WIDENS its declaration moves its own pins, and one that does not keeps them. The `datahub`
  amendment's widening is exactly such a move; `sutura-catalog-local`'s example digest moving would
  be the failure signal. **The knowledge capabilities already under the digest are unaffected**; they
  travel with the bundle as they do now.
- **A narrow source is servable, and the prompt has to say so honestly.** A bundle with zero metrics
  loads. What the prompt says about it is derived from the bundle rather than authored, which 0011
  already decided, so a deployment reading only DataHub is told there is no certified metric layer
  rather than being told nothing. Nothing in this record adds prose to the prompt.
- **The Beta flag is a real risk to price, and the declaration is what contains it.** `MetricInfo` is at
  schema version 4 in the release and 5 on the development branch; `SemanticModelInfo.datasets` is
  deprecated on the branch and not in the release; membership moved from the model side to the member
  side within one cycle. An adapter built against the metric entity this quarter would be revised.
  **Under decision 3 the adapter does not read the metric entity at all - a statement the `sutura`
  amendment narrows, because the certified path reads `MetricInfo.expression` BESIDE the deployment's
  own property.** The moving part of
  DataHub's model is outside what this connector depends on - `schemaMetadata`, the dataset property
  aspects and the glossary are the long-established part, and `schemaMetadata` even carries a deprecated
  foreign-key field superseded by a newer one, which is what an aspect that has been through a migration
  looks like. That is a durability argument for the narrow declaration, and it is the second one after
  correctness.

## What could not be determined

Stated rather than reasoned around, because an overstated claim here would send the implementation the
wrong way.

- **Whether a managed DataHub offering carries a structured metric shape the open-source model does
  not.** Nothing public says so, and no cloud-only schema documentation was found. The published
  difference is enablement - a version floor and a per-tenant flag - not a different schema.
- **Which lineage shape 1.7.0 intends for a metric that has a semantic model.** The aspect schema and
  the SDK tutorial say different things about whether the semantic model is a lineage hop. It does not
  bear on anything decided here, and it is a live contradiction in the upstream documentation.
- **Whether any `FILTER`-annotated field is emitted by anything.** The symbol is in the enum; a search
  of the public repository found no emitter. *No emitter found* is weaker than *no emitter exists*, and
  a deployment authoring annotations through the SDK could populate it by hand.
- **How much of a real DataHub instance's metric content is populated in practice.** That is a question
  about deployments and cannot be answered from a schema. It matters, because a `semanticModel` with no
  relationships and no `MEASURE` annotations contributes descriptions and nothing else - which would
  make DataHub the canonical narrow source 0011 describes rather than the rich one its entity list
  suggests.
- **The read path's cost, though its shape is now known and is the better news in this record.**
  DataHub publishes Python and Java SDKs and **no Rust client** - its integration tree holds one
  language and it is not this one - so the adapter is an HTTP client written here. Which surface is
  settled by DataHub's own guidance rather than by preference: its API overview says the GraphQL API
  assumes frontend callers, comes with caching and *"operations are intentionally limited in scope"*,
  and that *"care should be taken when used programmatically"*. **The OpenAPI v3 entity surface is the
  one to build against** - versioned paths under `/openapi/v3/entity/{entityName}[/{urn}[/{aspect}]]`
  with batch and scroll forms, an `If-Version-Match` header for reading a specific aspect version, and
  a spec the deployment serves for itself at `/openapi/v3/api-docs`, generated from the handlers rather
  than hand-written. Authentication is a personal access token as a bearer. What was **not** evaluated
  is the cost: how many requests a bundle takes, whether the scroll surface is usable for a whole
  catalog, and what keeps a generated client from drifting. That is the adapter's first engineering
  question and not this record's.
- **One thing worth knowing rather than deciding:** DataHub ships its own agent-facing package
  exposing catalog search and lineage traversal over the Model Context Protocol, and it can be
  configured to expose metadata **mutations**. It is not a competitor to the certified surface here -
  it answers *what metadata exists*, not *what is this number* - but a deployment that runs both is
  giving an agent two tools with very different guarantees, and which one an agent reaches for is a
  deployment question this repository cannot answer for it.

## Alternatives considered

**Composition first: block the connector on an assembler and a contribution manifest.** This is what
the first version of this record decided, and it is written out rather than deleted because the
reasoning that produced it is the reasoning somebody will produce again. It follows from reading
`agrees_with_the_oracle` as every adapter's contract: if a registered catalog must match the oracle,
and DataHub cannot, then something must supply the difference. **Rejected because the premise is
wrong** - the oracle is the golden adapter's contract, 0011 already decided that adapters declare, and
`sutura-catalog-local`'s own documentation already calls itself the reference and already prescribes
`of([..])` for an adapter mapping a fixed external schema. The cost of the mistake was not small: it
would have made a deployment that already runs DataHub wait for an assembler and a digest migration
before getting anything, and it would have put a *"one source may provide a given kind"* precedence
argument in front of a source that provides kinds nothing else does. Composition remains decided and
unbuilt in 0011, and it remains genuinely wanted - for the deployment that has DataHub's structure and
certified metrics elsewhere - but it is not this connector's precondition.

**Treat DataHub as a `Knowledge` source only.** The second of the three cases. Rejected because it
throws away the half of DataHub that is genuinely load-bearing: `schemaMetadata` is the authoritative
column list for a great many warehouses. Declaring descriptions only would be flattening a source to
its least interesting capability, which
[pluggable by declaration](0011-pluggable-by-declaration.md) argues against by name. It is also
self-defeating under decision 3: a glossary phrase needs a `Referent` naming a metric, so a
knowledge-only DataHub with no metrics anywhere contributes *nothing at all*, whereas a
structure-and-descriptions DataHub contributes a working bundle.

**Read `metricInfo.expression` through the authored-SQL hatch.** Rejected on two independent grounds,
either of which is sufficient: the dialect sets do not intersect and closing the gap is translation;
and no shipped binary can execute an authored expression, so the load would succeed and the question
would still be refused, one stage later and less clearly.

**Derive a `Measure` from `aggregationFunction` plus the annotated field.** Rejected because the
expression beside it is authored independently and nothing reconciles the two, so the derivation
certifies half a definition. This is the alternative that looks cheapest and is the one whose failure
mode is a wrong number under a certified name. **Declaring the capability unsupported is the cheaper
outcome and the honest one** - it costs one line and it tells a deployment the truth.

**Read the cardinality and use it.** Rejected, and it is the closest call in this record, because
cardinality is the one thing 0011 hoped a rich source would improve. On the semantic-model join it is
`optional`; on the physical relationship it defaults to `N_N`. Using it would mean treating a default
as a decision on the input where the default is *many-to-many* - so a deployment that never thought
about a relationship would get the same treatment as one that decided. Both happen to be refused here,
because `N_N` maps to no `JoinType`, which is why this is a decision about what the adapter may
*declare* rather than a live hazard. Declaring cardinality unsupported means a deployment reads that
fact in the declaration instead of discovering it when a dimension goes missing.

**Add an `N_N` variant to `JoinType`.** Not now, and not as part of a connector. A many-to-many
relationship cannot reach a dimension without changing a measure, so the variant's only behaviour
would be to be refused - and `AGENTS.md` records that a variant no test can provoke is one an enum
refuses to carry. If a source needs to *record* many-to-many for a human to read, that is a different
field from the one the join planner reads.

**Weaken `agrees_with_the_oracle` so a narrow source can pass it.** Rejected outright, and named so
that nobody proposes it as the cheap version of decision 6. That test is the reason two independently
written statements of one catalog can be compared at all, and a version of it that tolerated missing
measures would pass a golden adapter that had silently stopped reading them. The golden adapters keep
the strict test; a declaring adapter gets a different one.

## Amendment, 2026-09-02 (revised): a deployment-defined `sutura` structured property carries a certified metric, and it carries the WHOLE of one

**Status of the amendment: accepted, and this revision corrects its own first version.** The finding -
*DataHub's own measure surface is a raw expression string in a dialect that does not intersect ours* -
is unchanged and was re-verified. The first version of this amendment put the certified content in a
plurality of scalar structured properties under a `sutura.*` namespace carrying just the measure, and
argued from the shape that a measure is flat-expressible where a filter is not. **Both halves of that
were wrong, and a review measured them wrong:**

- **The transport is one string-valued property, not a namespace of granules.** `DataHub`'s
  `structuredProperty` has no nested or record value type, so a deployment cannot define a nested
  object under `sutura.*` at all. What it CAN define is **one structured property - under a name of
  its own - whose single scalar value is a JSON document**. `document::SuturaProperty` is that
  scalar, `sutura` is the field it arrives under on this adapter's own canonical shape rather than a
  urn this repository dictates (see the addendum's decision-7 bullet), and
  `SuturaProperty::assemble` is the scalar-to-nested decode - the issue #202 mechanism, implemented
  and exercised rather than described. This also collapses the old *"a structured property cannot
  carry the shape of a predicate"* argument: the scalar is a string, a JSON string carries anything
  closed, and the namespace is closed by `deny_unknown_fields` over the decoded document, not by what
  a scalar can hold.
- **The scope is the whole metric, not the measure.** The first version deferred `required_filters`,
  `dimensions`, `anchor` and a value allowlist to "the composition story in 0011". That route is
  CLOSED by 0011 itself: all four are fields on the one `Metric`, and 0011's `MetricCollision` rule
  means a second source can never attach them to a metric DataHub defines - "for metrics there is no
  precedence at all, declared or otherwise". So the `sutura` property is the ONLY channel, and issue
  #202's scope is closed past the measure: `required_filters`, `dimensions` (with `via` and
  `allowed_values`), `anchor` and `description` all ride it, over the same closed vocabularies a
  markdown metric uses.

### The transport, as defined

A deployment defines, on a metric entity, one string-valued structured property - under whatever
name it chooses - whose
value is the canonical `SuturaContent` document as JSON text: a `model`, a `measure` (the domain
`Measure` type, written exactly as a markdown metric writes its `measure:` key), a `time_column`,
non-empty `grains`, and optionally `description`, `required_filters`, `dimensions` and `anchor`.
The measure, filter operators, grains and allowed values are the domain's closed vocabularies
verbatim, and `deny_unknown_fields` - on the decoded document, on the measure and on the term inside
it, on a filter, on a dimension, on the anchor and on the range inside the anchor - refuses a
property this adapter does not define rather than guessing, naming the key.

**The range was the one level at which *at every depth* was one depth short, and it is closed rather
than recorded as a limit.** An anchor's `range` decodes through the domain's `TimeRangeInput`, which
carried no `deny_unknown_fields`, so a key written inside the range object was discarded in silence
instead of named - and because a question's `range` and a markdown metric's `anchor.range` decode
that same one type, it was the same hole on all three paths. The attribute is on that shape now, held
by `a_key_inside_an_anchor_range_is_refused_through_the_load_path` here and by
`a_key_inside_a_range_is_an_error_and_not_a_dropped_field` over the YAML question path, each red
without it.

**A metric that carries the property becomes a certified `Metric`; a metric that does not
stays the promotion candidate decision 4 describes, read and never converted.** The two halves are
the same `MetricAspect` and the distinction is an `Option` - the deployment's declaration, not an
adapter's inference.

### What this does to the decision

- **Decision 3's rows change from *does NOT provide* to a conditional provide.** The adapter's
  capability declaration provides `Structure`, `Descriptions` and `Relationships` unconditionally, and
  declares `Metrics`, `Grains`, `RequiredFilters`, `AllowedValues`, `Anchors` - and `Cardinality` -
  as **declared-and-empty may-provide kinds** (`DefinitionCapabilities::of_may_provide`, 0011's
  *declared-and-empty* state, built for this). `Cardinality` belongs among them because it is observed
  only as *a dimension reached through a relationship*, which happens exactly when a deployment
  declares a dimension with a `via`. Because undeclared-and-empty marks absence lawful, **a DataHub
  deployment that defined no metric content still loads** - models, prose and joins, no metrics -
  which is decision 3's original narrow deployment rather than the boot failure an unconditional
  declaration would have produced. That is what made the first version of this amendment a
  correctness defect: it widened the declaration unconditionally, so any DataHub deployment without
  the namespace (the ordinary one) failed its own fidelity check.
- **Decision 4's *never converted* is narrowed to *never converted where the property is absent*.**
  The raw `expression` string is still never executed, never translated and never certified against;
  the certified content comes from the structured property, not from the string. The two are both
  carried and are not reconciled - the string remains the promotion-candidate half, which is 0016's
  *"reconcile, never assume"* applied rather than abandoned.
- **The declaration moves a digest, and the *Consequences* bullet that said otherwise is corrected.**
  The first version of this amendment claimed *no committed digest moves*. Since 0011's manifest
  amendment the declaration travels in the `ContributionManifest` under the digest, so widening it
  moves EVERY digest this adapter produces - which is what happened and is expected; the definitions
  and digest pins for the `datahub` cell moved with it. Adapters whose declaration is unchanged keep
  their pins, which is why the test of the preservation is that `sutura-catalog-local`'s example
  digest did not move.

### What still has no source

**A real `AspectReader` and a served composition.** The adapter is a dev-dependency of `sutura-app`,
no composition root links it, and `sutura-serve` refuses `catalog.kind: datahub` by name. The only
reader is the recorded fixture source, so no library code shapes a request or maps a response, and
`.agents/skills/sutura/query-surface/SKILL.md`'s *Built and not wired* register records that nothing
serves it. The read path's COST is no longer the open measurement this record leaves - see
*Revision, 2026-09-04* below - but a reader is still owed.

### How it is proven

- The unit half is a fake reader over recorded documents carrying the property in its flat,
  scalar form - the simple aggregate, a ratio and a `count_if` all load as domain `Measure` shapes; a
  definitional filter, a dimension with its allowlist, an anchor and prose all load as the certified
  metric's fields; a metric without the property stays read-only; a metric that does not hold together
  is refused by the exact inner defect (`UnknownModel`, `UnknownMeasureColumn`, `NoGrains`); an
  unknown key at the content's top level is refused BY NAME; and a relationship this adapter cannot
  vouch for is refused. `crates/sutura-catalog-datahub`'s suite.
- The conformance matrix's `datahub` cell - already a `declaring` registration - expands the
  universal cells over the richer bundle: the pinned digest and definitions moved with it and the
  declaration-fidelity cell holds, over a declaration that mixes unconditional and may-provide kinds.
- The read path against a provisioned instance was the open measurement this record left, and
  *Revision, 2026-09-04* below closes the platform's half of it: the document is accepted under a
  property the deployment names, served back, and decoded into a certified `Metric`. What is still
  open is the READER - `sutura-catalog-datahub` has one `AspectReader` implementor outside a
  test, the recorded
  fixture source, so nothing in the library reaches a network.

### Addendum to the amendment: which half of decision 7 the deployment still owns

**Status: accepted.** The amendment above answered issue #202's feasibility question and left two of
its own arguments standing unreconciled. Both are answered here rather than in place, because an
accepted decision's body is not edited to agree with a later one - the amendment is the record.

**Decision 7 says the sentence *"configure DataHub like this"* is not ours to say, and the amendment
writes one property name and one document grammar.** Read together they contradict, so the boundary
is drawn rather than left to a reader:

- **The grammar of the decoded document is this repository's, and there was no version of this that
  left it open.** A decoder over a scalar has no way to negotiate the meaning of what it decodes:
  the closedness IS `deny_unknown_fields` at every depth over one known key set, and a key set the
  deployment chose would be no closedness at all - it would be the free-text escape hatch
  `docs/adr/0002` exists to refuse, reached by a longer route. So the document is dictated, and that
  is the price of the closed vocabulary rather than a preference.
- **Whether any metric carries it stays the deployment's, and that half is held by a mechanism.**
  The metric kinds are declared-and-empty may-provide (`DefinitionCapabilities::of_may_provide`) and
  `Definitions::assemble` has no minimum-metric refusal, so a deployment that defined nothing loads
  models, prose and joins. Decision 7's *"the absence of all of them is a supported configuration"*
  therefore survives intact for the property too, as a type rather than as a sentence.
- **What the property is called IN DataHub is still the deployment's, and nothing here names it.**
  The `sutura` key is a field on this adapter's OWN canonical shape (`document::MetricAspect`), not a
  DataHub structured-property urn - `document.rs`'s header is explicit that these shapes are the
  adapter's statement of aspect CONTENT and not DataHub's envelope, and no urn appears anywhere in
  the crate. Mapping a registered structured property - its namespace, its `SINGLE` cardinality, its
  string value type, the entity types it binds to - onto that field is the unbuilt HTTP
  `AspectReader`'s job. So *"under which name"* is genuinely unanswered rather than answered as
  `sutura`, and no urn is written here. What the revision below adds is that this is now a
  MEASUREMENT: a name that shares nothing with the field name carried the document through a real
  registry, so the independence is checked rather than asserted.

**And the *Required filters: absent, and absent in the way that matters* section makes TWO arguments,
where the amendment collapsed one.** The amendment answered the SHAPE argument - a scalar cannot
carry a predicate - and the second argument is untouched by it: *"a catalog that records a predicate
as a property records a fact, not a duty"*, so enforcing one would substitute our reading for the
author's, which `docs/adr/0001` refuses. That argument stands, and it is what still forbids reading
DataHub's own `filters` structured property or its `aggregationFunction`. What passes it is narrower
than a structured property in general: a deployment writing a predicate under the key
`required_filters`, inside a document written in this repository's grammar, is not recording a fact
for us to interpret - it is stating the duty in the one vocabulary where that key already MEANS
*applied to every question about this metric*. The substitution 0001 refuses is inferring a duty from
a field DataHub defines; reading one from a field the deployment wrote in our own grammar is the
author speaking.

### What this addendum does NOT establish

- **Nothing here has ever written or read the property through DataHub.** The venue exists and is
  reachable - `just datahub-acceptance` gets a `2xx` off the surface a reader would call - and the
  instance is empty, so DataHub's server-side structured-property validator has never seen one of
  these documents. `.agents/skills/sutura/query-surface/SKILL.md`'s *Built and not wired* register is
  where that limit is read from. **Superseded by the revision below.**
- **The document's size ceiling is unmeasured.** It grows with the metric - dimensions, allowed
  values, prose - and a deployment's DataHub validates the scalar for length. This crate adds no
  bound of its own, and no measurement here says what the ceiling is or what a metric that exceeds it
  does, because writing one through the platform is the same absent step as above. **Superseded by
  the revision below.**
- **Whether a per-deployment property is acceptable to MAINTAIN is still not decided by this record.**
  Issue #202 said that was not its job; it is not this addendum's either. What is decided is what the
  shape is, who owns which half of it, and which of the original arguments against it survive.

### Revision, 2026-09-04: the platform's half, measured

**Status: accepted.** The first two bullets above were true when written and are not now.
`a_document_served_by_a_real_datahub_decodes_into_a_certified_metric`, behind
`just datahub-acceptance`, asks the provisioned instance rather than its schema. What that run
records:

- **A deployment can define the property, under a name of its own, and the platform accepts this
  document as its value.** One string-valued, `SINGLE`, `metric`-bound property; the value written is
  the recorded corpus's own document, read through `fixture::FixtureReader` so it cannot drift from
  the one the unit half decodes. The cell registers it as `deployment_metric_document`, which shares
  nothing with the `sutura` field on `document::MetricAspect` - **that difference is the measurement**,
  because a cell registering `sutura` would pass equally whether the name were the deployment's
  choice or a constant this repository requires. Decision 7's *"not ours to say"* holds as a
  measurement rather than as a sentence.
- **What the instance serves DECODES into a certified metric.** The served aspect is mapped onto
  `document::MetricAspect` and is EQUAL to the one the recorded fixture carries, which is the
  strongest available statement that the fixture is faithful to the platform rather than to itself;
  `DataHubCatalog::load` then produces the closed-vocabulary `Measure`. That is issue #202's
  feasibility question answered against a running instance.
- **The read path's cost is a page per entity type, and it is eventually consistent.** ONE
  `GET /openapi/v3/entity/metric?aspects=structuredProperties&aspects=metricInfo` returns the metric
  with both aspects inline - the certified half and the promotion candidate's raw half in the same
  response - so a reader pages rather than fetching an entity per metric. But that surface is
  search-backed, and it lagged a synchronous write by ~2.2 s, where
  `GET /openapi/v3/entity/metric/{urn}` answered immediately. **A reader that pages does not get
  read-your-writes**, which is the limit this measurement adds to the cost answer rather than a
  detail of it: the version of the cell that paged once, immediately after writing, was red.
- **The ceiling is the deployment's Elasticsearch keyword length.** The platform's own refusal names
  it: *value is 131072 bytes which exceeds the maximum of 32766 UTF-8 bytes for structured property
  values indexed as Elasticsearch keywords (`structuredProperties.keywordMaxLength`)*. So what bounds
  a metric's document is an index setting rather than a constant in this repository. **What was
  measured is that the refusal names the setting** - nothing raised it and retried, so a
  deployment's ability to move it is DataHub's own documentation and not a finding here -
  which is why the cell asserts an order of magnitude of headroom against the number the refusal
  states rather than pinning the number.
- **`SINGLE` cardinality and the declared value type are enforced server-side**, each refused with
  its own reason (*has cardinality 1, but multiple values were assigned*; *should be a string*). "One
  string-valued property" is therefore the platform's rule and not this adapter's reading of it.

**What this revision does NOT reach, and the distinction is the whole of it.** There is still no
HTTP `AspectReader`: the requests and the mapping from the response shape
(`structuredProperties.properties[].values[].string`) onto `document::MetricAspect` are written in
that test file and nowhere in `src/`, so *A real `AspectReader` and a served composition* above
stands unaltered, and only the METRIC half of that snapshot came off the wire - the models and the
relationship are still the corpus's. Nothing is authenticated either: the tier runs with
metadata-service auth off. And the cell is `#[ignore]`d with no CI venue, because the nix sandbox has
no docker socket - so it is evidence of whatever the last `just datahub-acceptance` run reported.
