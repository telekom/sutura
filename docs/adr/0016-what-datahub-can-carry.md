---
title: What DataHub can carry, and the measure it cannot
description: A research spike, measured against the SemanticCatalog port rather than reasoned about - DataHub 1.7.0 does now have first-class metric and semanticModel entities, and what it holds a measure as is a raw expression string tagged with a dialect, so it carries the physical model, the descriptions, the joins with a declared cardinality and a rich glossary, and carries no measure this repository will execute, no definitional filter, no grain, no value allowlist and no anchor; therefore a DataHub adapter is a COMPOSITION step and not a registration, the catalog conformance matrix cannot take it as an entry, and the contribution manifest 0011 decided is on its critical path.
---

# What DataHub can carry, and the measure it cannot

Status: **accepted as a finding. No adapter is written, no crate exists, and no dependency was added.**
The finding is the deliverable; the decision it forces is about sequencing.

`AGENTS.md`'s crate table lists a `-datahub` crate as planned. Four accepted records name DataHub as a
metadata connector -
[pluggable by declaration](0011-pluggable-by-declaration.md),
[conformance packs](0012-conformance-packs-for-inputs-and-adapters.md),
[federating across different data systems](0007-federating-across-different-data-systems.md) and
[the plan](0009-the-plan-from-one-source-to-many.md) - and until this record
[the implementation plan](../implementation-plan.md)'s stack table had **no row for it at all.** This
record closes that gap, and it puts the question before the row because the answer decides the row's
dependencies.

## The question

**Can DataHub express a complete semantic model as this repository defines one, or only part of one?**

Three answers were possible and each implies different work:

| If DataHub supplies | It is | What it costs |
| --- | --- | --- |
| complete `Definitions` | a second `SemanticCatalog` implementor, under the definition digest | low - a registration, as the invariant promises |
| descriptions and a glossary only | a `Knowledge` source, which this repository confines to descriptive content with the prompt as its only consumer | a different feature, arguably a different port |
| **part** - the physical model and the prose, the measure still authored | a **merge** story that does not exist | expensive: the digest and *a catalog edit cannot change what executes* both bite |

**It is the third.** The reason is not the one that was expected, and the difference matters enough to
be the substance of this record: the expectation was that DataHub has no metric at all. As of
**1.7.0** it has one, it is `category: core`, and what it holds a measure as is **a raw expression
string tagged with a dialect.**

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
| `Relationship.join_type` | `SemanticModelRelationship.cardinality` | **partly** - see below |
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
and a rich glossary.** It does not supply **a measure this repository will execute, a definitional
filter in any form, a grain, a value allowlist, or an anchor.**

And the sharpest consequence is not in the mapping table. It is in the test suite:

**The catalog conformance matrix cannot take DataHub as an entry.**
`crates/sutura-app/tests/golden/catalogs.rs` expands seven behaviours over every registered catalog,
and its central one is `agrees_with_the_oracle`: every registered `SemanticCatalog` must produce
**the same `Definitions`** as the hand-written oracle over `examples/single-player`. That corpus
contains `measure: { simple: { aggregate: sum, column: mrr_cents } }` and
`required_filters: [{ equals: { column: status, value: active } }]` and `grains: [month]` and value
allowlists and an anchor. A source that cannot express five of those cannot pass that test, and no
amount of adapter code changes that - the missing thing is in the source.

So the invariant *"Adding a metadata provider or a data system is a registration, not a test edit"* is
**true as written and was never measured against a narrow source.** It holds for a provider that can
carry the whole model. DataHub is the first candidate that cannot, and the honest statement is the
scoped one: a registration is what it costs to add a provider that carries a bundle; a provider that
carries part of one costs the composition work first. That is not a defect in the invariant and it is
not a reason to weaken the row - it is the row's scope, found by trying to use it.

## The decision

**1. DataHub is adopted as a NARROW metadata source, and never as a source of a measure.**

It declares structure, descriptions, relationships-with-cardinality and glossary content. It declares
no metrics, in the sense
[pluggable by declaration](0011-pluggable-by-declaration.md) means declaring: the capability
vocabulary is what a provider says it provides, and a provider that does not declare metrics cannot
contribute one. This is 0011's own split - *"structure and meaning can be harvested; the measure is
declared"* - and this record supplies the mechanism-shaped reason for it that 0011 could only assert.

**2. `metricInfo.expression` is READ and REPORTED, never executed and never converted.**

A DataHub metric that exists is a **promotion candidate**: a name, a description, an owner, a semantic
model, a set of dimensional fields, and a string somebody wrote in a dialect we do not render. What a
DataHub adapter may do with the string is show it to a person deciding whether to author a `Measure`.
What it may not do is compile it, translate it, or infer a `Measure` from the `aggregationFunction`
beside it. **Unbuilt**, and it is a decision about what the adapter will be permitted to do rather
than a description of anything that exists.

**3. The DataHub adapter is BLOCKED on the composition work, and that is its dependency.**

A narrow source is only useful composed with something that carries the measures. 0011 decided the
whole of what that needs and none of it is built: an assembler in `sutura-app` over N
`SemanticCatalog` ports, one-source-per-kind-per-entity with a refusal naming both on a conflict, no
precedence for metrics ever, per-source required/optional availability, and **the contribution
manifest hashed as a third element beside the definitions and the knowledge** - without which
*"the bundle records that an optional source was unreachable"* is a claim the digest cannot make.
So the plan gains two rows rather than one: the composition step, and the connector on top of it.

**4. A relationship whose cardinality is absent or `N_N` is refused at load, by the adapter, naming
the relationship.** Not defaulted, in either direction. `ManyToOne` as a default assumes the fan-out
away and `OneToMany` refuses every dimension, so neither is a default - one is unsafe and the other is
a silent feature removal. The refusal is the adapter's own typed error, because mapping a source's
vocabulary is what an adapter is for.

**5. Lineage is out of scope, and `docs/architecture.md` overstated it.** That page says metrics,
dimensions, the glossary *and lineage* arrive through `SemanticCatalog`. There is no lineage type in
the workspace and none is planned, so the sentence claims an input that does not exist. Corrected on
this branch, in the same diff as this record, because an overstated claim is itself the defect.

## Consequences

- **The stack table gains a composition row and a connector row**, with the connector depending on the
  composition and on the conformance packs. Neither can start now, and the reason is written in the
  table rather than remembered.
- **`docs/implementation-plan-identity-and-services.md` said the remaining metadata connectors are
  all "`feat/source-registry`-shaped once the packs exist: a registration, a declaration, and
  fixtures." That is now known to be false for DataHub**, and it is corrected on this branch. The
  claim was reasonable when written - nothing had been measured against a real narrow source - and it
  is exactly the kind of sentence this record exists to replace with a checked one.
- **0011's dormant half stops being dormant.** That record says the metadata half of it - a second
  connector, the assembler, the contribution manifest - is *"in no branch in that stack"*, and says
  correctly that *"a decision whose record is accepted and whose branch does not exist is a decision,
  not progress."* This record schedules it, which is what makes the availability rules and the manifest
  worth having: the first narrow source is the first thing that needs them.
- **Every committed digest moves once, when the manifest lands.** 0011 already priced that and it is
  unchanged by this record; it is repeated here because the DataHub row is the reason the bill arrives.
- **The example corpus is not enough to test a narrow source, and that is a cost this record does not
  pay.** `agrees_with_the_oracle` compares against one hand-written oracle for one whole bundle. A
  narrow source's conformance question is a different one - *did it contribute exactly what it declared
  and nothing else* - and what that test looks like belongs with the assembler, not here.
- **The Beta flag is a real risk to price.** `MetricInfo` is at schema version 4 in the release and 5
  on the development branch; `SemanticModelInfo.datasets` is deprecated on the branch and not in the
  release; membership moved from the model side to the member side within one cycle. An adapter built
  against this model this quarter is an adapter that will be revised. That is an argument for the
  narrow declaration rather than against the connector: `schemaMetadata`, the dataset property aspects
  and the glossary are the long-established part of the model - `schemaMetadata` even carries a
  deprecated foreign-key field superseded by a newer one, which is what an aspect that has been through
  a migration looks like - and the metric entity is the part that is one release old and moving.

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

**Treat DataHub as a `Knowledge` source only.** The second of the three cases. Rejected because it
throws away the half of DataHub that is genuinely load-bearing: `schemaMetadata` is the authoritative
column list for a great many warehouses, and a relationship with a declared cardinality is the field
`Definitions::assemble` refuses without. Declaring descriptions only would be flattening a source to
its least interesting capability, which
[pluggable by declaration](0011-pluggable-by-declaration.md) argues against by name.

**Read `metricInfo.expression` through the authored-SQL hatch.** Rejected on two independent grounds,
either of which is sufficient: the dialect sets do not intersect and closing the gap is translation;
and no shipped binary can execute an authored expression, so the load would succeed and the question
would still be refused, one stage later and less clearly.

**Derive a `Measure` from `aggregationFunction` plus the annotated field.** Rejected because the
expression beside it is authored independently and nothing reconciles the two, so the derivation
certifies half a definition. This is the alternative that looks cheapest and is the one whose failure
mode is a wrong number under a certified name.

**Add an `N_N` variant to `JoinType`.** Not now, and not as part of a connector. A many-to-many
relationship cannot reach a dimension without changing a measure, so the variant's only behaviour
would be to be refused - and `AGENTS.md` records that a variant no test can provoke is one an enum
refuses to carry. If a source needs to *record* many-to-many for a human to read, that is a different
field from the one the join planner reads.

**Write the adapter now against a single-source deployment.** Tempting, because a DataHub instance
with hand-authored annotations could in principle carry enough to serve. Rejected: it would put the
narrow source on the path that assumes a whole bundle, and the first metric anybody promoted would
have to be written into DataHub's own metadata as a string this repository refuses. The composition
step is not a nicety in front of this connector; it is the thing that makes the connector coherent.
