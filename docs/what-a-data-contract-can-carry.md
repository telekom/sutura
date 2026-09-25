---
title: What an Open Data Contract Standard catalog can carry, and the measure it cannot
description: A research spike in the method of ADR 0016, measured against the SemanticCatalog port - the Open Data Contract Standard (ODCS, the Bitol project, version 3) carries a contract's tables and columns with type, description, required, unique, primary-key and a free-text classification, and an SLA block, but no metric or semantic-role entity at all, so an adapter over it is a declaring source that provides the physical model and descriptions, reads-and-ignores the SLA by declaration, and declares the metric, the grain, the filter, the join and the measure out.
---

# What an Open Data Contract Standard catalog can carry

Status: **a finding, and the first deliverable** of issue #973's plan. The issue lists data-contract
documents - the *"interface catalogues are often exported as data-contract YAML"* shape - among the
metadata connectors, and it is explicit that this one is *"reviewable, file-based metadata richer than
a dictionary, and needs no network at boot"* - the cheapest kind, the on-disk vocabulary next to OKF
and local. This record is the ADR-0016-shaped answer to the question that decides the adapter's whole
shape: **can the Open Data Contract Standard express a complete semantic model as this repository
defines one, or only part of one?** The answer, measured against the published JSON Schema rather than
guessed, is *the physical model, the column prose and the SLA - and nothing of a metric layer*. It is
richer than Frictionless Table Schema on the schema-object side (it names primary keys, `required`
and `unique` per column, and carries a free-text classification) and just as silent on the measure as
the OKF vocabulary - which makes the consequence the same as ADR 0016's and the OKF finding's: an
adapter that **declares** which kinds it provides and which it does not, and is tested against that
declaration, rather than an adapter held to the golden contract.

## What was measured against

Read out of the port, exactly as ADR 0016 and the OKF/OpenMetadata findings do. `SemanticCatalog::load`
returns a `PinnedDefinitions`; its halves are a `Definitions` and a `Knowledge`, and
`Definitions::assemble` is the only constructor of the first. So *can an ODCS source express our model*
is really *can it fill these fields and survive these checks*:

| What a complete semantic model supplies           | The type in `sutura-domain`                                                                                                                                                                                                                           |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| a physical table and the column set it exposes    | `Model` - `ModelName`, `SourceName`, `TableName`, `BTreeSet<ColumnName>`, `Description`                                                                                                                                                               |
| a join                                            | `Relationship` - a name, two `(ModelName, ColumnName)` endpoints, and a `JoinType` of `OneToOne`, `ManyToOne` or `OneToMany`                                                                                                                          |
| what a metric measures                            | `Measure` - `Simple(Term)` or `Ratio { numerator, denominator, zero_denominator }`, over a `Term` of `Aggregate(AggregatedColumn)` or `CountIf { column }`, where `Aggregate` is the closed set `sum`, `count`, `count_distinct`, `avg`, `min`, `max` |
| a predicate that is part of what the metric MEANS | `Vec<RequiredFilter>` - `Equals`, `NotEquals`, `IsTrue`, `IsNotNull`, values typed as `DimensionValue`                                                                                                                                                |
| when, and at what resolution                      | `ColumnName` plus a `BTreeSet<Grain>`                                                                                                                                                                                                                 |
| what it may be broken down by                     | `Dimension` - a column, optionally `via` one relationship, optionally an allowlist of at most `MAX_VALUES_PER_DIMENSION` values                                                                                                                       |
| the number it produced when it was certified      | `Option<Anchor>` - a `TimeRange` and a value as text                                                                                                                                                                                                  |
| who may see a metric                              | `Audience` - `Open`, or `Restricted(AudienceGrant)` of mapped identifiers, carried on `Metric` under `docs/adr/0028`                                                                                                                                  |
| what a reader has to know                         | `Knowledge` - phrases, caveats, reviewed absences and worked examples, each carrying a `Referent`                                                                                                                                                     |

Two of those are the checks rather than the fields. `Definitions::assemble` **refuses** a dimension
reached through a relationship whose declared cardinality may duplicate rows, as
`JoinWouldDuplicateRows`. And every field above is a type with no free-text SQL in it: `Measure` has
no `expression:` key and no `Option<String>` at any depth, which is the load-bearing half of
`docs/adr/0001-first-party-semantic-models.md`. The `Audience` row is new to
this finding versus the OKF record - ODCS is the first of the two on-disk vocabularies (OKF and this
one) to carry a free-text per-column classification, so it is the first with anything to say about
who may see a metric.

## The specification this names, and its version

The issue's *data-contract documents* are the **Open Data Contract Standard (ODCS)**, the Bitol
project's open specification for data contracts. Every field below was read out of the published JSON
Schema, `schema/odcs-json-schema-latest.json` in the `bitol-io/open-data-contract-standard`
repository, at the `v3.0.0` tag **and** at the `v3.0.2` (the last 3.0 patch), `v3.1.0` and `v3.2.0`
tags, so the lineage of each feature is attributed to the version that introduced it rather than to
the schema as a whole. `apiVersion` already carries the answer in the contract itself: the enum is
`v3.0.0` … `v3.2.0`, and a contract stamps the version it was written against. This record's ground
truth is version **3** as a line - `v3.0.0` for the shape that decides the adapter, with the later
patches and minors called out where they change a verdict. The record is deliberately honest about
what changes between them: three properties that matter to this repository (`enum`, the semantic role,
and relationships) do **not** exist in the 3.0 line and arrive in later minors, and the differences
are part of the decision rather than noise to smooth away.

## What the model actually carries, field by field

**The contract's top level** (`optional` except where noted) is `version`, `kind` (a required
`DataContract`), `apiVersion` (required), `id` (required), `name`, `tenant`, `tags`, `status`, the
`servers` array (where the data physically lives - the model's `SourceName`), `dataProduct`, `domain`,
`description` (`usage` / `purpose` / `limitations`), `schema` (an array of `SchemaObject`s), the
`team`, `roles` and `support` blocks, the SLA surface (`slaDefaultElement` + `slaProperties`), and
`customProperties`. `required` at the contract level is `version`, `apiVersion`, `kind`, `id`
(the 3.0 and 3.1 lines also require `status`; `v3.2.0` drops that requirement). `additionalProperties:
false` at the top - an unknown property at the contract level is **invalid**, which is exactly the
fidelity a `deny_unknown_fields` adapter enforces and is worth stating because it is the basis of the
reading rule this record recommends.

**`schema` is an array of `SchemaObject`s** - in practice the contract's physical tables. A
`SchemaObject` carries `name` (required), `logicalType` (a constant `object`), `physicalName` (the
physical table name, e.g. `table_1_2_0`), `dataGranularityDescription` (free text), `properties`
(an array of `SchemaProperty`, one per column), `quality` (an array of data-quality rules), and - from
v3.1.0 - a `relationships` array. Its `SchemaElement` base adds `description`, `businessName`,
`authoritativeDefinitions`, `tags`, and `customProperties`. So **the table/column split is present and
explicit**: the `SchemaObject` is the model, its `properties[].name` is the column set.

**Each `SchemaProperty`** (through the `SchemaBaseProperty` base, these fields present across all of v3)
carries:

- `name` (required),
- `logicalType` - an enum, version-dependent: `string`, `date`, `number`, `integer`, `object`,
  `array`, `boolean` in v3.0.0; `timestamp` and `time` join in v3.1.0; `map` and `vector` join in
  v3.2.0. This is a **data type, not a measure** - it describes the shape of a cell value and has no
  aggregation in it.
- `physicalType` (the source dialect's type, e.g. `VARCHAR(2)`),
- `primaryKey` + `primaryKeyPosition` (a boolean and an ordinal - the declared primary key),
- `required` (a boolean, meaning not-null),
- `unique` (a boolean),
- `partitioned` + `partitionKeyPosition`,
- `classification` - **a free-text string**, *"can be anything, like confidential, restricted, and public"*,
- `encryptedName`, `transformSourceObjects`, `transformLogic`, `transformDescription`,
- `examples` (sample values), `criticalDataElement`,
- `quality` (a per-column `DataQualityChecks` array).

**In v3.2.0** (the current v3 line, and the tagged release this record read as `main`) the base also
gains `enum` and `semanticType`. `enum` is an array of `EnumValue`s (each a `value` plus an optional
`label`, `description`, `tags`, `customProperties`, `authoritativeDefinitions`) - the **allowed
values** for the column. `semanticType` is an enum of `column` / `measure` / `dimension`, defaulting to
`column`, describing *the semantic role the property plays in the data model* - where `measure` is
documented as *an aggregated value (e.g. `SUM(revenue)`) whose aggregation expression is held in
`transformLogic`*. Neither exists in v3.0.0/v3.0.1/v3.0.2.

**Relationships.** There is no relationship construct at all in the 3.0 line. v3.1.0 adds a
`relationships` array at both levels together - schema-level on `SchemaObject` and property-level on
`SchemaBaseProperty`, the latter with `from` implicit; v3.2.0 keeps both, unchanged. A relationship
carries `type` (a constant `foreignKey`), `from` and `to` (references, single or arrayed for composite
keys), and `customProperties`. **It declares no cardinality** - there is no OneToOne / ManyToOne /
OneToMany marker anywhere in the shape, so no `JoinType` can be chosen, exactly the Table Schema
situation the OKF finding already described.

**SLA.** The SLA is `slaDefaultElement` (which element to check on) plus `slaProperties`, an array of
`ServiceLevelAgreementProperty` key/value pairs: `property` and `value` (required), plus `unit`,
`element`, `driver` (`regulatory` / `analytics` / `operational`), `schedule`, `scheduler`, and a
human `description`. It is the contract's service-level *promise* (freshness, availability - the
documented example is "99.9% of the time, data is available by 6 AM UTC"). There is no metric to attach
it to.

**Data quality** is a first-class, per-schema-object and per-column surface: `DataQualityChecks` is an
array of `DataQuality` rules, each with a `dimension` (a KPI enum of `accuracy`, `completeness`,
`conformity`, `consistency`, `coverage`, `timeliness`, `uniqueness`), a `type` (`text`, `library`,
`sql` or `custom`), a threshold family (`mustBe`, `mustNotBe`, `mustBeGreaterThan`,
`mustBeGreaterOrEqualTo`, `mustBeLessThan`, `mustBeLessOrEqualTo`, `mustBeBetween`,
`mustNotBeBetween`), a `library` metric enum (`nullValues`, `missingValues`, `invalidValues`,
`duplicateValues`, `rowCount`), and for `sql` a raw `query` string, plus `businessImpact`, `severity`,
`unit`, `schedule`, `name` and `description`. **There is no metric entity anywhere.**

## The three absent things that decide

**Metric is absent entirely - there is nothing to refuse, and that is the stricter statement.** The
word `metric` appears in the schema only inside the data-quality library enum (`nullValues`,
`missingValues`, `invalidValues`, `duplicateValues`, `rowCount`) and inside `slaProperties`'s
free-text property names - no `Measure`, no `Metric`, no `semanticModel` entity, and no metric-level
computation at any depth or any version. The schema's one gesture toward a semantic role is v3.2.0's
`semanticType` (`column` / `measure` / `dimension`), and it fails twice over: it does not exist in the
3.0 line at all, and where it does exist the `measure` role is documented as *an aggregated value whose
aggregation expression is held in `transformLogic`* - a free string in a dialect nothing parses, the
exact half-a-definition ADR 0016's refusal names. `Definitions::assemble`'s *a measure carried as a raw
expression string is half a definition* cannot even fire the way it does for DataHub, because there is
no metric slot to carry the string in. The honest `capabilities()` statement is a **declared absence**
of the metric layer - the same verdict, and the same "nothing to refuse because there is no slot"
qualifier, as the OKF finding.

**Quality rules and SLA are reported, not defined.** The ODCS quality surface is a promise about the
data, not a predicate that is part of what a metric means: even where a rule crosses onto
`RequiredFilter`'s operator family (`mustBe`, `mustNotBe`), it is applied to the data's *quality* at
*check time*, with a `severity` and a `businessImpact`, and it is attached to a column or table, never
to a metric - there is no metric to condition. And the SLA is explicitly a temporal/availability
promise (`freshness`, `availability`) with no semantic content: issue #973's own constraint says
*service levels read and ignored, by declaration*. Both are **reported-not-defined**: a `*mustBe*`
value is a scalar comparison over the column's own values, not a definitional filter, so it is read
and ignored by declaration rather than minted into a `RequiredFilter`.

**Cardinality is absent - a `foreignKey` licenses nothing.** The relationship shape names the two
endpoints and stops. It cannot distinguish a one-to-one from a one-to-many, so a relationship loaded
through it offers no answer to the `JoinWouldDuplicateRows` check - a relationship whose cardinality
may duplicate rows is exactly the one the assembler refuses to reach a dimension through. This is the
Table Schema situation verbatim, not DataHub's (`N_N` default) nor OpenMetadata's (declared
`relationshipType`): ODCS omits the property entirely, which is a default of "not stated" and licenses
nothing.

## The mapping

| The semantic-model field              | What ODCS offers                                                                                  | Verdict                                                                                                                      |
| ------------------------------------- | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `Model` (table + columns)             | `schema[]` `SchemaObject` + `properties[].name/physicalType/logicalType`                          | **provides**                                                                                                                 |
| `Description`                         | `SchemaElement.description` / `businessName`, contract `description.usage/purpose/limitations`    | **provides**                                                                                                                 |
| `ColumnTypes`                         | `properties[].logicalType` (enum) + `physicalType` (free text)                                    | **provides** - a source's own dictionary spelling, `ColumnType`-parsed                                                       |
| `ColumnDescriptions`                  | `properties[].description` / `businessName` (free text)                                           | **provides**                                                                                                                 |
| primary key                           | `SchemaBaseProperty.primaryKey` (+ `primaryKeyPosition` order)                                    | **provides** - evidence on `Model::with_primary_key`, licensing no join                                                      |
| `unique` / `required`                 | `SchemaBaseProperty.unique` / `required` (booleans)                                               | **reported, not defined** - value/constraint facts with no `Column`-level carrier, read-and-ignored like OKF's `constraints` |
| `Relationship` (join)                 | `schema[].relationships[]` (v3.1+): foreignKey, from/to, **no cardinality**                       | **declares, and does not license** (absent in the 3.0 line)                                                                  |
| `Measure`                             | none (v3.2's `semanticType: measure` is a transform string, absent in 3.0)                        | **absent, declared**                                                                                                         |
| `RequiredFilter`                      | `quality` rules (`mustBe` family) - data-quality thresholds, not definitional                     | **reported, not defined**                                                                                                    |
| `Grain` + time                        | `dataGranularityDescription` free text only                                                       | **absent, declared**                                                                                                         |
| `Dimension` with `via` / allowlist    | `semanticType: dimension` (v3.2+), `enum` allowed values (v3.2+); no `via`                        | **partial** - a bare column; something to say only on the current v3 line                                                    |
| `AllowedValues` (dimension allowlist) | `properties[].enum` `EnumValue[]` (v3.2+)                                                         | **provides** on v3.2+, **absent** on the 3.0 line                                                                            |
| `Anchor`                              | none                                                                                              | **absent, declared**                                                                                                         |
| `Audience` (who may see a metric)     | `properties[].classification` - a free-text string, e.g. `confidential` / `restricted` / `public` | **reported, not defined** - see the classification paragraph                                                                 |
| SLA                                   | `slaProperties[]` + `slaDefaultElement`                                                           | **reported, ignored by declaration** (issue #973's own constraint)                                                           |
| `Knowledge` with `Referent`           | `description` / `businessName` free text, `customProperties`                                      | **partial** - prose travels; referent-bearing phrases/caveats/worked examples do not                                         |

## The classification question, stated honestly

The issue's Where names *"classification → `Audience` where a metric exists"* - and the qualification
is the whole finding. `Audience` is carried **on `Metric`** in this repository
(`catalog::Metric.audience`, under `docs/adr/0028`); ODCS's `classification` is a free-text per-column
string with published examples `confidential`, `restricted`, `public` - **not** a closed scheme and
**not** attached to any metric. Classification only means something *for what it restricts*, and there
is no metric here to restrict, so it stays **reported, not defined**: read and never minted into an
`Audience`. This record deliberately does not import this repository's own `internal` /
`confidential` / `restricted` naming onto ODCS's open, unbounded string - the two are not the same
vocabulary, and no fixed mapping can be offered between them.

## Conclusion, and what it costs

**A strictly narrower source than Wren, and the cheapest connector - by declaration.** ODCS v3 can
carry the physical model, the column prose, column types, the primary key and the SLA; it cannot carry
a metric, a measure, a required filter, a grain, an anchor, or a reachable-through-relationship
dimension. An adapter over it is a **declaring** `SemanticCatalog` whose `capabilities()` provides
`Structure`, `Descriptions`, `ColumnTypes` and `ColumnDescriptions`; declares `Relationships` out
(deprecated, because unless the deployment pins v3.1+ there is no relationship slot, and even there no
cardinality licenses one); reads-and-ignores the SLA by declaration; reports-not-defines the quality
rules and the classification; and declares the metric layer, the `Audience`, the grain and the anchor
as deliberate absences. This is precisely the shape `sutura-catalog-datahub`, `sutura-catalog-okf` and
`sutura-catalog-openmetadata` already establish, and it is tested against that declaration in both
directions (it provides what it declares, and the bundle agrees that what it does not declare does not
arrive).

**Whether it is worth shipping**. Yes, for the reason any `declaring` catalog's describing half exists:
a source that provides tables and descriptions but no metric is a legitimate narrow source, and
`Definitions::assemble` refuses nothing it does not provide. Its concrete value over a bare directory
of descriptors is that the contract already carries structure - the primary key, `required`/`unique`
facts and the SLA - that a plain dictionary does not, and it is reviewable, file-based and needs no
network at boot, exactly issue #973's Why. The issue's two acceptance cells follow: a **catalog-golden
cell** (the fixture bundle measured against the adapter's declaration, the universal shape every
`declaring` adapter gets) and a **declaration-fidelity cell** (`MetadataCapabilities::checked_against`
over a real fixture), each narrower than an adapter that could offer the metric layer.

## Alternatives considered

**Which version to pin.** The 3.0 line is where the shape that decides the adapter settles: the
physical model, the prose, `primaryKey`/`required`/`unique` and the SLA all exist there, and none of
the three things that would change a verdict do. The `enum` allowed-values and `semanticType` role that
appear only in v3.2.0 would, if taken, map onto a dimension allowlist - a **provides** turn on
`AllowedValues` - and a reported-not-defined measure; but pinning the adapter to v3.2.0 would reject a
perfectly valid v3.0.0 or v3.1.0 contract that carries none of them, which is a narrower acceptance than
the natural reading, and a deployment's contracts are what they are. `Relationships` is not part of
this choice - it licenses nothing at any version, so it is read-and-ignored, never declared. The honest
reading follows the contract's own `apiVersion` stamp: an adapter reads whatever `apiVersion` the
document declares, takes the fields that version carries, and declares the version-dependent kinds
(`AllowedValues`) as conditional rather than unconditional. That is what `of_may_provide` exists for.

**Was `classification` really not `internal`/`confidential`/`restricted`?** No - ODCS's own published
vocabulary is open (`confidential`, `restricted`, `public`, or any string), so it cannot be read as a
closed classification scheme, and it sits on a column, not on a metric. Both of those are what make it
reported-not-defined rather than a supplied `Audience`.

## Limits, stated

Written read-only, against the published ODCS JSON Schema at the `v3.0.0`, `v3.0.2`, `v3.1.0` and
`v3.2.0` tags and the `SemanticCatalog`/`Definitions` port in this repository, **not** against any
provisioned contract corpus (the nix sandbox has no network beyond these schema reads, and no catalog
of real contracts was opened for this record). Three things are open to a live check and should be
before an adapter ships, and none of them changes the shape decision: (a) whether real contracts in the
wild attach the data-quality `mustBe`/`mustNotBe` rules densely enough to make "reported-not-defined"
more than a formality, (b) whether a real deployment's contracts pin v3.2.0 often enough that the
`enum`/`AllowedValues` and `semanticType` provides-turns are worth the conditional declaration, and
(c) as with the OKF finding, whether the free-text `description` is dense enough to load as `Knowledge`
or better left to the consuming prompt. The classification "reported, not defined" is the strongest
claim here and the one a reviewer should test first, because it is the only row that depends on an
*absence of a metric entity* rather than on a field's own shape.
