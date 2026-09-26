---
title: What an Open Data Contract Standard catalog can carry, and the measure it cannot
description: A research spike in the method of ADR 0016, measured against the SemanticCatalog port - the Open Data Contract Standard (ODCS, the Bitol project, version 3) carries a contract's tables and columns with type, description, nullability, primary-key and a free-text classification, and an SLA block, but no metric or semantic-role entity at all, so an adapter over it is a declaring source that provides the physical model and descriptions, may-provide a foreign key with single-column target-uniqueness evidence as a non-duplicating join, reports-not-defines the quality rules, the SLA and the classification, and declares the metric, the grain and the anchor absent.
---

# What an Open Data Contract Standard catalog can carry

Status: **a finding, and the first deliverable** of issue #973's plan. The issue lists data-contract
documents - the *"interface catalogues are often exported as data-contract YAML"* shape - among the
metadata connectors, and it is explicit that this one is *"reviewable, file-based metadata richer than
a dictionary, and needs no network at boot"* - the cheapest kind, the on-disk vocabulary next to OKF
and local. This record is the ADR-0016-shaped answer to the question that decides the adapter's whole
shape: **can the Open Data Contract Standard express a complete semantic model as this repository
defines one, or only part of one?** The answer, measured against the published JSON Schema rather than
guessed, is *the physical model, the column prose and a target-vouched-for join - and nothing of a
metric layer*. It is richer than Frictionless Table Schema on the schema-object side (it names
primary keys, `required` and `unique` per column, and carries a free-text classification, and from
v3.1.0 a foreign key whose target evidence can license a join) and just as silent on the measure as the
OKF vocabulary - which makes the consequence the same as ADR 0016's and the OKF finding's: an adapter
that **declares** which kinds it provides and which it does not, and is tested against that
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
| what a reader has to know                         | `Knowledge` - phrases, caveats, reviewed absences and worked examples, the phrases and caveats carrying a `Referent`                                                                                                                                  |

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
Schema, `schema/odcs-json-schema-latest.json` in `bitol-io/open-data-contract-standard`, at the
`v3.0.0`, `v3.0.2` (the last 3.0 patch), `v3.1.0` and `v3.2.0` tags, attributing each feature to the
version that introduced it rather than to the schema as a whole. `apiVersion` carries the answer in the
contract itself: at the `v3.2.0` tag its enum is `v3.2.0`/`v3.1.0`/`v3.0.2`/`v3.0.1`/`v3.0.0` plus three
`v2.2.x` backward-compat values, and a contract stamps the version it was written against. This
record's ground truth is version **3** as a line, with the patches and minors called out where they
change a verdict: relationships arrive at v3.1.0, and `enum`, the semantic role, `context` and
`synonyms` do not exist before v3.2.0 - the differences are part of the decision, not noise to smooth
over.

## What the model actually carries, field by field

**The contract's top level** (optional except where noted) is `version`, `kind` (required
`DataContract`), `apiVersion` (required), `id` (required), `name`, `tenant`, `tags`, `status`, `servers`
(where the data physically lives - the model's `SourceName`), `dataProduct` (`deprecated` since v3.1.0),
`domain`, `description` (`usage`/`purpose`/`limitations`), `price`, `contractCreatedTs`,
`authoritativeDefinitions` (v3.0.2+), `context` (v3.2.0), `schema` (an array of `SchemaObject`s),
`team`, `roles`, `support`, the SLA surface (`slaDefaultElement` + `slaProperties`), and
`customProperties`. `required` is `version`, `apiVersion`, `kind`, `id` (3.0/3.1 also require `status`;
v3.2.0 drops it). `additionalProperties: false` at the top - an unknown property at the contract level
is **invalid** at every version, the fidelity a `deny_unknown_fields` adapter enforces.

**`schema` is an array of `SchemaObject`s** - in practice the contract's physical tables. A
`SchemaObject` carries `name` (required), `logicalType` (a constant `object`), `physicalName` (the
physical table name, e.g. `table_1_2_0`), `dataGranularityDescription` (free text), `properties`
(an array of `SchemaProperty`, one per column), `quality` (an array of data-quality rules), and - from
v3.1.0 - a `relationships` array. Its `SchemaElement` base adds `description`, `businessName`,
`authoritativeDefinitions`, `tags`, and `customProperties`. So **the table/column split is present and
explicit**: the `SchemaObject` is the model, its `properties[].name` is the column set.

**Each `SchemaProperty`** (through the `SchemaBaseProperty` base, present across all of v3) carries
`name` (required); `logicalType` - a **data type, not a measure**, version-dependent: `string`, `date`,
`number`, `integer`, `object`, `array`, `boolean` in v3.0.0, `timestamp`/`time` joining at v3.1.0,
`map`/`vector` at v3.2.0; `physicalType` (the source dialect's type); `primaryKey` +
`primaryKeyPosition`; `required` (not-null) and `unique` (booleans); `partitioned` +
`partitionKeyPosition`; `classification` - a free-text string, *"can be anything, like confidential,
restricted, and public"*; transform metadata (`encryptedName`, `transformSourceObjects`,
`transformLogic`, `transformDescription`); `examples`, `criticalDataElement`; and `quality` (a
per-column `DataQualityChecks` array).

**v3.2.0** (the latest tag) adds four things, none present before it. `enum` is an array of
`EnumValue`s (a `value` plus optional `label`/`description`/`tags`) - **allowed values** for the
column. `semanticType` is `column`/`measure`/`dimension` (default `column`), where `measure` is
documented as *an aggregated value whose aggregation expression is held in `transformLogic`*. `synonyms`
(RFC-0041) helps tools resolve business vocabulary, and `context` (RFC-0038, contract and
`SchemaObject` level) carries `instructions`, `verifiedStatements` Q&A and `constraints` for AI agents.
`context` and `synonyms` are the closest thing ODCS has to this repository's `Knowledge`, but every
`Referent` here is metric-anchored and ODCS has none - so they stay prose that travels rather than
referent-bearing knowledge, the same limit `enum` and `semanticType: dimension` hit.

**Relationships.** No relationship construct exists in the 3.0 line. v3.1.0 adds a `relationships`
array at both levels together - schema-level on `SchemaObject`, property-level on `SchemaBaseProperty`
(`from` implicit) - v3.2.0 unchanged bar an optional `id`. A relationship carries `type` (constant
`foreignKey`), `from`/`to` (single or arrayed for composite keys) and `customProperties`. **It carries
no cardinality marker of its own**, but a single-column `from`/`to` pair resolves to two
`SchemaProperty` records, and `sutura-catalog-rdbms` already has the rule for exactly this shape: a
foreign key whose single-column target carries `primaryKey` or `unique` evidence maps to
`JoinType::ManyToOne`, refusing (`TargetUniquenessUnknown`) otherwise. ODCS v3.1+ carries that same
target evidence, so the honest verdict follows the rdbms precedent, not Table Schema's.

**SLA** is `slaDefaultElement` plus `slaProperties`: `property`/`value` (required), `unit`, `element`
and `driver` (examples `regulatory`/`analytics`/`operational`) at every v3 version; `schedule`,
`scheduler` and a human `description` join at v3.1.0 (v3.0.x lacks all three). The documented
example - "99.9% of the time, data is available by 6 AM UTC" - is itself v3.1.0+. It is a service-level
*promise* (freshness, availability), with no metric to attach it to.

**Data quality** is a first-class, per-schema-object and per-column surface: `DataQualityChecks`, an
array of `DataQuality` rules, each with a `dimension` (KPI enum: `accuracy`, `completeness`,
`conformity`, `consistency`, `coverage`, `timeliness`, `uniqueness`), a `type` (`text`/`library`/`sql`/
`custom`), and a closed threshold family (`mustBe`, `mustNotBe`, six `mustBe*Than`/`Between`
comparisons) across all of v3. The `library` type's own metric is version-dependent: a closed enum
(`nullValues`, `missingValues`, `invalidValues`, `duplicateValues`, `rowCount`) from v3.1.0, a free
`rule` string at v3.0.x. `sql` carries a raw `query` string. **There is no metric entity anywhere.**

## The three absent things that decide

**Metric is absent entirely - there is nothing to refuse, and that is the stricter statement.** The
word `metric` appears in the schema only inside the data-quality library enum and `slaProperties`'s
free-text property names - no `Measure`, no `Metric` entity, and no metric-level computation at any
depth or version. v3.2.0's `semanticType: measure` is documented as *an aggregated value whose
aggregation expression is held in `transformLogic`* - a free string in a dialect nothing parses, the
exact half-a-definition ADR 0016's refusal names - but `Definitions::assemble`'s refusal of that shape
cannot even fire the way it does for DataHub, because there is no metric slot to carry the string in.
The honest `capabilities()` statement is a **declared absence**, the same "nothing to refuse because
there is no slot" verdict as the OKF finding.

**Quality rules and SLA are reported, not defined.** Even where a rule crosses onto `RequiredFilter`'s
operator family (`mustBe`, `mustNotBe`), it is a scalar check on the column's own values at check time,
attached to a column or table, never to a metric - there is no metric to condition. The SLA is
explicitly a temporal/availability promise with no semantic content: issue #973's own constraint says
*service levels read and ignored, by declaration*. Both are read and ignored rather than minted into a
`RequiredFilter`.

**`Cardinality` is never declared, by the same reasoning `sutura-catalog-rdbms` gives.** A single-column
foreign key with `primaryKey`/`unique` target evidence licenses `Relationships` may-provide, but
`Cardinality` is a separate kind with nothing here to reach: `Definitions::assemble` refuses a
*dimension* reached through an under-licensed join, and ODCS has no metric to own a dimension in the
first place. A relationship loads as a non-duplicating join with no dimension ever asking to be reached
through it, so `produced` observes `Cardinality` absent and the declaration agrees - the rdbms adapter's
own verdict, unchanged.

## The mapping

| The semantic-model field              | What ODCS offers                                                                                             | Verdict                                                                                                                |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| `Model` (table + columns)             | `schema[]` `SchemaObject` + `properties[].name/physicalType/logicalType`                                     | **provides**                                                                                                           |
| `Description`                         | `SchemaElement.description` / `businessName`, contract `description.usage/purpose/limitations`               | **may-provide** - `description` is optional                                                                            |
| `ColumnTypes`                         | `properties[].logicalType` (enum) + `physicalType` (free text)                                               | **may-provide** - `logicalType` is optional; `ColumnType`-parsed                                                       |
| `ColumnDescriptions`                  | `properties[].description` / `businessName` (free text)                                                      | **may-provide** - optional per property                                                                                |
| primary key                           | `SchemaBaseProperty.primaryKey` (+ `primaryKeyPosition` order)                                               | **provides** - evidence on `Model::with_primary_key`, and target-uniqueness evidence for the relationship rule below   |
| `required`                            | `SchemaBaseProperty.required` (boolean, not-null)                                                            | **carried, not a declared kind** - maps onto `Column.nullable` (`Some(false)`); no `DefinitionKind` covers nullability |
| `unique`                              | `SchemaBaseProperty.unique` (boolean)                                                                        | **no `Column`-level carrier** of its own; feeds the relationship rule below as single-column target evidence           |
| `Relationship` (join)                 | `schema[].relationships[]` (v3.1+): foreignKey, from/to, single-column `primaryKey`/`unique` target evidence | **may-provide** on v3.1+ under the `sutura-catalog-rdbms` target-uniqueness rule (absent in the 3.0 line)              |
| `Measure`                             | none (v3.2's `semanticType: measure` is a transform string, absent in 3.0)                                   | **absent, declared**                                                                                                   |
| `RequiredFilter`                      | `quality` rules (`mustBe` family) - data-quality thresholds, not definitional                                | **reported, not defined**                                                                                              |
| `Grain` + time                        | `dataGranularityDescription` free text only                                                                  | **absent, declared**                                                                                                   |
| `Dimension` with `via` / allowlist    | `semanticType: dimension` (v3.2+) names a column's role; no `via`, no metric to own a `Dimension`            | **absent, declared** - a `Dimension` is only ever built inside a `Metric` (`Metric::new`), and there is none here      |
| `AllowedValues` (dimension allowlist) | `properties[].enum` `EnumValue[]` (v3.2+)                                                                    | **absent, declared** - `enum` has no `Dimension` to attach to, same reason as `Audience` below                         |
| `Anchor`                              | none                                                                                                         | **absent, declared**                                                                                                   |
| `Audience` (who may see a metric)     | `properties[].classification` - a free-text string, e.g. `confidential` / `restricted` / `public`            | **reported, not defined** - see the classification paragraph                                                           |
| SLA                                   | `slaProperties[]` + `slaDefaultElement`                                                                      | **reported, ignored by declaration** (issue #973's own constraint)                                                     |
| `Knowledge` with `Referent`           | `description` / `businessName` free text, `context` (v3.2.0, RFC-0038) and `synonyms` (v3.2.0, RFC-0041)     | **partial** - prose travels; every `Referent` is metric-anchored and there is no metric to anchor it to                |

## The classification question, stated honestly

The issue's Where names *"classification → `Audience` where a metric exists"* - and the qualification
is the whole finding. `Audience` is carried **on `Metric`** in this repository
(`catalog::Metric.audience`, under `docs/adr/0028`); ODCS's `classification` is a free-text per-column
string with published examples `confidential`, `restricted`, `public` - **not** a closed scheme and
**not** attached to any metric. Classification only means something *for what it restricts*, and there
is no metric here to restrict, so it stays **reported, not defined**: read and never minted into an
`Audience`.

## Conclusion, and what it costs

**A narrower source than Wren, and the cheapest connector - by declaration.** ODCS v3 can carry the
physical model, the column prose, column types, the primary key and (v3.1+) a non-duplicating join; it
cannot carry a metric, a required filter, a grain, an anchor, or a dimension. An adapter over it is a
**declaring** `SemanticCatalog` whose `capabilities()` provides `Structure`; may-provide `Descriptions`,
`ColumnTypes` and `ColumnDescriptions` (each optional in the schema) and `Relationships` on v3.1+ under the
`sutura-catalog-rdbms` target-uniqueness rule, and never declares `Cardinality`; reports-not-defines the
quality rules, the SLA and the classification; and declares `Metrics`, `RequiredFilters`, `Grains`,
`AllowedValues` and `Anchors` absent. This is precisely the shape `sutura-catalog-datahub`, `sutura-catalog-rdbms`,
`sutura-catalog-okf` and `sutura-catalog-openmetadata` already establish, and it is tested against that
declaration in both directions (it provides what it declares, and the bundle agrees that what it does
not declare does not arrive).

**Whether it is worth shipping**. Yes, for the reason any `declaring` catalog's describing half exists:
a source that provides tables and descriptions but no metric is a legitimate narrow source, and
`Definitions::assemble` refuses nothing it does not provide. Its concrete value over a bare directory of
descriptors is that the contract already carries structure - the primary key, nullability, a
target-vouched-for join - that a plain dictionary does not, and it is reviewable, file-based
and needs no network at boot, exactly issue #973's Why. The issue's two acceptance cells follow: a
**catalog-golden cell** (the fixture bundle measured against the adapter's declaration, the universal
shape every `declaring` adapter gets) and a **declaration-fidelity cell**
(`MetadataCapabilities::checked_against` over a real fixture), each narrower than an adapter that could
offer the metric layer.

## Alternatives considered

**Which version to pin.** The 3.0 line is where the shape that decides the adapter settles: the
physical model, the prose, `primaryKey`/`required`/`unique` and the SLA all exist there, and none of the
kinds that change verdict by version do. `Relationships` reads whatever `apiVersion` the document
declares and may-provides only where that version carries a `relationships` array (v3.1+) - `of_may_provide`
is the mechanism for exactly this. `enum` and `semanticType` (v3.2+) never turn into a **provides**,
because `AllowedValues` and `Dimension` need a metric this schema never has at any version, so pinning to
v3.2.0 would buy nothing a v3.0.0 contract does not already offer.

## Limits, stated

Written read-only, against the published ODCS JSON Schema at the `v3.0.0`, `v3.0.2`, `v3.1.0` and
`v3.2.0` tags and the `SemanticCatalog`/`Definitions` port in this repository, **not** against any
provisioned instance (no catalog of real contracts was opened for this record). Three things are open
to a live check and should be before an adapter ships, and none of them changes the shape decision: (a)
whether real contracts in the wild attach the data-quality `mustBe`/`mustNotBe` rules densely enough to
make "reported-not-defined" more than a formality, (b) whether a real deployment's contracts carry
single-column `primaryKey`/`unique` target evidence densely enough for the `Relationships` may-provide
turn to fire in practice rather than fall to `TargetUniquenessUnknown`, and (c) as with the OKF finding,
whether the free-text `description` is dense enough to load as `Knowledge` or better left to the
consuming prompt. The classification "reported, not defined" is the strongest claim here and the one a
reviewer should test first, because it is the only row that depends on an *absence of a metric entity*
rather than on a field's own shape.
